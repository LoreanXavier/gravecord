//! VoiceUserPopout (module 772800), opened from a voice user row of the channel list (module 340851, with the
//! preview state of module 588224): 100ms after the pointer enters the row (`setTimeout` of 340851) and 50ms more
//! (`J_(50)` of 588224) the popout opens 17px to the right of the row, unless the row is the account on another
//! client or the row's profile popout is open; it closes 175ms after the pointer has left the row and the popout.
//!
//! With the guild in `2026-04_voice_user_duration` (module 402313, `T`) it starts with `F`: the avatar, the name and
//! how long the person has been in the channel (`connectedAt`); then, or without `T` alone, the person's cards. With
//! neither, nothing opens. Styles: `*_utcclU` (module 8936, stylesheet 847810) over `popover_81IzEA` (module 489387).

use super::live_activity::Surface;
use super::member_avatars::HIDDEN_STATUS;
use super::voice_badge::HoverCard;
use super::{Root, color};
use crate::parity::Parity;
use dc_state::Id;
use gpui::{
    AnyElement, BoxShadow, Context, FontWeight, IntoElement, SharedString, div, hsla, point,
    prelude::*, px,
};
use serde_json::Value;

/// Module 402313: treatment 1 shows how long people have been in voice; the default is off.
pub(super) const DURATION_EXPERIMENT: &str = "2026-04_voice_user_duration";
/// 100ms of the row's `setTimeout`, then 50ms of `J_(50)` in module 588224.
pub(super) const OPEN_AFTER: u64 = 150;
/// `spacing: 17` of the row's popout.
pub(super) const SPACING: f32 = 17.;
/// `ActivityFlags.EMBEDDED` (module 652215 `jUm`).
const EMBEDDED: u64 = 1 << 8;

/// Module 845056 `N`: a game or something watched that says anything about itself, or anything listened to.
pub(super) fn voice_activity_shown(activity: &Value) -> bool {
    match activity["type"].as_u64() {
        Some(0 | 3) => [
            "name",
            "application_id",
            "assets",
            "state",
            "details",
            "party",
        ]
        .iter()
        .any(|key| !activity[*key].is_null()),
        Some(2) => true,
        _ => false,
    }
}

/// Module 765379: an activity flagged `EMBEDDED` (an Activity running in a call) is left to the embedded app cards.
fn embedded(activity: &Value) -> bool {
    activity["flags"].as_u64().unwrap_or(0) & EMBEDDED == EMBEDDED
}

/// The row's popout state that the card needs: the person, the channel and its guild.
#[derive(Clone, Copy, PartialEq)]
pub(super) struct VoiceUserTarget {
    pub guild: Id,
    pub channel: Id,
    pub user: Id,
}

impl Root {
    /// The person whose popout is open, for the live cards' assets (`live_users`).
    pub(super) fn voice_popout_user(&self) -> Option<Id> {
        match self.voice_hover_open()? {
            HoverCard::User(target) => Some(target.user),
            HoverCard::Channel { .. } => None,
        }
    }

    /// The pointer entered (`inside`) or left a voice user row (`ee` and `eI` of module 340851).
    pub(super) fn voice_row_hover(
        &mut self,
        target: VoiceUserTarget,
        inside: bool,
        cx: &mut Context<Self>,
    ) {
        let key = SharedString::from(format!("voice-member-{}", target.user));
        if !inside {
            self.voice_hover_leave(&key, cx);
            return;
        }
        // `isSelfOnOtherClient`: the account's own row for another session has no popout.
        let me = self.stores.current_user.as_ref().map(|me| me.id);
        let other_client = me == Some(target.user)
            && self
                .stores
                .voice_members(target.guild, target.channel)
                .iter()
                .find(|state| state.user_id == target.user)
                .is_some_and(|state| {
                    self.stores.session_id.as_deref() != Some(state.session_id.as_str())
                });
        // `shouldShowHoverPopout && !em`: not while the row's own profile popout is open.
        let profile_open = self
            .profile
            .as_ref()
            .is_some_and(|open| open.user == target.user);
        if other_client || profile_open {
            return;
        }
        self.voice_hover_enter(key, HoverCard::User(target), OPEN_AFTER, cx);
    }

