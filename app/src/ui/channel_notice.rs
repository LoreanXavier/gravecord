//! The channel notice under a server's header (module 260269 `ie` / `n5`, drawn by `tO`): the invite notice, "An
//! adventure begins. / Let's add some friends!" with its illustration and an "Invite to Server" button, and the quick
//! switcher notice, "Use Quick Switcher to get around Discord quickly. Just press:" over a button holding the
//! shortcut's keys.
//!
//! Candidates (`n6`): the boosting and banner notices are never offered (`n8`); the invite notice is offered to an
//! administrator and the quick switcher notice to an account in three servers or twenty direct messages (`x` of module
//! 174768), each once a day has passed since a notice was last hidden (`channelNoticePredicate`,
//! `lastHiddenChannelNotice`, `D2K` = one day). The official client then asks the service which candidate of the
//! `CHANNEL_NOTICES` group to show (`POST /dismissible-content/arbitrate`); this client shows the first candidate the
//! account has not dismissed (`dismissed_contents`), which is the same answer unless another group's content holds
//! the slot. Hiding one records the time and marks the content dismissed; the invite notice also has Clyde say where
//! the invite is (`DEn7nu`, `sendBotMessage`). Not here yet: the hub link and game claim notices, and the guild-level
//! notices of `n4` (public upsell, member cap, live channel, MFA warning, commands migration).
use super::{Root, color};
use dc_state::{Id, permissions};
use gpui::{AnyElement, Context, FontWeight, IntoElement, div, img, prelude::*, px};

/// `CHANNEL_NOTICE_INVITE` and `CHANNEL_NOTICE_QUICKSWITCHER` of module 554146.
const INVITE_CONTENT: u16 = 6;
const QUICK_SWITCHER_CONTENT: u16 = 8;
/// `n1`: when a channel notice was last hidden, in milliseconds since the epoch.
const LAST_HIDDEN: &str = "lastHiddenChannelNotice";
/// `D2K` of module 652215.
const HIDE_FOR_MS: i64 = 86_400_000;
/// `nP`: the invite illustration, 126 by 84.
const ILLUSTRATION: &str = "https://cdn.discordapp.com/assets/content/dc82534138c63ce764672d1e10188ceb29febbcf73dad31c313f8a4b41dcff96.svg";
/// `nF`'s picture (101 by 55), as wide as the 271px notice leaves past 50px on each side.
const QUICK_SWITCHER_PICTURE: &str = "https://discord.com/assets/1b763dab67be227b.svg";
const QUICK_SWITCHER_WIDTH: f32 = 271. - 100.;

/// Capture only: a run that has to see the notices after the invite one treats the invite notice as dismissed.
#[cfg(feature = "capture")]
pub(crate) static SKIP_INVITE: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

fn last_hidden() -> i64 {
    crate::storage::get(LAST_HIDDEN)
        .and_then(|value| value.as_i64())
        .unwrap_or(0)
}

fn skip_invite() -> bool {
    #[cfg(feature = "capture")]
    return SKIP_INVITE.load(std::sync::atomic::Ordering::Relaxed);
    #[cfg(not(feature = "capture"))]
    false
}

/// `n0.channelNoticePredicate`.
fn invite_offered(admin: bool, now: i64, hidden_at: i64) -> bool {
    admin && now - hidden_at >= HIDE_FOR_MS
}

/// `nW.channelNoticePredicate`: `x` (three servers or twenty direct messages) and a day since a notice was hidden.
fn quick_switcher_offered(guilds: usize, dms: usize, now: i64, hidden_at: i64) -> bool {
    (guilds >= 3 || dms >= 20) && now - hidden_at >= HIDE_FOR_MS
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Notice {
    Invite,
    QuickSwitcher,
}

impl Notice {
    fn content(self) -> u16 {
        match self {
            Notice::Invite => INVITE_CONTENT,
            Notice::QuickSwitcher => QUICK_SWITCHER_CONTENT,
        }
    }

    fn picture(self) -> &'static str {
        match self {
            Notice::Invite => ILLUSTRATION,
            Notice::QuickSwitcher => QUICK_SWITCHER_PICTURE,
        }
    }
}

impl Root {
    /// The first candidate in `n6`'s order that is offered and not dismissed.
    fn channel_notice_guild(&self) -> Option<(Id, Notice)> {
        let guild = self.guild?;
        let known = self.stores.guild(guild)?;
        let me = self.stores.me()?;
        let admin = permissions::can_in_guild(known, me, permissions::ADMINISTRATOR);
        let (now, hidden) = (now_ms(), last_hidden());
        let offered = [
            (
                Notice::Invite,
                invite_offered(admin, now, hidden) && !skip_invite(),
            ),
            (
                Notice::QuickSwitcher,
                quick_switcher_offered(
                    self.stores.guilds.len(),
                    self.stores.private_channels.len(),
                    now,
                    hidden,
                ),
            ),
        ];
        offered
            .into_iter()
            .find(|(notice, on)| *on && !self.stores.content_dismissed(notice.content()))
            .map(|(notice, _)| (guild, notice))
    }

