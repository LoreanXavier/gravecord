//! The server rail's motion. The pill is `item__o8_Xq` (stylesheet 441674): an 8px item with `opacity: 0` and
//! `translateX(-4px)`, which `.visible` brings to full opacity and no offset, and `.hovered` (20px) and `.selected`
//! (40px) make taller, all under `.full-motion { transition: height .2s ease-out, opacity .2s ease-out,
//! transform .2s ease-out }`. The tile is `childWrapper_JJ-hgR` (stylesheet 343178): `background-color`, `color` and
//! `scale` over .15s ease-out, brand coloured and white while the item is hovered or selected.

use super::{Css, Ease, animate, css, hovered, key};
use crate::ui::color;
use gpui::{AnyElement, IntoElement, ParentElement, Rgba, Styled, div, px};

/// `.full-motion .item__o8_Xq`.
const PILL: Css = css(200., Ease::EaseOut).full_motion();
/// `.childWrapper_JJ-hgR`.
const TILE: Css = css(150., Ease::EaseOut);

/// The key the hover probe of a rail row is stored under.
pub(in crate::ui) fn hover_key(row: &str) -> u64 {
    key(&("rail-row", row))
}

/// What a rail row shows at its left edge.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::ui) enum Mark {
    Hidden,
    Unread,
    Hovered,
    Selected,
}

impl Mark {
    /// The pill's classes: `selected` wins over `hovered`, which wins over the unread dot.
    pub(in crate::ui) fn of(selected: bool, hovered: bool, unread: bool) -> Self {
        if selected {
            Mark::Selected
        } else if hovered {
            Mark::Hovered
        } else if unread {
            Mark::Unread
        } else {
            Mark::Hidden
        }
    }

    /// (height, opacity, translateX) of the item in this state.
    pub(in crate::ui) fn target(self) -> [f64; 4] {
        match self {
            Mark::Hidden => [8., 0., -4., 0.],
            Mark::Unread => [8., 1., 0., 0.],
            Mark::Hovered => [20., 1., 0., 0.],
            Mark::Selected => [40., 1., 0., 0.],
        }
    }
}

/// The pill of the row named `row`, drawn at its current point in the transition. The row is 40px high; the pill
/// sits in an 8px wide box clipped at the row's left edge, so only its right half (4px) shows.
pub(in crate::ui) fn pill(row: &str, selected: bool, unread: bool) -> AnyElement {
    let mark = Mark::of(selected, hovered(hover_key(row)), unread);
    let [height, opacity, shift, _] = animate(key(&("rail-pill", row)), mark.target(), PILL);
    div()
        .absolute()
        .left(px(0.))
        .top(px(0.))
        .w(px(4.))
        .h(px(40.))
        .overflow_hidden()
        .child(
            div()
                .absolute()
                .left(px(shift as f32))
                .top(px((40. - height as f32) / 2.))
                .w(px(4.))
                .h(px(height as f32))
                .rounded_r(px(4.))
                .bg(color("--text-strong"))
                .opacity(opacity as f32),
        )
        .into_any_element()
}

/// The tile's background while the row is hovered or selected (`.wrapper:hover .childWrapper`), moving between
/// `rest` and `--background-brand` over .15s.
pub(in crate::ui) fn tile_background(row: &str, selected: bool, rest: &str) -> Rgba {
    let lit = selected || hovered(hover_key(row));
    super::color(
        key(&("rail-tile-bg", row)),
        color(if lit { "--background-brand" } else { rest }),
        TILE,
    )
}

/// The same for the tile's glyph or letters (`--text-default` to `--white`).
pub(in crate::ui) fn tile_foreground(row: &str, selected: bool) -> Rgba {
    let lit = selected || hovered(hover_key(row));
    super::color(
        key(&("rail-tile-fg", row)),
        color(if lit { "--white" } else { "--text-default" }),
        TILE,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selected_beats_hovered_beats_unread() {
        assert_eq!(Mark::of(true, true, true), Mark::Selected);
        assert_eq!(Mark::of(false, true, true), Mark::Hovered);
        assert_eq!(Mark::of(false, false, true), Mark::Unread);
        assert_eq!(Mark::of(false, false, false), Mark::Hidden);
    }

    #[test]
    fn the_pill_heights_are_the_stylesheets() {
        assert_eq!(Mark::Unread.target()[0], 8.);
        assert_eq!(Mark::Hovered.target()[0], 20.);
        assert_eq!(Mark::Selected.target()[0], 40.);
        // Hidden: transparent and a whole 4px left of its place.
        assert_eq!(Mark::Hidden.target()[1..3], [0., -4.]);
    }

    #[test]
    fn hovering_a_hidden_pill_runs_for_200ms_ease_out() {
        super::super::reset_for_tests();
        super::super::freeze(Some(0.));
        super::super::begin_frame(false);
        let k = key("p");
        animate(k, Mark::Hidden.target(), PILL);
        super::super::freeze(Some(100.));
        super::super::begin_frame(false);
        let at = animate(k, Mark::Hovered.target(), PILL);
        assert_eq!(at, Mark::Hidden.target());
        super::super::freeze(Some(200.));
        super::super::begin_frame(false);
        let eased = Ease::EaseOut.apply(0.5);
        let at = animate(k, Mark::Hovered.target(), PILL);
        assert!((at[0] - (8. + 12. * eased)).abs() < 1e-9);
        assert!((at[1] - eased).abs() < 1e-9);
        assert!((at[2] - (-4. + 4. * eased)).abs() < 1e-9);
    }
}
