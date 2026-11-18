//! GameActivityToggle (Vencord `src/plugins/gameActivityToggle`, GPL-3.0-or-later): a user panel button before the
//! microphone that turns "Share my activity" (`status.showCurrentGame`) on and off; while it is off the button glows
//! red and the gamepad is struck through. Not part of the pinned client.

use super::super::motion::{self, Transitions as _};
use super::super::tooltip::{Side, Tip};
use super::super::{Root, color, icon};
use crate::plugins::State;
use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyElement, Context, InteractiveElement, IntoElement, ParentElement,
    StatefulInteractiveElement, Styled, div, px,
};

const ID: &str = "GameActivityToggle";

/// `oldIcon`, when the plugin is on.
fn read(state: &State) -> Option<bool> {
    state
        .is_on(ID)
        .then(|| state.value(ID, "oldIcon") == serde_json::Value::Bool(true))
}

/// `tooltipText`.
pub(super) fn tooltip(showing: bool) -> &'static str {
    if showing {
        "Disable Game Activity"
    } else {
        "Enable Game Activity"
    }
}

impl Root {
    /// The button, or nothing; with it the panel's buttons take 40px more (the name tag's hover box shrinks by that).
    pub(in crate::ui) fn game_activity_toggle(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if crate::plugins::bypassed() {
            return None;
        }
        let old_icon = crate::plugins::with(read)?;
        // `ShowCurrentGame` is "Share my activity" of Activity Privacy.
        let showing = self.pref_on("activity_privacy_setting", true);
        let glyph = if showing {
            icon("plugin_game", 20., "--interactive-text-default")
                .group_hover("game-activity-toggle", |style| {
                    style.text_color(color("--interactive-text-hover"))
                })
                .into_any_element()
        } else if old_icon {
            // The old icon keeps the gamepad in the button's colour and only the line red.
            div()
                .relative()
                .size(px(20.))
                .child(icon("plugin_game_cut_old", 20., "--icon-voice-muted"))
                .child(div().absolute().inset_0().child(icon(
                    "plugin_game_line_old",
                    20.,
                    "--status-danger",
                )))
                .into_any_element()
        } else {
            icon("plugin_game_off", 20., "--status-danger").into_any_element()
        };
        // The panel `Button` (`.GREEN,positionKeyStemOverride:`): 32px, 8px corners; `redGlow` while sharing is off.
        let button = div()
            .id("game-activity-toggle")
            .group("game-activity-toggle")
            .size(px(32.))
            .rounded(px(8.))
            .flex()
            .items_center()
            .justify_center()
            .cursor_pointer()
            .map(|this| {
                if showing {
                    this.hover(|style| style.bg(color("--control-secondary-background-hover")))
                } else {
                    this.bg(color("--background-voice-muted"))
                        .hover(|style| style.bg(color("--opacity-red-24")))
                }
            })
            .transition(
                motion::Props::BACKGROUND,
                motion::css(200., motion::Ease::Ease),
            )
            .child(glyph)
            .on_click(cx.listener(|this, _, _, cx| {
                this.toggle_pref("activity_privacy_setting", true);
                cx.notify();
            }));
        Some(
            div()
                .id("game-activity-toggle-wrap")
                .ml(px(12.))
                .flex_none()
                .child(self.with_tip(
                    "game-activity-toggle",
                    Tip::text(tooltip(showing), Side::Top),
                    button,
                    cx,
                ))
                .into_any_element(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn the_button_reads_its_settings_and_names_the_next_state() {
        assert_eq!(read(&State::default()), None);
        let mut state = State::default();
        state.set_enabled(ID, true);
        assert_eq!(read(&state), Some(false));
        state.set_value(ID, "oldIcon", &json!(true));
        assert_eq!(read(&state), Some(true));
        assert_eq!(tooltip(true), "Disable Game Activity");
        assert_eq!(tooltip(false), "Enable Game Activity");
    }
}
