//! The notice bar under the title bar (`notice_Q02kKo`, stylesheet 685143; the component is module 417098, the notices
//! and their order are the table of module 596339).
//!
//! The official notice store shows the first notice type, in the table's order, whose predicate holds and that was
//! not dismissed. This client has two of those notices so far:
//!
//! - `VOICE_DISABLED`, "Your voice has been disconnected because you connected at another location.", whose predicate
//!   is `getRemoteDisconnectVoiceChannelId() != null` (`RTCConnectionStore`, module 495867). Its bar is `WARNING`, its
//!   button is Reconnect (`vD60Pv`), which selects that voice channel again, and its close button clears the channel
//!   (`clearRemoteDisconnectVoiceChannelId`).
//! - `BLOCK_USER_FEEDBACK_NAGBAR`, "How is blocking users on Discord working for you?", whose predicate is `Cm` of
//!   module 813393 (someone the account blocked between a week and 30 days ago) and that shows in one session of ten
//!   (`sampleRate: .1`). Its bar is `BRAND`; "Give feedback" opens the block survey (module 307750) and, like the
//!   close button, dismisses it for 180 days (`hideNagbarBlockUserFeedbackNotice-untilAtLeast`).

use super::{Root, color, icon};
use dc_state::Id;
use gpui::{AnyElement, Context, FontWeight, IntoElement, div, prelude::*, px};
use std::cell::Cell;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

/// `cF[BLOCK_USER_FEEDBACK_NAGBAR]` of module 596339: the local storage key of its dismissal.
const BLOCK_FEEDBACK_KEY: &str = "hideNagbarBlockUserFeedbackNotice";
const WEEK_MS: i64 = 7 * 86_400_000;
const DAYS_30_MS: i64 = 30 * 86_400_000;
/// `dismissUntil`: 180 days on.
const BLOCK_FEEDBACK_DISMISS_DAYS: i64 = 180;
/// How long the predicate's answer is kept before the relationships are read again (the bar is drawn every frame).
const PREDICATE_TTL: Duration = Duration::from_secs(30);

thread_local! {
    static BLOCK_PREDICATE: Cell<Option<(Instant, bool)>> = const { Cell::new(None) };
}

/// `cH[type] = Math.random() <= sampleRate`, once per session.
fn block_feedback_sampled() -> bool {
    static SAMPLED: OnceLock<bool> = OnceLock::new();
    #[cfg(feature = "capture")]
    if SAMPLE_FORCED.get().is_some() {
        return true;
    }
    *SAMPLED.get_or_init(|| rand::random::<f64>() <= 0.1)
}

/// Capture only: the harness put this session in the sample.
#[cfg(feature = "capture")]
static SAMPLE_FORCED: OnceLock<()> = OnceLock::new();

/// `Cm` of module 813393: a blocked person whose relationship began more than a week and less than 30 days ago.
pub(super) fn recently_blocked<'a>(
    blocked_since: impl IntoIterator<Item = &'a str>,
    now_ms: i64,
) -> bool {
    blocked_since.into_iter().any(|since| {
        chrono::DateTime::parse_from_rfc3339(since).is_ok_and(|since| {
            let age = now_ms - since.timestamp_millis();
            age > WEEK_MS && age < DAYS_30_MS
        })
    })
}

/// `cz` for a notice with an `untilAtLeast` date: dismissed while that date is ahead.
fn block_feedback_dismissed(now: chrono::DateTime<chrono::Utc>) -> bool {
    crate::storage::get(&format!("{BLOCK_FEEDBACK_KEY}-untilAtLeast"))
        .and_then(|until| {
            until
                .as_str()
                .and_then(|text| chrono::DateTime::parse_from_rfc3339(text).ok())
        })
        .is_some_and(|until| until > now)
}

/// The notice types this client shows, in the order of the table in module 596339 (`H.kqX`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Notice {
    /// `VOICE_DISABLED`: the voice call was taken over by another session of this account; (guild, channel).
    VoiceDisabled(Id, Id),
    /// `BLOCK_USER_FEEDBACK_NAGBAR`.
    BlockFeedback,
}

/// `getRemoteDisconnectVoiceChannelId` and the notices it drives.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct State {
    /// The voice channel (and its guild, 0 for a DM) that another session took the call from.
    pub remote_disconnect: Option<(Id, Id)>,
}

impl State {
    /// The notice shown now: the first whose predicate holds.
    pub(super) fn shown(&self) -> Option<Notice> {
        self.remote_disconnect
            .map(|(guild, channel)| Notice::VoiceDisabled(guild, channel))
    }

