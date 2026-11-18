//! "This channel will now be marked as unread for every new message." (module 626977 `_`): the bar over a server
//! channel's chat once `UnreadSettingNoticeStore2` (module 496990, `dc_state::unread_notice`) has upgraded the channel
//! to unreads on every message, with Undo, which sets its unreads back to mentions only. The store is fed here: the
//! channel in view (`CHANNEL_SELECT`), the account's own messages (`MESSAGE_CREATE`), a fresh connection
//! (`CONNECTION_OPEN`) and the 15-second viewing clock.

use super::{Root, color, icon};
use dc_state::Id;
use dc_state::unread_notice::{
    PERSIST_KEY, TICK_MS, UNREADS_ONLY_MENTIONS, UnreadSettingNotice, channel_flags,
    with_unread_setting,
};
use gpui::{
    AnyElement, Context, FontWeight, IntoElement, ParentElement, Styled, div, prelude::*, px,
};

/// The "Learn More" of the notice (`$link` of `P6Y2N1`).
const LEARN_MORE: &str = "https://support.discord.com/hc/en-us/articles/21084266106775-New-Server-Notification-Settings-Experiment";

/// `r.B4` (module 95701): the channel kinds the notice is shown for.
fn shows_notice(kind: dc_state::ChannelKind) -> bool {
    use dc_state::ChannelKind;
    matches!(
        kind,
        ChannelKind::Text | ChannelKind::Category | ChannelKind::Other(5 | 15 | 21)
    )
}

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

#[derive(Default)]
pub(super) struct State {
    store: UnreadSettingNotice,
    /// `showUnreadsNotice`: the channel whose notice is up (cleared when another channel opens).
    shown: Option<Id>,
    /// The channel and last action time the upgrade was last tried for (the hook's dependencies).
    checked: Option<(Id, i64)>,
    clock: bool,
}

impl State {
    pub(super) fn load() -> Self {
        Self {
            store: crate::storage::get(PERSIST_KEY)
                .map(|value| UnreadSettingNotice::from_value(&value))
                .unwrap_or_default(),
            ..Self::default()
        }
    }

    /// The store's state goes to storage off the UI thread: the storage file is written through on every set.
    fn save(&self) {
        if !crate::storage::settings_writes_blocked() {
            let value = self.store.to_value();
            std::thread::spawn(move || crate::storage::set(PERSIST_KEY, value));
        }
    }
}

impl Root {
    /// Each frame: the channel in view is timed, and the hook of module 626977 tries the upgrade when the channel or
    /// its last action changed.
    pub(super) fn unread_notice_frame(&mut self, cx: &mut Context<Self>) {
        let now = now_ms();
        if self.unread_notice.store.current() != self.channel {
            let counted = self
                .unread_notice
                .store
                .channel_select(&self.stores, self.channel, now);
            if self.unread_notice.shown != self.channel {
                self.unread_notice.shown = None;
            }
            if counted {
                self.unread_notice.save();
            }
        }
        if self.stores.notification_flags & 16 != 0 && !self.unread_notice.clock {
            self.unread_notice.clock = true;
            self.start_unread_notice_clock(cx);
        }
        let Some(channel) = self.channel else {
            return;
        };
        let key = (channel, self.unread_notice.store.last_action_time(channel));
        if self.unread_notice.checked == Some(key) || self.unread_notice.shown == Some(channel) {
            return;
        }
        self.unread_notice.checked = Some(key);
        let kind = self
            .stores
            .guild_of_channel(channel)
            .and_then(|guild| guild.channels.iter().find(|known| known.id == channel))
            .map(|known| known.kind);
        if !kind.is_some_and(shows_notice) {
            return;
        }
        if let Some((guild, flags)) =
            self.unread_notice
                .store
                .maybe_auto_upgrade(&self.stores, channel, now)
        {
            self.unread_notice.shown = Some(channel);
            self.unread_notice.save();
            self.patch_channel_override(guild, channel, serde_json::json!({ "flags": flags }));
        }
    }

