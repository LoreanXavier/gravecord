//! The horizontal scroller of module 151869 (`R`, `scrollStep: "page"`): a row of equal cards `gap` apart in a
//! `client`-wide window, and the page steps its arrows take (`F` with `y` and `I`, keeping 16px clear of the edges).

/// The 16px the steps keep between a card and the window's edge.
const EDGE: f32 = 16.;

#[derive(Clone, Copy, Debug)]
pub(super) struct Row {
    pub card: f32,
    pub gap: f32,
    pub client: f32,
    pub count: usize,
}

impl Row {
    fn bounds(&self, index: usize) -> (f32, f32) {
        let start = index as f32 * (self.card + self.gap);
        (start, start + self.card)
    }

    /// The scroll position of the last page.
    pub fn max_scroll(&self) -> f32 {
        let width = self.count as f32 * self.card + self.count.saturating_sub(1) as f32 * self.gap;
        (width - self.client).max(0.)
    }

    /// `y`: the first (`left`) or last (`right`) card that shows past the margins, and whether it is cut.
    fn visible_card(&self, left: bool, scroll: f32) -> (usize, bool) {
        let from = scroll + EDGE;
        let to = scroll + self.client - EDGE;
        if left {
            for index in 0..self.count {
                let (start, end) = self.bounds(index);
                if end > from && start < to {
                    return (index, start < from - 1.);
                }
            }
            (0, false)
        } else {
            for index in (0..self.count).rev() {
                let (start, end) = self.bounds(index);
                if end > from && start < to {
                    return (index, end > to + 1.);
                }
            }
            (self.count.saturating_sub(1), false)
        }
    }

    /// `F`: the next page to the left ends at the first visible card, to the right starts at the last one (16px
    /// clear), or one card further when that is already where the scroller is.
    pub fn page_step(&self, left: bool, scroll: f32) -> f32 {
        if self.count == 0 {
            return 0.;
        }
        let direction: isize = if left { -1 } else { 1 };
        let (index, cut) = self.visible_card(left, scroll);
        let clamp = |index: isize| index.clamp(0, self.count as isize - 1) as usize;
        let target = if cut {
            index
        } else {
            clamp(index as isize + direction)
        };
        let position = |index: usize| {
            let (start, end) = self.bounds(index);
            let value = if left {
                end - self.client + EDGE
            } else {
                start - EDGE
            };
            value.clamp(0., self.max_scroll())
        };
        let first = position(target);
        if (first - scroll).abs() > 0.5 {
            return first;
        }
        let next = clamp(target as isize + direction);
        if next == target {
            scroll
        } else {
            position(next)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn page_steps_move_to_the_cut_card_and_stop_at_the_ends() {
        let row = Row {
            card: 766.66 * 0.44 - 6.,
            gap: 16.,
            client: 766.66,
            count: 8,
        };
        assert_eq!(row.page_step(true, 0.), 0.);
        let first = row.page_step(false, 0.);
        assert!(
            (first - (2. * (row.card + 16.) - 16.)).abs() < 0.01,
            "{first}"
        );
        let end = row.max_scroll();
        assert!((row.page_step(false, end) - end).abs() < 0.01);
        assert!(row.page_step(true, first) < first);
    }

    #[test]
    fn a_row_that_fits_has_no_page() {
        let row = Row {
            card: 81.83,
            gap: 16.,
            client: 766.66,
            count: 8,
        };
        assert!(row.max_scroll() < 0.5);
        assert_eq!(row.page_step(false, 0.), 0.);
    }
}