    /// `VOICE_STATE_UPDATES` with another session of this account took the call: `S = i.channelId`.
    pub(super) fn voice_taken(&mut self, guild: Id, channel: Id) {
        self.remote_disconnect = Some((guild, channel));
    }

    /// A new connection of this session (`S = null` in module 495867), `CLEAR_REMOTE_DISCONNECT_VOICE_CHANNEL_ID` or
    /// `CONNECTION_OPEN`.
    pub(super) fn clear(&mut self) {
        self.remote_disconnect = None;
    }
}

impl Root {
    /// The notice shown now: the voice one first (it is earlier in the table), then the block survey's.
    pub(super) fn shown_notice(&self) -> Option<Notice> {
        if let Some(notice) = self.vv.notices.shown() {
            return Some(notice);
        }
        self.block_feedback_holds().then_some(Notice::BlockFeedback)
    }

    /// `BLOCK_USER_FEEDBACK_NAGBAR`'s predicate, not dismissed, in a sampled session.
    fn block_feedback_holds(&self) -> bool {
        let now = Instant::now();
        let cached = BLOCK_PREDICATE.with(|cell| {
            cell.get()
                .filter(|(at, _)| now.duration_since(*at) < PREDICATE_TTL)
        });
        let holds = match cached {
            Some((_, holds)) => holds,
            None => {
                let blocked = self
                    .stores
                    .relationships
                    .iter()
                    .filter(|(_, kind)| **kind == 2)
                    .filter_map(|(user, _)| self.stores.relationship_since.get(user))
                    .map(String::as_str);
                let holds = recently_blocked(blocked, chrono::Utc::now().timestamp_millis());
                BLOCK_PREDICATE.with(|cell| cell.set(Some((now, holds))));
                holds
            }
        };
        holds && !block_feedback_dismissed(chrono::Utc::now()) && block_feedback_sampled()
    }

    /// `oT.w(dismissUntil)`: `NOTICE_DISMISS` with `untilAtLeast` 180 days on.
    fn dismiss_block_feedback(&mut self) {
        let until = chrono::Utc::now() + chrono::Duration::days(BLOCK_FEEDBACK_DISMISS_DAYS);
        crate::storage::set(BLOCK_FEEDBACK_KEY, serde_json::json!(true));
        crate::storage::set(
            &format!("{BLOCK_FEEDBACK_KEY}-untilAtLeast"),
            serde_json::json!(until.to_rfc3339_opts(chrono::SecondsFormat::Millis, true)),
        );
    }