    /// Module 772800 `A`, or `None` when it renders nothing (`!J`).
    ///
    /// GAP: of the cards only the activities (module 845056 `m`, drawn with the profile's live activity cards rather
    /// than module 329554's voice activity card) are drawn. The stream card (module 727353 `P`, with Watch Stream),
    /// the embedded Activity cards (module 685399 through 329554) and the voice channel game card (module 655724 `B`,
    /// `$` of 208284/729829) are not, so they neither show nor make the popout open; the stream's own activity is not
    /// left out of the list either.
    pub(super) fn voice_user_card(
        &self,
        target: VoiceUserTarget,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let VoiceUserTarget {
            guild,
            channel,
            user,
        } = target;
        let duration = self.guild_experiment_on(DURATION_EXPERIMENT, guild);
        let picks: Vec<(usize, bool)> = self
            .stores
            .presence
            .get(user, Some(guild))
            .map(|presence| {
                presence
                    .visible_activities()
                    .iter()
                    .enumerate()
                    .filter(|(_, activity)| voice_activity_shown(activity) && !embedded(activity))
                    .map(|(index, _)| (index, true))
                    .collect()
            })
            .unwrap_or_default();
        let cards: Vec<AnyElement> = if picks.is_empty() {
            Vec::new()
        } else {
            self.live_activity_cards_picked(
                user,
                Surface::Popout,
                color("--text-strong").into(),
                color("--text-subtle").into(),
                Some(&picks),
                cx,
            )
        };
        let has_cards = !cards.is_empty();
        if !has_cards && !duration {
            return None;
        }
        // `scroller_utcclU` (inside `experimentContainer_utcclU`) or `container_utcclU`: the cards 12px apart, each
        // after the first under a 1px rule with 12px above it.
        let rule = if duration {
            "--border-subtle"
        } else {
            "--border-muted"
        };
        let list = |cards: Vec<AnyElement>| {
            cards.into_iter().enumerate().map(move |(index, card)| {
                div()
                    .when(index > 0, |this| {
                        this.border_t_1().border_color(color(rule)).pt(px(12.))
                    })
                    .child(card)
                    .into_any_element()
            })
        };
        let popover = || {
            // `popover_81IzEA`: `--background-surface-high`, `--radius-md`, a 1px `--border-subtle` ring inside and
            // `--shadow-high`.
            div()
                .bg(color("--background-surface-high"))
                .rounded(px(12.))
                .border_1()
                .border_color(color("--border-subtle"))
                .shadow(vec![BoxShadow {
                    color: hsla(0., 0., 0., 0.24),
                    offset: point(px(0.), px(12.)),
                    blur_radius: px(24.),
                    spread_radius: px(0.),
                    inset: false,
                }])
                .flex()
                .flex_col()
        };
        if !duration {
            // `container_utcclU`: 280px, 16px inside, at most 90% of the window tall.
            return Some(
                popover()
                    .parity("voice-user-popout")
                    .w(px(280.))
                    .p(px(16.))
                    .gap(px(12.))
                    .children(list(cards))
                    .into_any_element(),
            );
        }
        let header = self.voice_user_duration(guild, channel, user, cx)?;
        Some(
            popover()
                .parity("voice-user-popout")
                .w(px(280.))
                .child(header)
                .when(has_cards, |this| {
                    // `scroller_utcclU`: a popover of its own with square top corners, 16px inside, at most 404px.
                    this.child(
                        popover()
                            .rounded_t(px(0.))
                            .max_h(px(404.))
                            .p(px(16.))
                            .gap(px(12.))
                            .children(list(cards)),
                    )
                })
                .into_any_element(),
        )
    }

    /// `F`: `durationContainer_utcclU` on `--opacity-4` with the top corners round, 12px by 16px inside: the 32px avatar
    /// (streaming, or the status unless offline), then the name (`body-sm/medium`, the member's nick or the display
    /// name, one line) and the time since `connectedAt`, 12px apart and pushed to the two ends.
    fn voice_user_duration(
        &self,
        guild: Id,
        channel: Id,
        user: Id,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let known = self
            .stores
            .users
            .get(&user)
            .cloned()
            .or_else(|| self.stores.current_user.clone().filter(|me| me.id == user))?;
        let name = self
            .stores
            .members
            .get(guild, user)
            .and_then(|member| member.nick.clone())
            .unwrap_or_else(|| known.display_name().to_owned());
        let status = match self.stores.status(user) {
            "offline" | "invisible" => HIDDEN_STATUS,
            status => status,
        };
        let connected = self
            .stores
            .voice_members(guild, channel)
            .iter()
            .find(|state| state.user_id == user)
            .and_then(|state| state.connected_at)
            .map(|seconds| seconds * 1000);
        Some(
            div()
                .parity("voice-user-duration")
                .flex()
                .items_center()
                .justify_between()
                .gap(px(8.))
                .py(px(12.))
                .px(px(16.))
                .rounded_t(px(12.))
                .bg(color("--opacity-4"))
                .child(self.member_avatar(&known, status, false, false, cx))
                .child(
                    div()
                        .flex_grow(1.)
                        .min_w(px(0.))
                        .overflow_hidden()
                        .flex()
                        .items_center()
                        .justify_between()
                        .gap(px(12.))
                        .child(
                            div()
                                .min_w(px(0.))
                                .overflow_hidden()
                                .whitespace_nowrap()
                                .text_ellipsis()
                                .text_size(px(14.))
                                .line_height(px(18.))
                                .font_weight(FontWeight(500.))
                                .text_color(color("--text-default"))
                                .child(SharedString::from(name)),
                        )
                        .children(connected.map(|start| self.elapsed_timer(start, cx))),
                )
                .into_any_element(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::voice_activity_shown;
    use serde_json::json;

    #[test]
    fn games_and_watching_need_something_listening_needs_nothing() {
        assert!(voice_activity_shown(&json!({"type": 0, "name": "Game"})));
        assert!(!voice_activity_shown(&json!({"type": 3})));
        assert!(voice_activity_shown(&json!({"type": 2})));
        assert!(!voice_activity_shown(
            &json!({"type": 4, "state": "custom"})
        ));
        assert!(!voice_activity_shown(&json!({"type": 1, "name": "Stream"})));
    }
}
