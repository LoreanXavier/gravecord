//! Composer notice: modules 620014 and 960850; `cooldownWrapper_bbCgC2` in the channel DOM.
use super::{Root, color, icon, tooltip};
use crate::parity::Parity;
use gpui::{AnyElement, Context, FontWeight, IntoElement, div, prelude::*, px};

fn interval_text(seconds: u64) -> String {
    let unit =
        |count: u64, name: &str| format!("{count} {name}{}", if count == 1 { "" } else { "s" });
    let duration = if seconds < 60 {
        format!("{}.", unit(seconds, "second"))
    } else if seconds < 3600 {
        format!(
            "{} {}",
            unit(seconds / 60, "minute"),
            if seconds.is_multiple_of(60) {
                String::new()
            } else {
                unit(seconds % 60, "second")
            }
        )
    } else {
        let minutes = (seconds % 3600) / 60;
        format!(
            "{} {} {}",
            unit(seconds / 3600, "hour"),
            if minutes == 0 {
                String::new()
            } else {
                unit(minutes, "minute")
            },
            if seconds.is_multiple_of(60) {
                String::new()
            } else {
                unit(seconds % 60, "second")
            }
        )
    };
    format!("Slowmode is enabled. Members can send one message every {duration}")
}

impl Root {
    pub(super) fn slowmode_notice(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let channel = self.current_channel()?;
        if channel.rate_limit_per_user == 0 {
            return None;
        }
        let bypass = self
            .current_guild()
            .zip(self.stores.me())
            .is_some_and(|(guild, me)| {
                dc_state::permissions::can_in_channel(
                    guild,
                    channel,
                    me,
                    dc_state::permissions::BYPASS_SLOWMODE,
                )
            });
        let label = if bypass {
            "Slowmode Immune"
        } else {
            "Slowmode is enabled."
        };
        let notice = div()
            .parity("slowmode-notice")
            .py(px(3.))
            .pl(px(8.))
            .pr(px(16.))
            .flex()
            .items_center()
            .child(
                div()
                    .ml(px(4.))
                    .flex()
                    .items_center()
                    .gap(px(4.))
                    .text_size(px(12.))
                    .line_height(px(16.))
                    .font_weight(FontWeight(500.))
                    .text_color(color("--text-muted"))
                    .child(icon("slowmode", 12., "--text-muted"))
                    .child(label),
            );
        Some(
            div()
                .absolute()
                .right_0()
                .top(px(-23.))
                .child(self.with_tip(
                    "slowmode",
                    tooltip::Tip::body(
                        tooltip::Body::Text(interval_text(channel.rate_limit_per_user).into()),
                        tooltip::Side::Top,
                    ),
                    notice,
                    cx,
                ))
                .into_any_element(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn interval_notice_uses_the_pinned_plural_forms() {
        assert_eq!(
            interval_text(1),
            "Slowmode is enabled. Members can send one message every 1 second."
        );
        assert_eq!(
            interval_text(5),
            "Slowmode is enabled. Members can send one message every 5 seconds."
        );
        assert_eq!(
            interval_text(60),
            "Slowmode is enabled. Members can send one message every 1 minute "
        );
        assert_eq!(
            interval_text(61),
            "Slowmode is enabled. Members can send one message every 1 minute 1 second"
        );
        assert_eq!(
            interval_text(3600),
            "Slowmode is enabled. Members can send one message every 1 hour  "
        );
        assert_eq!(
            interval_text(3661),
            "Slowmode is enabled. Members can send one message every 1 hour 1 minute 1 second"
        );
    }
}
