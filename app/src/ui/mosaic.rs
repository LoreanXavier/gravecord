//! The media mosaic of a message with several pictures (module 394839 and `oneByOneGrid_idVBIj` and its siblings in the
//! stylesheet): one picture alone, two side by side, three as one tall and two short, four as a square grid, and more
//! as a row of one or two followed by rows of three squares. The block is at most 550 wide, with 4px between cells.

pub(super) const MAX_WIDTH: f32 = 550.;
const GAP: f32 = 4.;
/// `oneByTwoGrid`: a row of one or two pictures is at most this tall.
const ROW_MAX_HEIGHT: f32 = 280.;
/// The three-picture block and the cells of the four-picture grid.
const TALL: f32 = 350.;
const SHORT: f32 = 173.;

#[derive(Clone, Debug, PartialEq)]
pub(super) enum Block {
    /// One or two cells side by side (`oneByTwoGrid`), each `(width, height)`.
    Row(Vec<(f32, f32)>),
    /// `oneByTwoLayoutThreeGrid`: a tall cell on the left, two short ones stacked on the right.
    Three { left: (f32, f32), right: (f32, f32) },
    /// `twoByTwoGrid` or `threeByThreeGrid`: `columns` cells to a row.
    Grid {
        columns: usize,
        cell: (f32, f32),
        count: usize,
    },
}

/// A picture scaled into `max_width` by `max_height`, keeping its shape.
fn contain(size: (u32, u32), max_width: f32, max_height: f32) -> (f32, f32) {
    let (width, height) = (size.0.max(1) as f32, size.1.max(1) as f32);
    let scale = (max_width / width).min(max_height / height).min(1.);
    (width * scale, height * scale)
}

/// The blocks for `sizes.len()` pictures in a column `width` wide, top to bottom, and how many pictures each takes.
pub(super) fn layout(sizes: &[(u32, u32)], width: f32) -> Vec<(Block, usize)> {
    let width = width.min(MAX_WIDTH);
    let row = |items: &[(u32, u32)]| {
        let cell = ((width - GAP * (items.len() as f32 - 1.)) / items.len() as f32).round();
        // Each picture is fitted into a square cell; the row is as tall as its tallest, and the rest stretch.
        let height = items
            .iter()
            .map(|size| contain(*size, cell, cell).1)
            .fold(0., f32::max)
            .min(ROW_MAX_HEIGHT);
        Block::Row(vec![(cell, height.round()); items.len()])
    };
    match sizes.len() {
        0 | 1 => Vec::new(),
        2 => vec![(row(sizes), 2)],
        3 => {
            let left = (2. * (width - GAP) / 3.).round();
            vec![(
                Block::Three {
                    left: (left, TALL),
                    right: ((left / 2.).round(), SHORT),
                },
                3,
            )]
        }
        4 => {
            let cell = ((width - GAP) / 2.).round();
            vec![(
                Block::Grid {
                    columns: 2,
                    cell: (cell, SHORT),
                    count: 4,
                },
                4,
            )]
        }
        count => {
            let lead = count % 3;
            let cell = ((width - 2. * GAP) / 3.).round();
            let mut blocks = Vec::new();
            if lead > 0 {
                blocks.push((row(&sizes[..lead]), lead));
            }
            blocks.push((
                Block::Grid {
                    columns: 3,
                    cell: (cell, cell),
                    count: count - lead,
                },
                count - lead,
            ));
            blocks
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn square(count: usize) -> Vec<(u32, u32)> {
        vec![(1000, 800); count]
    }

    #[test]
    fn three_pictures_are_one_tall_and_two_short() {
        // The measured official block: 364 wide and 350 tall beside two 182 by 173 cells.
        let blocks = layout(&square(3), 550.);
        assert_eq!(
            blocks,
            vec![(
                Block::Three {
                    left: (364., 350.),
                    right: (182., 173.)
                },
                3
            )]
        );
    }

    #[test]
    fn two_pictures_share_the_row() {
        let blocks = layout(&square(2), 550.);
        match &blocks[0].0 {
            Block::Row(cells) => {
                assert_eq!(cells.len(), 2);
                assert_eq!(cells[0].0, 273.);
                assert!(cells[0].1 <= 273.);
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn four_make_a_grid_and_more_lead_with_a_row() {
        assert!(matches!(
            layout(&square(4), 550.)[0].0,
            Block::Grid {
                columns: 2,
                count: 4,
                ..
            }
        ));
        let five = layout(&square(5), 550.);
        assert_eq!(five.len(), 2);
        assert!(matches!(&five[0].0, Block::Row(cells) if cells.len() == 2));
        assert!(matches!(
            five[1].0,
            Block::Grid {
                columns: 3,
                count: 3,
                cell: (181., 181.)
            }
        ));
        let seven = layout(&square(7), 550.);
        assert!(matches!(&seven[0].0, Block::Row(cells) if cells.len() == 1));
        let six = layout(&square(6), 550.);
        assert_eq!(six.len(), 1);
    }

    #[test]
    fn a_narrow_column_shrinks_the_block() {
        let blocks = layout(&square(3), 400.);
        assert!(matches!(blocks[0].0, Block::Three { left, .. } if left.0 < 364.));
    }
}