    /// The bar, when a notice holds.
    pub(super) fn notice_bar(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        // `VIEWING_ROLES` comes before `VOICE_DISABLED` in the table (`ui/view_as_role.rs`).
        if let Some(bar) = self.view_as_role_bar(cx) {
            return Some(bar);
        }
        let (guild, channel) = match self.shown_notice()? {
            Notice::VoiceDisabled(guild, channel) => (guild, channel),
            Notice::BlockFeedback => return Some(self.block_feedback_bar(cx)),
        };
        let text = color("--notice-text-warning");
        Some(
            div()
                .id("app-notice")
                .relative()
                .flex_none()
                .w_full()
                .h(px(36.))
                // `padding-inline: 4px 28px`, centred text on a 36px line.
                .pl(px(4.))
                .pr(px(28.))
                .flex()
                .items_center()
                .justify_center()
                .bg(color("--notice-background-warning"))
                .border_t_1()
                .border_l_1()
                .border_color(color("--app-frame-border"))
                .rounded_tl(px(8.))
                .text_size(px(14.))
                .line_height(px(36.))
                .font_weight(FontWeight(500.))
                .text_color(text)
                .child(dc_intl::tr(
                    "Your voice has been disconnected because you connected at another location.",
                ))
                // `button_Q02kKo`: 24px, a 1px border in the notice's text colour, 3px corners, 10px from the text.
                .child(
                    div()
                        .id("app-notice-reconnect")
                        .ml(px(10.))
                        .h(px(24.))
                        .px(px(10.))
                        .flex()
                        .items_center()
                        .border_1()
                        .border_color(text)
                        .rounded(px(3.))
                        .line_height(px(22.))
                        .cursor_pointer()
                        .hover(|style| style.bg(color("--background-mod-subtle")))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.notice_reconnect(guild, channel, cx);
                        }))
                        .child(dc_intl::tr("Reconnect")),
                )
                // `closeButton_Q02kKo`: 36px square at the right, the small X (`XSmallIcon`, size sm).
                .child(
                    div()
                        .id("app-notice-close")
                        .absolute()
                        .top(px(0.))
                        .right(px(0.))
                        .size(px(36.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .cursor_pointer()
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.vv.notices.clear();
                            cx.notify();
                        }))
                        .child(icon("mod_close", 18., "--notice-text-warning")),
                )
                .into_any_element(),
        )
    }

    /// `colorBrand_Q02kKo`: `--brand-500` behind white text, the button white on hover with brand text.
    fn block_feedback_bar(&self, cx: &mut Context<Self>) -> AnyElement {
        div()
            .id("app-notice")
            .relative()
            .flex_none()
            .w_full()
            .h(px(36.))
            .pl(px(4.))
            .pr(px(28.))
            .flex()
            .items_center()
            .justify_center()
            .bg(color("--brand-500"))
            .border_t_1()
            .border_l_1()
            .border_color(color("--app-frame-border"))
            .rounded_tl(px(8.))
            .text_size(px(14.))
            .line_height(px(36.))
            .font_weight(FontWeight(500.))
            .text_color(color("--white"))
            .child(dc_intl::t("0klLS7"))
            .child(
                div()
                    .id("app-notice-block-feedback")
                    .ml(px(10.))
                    .h(px(24.))
                    .px(px(10.))
                    .flex()
                    .items_center()
                    .border_1()
                    .border_color(color("--white"))
                    .rounded(px(3.))
                    .line_height(px(22.))
                    .cursor_pointer()
                    .hover(|style| style.bg(color("--white")).text_color(color("--brand-500")))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.open_feedback(super::feedback::Kind::Block, cx);
                        this.dismiss_block_feedback();
                        cx.notify();
                    }))
                    .child(dc_intl::t("e4y2VM")),
            )
            .child(
                div()
                    .id("app-notice-close")
                    .absolute()
                    .top(px(0.))
                    .right(px(0.))
                    .size(px(36.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .cursor_pointer()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.dismiss_block_feedback();
                        cx.notify();
                    }))
                    .child(icon("mod_close", 18., "--white")),
            )
            .into_any_element()
    }

    /// Capture only: `notice block-feedback`, the block survey's bar as if its predicate held in a sampled session
    /// (the predicate's cache is set and a stored dismissal is cleared).
    #[cfg(feature = "capture")]
    pub(crate) fn script_block_feedback_notice(&mut self) -> String {
        BLOCK_PREDICATE.with(|cell| cell.set(Some((Instant::now(), true))));
        let _ = SAMPLE_FORCED.set(());
        // A dismissal an earlier capture stored (the capture storage, not the account's) would hide it.
        crate::storage::remove(BLOCK_FEEDBACK_KEY);
        crate::storage::remove(&format!("{BLOCK_FEEDBACK_KEY}-untilAtLeast"));
        format!("block feedback notice: {:?}", self.shown_notice())
    }

    /// Reconnect: `selectVoiceChannel(remoteDisconnectVoiceChannelId)` when the channel is still known. Joining sends
    /// this session's voice state, which takes the call back from the other device.
    pub(super) fn notice_reconnect(&mut self, guild: Id, channel: Id, cx: &mut Context<Self>) {
        let known = if guild == 0 {
            self.stores
                .private_channels
                .iter()
                .any(|dm| dm.id == channel)
        } else {
            self.stores
                .guild(guild)
                .is_some_and(|known| known.channels.iter().any(|found| found.id == channel))
        };
        if known {
            self.join_voice(guild, channel, cx);
        } else {
            self.vv.notices.clear();
        }
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_block_survey_asks_about_blocks_between_a_week_and_a_month_old() {
        let now = chrono::DateTime::parse_from_rfc3339("2026-11-18T12:00:00Z")
            .unwrap()
            .timestamp_millis();
        assert!(!recently_blocked(["2026-11-15T12:00:00+00:00"], now));
        assert!(recently_blocked(["2026-11-01T12:00:00+00:00"], now));
        assert!(!recently_blocked(["2026-09-01T12:00:00+00:00"], now));
        assert!(!recently_blocked(["not a date"], now));
        assert!(!recently_blocked(std::iter::empty(), now));
    }

    #[test]
    fn the_notice_holds_from_the_takeover_until_cleared() {
        let mut notices = State::default();
        assert_eq!(notices.shown(), None);
        notices.voice_taken(0, 9);
        assert_eq!(notices.shown(), Some(Notice::VoiceDisabled(0, 9)));
        notices.clear();
        assert_eq!(notices.shown(), None);
    }
}
