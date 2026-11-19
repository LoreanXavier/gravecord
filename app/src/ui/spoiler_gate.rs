//! Spoiler channels: the gate in place of a spoiler channel's chat until the account agrees (module 873614 on the
//! gated-content layout of module 506164), the store of those agreements (ChannelSpoilerAgreeStore, module 824492),
//! which channel gates the one in view (module 288254: the channel itself, or a thread's parent) and the channel
//! menu's "Re-hide Spoiler Channel" (module 823255 `v`).
//!
//! Module 824492 is a DeviceSettingsStore persisted under `ChannelSpoilerAgreeStore` as
//! `{users: {<user>: {channels: {<channel>: true}}}}`; the copy in memory is read when drawing and the file is
//! written off the UI thread.

use super::channel_settings::IS_SPOILER_CHANNEL;
use super::{Root, color, icon};
use dc_state::{Channel, Id};
use gpui::{AnyElement, Context, FontWeight, IntoElement, div, prelude::*, px};
use serde_json::{Value, json};
use std::collections::BTreeSet;
use std::sync::{Mutex, OnceLock};

/// The persist key of module 824492.
const STORAGE_KEY: &str = "ChannelSpoilerAgreeStore";

/// The agreements as `(user, channel)` pairs, read from storage on first use.
fn agreements() -> &'static Mutex<BTreeSet<(Id, Id)>> {
    static CELL: OnceLock<Mutex<BTreeSet<(Id, Id)>>> = OnceLock::new();
    CELL.get_or_init(|| {
        Mutex::new(
            crate::storage::get(STORAGE_KEY)
                .map(|state| read_state(&state))
                .unwrap_or_default(),
        )
    })
}

/// `initialize` of module 824492: `{users: {<user>: {channels: {<channel>: true}}}}`.
fn read_state(state: &Value) -> BTreeSet<(Id, Id)> {
    state["users"]
        .as_object()
        .into_iter()
        .flatten()
        .filter_map(|(user, entry)| Some((user.parse::<Id>().ok()?, entry)))
        .flat_map(|(user, entry)| {
            entry["channels"]
                .as_object()
                .into_iter()
                .flatten()
                .filter(|(_, agreed)| agreed.as_bool() == Some(true))
                .filter_map(move |(channel, _)| Some((user, channel.parse::<Id>().ok()?)))
        })
        .collect()
}

/// The persisted shape of the agreements (`getState` of module 824492).
fn write_state(pairs: &BTreeSet<(Id, Id)>) -> Value {
    let mut users = serde_json::Map::new();
    for (user, channel) in pairs {
        let entry = users
            .entry(user.to_string())
            .or_insert_with(|| json!({"channels": {}}));
        entry["channels"][channel.to_string()] = json!(true);
    }
    json!({ "users": users })
}

/// `didAgree` of module 824492.
pub(super) fn did_agree(user: Option<Id>, channel: Id) -> bool {
    user.is_some_and(|user| {
        agreements()
            .lock()
            .is_ok_and(|pairs| pairs.contains(&(user, channel)))
    })
}

/// `CHANNEL_SPOILER_AGREE` (`true`) and `CHANNEL_SPOILER_AGREE_CLEAR` (`false`) of module 824492.
fn set_agreed(user: Id, channel: Id, agreed: bool) {
    let state = {
        let Ok(mut pairs) = agreements().lock() else {
            return;
        };
        let changed = if agreed {
            pairs.insert((user, channel))
        } else {
            pairs.remove(&(user, channel))
        };
        if !changed {
            return;
        }
        write_state(&pairs)
    };
    std::thread::spawn(move || crate::storage::set(STORAGE_KEY, state));
}

/// `isSpoilerChannel` of module 95701.
pub(super) fn is_spoiler_channel(channel: &Channel) -> bool {
    channel.flags & IS_SPOILER_CHANNEL != 0
}

/// `s` of module 288254: the channel that gates `channel` (itself, else its parent), when it is a spoiler channel the
/// account has not agreed to.
pub(super) fn gating_channel(
    channel: Option<&Channel>,
    parent: Option<&Channel>,
    agreed: impl Fn(Id) -> bool,
) -> Option<Id> {
    let channel = channel?;
    if is_spoiler_channel(channel) {
        return (!agreed(channel.id)).then_some(channel.id);
    }
    parent
        .filter(|parent| is_spoiler_channel(parent) && !agreed(parent.id))
        .map(|parent| parent.id)
}

