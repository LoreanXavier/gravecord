//! Guild rail number badge: 396655, 771640, 812993 and BlobMask 862328.
use super::{Root, color};
use crate::parity::Parity;
use dc_state::{Guild, unread::Counts};
use gpui::{Div, FontWeight, div, prelude::*, px};

pub(super) fn rail_number(count: u64, low_importance: bool, probe: bool) -> Div {
    let width = if count < 10 {
        16.
    } else if count < 100 {
        22.
    } else {
        30.
    };
    let text = if count < 1000 {
        count.to_string()
    } else {
        format!("{}K+", (count / 1000).min(9))
    };
    div()
        .absolute()
        .right(px(14.))
        .bottom(px(-2.))
        .w(px(width + 4.))
        .h(px(20.))
        .rounded_full()
        .bg(color("--background-base-lowest"))
        .child(
            div()
                .when(probe, |badge| badge.parity("rail-mention-badge"))
                .absolute()
                .left(px(2.))
                .top(px(2.))
                .w(px(width))
                .h(px(16.))
                .rounded_full()
                .bg(color(if low_importance {
                    "--background-mod-strong"
                } else {
                    "--background-feedback-notification"
                }))
                .flex()
                .items_center()
                .justify_center()
                .text_size(px(12.))
                .line_height(px(16.))
                .font_weight(FontWeight(600.))
                .text_color(color("--white"))
                .text_center()
                .child(text),
        )
}

impl Root {
    pub(super) fn guild_rail_counts(&self, guild: &Guild) -> Counts {
        #[cfg(feature = "capture")]
        if guild.id == 1568692564637524109
            && let Some((count, low)) = self.capture_badge
        {
            return Counts {
                mentions: count,
                high_mentions: if low { 0 } else { count },
                unread: count > 0,
            };
        }
        dc_state::unread::guild_counts(&self.stores, guild, chrono::Utc::now())
    }

    #[cfg(feature = "capture")]
    pub(crate) fn script_badge(&mut self, words: &[&str], cx: &mut gpui::Context<Self>) -> String {
        if self.guild != Some(1568692564637524109)
            || std::env::var("GRAVECORD_NO_SETTINGS_WRITE").as_deref() != Ok("1")
        {
            return "badge preview requires fixture guild and settings-write guard".into();
        }
        if words == ["clear"] {
            self.capture_badge = None;
            cx.notify();
            return "badge preview cleared".into();
        }
        let Some(count) = words
            .first()
            .and_then(|value| value.parse::<u64>().ok())
            .filter(|count| *count <= 99999)
        else {
            return "usage: badge fixture <0..99999> [low] | clear".into();
        };
        let low = words.get(1) == Some(&"low");
        self.capture_badge = Some((count, low));
        cx.notify();
        format!("fixture badge preview: count={count} low={low}; view only")
    }
}