    /// `markAsDismissed` of `n5`, and for the invite notice `nw.handleClose`.
    fn hide_channel_notice(&mut self, guild: Id, notice: Notice, cx: &mut Context<Self>) {
        crate::storage::set(LAST_HIDDEN, serde_json::json!(now_ms()));
        self.dismiss_profile_content(notice.content());
        if notice == Notice::Invite
            && let Some(channel) = self.channel.filter(|_| self.guild == Some(guild))
        {
            let text = "Hello. Beep. Boop. If you wanna invite friends to this server, click the server name in the \
                        top left and select \"Invite to Server\". Beep!";
            self.clyde_says(channel, text, "channel_notice_invite");
        }
        cx.notify();
    }

    /// The illustration of the notice shown now, if one is: a picture of the first screen (`start_cache`).
    pub(super) fn channel_notice_picture(&self) -> Option<&'static str> {
        self.channel_notice_guild()
            .map(|(_, notice)| notice.picture())
    }

    /// The illustration, asked for when a server is opened.
    pub(super) fn channel_notice_wanted(&mut self) {
        if let Some((_, notice)) = self.channel_notice_guild()
            && !self.images.contains_key(notice.picture())
        {
            let picture = notice.picture().to_owned();
            self.request_image(picture.clone(), picture);
        }
    }

    /// `nX.e` with `key_jOZ0iO`: `mod+k` as key caps, upper case 12px semibold, 3px apart.
    fn quick_switcher_keys() -> gpui::Div {
        let key = |text: &'static str| {
            div()
                .relative()
                .h(px(23.))
                .min_w(px(14.))
                .pt(px(3.))
                .pb(px(4.))
                .px(px(6.))
                .rounded(px(4.))
                .border_1()
                .border_color(color("--border-subtle"))
                .bg(color("--control-overlay-primary-background-default"))
                .text_color(color("--control-overlay-primary-text-default"))
                .text_size(px(12.))
                .line_height(px(12.))
                .font_weight(FontWeight(600.))
                .child(text)
                // `box-shadow: inset 0 -4px 0 var(--background-mod-muted)`.
                .child(
                    div()
                        .absolute()
                        .left_0()
                        .right_0()
                        .bottom_0()
                        .h(px(4.))
                        .bg(color("--background-mod-muted")),
                )
        };
        div().flex().gap(px(3.)).child(key("CTRL")).child(key("K"))
    }

    /// `tO` with `nw`'s or `nF`'s content (`channelNotice_jOZ0iO`; sizes from the fixture capture
    /// `design/dom/channel/edited-tooltip-darker-cozy.json`).
    pub(super) fn channel_notice(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let (guild, notice) = self.channel_notice_guild()?;
        let picture = self.images.get(notice.picture()).cloned();
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
                this.hide_channel_notice(guild, notice, cx);
            }))
            .child(
                gpui::svg()
                    .path("icons/x_small.svg")
                    .size(px(18.))
                    .text_color(color("--interactive-text-default")),
            );
        let illustration = match notice {
            Notice::Invite => div().mx(px(46.)).flex().justify_center().child(
                div()
                    .w(px(126.))
                    .h(px(87.))
                    .children(picture.map(|picture| img(picture).w(px(126.)).h(px(84.)))),
            ),
            Notice::QuickSwitcher => {
                let height = QUICK_SWITCHER_WIDTH * 55. / 101.;
                div().mx(px(50.)).h(px(height)).children(
                    picture.map(|picture| img(picture).w(px(QUICK_SWITCHER_WIDTH)).h(px(height))),
                )
            }
        };
        let button = div()
            .id("channel-notice-button")
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
            .text_color(gpui::white());
        let (button, message) = match notice {
            Notice::Invite => (
                button
                    .on_click(cx.listener(|this, _, _, cx| this.open_invite(cx)))
                    .child(dc_intl::tr("Invite to Server")),
                vec!["An adventure begins.", "Let's add some friends!"],
            ),
            // `WU("CHANNEL_NOTICE")`: the button opens the quick switcher; the notice stays.
            Notice::QuickSwitcher => (
                button
                    .on_click(
                        cx.listener(|this, _, window, cx| this.open_quick_switcher(window, cx)),
                    )
                    .child(Self::quick_switcher_keys()),
                vec!["Use Quick Switcher to get around Discord quickly. Just press:"],
            ),
        };
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
                        .children(
                            message
                                .into_iter()
                                .map(|line| div().child(dc_intl::tr(line))),
                        )
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

    #[test]
    fn the_quick_switcher_notice_is_for_three_servers_or_twenty_direct_messages() {
        let day = HIDE_FOR_MS;
        assert!(quick_switcher_offered(3, 0, 10 * day, 0));
        assert!(quick_switcher_offered(1, 20, 10 * day, 0));
        assert!(!quick_switcher_offered(2, 19, 10 * day, 0));
        assert!(!quick_switcher_offered(3, 0, 10 * day, 10 * day - 1));
    }
}
