//! Blocked and ignored people in voice channels, and the warnings about them.
//!
//! - `VoiceChannelBlockedUsersStore` (module 148354): per channel, the blocked and the ignored people whose voice state
//!   came in a `VOICE_STATE_UPDATES` (or whose relationship changed while in a channel) since the connection opened.
//!   States that arrive with the guild at connection open are not counted, as in the official store, which only
//!   resets on `CONNECTION_OPEN`.
//! - `shared-spaces-warning-storage` (module 549022): when a warning was last dismissed, per person and overall, kept in
//!   local storage (the zustand `persist` shape, `{state, version}`). A person dismissed in the last two days, or anyone
//!   within an hour of the last dismissal, is not warned about again.
//! - "Join voice?" (module 525828) before joining a voice channel with such people (`handleVoiceConnect` of module
//!   262763), and "Leave voice?" (module 552258, opened by `k` of module 227724) when one of them joins the channel of
//!   this client's call. Both are module 212758 on the dialog with a header graphic (module 772707).

use super::{Root, avatar_source, color, graphic_modal, icon, modal};
use dc_state::Id;
use gpui::{AnyElement, Context, FontWeight, IntoElement, div, img, prelude::*, px};
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};

const STORAGE_KEY: &str = "shared-spaces-warning-storage";
const HOUR_MS: i64 = 3_600_000;
const DAY_MS: i64 = 24 * HOUR_MS;
/// `o` of module 549022: a person dismissed this recently is not warned about again.
const USER_WINDOW_MS: i64 = 2 * DAY_MS;
/// `d`: after any dismissal, nobody is warned about for an hour.
const GLOBAL_WINDOW_MS: i64 = HOUR_MS;

/// The warnings' picture (module 117816), 288 by 162.
pub(super) const WARNING_ART: graphic_modal::Graphic = graphic_modal::Graphic {
    url: "https://cdn.discordapp.com/assets/content/971d3a140aa11a3830346a83266ea28b2f4e586e9b4f02eed4315538f6a9ebbe.svg",
    width: 288.,
    height: 162.,
};

/// The dismissal times of module 549022.
#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct WarningDismissals {
    users: HashMap<Id, i64>,
    global: Option<i64>,
}

impl WarningDismissals {
    pub(super) fn load() -> Self {
        let state = crate::storage::get(STORAGE_KEY)
            .map(|stored| stored["state"].clone())
            .unwrap_or(Value::Null);
        let users = state["userDismissTimestamps"]
            .as_object()
            .into_iter()
            .flatten()
            .filter_map(|(user, at)| Some((user.parse().ok()?, at.as_i64()?)))
            .collect();
        Self {
            users,
            global: state["globalDismissTimestamp"].as_i64(),
        }
    }

    fn save(&self) {
        let users: serde_json::Map<String, Value> = self
            .users
            .iter()
            .map(|(user, at)| (user.to_string(), json!(at)))
            .collect();
        let mut stored = crate::storage::get(STORAGE_KEY).unwrap_or_else(|| json!({}));
        if !stored.is_object() {
            stored = json!({});
        }
        let state = &mut stored["state"];
        if !state.is_object() {
            *state = json!({"channelDismissTimestamps": {}, "queuedWarning": false});
        }
        state["userDismissTimestamps"] = Value::Object(users);
        state["globalDismissTimestamp"] = json!(self.global);
        stored["version"] = json!(0);
        crate::storage::set(STORAGE_KEY, stored);
    }

    /// `u`: any warning was dismissed in the last hour.
    fn global_recent(&self, now: i64) -> bool {
        self.global.unwrap_or(0) > now - GLOBAL_WINDOW_MS
    }

    /// `a0` (`_`): this person was dismissed in the last two days, or (unless `ignore_global`) anyone in the last hour.
    pub(super) fn user_dismissed(&self, user: Id, ignore_global: bool, now: i64) -> bool {
        (!ignore_global && self.global_recent(now))
            || self.users.get(&user).copied().unwrap_or(0) > now - USER_WINDOW_MS
    }

    /// `oE`: no warning is due for these people.
    pub(super) fn all_dismissed(&self, users: &[Id], now: i64) -> bool {
        self.global_recent(now)
            || users
                .iter()
                .all(|user| self.user_dismissed(*user, true, now))
    }

