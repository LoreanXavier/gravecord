//! MoreQuickReactions (Vencord `src/plugins/moreQuickReactions`, formerly BetterQuickReact; GPL-3.0-or-later): the
//! message hover bar shows `reactionCount` quick reactions instead of 3; the message menu's quick reaction row is no
//! longer cut at 4 and becomes a grid of `columns` 44px cells (33px with `compactMode`: each button at 75% scale, its
//! emoji at 125%) showing `rows` rows at a time, padded 4px 1px 6px and centred; with `scroll` the wheel moves it a
//! row (of `columns`) at a time. `frequentEmojis` leaves the favourite emoji out of the list (the pinned client puts
//! them first; the app's list never had them, so with it off they are put first here). Not part of the pinned client.

use super::super::Root;
use crate::plugins::State;
use dc_state::Id;
use serde_json::Value;
use std::cell::Cell;

/// The backup's (and the registry's) name of the plugin.
const ID: &str = "BetterQuickReact";

#[derive(Clone, Copy, Debug, PartialEq)]
pub(in crate::ui) struct Grid {
    pub reaction_count: usize,
    pub frequent_only: bool,
    pub rows: usize,
    pub columns: usize,
    pub compact: bool,
    pub scroll: bool,
}

impl Grid {
    /// `getMaxQuickReactions`.
    pub(in crate::ui) fn visible(self) -> usize {
        self.rows * self.columns
    }
}

fn settings_of(state: &State) -> Option<Grid> {
    if !state.is_on(ID) {
        return None;
    }
    let key = |name: &str| state.value(ID, name);
    let flag = |name: &str| state.value(ID, name) == Value::Bool(true);
    let number = |value: Value, fallback: f64| value.as_f64().unwrap_or(fallback).max(0.) as usize;
    Some(Grid {
        reaction_count: number(key("reactionCount"), 5.),
        frequent_only: flag("frequentEmojis"),
        rows: number(key("rows"), 2.).max(1),
        columns: number(key("columns"), 4.).max(1),
        compact: flag("compactMode"),
        scroll: flag("scroll"),
    })
}

/// The plugin's settings while it is on.
pub(in crate::ui) fn grid() -> Option<Grid> {
    if crate::plugins::bypassed() {
        return None;
    }
    crate::plugins::with(settings_of)
}

thread_local! {
    /// The grid's first shown reaction, for the message whose menu is open (`useState(0)` per menu).
    static OFFSET: Cell<(Id, usize)> = const { Cell::new((0, 0)) };
}

/// `onWheelWrapper`: a notch moves a row; it stays within the list.
fn scrolled(offset: usize, delta_y: f32, grid: Grid, len: usize) -> usize {
    let max = len.saturating_sub(grid.visible());
    let next = if delta_y < 0. {
        offset.saturating_sub(grid.columns)
    } else {
        offset + grid.columns
    };
    next.min(max)
}

/// The first shown reaction of `message`'s menu.
pub(in crate::ui) fn offset(message: Id) -> usize {
    OFFSET.with(|cell| {
        let (known, offset) = cell.get();
        if known == message { offset } else { 0 }
    })
}

/// A wheel step over the grid (`deltaY`, Shift held or none ignored): true when it moved.
pub(in crate::ui) fn scroll(message: Id, delta_y: f32, shift: bool, len: usize) -> bool {
    let Some(grid) = grid().filter(|grid| grid.scroll) else {
        return false;
    };
    if delta_y == 0. || shift {
        return false;
    }
    let next = scrolled(offset(message), delta_y, grid, len);
    OFFSET.with(|cell| cell.set((message, next)));
    true
}

/// The grid's cell: 44px, 33px compact.
fn cell(grid: Grid) -> f32 {
    if grid.compact { 33. } else { 44. }
}

/// The menu's width: at least the grid's, with its 1px sides and the menu's 8px padding.
pub(in crate::ui) fn menu_width(normal: f32) -> f32 {
    match grid() {
        Some(grid) => {
            let columns = grid.columns as f32;
            normal.max(columns * cell(grid) + (columns - 1.) * 4. + 2. + 16.)
        }
        None => normal,
    }
}

impl Root {
    /// The menu's quick reactions as the plugin's grid: `rows` rows of `columns` from the scroll offset.
    pub(in crate::ui) fn more_quick_grid_row(
        &self,
        message: Id,
        reactions: Vec<&'static dc_tokens::emoji_data::Emoji>,
        grid: Grid,
        cx: &mut gpui::Context<Self>,
    ) -> gpui::AnyElement {
        use super::super::color;
        use gpui::{
            InteractiveElement, IntoElement, ParentElement, StatefulInteractiveElement, Styled,
            div, img, px,
        };
        let len = reactions.len();
        let start = offset(message).min(len);
        let size = cell(grid);
        let shown: Vec<_> = reactions
            .into_iter()
            .skip(start)
            .take(grid.visible())
            .collect();
        let rows = shown.chunks(grid.columns).map(|row| {
            div()
                .flex()
                .justify_center()
                .gap(px(4.))
                .children(row.iter().enumerate().map(|(index, emoji)| {
                    let surrogates = emoji.surrogates;
                    let picture = self.images.get(&format!("emoji:{surrogates}")).cloned();
                    div()
                        .id(gpui::SharedString::from(format!(
                            "quickreact-{surrogates}-{index}"
                        )))
                        .size(px(size))
                        .rounded(px(if grid.compact { 6. } else { 8. }))
                        .bg(color("--background-mod-subtle"))
                        .hover(|style| style.bg(color("--background-mod-normal")))
                        .active(|style| style.bg(color("--background-mod-muted")))
                        .cursor_pointer()
                        .flex()
                        .items_center()
                        .justify_center()
                        // Compact: the button at 75%, its emoji at 125% (25px drawn at 75%).
                        .children(picture.map(|picture| {
                            img(picture).size(px(if grid.compact { 18.75 } else { 20. }))
                        }))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.react_from_menu(message, surrogates, cx)
                        }))
                }))
        });
        div()
            .id("quick-react-grid")
            .pt(px(4.))
            .px(px(1.))
            .pb(px(6.))
            .flex()
            .flex_col()
            .gap(px(4.))
            .children(rows)
            .on_scroll(cx.listener(move |_, event: &gpui::ScrollEvent, _, cx| {
                let delta = match event.delta {
                    gpui::ScrollDelta::Lines(lines) => -lines.y,
                    gpui::ScrollDelta::Pixels(pixels) => -f32::from(pixels.y),
                };
                if scroll(message, delta, event.modifiers.shift, len) {
                    cx.stop_propagation();
                    cx.notify();
                }
            }))
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_grid_scrolls_a_row_at_a_time() {
        let mut state = State::default();
        assert_eq!(settings_of(&state), None);
        state.set_enabled(ID, true);
        let grid = settings_of(&state).unwrap();
        assert_eq!((grid.reaction_count, grid.rows, grid.columns), (5, 2, 4));
        assert!(grid.frequent_only && grid.scroll && !grid.compact);
        // 20 reactions, 8 shown: 0 → 4 → 8 → 12 (the last full page) and no further.
        assert_eq!(scrolled(0, 1., grid, 20), 4);
        assert_eq!(scrolled(8, 1., grid, 20), 12);
        assert_eq!(scrolled(12, 1., grid, 20), 12);
        assert_eq!(scrolled(4, -1., grid, 20), 0);
        assert_eq!(scrolled(0, 1., grid, 5), 0);
    }
}