impl Root {
    /// The spoiler channel gating the channel or thread in view (`Uf` of module 288254), if any.
    pub(super) fn spoiler_gating_channel(&self) -> Option<Id> {
        let id = self.channel?;
        let guild = self.current_guild()?;
        let me = self.stores.current_user.as_ref().map(|user| user.id);
        let agreed = |channel: Id| did_agree(me, channel);
        if let Some(channel) = guild.channels.iter().find(|known| known.id == id) {
            let parent = channel
                .parent_id
                .and_then(|parent| guild.channels.iter().find(|known| known.id == parent));
            return gating_channel(Some(channel), parent, agreed);
        }
        // A thread: its parent channel gates it.
        let parent = guild
            .threads
            .iter()
            .find(|thread| thread.id == id)?
            .parent_id;
        let parent = guild.channels.iter().find(|known| known.id == parent)?;
        gating_channel(Some(parent), None, agreed)
    }

    /// `v` of module 823255: "Re-hide Spoiler Channel" for a spoiler channel the account agreed to.
    pub(super) fn spoiler_rehide_shown(&self, channel: &Channel) -> bool {
        let me = self.stores.current_user.as_ref().map(|user| user.id);
        is_spoiler_channel(channel) && did_agree(me, channel.id)
    }

    /// `clearSpoilerAgree` of module 778858.
    pub(super) fn clear_spoiler_agree(&mut self, channel: Id, cx: &mut Context<Self>) {
        if let Some(me) = self.stores.current_user.as_ref().map(|user| user.id) {
            set_agreed(me, channel, false);
        }
        cx.notify();
    }

    /// `spoilerAgree` of module 778858.
    fn spoiler_agree(&mut self, channel: Id, cx: &mut Context<Self>) {
        if let Some(me) = self.stores.current_user.as_ref().map(|user| user.id) {
            set_agreed(me, channel, true);
        }
        cx.notify();
    }

    /// `nsfwReturnToSafety` of module 778858: the server's default channel, unless there is none or it is NSFW or
    /// gated itself, then Friends.
    fn spoiler_go_back(&mut self, guild: Option<Id>, cx: &mut Context<Self>) {
        let me = self.stores.current_user.as_ref().map(|user| user.id);
        let target = guild.and_then(|guild| {
            let known = self.stores.guild(guild)?;
            let default = super::server_welcome_new::default_channel(known, self.stores.me()?)?;
            let channel = known
                .channels
                .iter()
                .find(|channel| channel.id == default)?;
            let parent = channel
                .parent_id
                .and_then(|parent| known.channels.iter().find(|known| known.id == parent));
            let gated = gating_channel(Some(channel), parent, |id| did_agree(me, id)).is_some();
            (!channel.nsfw && !gated).then_some((guild, default))
        });
        match target {
            Some((guild, channel)) => self.select(guild, Some(channel)),
            None => self.select_home(None),
        }
        cx.notify();
    }

