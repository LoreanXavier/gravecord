//! The channel notice under a server's header (module 260269 `ie` / `n5`): for now the invite notice, "An adventure
//! begins. / Let's add some friends!" with its illustration and an "Invite to Server" button.
//!
//! Candidates (`n6`): the boosting and banner notices are never offered (`n8`); the invite notice is offered to an
//! administrator once a day has passed since a notice was last hidden (`n0.channelNoticePredicate`,
//! `lastHiddenChannelNotice`, `D2K` = one day). The official client then asks the service which candidate of the
//! `CHANNEL_NOTICES` group to show (`POST /dismissible-content/arbitrate`); this client shows the first candidate the
//! account has not dismissed (`dismissed_contents`), which is the same answer unless another group's content holds
//! the slot. Hiding it (`nw.handleClose`) records the time, marks the content dismissed and has Clyde say where the
//! invite is (`DEn7nu`, `sendBotMessage`). Not here yet: the hub link, quick switcher and game claim notices, and the
//! guild-level notices of `n4` (public upsell, member cap, live channel, MFA warning, commands migration).
use super::{Root, color};
use dc_state::{Id, permissions};
use gpui::{AnyElement, Context, FontWeight, IntoElement, div, img, prelude::*, px};

/// `CHANNEL_NOTICE_INVITE` of module 554146.
const INVITE_CONTENT: u16 = 6;
/// `n1`: when a channel notice was last hidden, in milliseconds since the epoch.
const LAST_HIDDEN: &str = "lastHiddenChannelNotice";
/// `D2K` of module 652215.
const HIDE_FOR_MS: i64 = 86_400_000;
/// `nP`: the invite illustration, 126 by 84.
const ILLUSTRATION: &str = "https://cdn.discordapp.com/assets/content/dc82534138c63ce764672d1e10188ceb29febbcf73dad31c313f8a4b41dcff96.svg";

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

fn last_hidden() -> i64 {
    crate::storage::get(LAST_HIDDEN)
        .and_then(|value| value.as_i64())
        .unwrap_or(0)
}

/// `n0.channelNoticePredicate`.
fn invite_offered(admin: bool, now: i64, hidden_at: i64) -> bool {
    admin && now - hidden_at >= HIDE_FOR_MS
}

impl Root {
    fn channel_notice_guild(&self) -> Option<Id> {
        let guild = self.guild?;
        let known = self.stores.guild(guild)?;
        let me = self.stores.me()?;
        let admin = permissions::can_in_guild(known, me, permissions::ADMINISTRATOR);
        (invite_offered(admin, now_ms(), last_hidden())
            && !self.stores.content_dismissed(INVITE_CONTENT))
        .then_some(guild)
    }

    /// `markAsDismissed` of `n5` then `nw.handleClose`.
    fn hide_channel_notice(&mut self, guild: Id, cx: &mut Context<Self>) {
        crate::storage::set(LAST_HIDDEN, serde_json::json!(now_ms()));
        self.dismiss_profile_content(INVITE_CONTENT);
        if let Some(channel) = self.channel.filter(|_| self.guild == Some(guild)) {
            let text = "Hello. Beep. Boop. If you wanna invite friends to this server, click the server name in the \
                        top left and select \"Invite to Server\". Beep!";
            self.clyde_says(channel, text, "channel_notice_invite");
        }
        cx.notify();
    }

    /// The illustration, asked for when a server is opened.
    pub(super) fn channel_notice_wanted(&mut self) {
        if self.channel_notice_guild().is_some() && !self.images.contains_key(ILLUSTRATION) {
            self.request_image(ILLUSTRATION.to_owned(), ILLUSTRATION.to_owned());
        }
    }

    /// `tO` with `nw`'s content (`channelNotice_jOZ0iO`; sizes from the fixture capture
    /// `design/dom/channel/edited-tooltip-darker-cozy.json`).
    pub(super) fn channel_notice(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let guild = self.channel_notice_guild()?;
        let picture = self.images.get(ILLUSTRATION).cloned();
        let close = div()
            .id("channel-notice-close")
            .absolute()
            .top(px(12.))
            .right(px(16.))
            .size(px(18.))
            .cursor_pointer()
            .text_color(color("--interactive-text-default"))
            .hover(|style| style.text_color(color("--interactive-text-hover")))
            .on_click(cx.listener(move |this, _, _, cx| {
                cx.stop_propagation();
                this.hide_channel_notice(guild, cx);
            }))
            .child(
                gpui::svg()
                    .path("icons/x_small.svg")
                    .size(px(18.))
                    .text_color(color("--interactive-text-default")),
            );
        let illustration = div().mx(px(46.)).flex().justify_center().child(
            div()
                .w(px(126.))
                .h(px(87.))
                .children(picture.map(|picture| img(picture).w(px(126.)).h(px(84.)))),
        );
        let button = div()
            .id("channel-notice-invite")
            .mt(px(16.))
            .h(px(32.))
            .w_full()
            .rounded(px(8.))
            .border_1()
            .border_color(gpui::hsla(0., 0., 1., 0.08))
            .bg(color("--control-primary-background-default"))
            .hover(|style| style.bg(color("--control-primary-background-hover")))
            .cursor_pointer()
            .flex()
            .items_center()
            .justify_center()
            .text_size(px(14.))
            .line_height(px(18.))
            .font_weight(FontWeight(500.))
            .text_color(gpui::white())
            .on_click(cx.listener(|this, _, _, cx| this.open_invite(cx)))
            .child(dc_intl::tr("Invite to Server"));
        Some(
            div()
                .id("channel-notice")
                .relative()
                .flex_none()
                .p(px(16.))
                .border_b_1()
                .border_color(color("--border-subtle"))
                .flex()
                .flex_col()
                .child(close)
                .child(illustration)
                .child(
                    div()
                        .mt(px(16.))
                        .text_center()
                        .text_size(px(14.))
                        .line_height(px(18.))
                        .text_color(color("--text-default"))
                        .child(div().child(dc_intl::tr("An adventure begins.")))
                        .child(div().child(dc_intl::tr("Let's add some friends!")))
                        .child(button),
                )
                .into_any_element(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_invite_notice_waits_a_day_after_a_notice_was_hidden_and_is_for_administrators() {
        let day = HIDE_FOR_MS;
        assert!(invite_offered(true, 10 * day, 0));
        assert!(!invite_offered(false, 10 * day, 0));
        assert!(!invite_offered(true, 10 * day, 10 * day - 1));
        assert!(invite_offered(true, 10 * day, 9 * day));
    }
}
