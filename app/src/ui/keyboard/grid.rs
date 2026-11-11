//! Arrow-key movement over the rows of cells of the emoji and sticker pickers (the grid navigator's reducer, module 788413; keys read in 118057): the
//! search field keeps focus while the arrows move the highlighted cell. Headings and upsells hold no cells, so only the
//! rows of cells count; a row shorter than the one above (the last of a section) keeps the column as far as it goes.

/// A cell by the position of its row among the rows of cells, and its column.
pub(in crate::ui) type At = (usize, usize);

/// Where the arrow `key` takes `at` in rows of cells with the given lengths. `None` when it stays (the first or last
/// cell, or a key that is not an arrow).
pub(in crate::ui) fn step(rows: &[usize], at: At, key: &str, ctrl: bool) -> Option<At> {
    let (row, col) = at;
    let len = *rows.get(row)?;
    let moved = match key {
        "left" if col > 0 => (row, col - 1),
        "left" => {
            let above = row.checked_sub(1)?;
            (above, rows[above].checked_sub(1)?)
        }
        "right" if col + 1 < len => (row, col + 1),
        "right" => {
            (row + 1 < rows.len()).then_some(())?;
            (row + 1, 0)
        }
        "up" => {
            let above = row.checked_sub(1)?;
            (above, col.min(rows[above].checked_sub(1)?))
        }
        "down" => {
            (row + 1 < rows.len()).then_some(())?;
            (row + 1, col.min(rows[row + 1].checked_sub(1)?))
        }
        // Home and End go to the ends of the row, with Ctrl to the first and last cell.
        "home" if ctrl => (0, 0),
        "home" => (row, 0),
        "end" if ctrl => {
            let last = rows.len().checked_sub(1)?;
            (last, rows[last].checked_sub(1)?)
        }
        "end" => (row, len.checked_sub(1)?),
        _ => return None,
    };
    (moved != at).then_some(moved)
}

/// The cell an arrow reaches when none is highlighted yet: the first one.
pub(in crate::ui) fn first(rows: &[usize]) -> Option<At> {
    rows.iter().position(|len| *len > 0).map(|row| (row, 0))
}

/// Brings list row `row` into view. Rows further off than the list has measured cannot be revealed in place, so those
/// are scrolled to by index: a row above the view goes to the top, one below it lands near the bottom.
pub(in crate::ui) fn reveal_row(list: &gpui::ListState, row: usize) {
    // The grid shows about five rows of cells and a heading.
    const VISIBLE: usize = 5;
    let top = list.logical_scroll_top().item_ix;
    if row < top {
        list.scroll_to(gpui::ListOffset {
            item_ix: row,
            offset_in_item: gpui::px(0.),
        });
    } else if row > top + VISIBLE {
        list.scroll_to(gpui::ListOffset {
            item_ix: row - VISIBLE,
            offset_in_item: gpui::px(0.),
        });
    } else {
        list.scroll_to_reveal_item(row);
    }
}

impl crate::ui::Root {
    /// An arrow, Home or End while the emoji or sticker tab is open: the grid takes it, the search field keeps focus
    /// (module 118057). True when the key was used.
    pub(super) fn grid_key(
        &mut self,
        keystroke: &gpui::Keystroke,
        cx: &mut gpui::Context<Self>,
    ) -> bool {
        use crate::ui::picker::Tab;
        let held = &keystroke.modifiers;
        if held.alt
            || held.shift
            || held.platform
            || (held.control && !matches!(keystroke.key.as_str(), "home" | "end"))
        {
            return false;
        }
        let Some(tab) = self.picker.as_ref().map(|picker| picker.tab) else {
            return false;
        };
        let (key, ctrl) = (keystroke.key.as_str(), held.control);
        match tab {
            Tab::Emoji => self.emoji_grid_key(key, ctrl, cx),
            Tab::Stickers => self.sticker_grid_key(key, ctrl, cx),
            Tab::Gifs => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn right_and_left_wrap_to_the_next_and_previous_row() {
        let rows = [3, 3, 2];
        assert_eq!(step(&rows, (0, 1), "right", false), Some((0, 2)));
        assert_eq!(step(&rows, (0, 2), "right", false), Some((1, 0)));
        assert_eq!(step(&rows, (1, 0), "left", false), Some((0, 2)));
        assert_eq!(step(&rows, (2, 1), "right", false), None);
        assert_eq!(step(&rows, (0, 0), "left", false), None);
    }

    #[test]
    fn up_and_down_keep_the_column_and_stop_at_the_ends() {
        let rows = [4, 4, 2];
        assert_eq!(step(&rows, (0, 3), "down", false), Some((1, 3)));
        assert_eq!(step(&rows, (1, 3), "down", false), Some((2, 1)));
        assert_eq!(step(&rows, (2, 1), "up", false), Some((1, 1)));
        assert_eq!(step(&rows, (0, 2), "up", false), None);
        assert_eq!(step(&rows, (2, 0), "down", false), None);
    }

    #[test]
    fn other_keys_and_empty_grids_do_nothing() {
        assert_eq!(step(&[3], (0, 0), "home", false), None);
        assert_eq!(step(&[], (0, 0), "down", false), None);
        assert_eq!(first(&[0, 2]), Some((1, 0)));
    }

    #[test]
    fn home_and_end_go_to_the_row_ends_and_with_ctrl_to_the_grid_ends() {
        let rows = [4, 4, 2];
        assert_eq!(step(&rows, (1, 2), "home", false), Some((1, 0)));
        assert_eq!(step(&rows, (1, 2), "end", false), Some((1, 3)));
        assert_eq!(step(&rows, (1, 2), "home", true), Some((0, 0)));
        assert_eq!(step(&rows, (0, 0), "end", true), Some((2, 1)));
        assert_eq!(step(&rows, (0, 0), "home", true), None);
        assert_eq!(first(&[]), None);
    }
}