    /// `$Y` and `Cs`: these people were dismissed now.
    pub(super) fn dismiss(&mut self, users: &[Id], now: i64) {
        for user in users {
            self.users.insert(*user, now);
        }
        self.global = Some(now);
        self.save();
    }
}

/// `VoiceChannelBlockedUsersStore` (module 148354).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct BlockedInVoice {
    blocked: HashMap<Id, HashSet<Id>>,
    ignored: HashMap<Id, HashSet<Id>>,
}

impl BlockedInVoice {
    pub(super) fn reset(&mut self) {
        self.blocked.clear();
        self.ignored.clear();
    }

    pub(super) fn blocked_in(&self, channel: Id) -> Vec<Id> {
        let mut users: Vec<Id> = self
            .blocked
            .get(&channel)
            .into_iter()
            .flatten()
            .copied()
            .collect();
        users.sort_unstable();
        users
    }

    pub(super) fn ignored_in(&self, channel: Id) -> Vec<Id> {
        let mut users: Vec<Id> = self
            .ignored
            .get(&channel)
            .into_iter()
            .flatten()
            .copied()
            .collect();
        users.sort_unstable();
        users
    }

    /// The person left `channel` (`VOICE_STATE_UPDATES` with an `oldChannelId`).
    pub(super) fn left(&mut self, channel: Id, user: Id) {
        for map in [&mut self.blocked, &mut self.ignored] {
            if let Some(set) = map.get_mut(&channel) {
                set.remove(&user);
                if set.is_empty() {
                    map.remove(&channel);
                }
            }
        }
    }

    /// `h`: the person is in `channel`; they are counted if blocked or ignored, and dropped if no longer. True when
    /// they were added to a set they were not in (the store's `r`, which opens "Leave voice?").
    pub(super) fn place(&mut self, channel: Id, user: Id, blocked: bool, ignored: bool) -> bool {
        let mut added = false;
        for (map, now) in [(&mut self.blocked, blocked), (&mut self.ignored, ignored)] {
            let set = map.entry(channel).or_default();
            if now {
                added |= set.insert(user);
            } else {
                set.remove(&user);
            }
            if set.is_empty() {
                map.remove(&channel);
            }
        }
        added
    }
}

pub(super) fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

/// The description of "Join voice?" (module 525828) for these counts.
pub(super) fn join_description(blocked: usize, ignored: usize) -> String {
    if blocked > 0 && ignored > 0 {
        dc_intl::t("X/iIjg").to_owned()
    } else if blocked > 0 {
        dc_intl::format("WgQ12s", &[("n", (blocked as i64).into())])
    } else {
        dc_intl::format("bwibp1", &[("n", (ignored as i64).into())])
    }
}

impl Root {
    fn person_is_blocked_or_ignored(&self, user: Id) -> (bool, bool) {
        let blocked = self.is_blocked(user);
        (blocked, !blocked && self.stores.is_ignored(user))
    }

    /// The voice channel a person is in, by their voice state in `guild` (0 for calls outside a server).
    pub(super) fn voice_channel_of(&self, guild: Id, user: Id) -> Option<Id> {
        self.stores
            .voice_states
            .get(&guild)?
            .iter()
            .find(|state| state.user_id == user)
            .map(|state| state.channel_id)
            .filter(|channel| *channel != 0)
    }

    /// `VOICE_STATE_UPDATES` of module 148354, for one state: `old` is the channel the person was in before it.
    pub(super) fn blocked_voice_state(
        &mut self,
        user: Id,
        old: Option<Id>,
        new: Option<Id>,
        cx: &mut Context<Self>,
    ) {
        if self
            .stores
            .current_user
            .as_ref()
            .is_some_and(|me| me.id == user)
        {
            return;
        }
        if let Some(old) = old {
            self.blocked_in_voice.left(old, user);
        }
        if let Some(new) = new {
            self.blocked_place(new, user, cx);
        }
    }