    /// Module 873614 in the scroller of `wrapper_wmABvb` (`--background-base-lower`, filling the chat): the gated
    /// content of module 506164 (`gatedContent_xRyu39`: a column centred both ways, 16px gaps). Its text block (8px
    /// gaps): the title (`body-lg/semibold`, 20/24, at most 640 wide, centred) as the channel's icon (`md`, `TEXT_DEFAULT`)
    /// 4px from its name cut with an ellipsis; the topic (`body-md/medium`, 16/20, `text-muted`, three lines at most, at
    /// most 540 wide) over the 1px `--border-subtle` divider (16px above and below); the description (at most 440
    /// wide). Then Go Back (`secondary`) and Continue (`primary`), 16px apart.
    pub(super) fn spoiler_gate(&self, gating: Id, cx: &mut Context<Self>) -> AnyElement {
        let guild = self.current_guild();
        let guild_id = guild.map(|guild| guild.id);
        let channel =
            guild.and_then(|guild| guild.channels.iter().find(|known| known.id == gating));
        let rules = guild.and_then(|guild| guild.rules_channel_id);
        let title: AnyElement = match channel {
            Some(channel) => div()
                .flex()
                .items_center()
                .justify_center()
                .gap(px(4.))
                .overflow_hidden()
                .child(div().flex_none().flex().items_center().child(icon(
                    super::channel_icon::name(
                        &self.channel_icon_facts(channel, rules == Some(channel.id)),
                    ),
                    24.,
                    "--text-default",
                )))
                .child(
                    div()
                        .min_w(px(0.))
                        .overflow_hidden()
                        .text_ellipsis()
                        .whitespace_nowrap()
                        .child(channel.name.clone()),
                )
                .into_any_element(),
            None => dc_intl::t("q38/ae").into_any_element(),
        };
        // GAP: the topic is plain text (module 46054 `parseTruncatedTopic` renders its markdown), and "See More"
        // (the channel topic dialog of module 181094, not built) is not drawn.
        let topic = channel
            .and_then(|channel| channel.topic.as_deref())
            .map(str::trim)
            .filter(|topic| !topic.is_empty())
            .map(|topic| {
                div()
                    .w_full()
                    .flex()
                    .flex_col()
                    .items_center()
                    .child(
                        div()
                            .max_w(px(540.))
                            .overflow_hidden()
                            .text_center()
                            .text_size(px(16.))
                            .line_height(px(20.))
                            .font_weight(FontWeight(500.))
                            .text_color(color("--text-muted"))
                            .line_clamp(3)
                            .child(topic.to_owned()),
                    )
                    .child(
                        div()
                            .w_full()
                            .max_w(px(540.))
                            .h(px(1.))
                            .my(px(16.))
                            .bg(color("--border-subtle")),
                    )
            });
        let back = super::modal::content_button(
            super::modal::button(
                "spoiler-gate-back",
                dc_intl::t("/g10LC"),
                super::modal::Tone::Secondary,
                move |this, _, cx| this.spoiler_go_back(guild_id, cx),
            ),
            cx,
        );
        let agree = super::modal::content_button(
            super::modal::button(
                "spoiler-gate-continue",
                dc_intl::t("KmRwcW"),
                super::modal::Tone::Primary,
                move |this, _, cx| this.spoiler_agree(gating, cx),
            ),
            cx,
        );
        div()
            .id("spoiler-gate")
            .flex_1()
            .min_h(px(1.))
            .min_w(px(1.))
            .bg(color("--background-base-lower"))
            .overflow_y_scroll()
            .child(
                div()
                    .size_full()
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .gap(px(16.))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .items_center()
                            .justify_center()
                            .gap(px(8.))
                            .child(
                                div()
                                    .max_w(px(640.))
                                    .text_center()
                                    .text_size(px(20.))
                                    .line_height(px(24.))
                                    .font_weight(FontWeight(600.))
                                    .text_color(color("--text-strong"))
                                    .child(title),
                            )
                            .children(topic)
                            .child(
                                div()
                                    .max_w(px(440.))
                                    .text_center()
                                    .text_size(px(16.))
                                    .line_height(px(20.))
                                    .font_weight(FontWeight(500.))
                                    .text_color(color("--text-muted"))
                                    .child(dc_intl::t("08bm2Z")),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_center()
                            .gap(px(16.))
                            .child(back)
                            .child(agree),
                    ),
            )
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn channel(id: Id, flags: u64, parent: Option<Id>) -> Channel {
        Channel {
            id,
            name: String::new(),
            kind: dc_state::ChannelKind::Text,
            position: 0,
            parent_id: parent,
            last_message_id: None,
            overwrites: Vec::new(),
            user_limit: 0,
            topic: None,
            rate_limit_per_user: 0,
            flags,
            nsfw: false,
            default_auto_archive_duration: None,
            available_tags: Vec::new(),
            default_sort_order: None,
            default_forum_layout: 0,
        }
    }

    #[test]
    fn the_channel_or_its_parent_gates_until_agreed() {
        let spoiler = channel(1, IS_SPOILER_CHANNEL, None);
        let plain = channel(2, 0, Some(1));
        assert_eq!(gating_channel(Some(&spoiler), None, |_| false), Some(1));
        assert_eq!(gating_channel(Some(&spoiler), None, |_| true), None);
        assert_eq!(
            gating_channel(Some(&plain), Some(&spoiler), |_| false),
            Some(1)
        );
        assert_eq!(gating_channel(Some(&plain), Some(&spoiler), |_| true), None);
        assert_eq!(gating_channel(Some(&plain), None, |_| false), None);
        assert_eq!(gating_channel(None, Some(&spoiler), |_| false), None);
    }

    #[test]
    fn agreements_keep_the_official_shape() {
        let pairs = BTreeSet::from([(10, 1), (10, 2), (11, 3)]);
        let state = write_state(&pairs);
        assert_eq!(state["users"]["10"]["channels"]["2"], true);
        assert_eq!(read_state(&state), pairs);
        assert!(read_state(&json!({"users": {"10": {"channels": {"5": false}}}})).is_empty());
    }
}
