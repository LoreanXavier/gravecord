//! ReadAllNotificationsButton (Vencord `src/plugins/readAllNotificationsButton`, GPL-3.0-or-later): a "Read All"
//! button under the Home button of the server list (`ServerListRenderPosition.Above`), or with `compact` a small text
//! button, that marks every server's unread channels and threads read in one `BULK_ACK`. Not part of the pinned
//! client.

use super::super::tooltip::{Body, Tip};
use super::super::{Root, color, icon};
use crate::plugins::State;
use dc_state::Id;
use gpui::{
    AnyElement, Context, InteractiveElement, IntoElement, ParentElement,
    StatefulInteractiveElement, Styled, div, px,
};
use super::super::motion::Transitions as _;
use serde_json::Value;

const ID: &str = "ReadAllNotificationsButton";

/// `None` while off, else whether the button is the compact text one.
fn compact_in(state: &State) -> Option<bool> {
    state
        .is_on(ID)
        .then(|| state.value(ID, "compact") == Value::Bool(true))
}

impl Root {
    /// The button, in the server list after the Home button.
    pub(in crate::ui) fn read_all_button(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if crate::plugins::bypassed() {
            return None;
        }
        let compact = crate::plugins::with(compact_in)?;
        if compact {
            // `TextButton variant="secondary"`.
            return Some(
                div()
                    .id("read-all-compact")
                    .flex_none()
                    .px(px(4.))
                    .text_size(px(12.))
                    .line_height(px(16.))
                    .text_color(color("--text-default"))
                    .cursor_pointer()
                    .hover(|style| style.underline())
                    .on_click(cx.listener(|this, _, _, cx| this.read_all(cx)))
                    .child("Read All")
                    .into_any_element(),
            );
        }
        let button = div()
            .id("read-all-tile")
            .flex_none()
            .size(px(40.))
            .rounded(px(12.))
            .bg(color("--background-mod-subtle"))
            .hover(|style| style.bg(color("--background-brand")))
            .rule(super::super::motion::Rule::CircleIconButton)
            .flex()
            .items_center()
            .justify_center()
            .child(icon("plugin_double_check", 20., "--text-default"));
        Some(
            self.with_tip(
                "read-all",
                Tip::rail(Body::Heading("Read All".into())),
                button,
                cx,
            )
            .cursor_pointer()
            .on_click(cx.listener(|this, _, _, cx| this.read_all(cx)))
            .into_any_element(),
        )
    }

    /// `onClick`: every server's channels and joined threads that have something unread, acked at their newest
    /// message.
    fn read_all(&mut self, cx: &mut Context<Self>) {
        let guilds: Vec<Id> = self.stores.guilds.iter().map(|guild| guild.id).collect();
        let reads: Vec<(Id, Id)> = guilds
            .into_iter()
            .flat_map(|guild| self.guild_unread_reads(guild))
            .collect();
        self.mark_read_bulk(&reads, cx);
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn the_button_is_a_tile_or_compact_text() {
        assert_eq!(compact_in(&State::default()), None);
        let mut state = State::default();
        state.set_enabled(ID, true);
        assert_eq!(compact_in(&state), Some(false));
        state.set_value(ID, "compact", &json!(true));
        assert_eq!(compact_in(&state), Some(true));
    }
}
