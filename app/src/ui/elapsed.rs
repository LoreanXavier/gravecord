//! The live elapsed time of a call (module 693879 `z`): `body-xs/normal` 12px / 16px in the code font
//! (`fontCode_1a6Bd3`, `--font-code`) with tabular numbers, a `role="timer"`, and the text of module 583846 `W6`:
//! `m:ss`, or `h:mm:ss` from an hour on. The official component re-renders itself every second (`useNow`); here a
//! drawn timer marks the view it is drawn in (`regions::note_drawn`) and one wake at the next whole second redraws
//! that view and nothing else, scheduled again only while some timer is still drawn.

use super::{MONO, Root, color};
use gpui::{Context, Div, ParentElement, Styled, div, px};
use std::cell::Cell;
use std::time::Duration;

thread_local! {
    /// When the pending wake fires (unix milliseconds), so a frame schedules at most one.
    static NEXT: Cell<Option<u64>> = const { Cell::new(None) };
}

pub(super) fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_millis() as u64)
}

/// Module 583846 `W6` of `{start}` at `now` (both unix milliseconds): a start in the future reads as no time.
pub(super) fn text(start_ms: u64, now_ms: u64) -> String {
    let seconds = now_ms.saturating_sub(start_ms) / 1000;
    let (hours, minutes, seconds) = (seconds / 3600, seconds / 60 % 60, seconds % 60);
    if hours > 0 {
        format!("{hours}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes}:{seconds:02}")
    }
}

/// A snowflake's creation time (`extractTimestamp`), in unix milliseconds.
pub(super) fn snowflake_ms(id: u64) -> u64 {
    (id >> 22) + 1_420_070_400_000
}

/// `2026-10-gdm-duration-timer` (module 181254 `ex`, location `PrivateChannelCallDuration`): variation 1 shows the
/// call time in the DM list; the default is off.
const DM_DURATION_EXPERIMENT: &str = "2026-10-gdm-duration-timer";

impl Root {
    /// Module 181254 `eI`/`e0`: the start of a DM or group DM call shown in its list row: this client's voice
    /// connection is the channel's call (`RTCConnectionStore.getVoiceChannelId() === id`), the call is active
    /// (`CallStore.isCallActive(id)`), and the time counts from the call message's snowflake, not from joining.
    pub(super) fn private_call_start(&self, channel: dc_state::Id) -> Option<u64> {
        if self.stores.experiments.variant(DM_DURATION_EXPERIMENT) != Some(1) {
            return None;
        }
        self.call
            .as_ref()
            .filter(|call| call.guild == 0 && call.channel == channel)?;
        if !self.stores.calls.is_call_active(channel, None) {
            return None;
        }
        self.stores.calls.get_message_id(channel).map(snowflake_ms)
    }

    /// `VoiceChannelStartTimeStore.getStartTime` (module 725613): when a guild voice channel's call began.
    pub(super) fn voice_call_start(&self, channel: &dc_state::Channel) -> Option<u64> {
        if channel.kind != dc_state::ChannelKind::Voice {
            return None;
        }
        let guild = self.stores.guild_of_channel(channel.id)?.id;
        self.stores.channel_status.get_start_time(guild, channel.id)
    }

    /// The timer of a call that started at `start_ms` (unix milliseconds), in `--text-feedback-positive` as every
    /// caller of module 693879 draws it.
    pub(super) fn elapsed_timer(&self, start_ms: u64, cx: &mut Context<Self>) -> Div {
        let now = now_ms();
        super::regions::note_drawn();
        // The next whole second of this timer.
        let wake_at = now + 1000 - now.saturating_sub(start_ms) % 1000;
        let earlier = NEXT.with(|next| {
            next.get()
                .is_none_or(|pending| wake_at < pending || pending <= now)
        });
        if earlier {
            NEXT.with(|next| next.set(Some(wake_at)));
            cx.spawn(async move |this, cx| {
                cx.background_executor()
                    .timer(Duration::from_millis(wake_at.saturating_sub(now_ms())))
                    .await;
                let _ = this.update(cx, |_, cx| {
                    if NEXT.with(|next| next.get()) == Some(wake_at) {
                        NEXT.with(|next| next.set(None));
                        super::regions::wake(cx);
                    }
                });
            })
            .detach();
        }
        div()
            .text_size(px(12.))
            .line_height(px(16.))
            .font_family(MONO)
            .text_color(color("--text-feedback-positive"))
            .whitespace_nowrap()
            .child(text(start_ms, now))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_text_reads_as_w6() {
        assert_eq!(text(0, 0), "0:00");
        assert_eq!(text(0, 59_999), "0:59");
        assert_eq!(text(0, 61_000), "1:01");
        assert_eq!(text(0, 3_599_000), "59:59");
        assert_eq!(text(0, 3_600_000), "1:00:00");
        assert_eq!(text(0, 90_061_000), "25:01:01");
        assert_eq!(text(5_000, 1_000), "0:00");
    }

    #[test]
    fn snowflakes_read_as_their_time() {
        // The snowflake of Discord's documentation: 2016-04-30 11:18:25.796 UTC.
        let id: u64 = 175_928_847_299_117_063;
        assert_eq!(snowflake_ms(id), 1_462_015_105_796);
    }
}
