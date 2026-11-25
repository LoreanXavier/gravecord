//! Which rows of the member list are built. The list's rows have fixed heights, so the rows far above and below the
//! visible part are left out and stood in for by spacers of their exact total height: the layout is the same, and a
//! server with thousands of loaded members costs a screenful of rows per frame instead of all of them.

use gpui::ScrollHandle;
use std::ops::Range;

/// Rows built beyond each edge of the visible part, so a wheel turn never shows a spacer.
const OVERSCAN: usize = 8;

#[derive(Default)]
pub(super) struct State {
    /// The scroll position of the list, read back to know which rows are in view.
    pub scroll: ScrollHandle,
    /// The guild, channel, scroll offset and height the member list ranges were last looked at for.
    pub seen: Option<(u64, u64, f32, f32)>,
    /// The pending range request (289287 `updateSubscription`, debounced by 50 ms); dropping it cancels it.
    pub due: Option<gpui::Task<()>>,
}

/// The member list's row height in the official range maths (289287 `rowHeight: 42`).
pub(super) const OFFICIAL_ROW: f32 = 42.;

/// Where `pixels` down the rows falls in rows of the official 42px: the row it lands in plus the part of it, so the
/// ranges follow the rows drawn even where a row with an activity line is taller.
pub(super) fn official_offset(heights: &[f32], pixels: f32) -> f32 {
    let mut at = 0.;
    for (index, height) in heights.iter().enumerate() {
        if pixels < at + height {
            return (index as f32 + (pixels - at) / height) * OFFICIAL_ROW;
        }
        at += height;
    }
    heights.len() as f32 * OFFICIAL_ROW + (pixels - at).max(0.)
}

/// The rows to build: those meeting the part of the list that is on screen, plus the overscan.
///
/// `heights` are the rows' heights in order, `scrolled` how far the list is scrolled (the content above the window),
/// `view` the height of the window, and `before` the height of what sits above the first row (the inventory section).
pub(super) fn visible(heights: &[f32], scrolled: f32, view: f32, before: f32) -> Range<usize> {
    let top = scrolled - before;
    let bottom = top + view;
    let mut start = heights.len();
    let mut end = heights.len();
    let mut at = 0.;
    for (index, height) in heights.iter().enumerate() {
        let next = at + height;
        if start == heights.len() && next > top {
            start = index;
        }
        if at >= bottom {
            end = index;
            break;
        }
        at = next;
    }
    if start == heights.len() {
        // Scrolled past every row, or the list is empty: the last rows are the nearest to what is in view.
        start = heights.len().saturating_sub(1);
    }
    start.saturating_sub(OVERSCAN)..(end + OVERSCAN).min(heights.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offsets_count_in_official_rows() {
        assert_eq!(official_offset(&[42.; 10], 84.), 84.);
        // Two 44px rows above: 100px is 12px into the third row.
        assert!((official_offset(&[44., 44., 42.], 100.) - (2. + 12. / 42.) * 42.).abs() < 1e-3);
        assert_eq!(official_offset(&[42.; 2], 200.), 84. + 116.);
    }

    #[test]
    fn a_short_list_is_built_whole() {
        assert_eq!(visible(&[42.; 5], 0., 600., 0.), 0..5);
        assert_eq!(visible(&[], 0., 600., 0.), 0..0);
    }

    #[test]
    fn a_long_list_builds_the_window_and_the_overscan() {
        let heights = vec![40.; 1000];
        // 4000px down: row 100 is the first in view; 600px of window shows rows 100..115.
        let range = visible(&heights, 4000., 600., 0.);
        assert_eq!(range, 92..123);
        // At the top the overscan stays inside the list.
        assert_eq!(visible(&heights, 0., 600., 0.), 0..23);
    }

    #[test]
    fn what_sits_above_the_rows_moves_the_window_up() {
        let heights = vec![40.; 1000];
        assert_eq!(visible(&heights, 4000., 600., 400.), 82..113);
    }

    #[test]
    fn scrolled_past_the_end_keeps_the_last_rows() {
        let heights = vec![40.; 20];
        let range = visible(&heights, 99999., 600., 0.);
        assert_eq!(range.end, 20);
        assert!(range.start >= 11);
    }
}