    /// `RELATIONSHIP_ADD`, `_UPDATE` and `_REMOVE` of module 148354 (`f`): the person's channel is counted again.
    pub(super) fn blocked_relationship_changed(&mut self, data: &Value, cx: &mut Context<Self>) {
        let Some(user) = data["id"].as_str().and_then(|id| id.parse::<Id>().ok()) else {
            return;
        };
        let channel = self
            .stores
            .voice_states
            .keys()
            .copied()
            .collect::<Vec<_>>()
            .into_iter()
            .find_map(|guild| self.voice_channel_of(guild, user));
        if let Some(channel) = channel {
            self.blocked_place(channel, user, cx);
        }
    }

    /// `h` of module 148354, and the "Leave voice?" it opens when someone newly counted is in the channel of the call
    /// here and was not dismissed lately.
    fn blocked_place(&mut self, channel: Id, user: Id, cx: &mut Context<Self>) {
        let (blocked, ignored) = self.person_is_blocked_or_ignored(user);
        let added = self.blocked_in_voice.place(channel, user, blocked, ignored);
        let in_call = self
            .call
            .as_ref()
            .is_some_and(|call| call.channel == channel);
        if added
            && in_call
            && self.guild_of_channel(channel).is_some()
            && !self
                .warning_dismissals
                .user_dismissed(user, false, now_ms())
            && self.stores.users.contains_key(&user)
        {
            if let Some(known) = self.stores.users.get(&user).cloned() {
                let (key, url) = avatar_source(&known);
                self.request_image(key, url);
            }
            self.request_graphic(WARNING_ART);
            self.voice_alert =
                Some(super::voice_alerts::VoiceAlert::BlockedJoined { channel, user });
            cx.notify();
        }
    }

    /// `handleVoiceConnect` of module 262763: "Join voice?" stands in for joining a channel with blocked or ignored
    /// people, unless this client is already in it or every one of them was dismissed lately.
    pub(super) fn join_blocked_alert(
        &self,
        guild: Id,
        channel: Id,
    ) -> Option<super::voice_alerts::VoiceAlert> {
        if self
            .call
            .as_ref()
            .is_some_and(|call| call.channel == channel)
        {
            return None;
        }
        let blocked = self.blocked_in_voice.blocked_in(channel);
        let ignored = self.blocked_in_voice.ignored_in(channel);
        if blocked.is_empty() && ignored.is_empty() {
            return None;
        }
        let everyone: Vec<Id> = blocked.iter().chain(&ignored).copied().collect();
        if self.warning_dismissals.all_dismissed(&everyone, now_ms()) {
            return None;
        }
        Some(super::voice_alerts::VoiceAlert::JoinBlocked {
            guild,
            channel,
            blocked,
            ignored,
        })
    }

    /// "Join" of module 525828 (`onDismissAndStay`, also its close): everyone shown counts as dismissed, and the
    /// join goes on past this question (`bypassBlockedWarningModal`, `bypassChangeModal`).
    pub(super) fn join_past_blocked(
        &mut self,
        guild: Id,
        channel: Id,
        people: Vec<Id>,
        cx: &mut Context<Self>,
    ) {
        self.voice_alert = None;
        self.warning_dismissals.dismiss(&people, now_ms());
        self.join_voice(guild, channel, cx);
    }

    /// "Stay here" of module 552258 (also its close): this person counts as dismissed.
    pub(super) fn stay_past_blocked(&mut self, user: Id) {
        self.voice_alert = None;
        self.warning_dismissals.dismiss(&[user], now_ms());
    }