    /// `setInterval(M, 15s)` while the new notification settings are on.
    fn start_unread_notice_clock(&mut self, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(TICK_MS))
                    .await;
                let alive = this.update(cx, |this, cx| {
                    if this.stores.notification_flags & 16 == 0 {
                        this.unread_notice.clock = false;
                        return false;
                    }
                    if this.unread_notice.store.tick(&this.stores, now_ms()) {
                        this.unread_notice.save();
                        cx.notify();
                    }
                    true
                });
                if !matches!(alive, Ok(true)) {
                    break;
                }
            }
        })
        .detach();
    }

    /// The gateway events the store listens to.
    pub(super) fn unread_notice_event(&mut self, name: &str, data: &serde_json::Value) {
        let now = now_ms();
        match name {
            "READY" => {
                self.unread_notice.store.connection_open(self.channel, now);
                self.unread_notice.save();
            }
            "MESSAGE_CREATE" => {
                let me = self.stores.current_user.as_ref().map(|user| user.id);
                let author = data["author"]["id"]
                    .as_str()
                    .and_then(|id| id.parse::<Id>().ok());
                let channel = data["channel_id"].as_str().and_then(|id| id.parse().ok());
                if let (Some(me), Some(author), Some(channel)) = (me, author, channel)
                    && me == author
                    && self
                        .unread_notice
                        .store
                        .message_sent(&self.stores, channel, now)
                {
                    self.unread_notice.save();
                }
            }
            _ => {}
        }
    }

    /// Undo: the channel's unreads go back to mentions only.
    fn unread_notice_undo(&mut self, channel: Id) {
        let Some(guild) = self.stores.guild_of_channel(channel).map(|guild| guild.id) else {
            return;
        };
        let flags = with_unread_setting(
            channel_flags(&self.stores, guild, channel),
            UNREADS_ONLY_MENTIONS,
        );
        self.patch_channel_override(guild, channel, serde_json::json!({ "flags": flags }));
        self.unread_notice.shown = None;
    }

    /// The bar (`.notice_ipBn6f`: `--background-mod-normal`, 8px 16px, the text `body-sm/medium` filling the row
    /// before a `sm` secondary Undo button with UndoIcon).
    pub(super) fn unread_settings_notice(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let channel = self
            .channel
            .filter(|id| self.unread_notice.shown == Some(*id))?;
        // `P6Y2N1`: the sentence, then its `$link` run.
        let runs = dc_intl::format_runs("P6Y2N1", &[]);
        let before: String = runs
            .iter()
            .take_while(|(_, tag)| tag.is_none())
            .map(|(text, _)| text.as_str())
            .collect();
        let link: String = runs
            .iter()
            .filter(|(_, tag)| tag.is_some())
            .map(|(text, _)| text.as_str())
            .collect();
        let text = self.text_with_link(
            "unread-notice-text",
            &before,
            &link,
            "",
            cx,
            |this, cx| this.follow_link(LEARN_MORE, "Learn More", cx),
        );
        // `sm` Button: 30px inside a 1px border, 3px by 11px, the 16px icon 4px before the label.
        let undo = div()
            .id("unread-notice-undo")
            .flex_none()
            .h(px(32.))
            .px(px(11.))
            .rounded(px(8.))
            .border_1()
            .border_color(color("--control-secondary-border-default"))
            .bg(color("--control-secondary-background-default"))
            .hover(|style| style.bg(color("--control-secondary-background-hover")))
            .flex()
            .items_center()
            .gap(px(4.))
            .cursor_pointer()
            .text_size(px(14.))
            .line_height(px(18.))
            .font_weight(FontWeight(500.))
            .text_color(color("--control-secondary-text-default"))
            .child(icon("undo", 16., "--control-secondary-text-default"))
            .child(dc_intl::t("KyUKhT").to_owned())
            .on_click(cx.listener(move |this, _, _, cx| {
                this.unread_notice_undo(channel);
                cx.notify();
            }));
        Some(
            div()
                .flex_none()
                .px(px(16.))
                .py(px(8.))
                .bg(color("--background-mod-normal"))
                .flex()
                .items_center()
                .justify_between()
                .child(
                    div()
                        .flex_1()
                        .text_size(px(14.))
                        .line_height(px(18.))
                        .font_weight(FontWeight(500.))
                        .text_color(color("--text-default"))
                        .child(text),
                )
                .child(undo)
                .into_any_element(),
        )
    }

    /// Capture runs, in memory only: `unread-notice young` stamps the open server's membership as of now (what
    /// GUILD_MEMBER_UPDATE carries), `unread-notice sent` feeds the store the account's own MESSAGE_CREATE in the open
    /// channel (an inert run sends nothing, so no echo comes), `unread-notice state` reports.
    #[cfg(feature = "capture")]
    pub(crate) fn script_unread_notice(&mut self, args: &[&str]) -> String {
        match args {
            ["young"] => {
                let now = chrono::Utc::now().to_rfc3339();
                if let Some(guild) = self.guild
                    && let Some(known) = self
                        .stores
                        .guilds
                        .iter_mut()
                        .find(|known| known.id == guild)
                {
                    known.joined_at = now;
                }
                format!("unread notice young guild={}", self.guild.is_some())
            }
            ["sent"] => {
                let me = self
                    .stores
                    .current_user
                    .as_ref()
                    .map(|user| user.id)
                    .unwrap_or(0);
                let channel = self.channel.unwrap_or(0);
                let data = serde_json::json!({
                    "author": {"id": me.to_string()}, "channel_id": channel.to_string(),
                });
                self.unread_notice_event("MESSAGE_CREATE", &data);
                format!(
                    "unread notice sent eligible={}",
                    self.unread_notice
                        .store
                        .eligible(&self.stores, channel, now_ms())
                )
            }
            _ => format!(
                "unread notice shown={:?} records={}",
                self.unread_notice.shown,
                self.unread_notice.store.channels.len()
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_notice_is_for_text_categories_forums_announcements_and_apps() {
        use dc_state::ChannelKind;
        assert!(shows_notice(ChannelKind::Text));
        assert!(shows_notice(ChannelKind::Other(5)));
        assert!(shows_notice(ChannelKind::Other(15)));
        assert!(shows_notice(ChannelKind::Other(21)));
        assert!(!shows_notice(ChannelKind::Voice));
        assert!(!shows_notice(ChannelKind::Other(11)));
        assert!(!shows_notice(ChannelKind::Other(16)));
    }
}
