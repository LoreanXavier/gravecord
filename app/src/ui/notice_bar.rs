//! The notice bar under the title bar (`notice_Q02kKo`, stylesheet 685143; the component is module 417098, the notices
//! and their order are the table of module 596339).
//!
//! The official notice store shows the first notice type, in the table's order, whose predicate holds and that was
//! not dismissed. This client has one of those notices so far: `VOICE_DISABLED`, "Your voice has been disconnected
//! because you connected at another location.", whose predicate is `getRemoteDisconnectVoiceChannelId() != null`
//! (`RTCConnectionStore`, module 495867). Its bar is `WARNING`, its button is Reconnect (`vD60Pv`), which selects that
//! voice channel again, and its close button clears the channel (`clearRemoteDisconnectVoiceChannelId`).

use super::{Root, color, icon};
use dc_state::Id;
use gpui::{AnyElement, Context, FontWeight, IntoElement, div, prelude::*, px};

/// The notice types this client shows, in the order of the table in module 596339 (`H.kqX`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Notice {
    /// `VOICE_DISABLED`: the voice call was taken over by another session of this account; (guild, channel).
    VoiceDisabled(Id, Id),
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
    /// The bar, when a notice holds.
    pub(super) fn notice_bar(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let Notice::VoiceDisabled(guild, channel) = self.vv.notices.shown()?;
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
    fn the_notice_holds_from_the_takeover_until_cleared() {
        let mut notices = State::default();
        assert_eq!(notices.shown(), None);
        notices.voice_taken(0, 9);
        assert_eq!(notices.shown(), Some(Notice::VoiceDisabled(0, 9)));
        notices.clear();
        assert_eq!(notices.shown(), None);
    }
}