    /// One `infoRow_OBZokR row_OObHCR`: the icon box (`icon_OObHCR`, at least 32 wide) and the `body-md/medium` text;
    /// the row's padding and gap are `row_OObHCR`'s 12px, whose sheet loads after module 212758's.
    fn warning_row(icon_box: AnyElement, text: AnyElement) -> AnyElement {
        div()
            .w_full()
            .p(px(12.))
            .flex()
            .items_center()
            .gap(px(12.))
            .child(
                div()
                    .flex_none()
                    .min_w(px(32.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(icon_box),
            )
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .text_size(px(16.))
                    .line_height(px(20.))
                    .font_weight(FontWeight(500.))
                    .text_color(color("--text-default"))
                    .child(text),
            )
            .into_any_element()
    }

    /// `I` of module 90386: one person's 32px avatar, or for more the first two side by side (module 954376).
    fn warning_faces(&self, users: &[Id]) -> AnyElement {
        let face = |user: Id| {
            let picture = self
                .stores
                .users
                .get(&user)
                .and_then(|known| self.images.get(&avatar_source(known).0).cloned());
            div()
                .flex_none()
                .size(px(32.))
                .rounded_full()
                .overflow_hidden()
                .bg(color("--background-mod-strong"))
                .children(picture.map(|picture| img(picture).size(px(32.)).rounded_full()))
        };
        match users {
            [one] => face(*one).into_any_element(),
            // GAP: module 954376's stacked pair (its overlap and cut-out) is drawn as two faces 4px apart.
            _ => div()
                .flex()
                .gap(px(4.))
                .children(users.iter().take(2).map(|user| face(*user)))
                .into_any_element(),
        }
    }

    /// `T` of module 90386 as runs: the names `body-md/semibold`.
    fn warning_here_line(&self, guild: Option<Id>, shown: &[Id], count: usize) -> AnyElement {
        let names: Vec<String> = shown
            .iter()
            .take(2)
            // `getName` of module 562153: the server nickname, else the display name.
            .map(|user| {
                guild
                    .and_then(|guild| self.stores.members.get(guild, *user))
                    .and_then(|member| member.nick.clone())
                    .or_else(|| {
                        self.stores
                            .users
                            .get(user)
                            .map(|known| known.display_name().to_owned())
                    })
                    .unwrap_or_default()
            })
            .collect();
        let runs = super::voice_alerts::here_runs(&names, count);
        let mut text = String::new();
        let mut bold = Vec::new();
        for (run, hook) in runs {
            let start = text.len();
            text.push_str(&run);
            if hook.is_some() {
                bold.push((
                    start..text.len(),
                    gpui::HighlightStyle {
                        font_weight: Some(FontWeight(600.)),
                        ..Default::default()
                    },
                ));
            }
        }
        gpui::StyledText::new(text)
            .with_highlights(bold)
            .into_any_element()
    }

    /// The divider between the rows (module 922529 on 202501: a 1px `--border-subtle` line, `margin-inline` 16 and 14).
    fn warning_divider() -> AnyElement {
        div()
            .ml(px(16.))
            .mr(px(14.))
            .h(px(1.))
            .bg(color("--border-subtle"))
            .into_any_element()
    }

    /// The "You will be able to hear each other" row (`WarningIcon` in `icon_OObHCR`).
    fn warning_hear_row(text: &'static str) -> AnyElement {
        Self::warning_row(
            icon("warning_triangle", 24., "--text-default").into_any_element(),
            div().child(dc_intl::t(text)).into_any_element(),
        )
    }

    /// Module 525828.
    pub(super) fn join_blocked_modal(
        &self,
        guild: Id,
        channel: Id,
        blocked: &[Id],
        ignored: &[Id],
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let people: Vec<Id> = blocked.iter().chain(ignored).copied().collect();
        let both = !blocked.is_empty() && !ignored.is_empty();
        let (shown, count): (Vec<Id>, usize) = if both {
            (people.iter().take(2).copied().collect(), people.len())
        } else if !blocked.is_empty() {
            (blocked.iter().take(2).copied().collect(), blocked.len())
        } else {
            (ignored.iter().take(2).copied().collect(), ignored.len())
        };
        let body = div()
            .w_full()
            .rounded(px(4.))
            .flex()
            .flex_col()
            .child(Self::warning_row(
                self.warning_faces(&shown),
                self.warning_here_line(Some(guild), &shown, count),
            ))
            .child(Self::warning_divider())
            .child(Self::warning_hear_row("7jN3FC"));
        let join = people.clone();
        let close = people.clone();
        self.graphic_modal(
            graphic_modal::Spec {
                id: "voice-join-blocked-modal",
                graphic: Some(WARNING_ART),
                title: dc_intl::t("zPUmzZ").into(),
                subtitle: Some(
                    div()
                        .child(join_description(blocked.len(), ignored.len()))
                        .into_any_element(),
                ),
                body: Some(body.into_any_element()),
                buttons: vec![
                    modal::button(
                        "voice-join-blocked-join",
                        dc_intl::t("VJlc0S"),
                        modal::Tone::Secondary,
                        move |this, _, cx| this.join_past_blocked(guild, channel, join.clone(), cx),
                    ),
                    modal::button(
                        "voice-join-blocked-dont",
                        dc_intl::t("rOXspL"),
                        modal::Tone::Primary,
                        |this, _, _| this.voice_alert = None,
                    ),
                ],
                close: Box::new(move |this, _, cx| {
                    this.join_past_blocked(guild, channel, close.clone(), cx)
                }),
            },
            cx,
        )
    }

    /// Module 552258.
    pub(super) fn blocked_joined_modal(
        &self,
        channel: Id,
        user: Id,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let blocked = self.is_blocked(user);
        let guild = self.guild_of_channel(channel);
        let body = div()
            .w_full()
            .rounded(px(4.))
            .flex()
            .flex_col()
            .child(Self::warning_row(
                self.warning_faces(&[user]),
                self.warning_here_line(guild, &[user], 1),
            ))
            .child(Self::warning_divider())
            .child(Self::warning_hear_row("+4O9nX"));
        self.graphic_modal(
            graphic_modal::Spec {
                id: "voice-blocked-joined-modal",
                graphic: Some(WARNING_ART),
                title: dc_intl::t("1/gpFh").into(),
                subtitle: Some(
                    div()
                        .child(dc_intl::t(if blocked { "QnTzrp" } else { "QsWbfY" }))
                        .into_any_element(),
                ),
                body: Some(body.into_any_element()),
                buttons: vec![
                    modal::button(
                        "voice-blocked-joined-stay",
                        dc_intl::t("Z+/hfb"),
                        modal::Tone::Secondary,
                        move |this, _, _| this.stay_past_blocked(user),
                    ),
                    modal::button(
                        "voice-blocked-joined-leave",
                        dc_intl::t("Hi1/aQ"),
                        modal::Tone::Primary,
                        |this, _, cx| {
                            this.voice_alert = None;
                            this.disconnect_from_call(cx);
                        },
                    ),
                ],
                close: Box::new(move |this, _, _| this.stay_past_blocked(user)),
            },
            cx,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dismissals_last_two_days_per_person_and_an_hour_for_everyone() {
        let mut store = WarningDismissals::default();
        let now = 10 * DAY_MS;
        assert!(!store.user_dismissed(1, false, now));
        store.users.insert(1, now - DAY_MS);
        store.global = Some(now - 2 * HOUR_MS);
        assert!(store.user_dismissed(1, true, now));
        assert!(!store.user_dismissed(2, false, now));
        // Within the hour after any dismissal nobody is warned about.
        store.global = Some(now - HOUR_MS / 2);
        assert!(store.user_dismissed(2, false, now));
        assert!(!store.user_dismissed(2, true, now));
        assert!(store.all_dismissed(&[2, 3], now));
        store.global = Some(now - 2 * HOUR_MS);
        assert!(!store.all_dismissed(&[1, 2], now));
        assert!(store.all_dismissed(&[1], now));
        // Two days on, the person is due again.
        assert!(!store.user_dismissed(1, true, now + DAY_MS + 1));
    }

    #[test]
    fn the_store_counts_people_who_come_and_drops_them_when_they_go() {
        let mut store = BlockedInVoice::default();
        assert!(store.place(10, 1, true, false));
        // Already counted: no new warning.
        assert!(!store.place(10, 1, true, false));
        assert!(store.place(10, 2, false, true));
        assert_eq!(store.blocked_in(10), vec![1]);
        assert_eq!(store.ignored_in(10), vec![2]);
        // Unblocked while in the channel.
        assert!(!store.place(10, 1, false, false));
        assert!(store.blocked_in(10).is_empty());
        store.left(10, 2);
        assert!(store.ignored_in(10).is_empty());
        assert_eq!(store, BlockedInVoice::default());
    }

    #[test]
    fn the_join_question_names_blocked_ignored_or_both() {
        assert_eq!(join_description(1, 1), dc_intl::t("X/iIjg"));
        assert_eq!(
            join_description(1, 0),
            "Someone you blocked is here. If you join, they will still be blocked."
        );
        assert_eq!(
            join_description(0, 2),
            "People you ignored are here. If you join, they will still be ignored."
        );
    }
}
