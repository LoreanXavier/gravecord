//! Moving and resizing a widget while the overlay is unlocked: the anchor maths of module 5463 and the drag
//! container of module 129537 (`wrapper_yjvjn7`).
//!
//! A widget is placed by an anchor: one of top/bottom and one of left/right, each a distance from that edge of the
//! overlay window. The store keeps them as fractions of the window (`NO`, module 5463); the container works in pixels
//! (`Nv`). While dragging, the widget follows the pointer with the grab offset kept, stays inside the window (`fh`),
//! and is anchored to whichever edges it is nearer (`h1`, `snapOrientation` on by default). A press only starts a move
//! once the pointer has gone `sqrt(3)` px (squared distance under 3 is ignored). On release the anchor is oriented once
//! more and handed to `onUpdate`, which stores it as fractions (`LAYOUT_UPDATE_WIDGET`) and brings the widget to the
//! top (`LAYOUT_SET_TOP_WIDGET`).

use crate::layout::Anchor;

/// `padding` and `borderWidth` of the container (both defaults of module 129537): the widget's content sits 10px in.
pub const PADDING: f32 = 8.;
pub const BORDER: f32 = 2.;
/// `borderRadius` 12 plus the border and the padding.
pub const RADIUS: f32 = 12. + BORDER + PADDING;
/// `--custom-drag-resize-container-handle-size` and `-bleed` (stylesheet 419070): handles reach 6px outside the box.
pub const HANDLE_SIZE: f32 = 8.;
pub const HANDLE_BLEED: f32 = 2.;

/// All four distances of a box from the window's edges (`Ly`'s result).
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Edges {
    pub top: f32,
    pub left: f32,
    pub bottom: f32,
    pub right: f32,
}

/// The window a widget moves in (`container` of module 427830: `{minX: 0, minY: 0, maxX: width, maxY: height}`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Container {
    pub min_x: f32,
    pub min_y: f32,
    pub max_x: f32,
    pub max_y: f32,
}

impl Container {
    pub fn window(width: f32, height: f32) -> Self {
        Self {
            min_x: 0.,
            min_y: 0.,
            max_x: width,
            max_y: height,
        }
    }
}

/// `Ly`: the four edges of a `w` by `h` box placed by a pixel anchor in a `width` by `height` window. A missing pair
/// starts at 0; a given side fixes the other.
pub fn resolve(anchor: Anchor, width: f32, height: f32, w: f32, h: f32) -> Edges {
    let (top, bottom) = match (anchor.top, anchor.bottom) {
        (None, None) => (0., height - h),
        (None, Some(bottom)) => (height - (bottom + h), bottom),
        (Some(top), _) => (top, height - (top + h)),
    };
    let (left, right) = match (anchor.left, anchor.right) {
        (None, None) => (0., width - w),
        (None, Some(right)) => (width - (right + w), right),
        (Some(left), right) => (left, right.unwrap_or(width - (left + w))),
    };
    Edges {
        top,
        left,
        bottom,
        right,
    }
}

/// `h1`: anchored to the nearer edge of each pair (a tie, or touching the top or left, keeps top or left).
pub fn orient(edges: Edges) -> Anchor {
    let top = edges.top == 0. || edges.top <= edges.bottom;
    let left = edges.left == 0. || edges.left <= edges.right;
    Anchor {
        top: top.then_some(edges.top),
        left: left.then_some(edges.left),
        bottom: (!top).then_some(edges.bottom),
        right: (!left).then_some(edges.right),
    }
}

/// `fh`: a box past an edge is pushed back inside (bottom and right first, then top and left).
pub fn clamp_inside(edges: Edges) -> Edges {
    let Edges {
        mut top,
        mut left,
        mut bottom,
        mut right,
    } = edges;
    if bottom < 0. {
        top += bottom;
        bottom = 0.;
    }
    if top < 0. {
        bottom += top;
        top = 0.;
    }
    if right < 0. {
        left += right;
        right = 0.;
    }
    if left < 0. {
        right += left;
        left = 0.;
    }
    Edges {
        top,
        left,
        bottom,
        right,
    }
}

/// `af`: the sides of `pattern` that are set, taken from `edges`.
pub fn keep_sides(edges: Edges, pattern: Anchor) -> Anchor {
    Anchor {
        top: pattern.top.map(|_| edges.top),
        left: pattern.left.map(|_| edges.left),
        bottom: pattern.bottom.map(|_| edges.bottom),
        right: pattern.right.map(|_| edges.right),
    }
}

/// `NO`: a pixel anchor as fractions of the window, each clamped to 0..1.
pub fn to_fractions(anchor: Anchor, width: f32, height: f32) -> Anchor {
    let at = |value: Option<f32>, size: f32| value.map(|value| (value / size).clamp(0., 1.));
    Anchor {
        top: at(anchor.top, height),
        left: at(anchor.left, width),
        bottom: at(anchor.bottom, height),
        right: at(anchor.right, width),
    }
}

/// `x` of module 129537: a widget without either side of a pair starts at the top or left.
pub fn with_defaults(anchor: Anchor) -> Anchor {
    let mut anchor = anchor;
    if anchor.left.is_none() && anchor.right.is_none() {
        anchor.left = Some(0.);
    }
    if anchor.top.is_none() && anchor.bottom.is_none() {
        anchor.top = Some(0.);
    }
    anchor
}

/// The container's operations (`Operations` of module 129537).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Operation {
    Move,
    North,
    West,
    East,
    South,
    NorthWest,
    NorthEast,
    SouthWest,
    SouthEast,
}

impl Operation {
    /// `S`: the corner a resize holds still, as the sides it keeps.
    fn held(self, edges: Edges) -> Anchor {
        let Edges {
            top,
            left,
            bottom,
            right,
        } = edges;
        match self {
            Self::East | Self::NorthEast | Self::North => Anchor {
                bottom: Some(bottom),
                left: Some(left),
                ..Anchor::default()
            },
            Self::West | Self::NorthWest => Anchor {
                bottom: Some(bottom),
                right: Some(right),
                ..Anchor::default()
            },
            Self::SouthEast | Self::Move => Anchor {
                top: Some(top),
                left: Some(left),
                ..Anchor::default()
            },
            Self::South | Self::SouthWest => Anchor {
                top: Some(top),
                right: Some(right),
                ..Anchor::default()
            },
        }
    }
}

/// `Math.pow(dx, 2) + Math.pow(dy, 2) < 3`: a press that has not moved this far is still a click.
pub const START_DISTANCE_SQUARED: f32 = 3.;

/// One move or resize in progress (`dragState` and `state.operation` of module 129537).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Drag {
    pub operation: Operation,
    start: (f32, f32),
    offset: (f32, f32),
    origin_size: (f32, f32),
    oriented: Anchor,
    /// The pointer has gone past the start distance (`operationStarted`).
    pub started: bool,
    /// The pixel anchor shown now (`this.anchor`).
    pub anchor: Anchor,
    /// The box's size now, in pixels.
    pub size: (f32, f32),
}

impl Drag {
    /// `handleOperationStart`: the press at `pointer` on a widget placed by `anchor` (pixels) and measured `size`.
    pub fn start(
        operation: Operation,
        pointer: (f32, f32),
        anchor: Anchor,
        size: (f32, f32),
        container: Container,
    ) -> Self {
        let anchor = with_defaults(anchor);
        let edges = resolve(anchor, container.max_x, container.max_y, size.0, size.1);
        let mut drag = Self {
            operation,
            start: pointer,
            offset: (0., 0.),
            origin_size: size,
            oriented: Anchor::default(),
            started: false,
            anchor,
            size,
        };
        if operation == Operation::Move {
            drag.offset = (pointer.0 - edges.left, pointer.1 - edges.top);
        } else {
            drag.oriented = operation.held(edges);
            drag.anchor = drag.oriented;
        }
        drag
    }

    /// `handleMouseMove`: the pointer moved with the button down. Returns whether the widget moved.
    pub fn moved(
        &mut self,
        pointer: (f32, f32),
        container: Container,
        min_size: (f32, f32),
    ) -> bool {
        let (dx, dy) = (self.start.0 - pointer.0, self.start.1 - pointer.1);
        if !self.started && dx * dx + dy * dy < START_DISTANCE_SQUARED {
            return false;
        }
        let x = pointer.0.clamp(container.min_x, container.max_x);
        let y = pointer.1.clamp(container.min_y, container.max_y);
        if self.operation == Operation::Move {
            // `handleDragMove` with `snapOrientation` on.
            let placed = Anchor {
                top: Some(y - self.offset.1),
                left: Some(x - self.offset.0),
                ..Anchor::default()
            };
            let edges = clamp_inside(resolve(
                placed,
                container.max_x,
                container.max_y,
                self.size.0,
                self.size.1,
            ));
            self.anchor = orient(edges);
        } else {
            self.resize_to(x, y, container, min_size);
        }
        self.started = true;
        true
    }

    /// `handleResizeMove` with the default `resizeValidation` (the computed size as it is).
    fn resize_to(&mut self, x: f32, y: f32, container: Container, min_size: (f32, f32)) {
        use Operation::*;
        let mut width_change = 0.;
        let mut height_change = 0.;
        match self.operation {
            East | SouthEast | NorthEast => width_change -= self.start.0 - x,
            West | SouthWest | NorthWest => width_change += self.start.0 - x,
            _ => {}
        }
        match self.operation {
            South | SouthWest | SouthEast => height_change -= self.start.1 - y,
            North | NorthWest | NorthEast => height_change += self.start.1 - y,
            _ => {}
        }
        let size = (
            (self.origin_size.0 + width_change).max(min_size.0),
            (self.origin_size.1 + height_change).max(min_size.1),
        );
        let edges = clamp_inside(resolve(
            self.oriented,
            container.max_x,
            container.max_y,
            size.0,
            size.1,
        ));
        let kept = keep_sides(edges, self.oriented);
        self.size = size;
        if kept != self.oriented {
            self.oriented = kept;
        }
        self.anchor = self.oriented;
    }

    /// `handleOperationEnd`: the anchor the widget keeps (pixels, oriented once more), or none when the press never
    /// became a move.
    pub fn end(&self, container: Container, measured: (f32, f32)) -> Option<Anchor> {
        if !self.started {
            return None;
        }
        let edges = resolve(
            self.anchor,
            container.max_x,
            container.max_y,
            measured.0,
            measured.1,
        );
        Some(orient(edges))
    }
}

/// The pointer over a resize handle (`resizeNSCursor`, `resizeEWCursor`, `resizeNWSECursor`, `resizeNESWCursor`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cursor {
    NorthSouth,
    EastWest,
    NorthWestSouthEast,
    NorthEastSouthWest,
}

/// One resize handle (`renderResizeHandles`): its operation, its cursor, and where it sits from the container's
/// padding box (the containing block of `position: absolute`). `None` sides are not set.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Handle {
    pub operation: Operation,
    pub cursor: Cursor,
    pub anchor: Anchor,
    /// `None`: stretched between the two sides of that axis (`inset-inline: 0` or `top: 0; bottom: 0`).
    pub width: Option<f32>,
    pub height: Option<f32>,
}

/// `--custom-drag-resize-container-handle-offset`: `bleed - size`, -6 px.
pub const HANDLE_OFFSET: f32 = HANDLE_BLEED - HANDLE_SIZE;

/// The handles a widget gets (`resizeY`: north and south; `resizeX`: east and west; both: the four corners too), in
/// the order the container draws them. The corners are twice the handle size.
pub fn handles(resize_x: bool, resize_y: bool) -> Vec<Handle> {
    let o = Some(HANDLE_OFFSET);
    let zero = Some(0.);
    let side = |operation, cursor, anchor, width, height| Handle {
        operation,
        cursor,
        anchor,
        width,
        height,
    };
    let at = |top, left, bottom, right| Anchor {
        top,
        left,
        bottom,
        right,
    };
    let mut out = Vec::new();
    if resize_y {
        out.push(side(
            Operation::North,
            Cursor::NorthSouth,
            at(o, zero, None, zero),
            None,
            Some(HANDLE_SIZE),
        ));
        out.push(side(
            Operation::South,
            Cursor::NorthSouth,
            at(None, zero, o, zero),
            None,
            Some(HANDLE_SIZE),
        ));
    }
    if resize_x {
        out.push(side(
            Operation::East,
            Cursor::EastWest,
            at(zero, None, zero, o),
            Some(HANDLE_SIZE),
            None,
        ));
        out.push(side(
            Operation::West,
            Cursor::EastWest,
            at(zero, o, zero, None),
            Some(HANDLE_SIZE),
            None,
        ));
    }
    if resize_x && resize_y {
        let corner = Some(HANDLE_SIZE * 2.);
        for (operation, cursor, anchor) in [
            (
                Operation::NorthWest,
                Cursor::NorthWestSouthEast,
                at(o, o, None, None),
            ),
            (
                Operation::NorthEast,
                Cursor::NorthEastSouthWest,
                at(o, None, None, o),
            ),
            (
                Operation::SouthWest,
                Cursor::NorthEastSouthWest,
                at(None, o, o, None),
            ),
            (
                Operation::SouthEast,
                Cursor::NorthWestSouthEast,
                at(None, None, o, o),
            ),
        ] {
            out.push(side(operation, cursor, anchor, corner, corner));
        }
    }
    out
}

/// `f` of module 129537: a requested size is never under the minimum (auto stays auto).
pub fn requested_size(
    size: (Option<f32>, Option<f32>),
    min: (f32, f32),
) -> (Option<f32>, Option<f32>) {
    (size.0.map(|w| w.max(min.0)), size.1.map(|h| h.max(min.1)))
}

#[cfg(test)]
mod tests {
    use super::*;

    const WINDOW: Container = Container {
        min_x: 0.,
        min_y: 0.,
        max_x: 1280.,
        max_y: 720.,
    };

    #[test]
    fn resolve_fills_the_missing_sides() {
        let anchor = Anchor {
            top: Some(100.),
            left: None,
            bottom: None,
            right: Some(20.),
        };
        assert_eq!(
            resolve(anchor, 1280., 720., 300., 200.),
            Edges {
                top: 100.,
                left: 960.,
                bottom: 420.,
                right: 20.
            }
        );
        // Nothing set: the top left corner.
        let edges = resolve(Anchor::default(), 1280., 720., 300., 200.);
        assert_eq!(
            (edges.top, edges.left, edges.bottom, edges.right),
            (0., 0., 520., 980.)
        );
    }

    #[test]
    fn a_widget_is_anchored_to_its_nearer_edges() {
        let near_top_left = orient(Edges {
            top: 10.,
            left: 10.,
            bottom: 500.,
            right: 900.,
        });
        assert_eq!(near_top_left.top, Some(10.));
        assert_eq!(near_top_left.left, Some(10.));
        assert_eq!(near_top_left.bottom, None);
        let near_bottom_right = orient(Edges {
            top: 500.,
            left: 900.,
            bottom: 10.,
            right: 10.,
        });
        assert_eq!(near_bottom_right.bottom, Some(10.));
        assert_eq!(near_bottom_right.right, Some(10.));
        assert_eq!(near_bottom_right.top, None);
        // Touching the top keeps it even when the bottom is nearer (a box taller than half the window).
        assert_eq!(
            orient(Edges {
                top: 0.,
                left: 0.,
                bottom: -5.,
                right: 0.
            })
            .top,
            Some(0.)
        );
    }

    #[test]
    fn a_box_past_an_edge_is_pushed_back_inside() {
        let pushed = clamp_inside(Edges {
            top: -30.,
            left: 1000.,
            bottom: 550.,
            right: -20.,
        });
        assert_eq!(
            pushed,
            Edges {
                top: 0.,
                left: 980.,
                bottom: 520.,
                right: 0.
            }
        );
    }

    #[test]
    fn fractions_round_trip_through_pixels() {
        let pixels = Anchor {
            top: Some(252.),
            left: Some(64.),
            ..Anchor::default()
        };
        let fractions = to_fractions(pixels, 1280., 720.);
        assert_eq!(fractions.top, Some(0.35));
        assert_eq!(fractions.left, Some(0.05));
        let back = crate::layout::anchor_pixels(fractions, 1280., 720.);
        assert_eq!(back.top, Some(252.));
        assert_eq!(back.left, Some(64.));
        assert_eq!(
            to_fractions(
                Anchor {
                    top: Some(-4.),
                    ..Anchor::default()
                },
                10.,
                10.
            )
            .top,
            Some(0.)
        );
    }

    #[test]
    fn a_press_moves_only_past_the_start_distance() {
        let anchor = Anchor {
            top: Some(252.),
            left: Some(12.),
            ..Anchor::default()
        };
        let mut drag = Drag::start(Operation::Move, (100., 300.), anchor, (290., 196.), WINDOW);
        assert!(!drag.moved((101., 301.), WINDOW, (0., 0.)));
        assert_eq!(drag.end(WINDOW, (290., 196.)), None);
        assert!(drag.moved((102., 300.), WINDOW, (0., 0.)));
        assert!(drag.started);
        // Once started, any move counts.
        assert!(drag.moved((101., 300.), WINDOW, (0., 0.)));
    }

    #[test]
    fn a_drag_keeps_the_grab_offset_and_snaps_to_the_nearer_edges() {
        let anchor = Anchor {
            top: Some(252.),
            left: Some(12.),
            ..Anchor::default()
        };
        let mut drag = Drag::start(Operation::Move, (100., 300.), anchor, (290., 196.), WINDOW);
        drag.moved((1100., 600.), WINDOW, (0., 0.));
        // Grabbed 88px right of the left edge and 48 under the top: the box's top left goes to 1012, 552, which
        // pushes it 22px past the right edge and 28 past the bottom; it is anchored bottom right, flush.
        assert_eq!(
            drag.anchor,
            Anchor {
                top: None,
                left: None,
                bottom: Some(0.),
                right: Some(0.)
            }
        );
        let end = drag.end(WINDOW, (290., 196.)).unwrap();
        assert_eq!(end.bottom, Some(0.));
        assert_eq!(end.right, Some(0.));
        // Stored as fractions of the window.
        let stored = to_fractions(end, 1280., 720.);
        assert_eq!((stored.bottom, stored.right), (Some(0.), Some(0.)));
        // Dragged out past the window's own edge: the pointer is clamped to the window first.
        let mut out = Drag::start(Operation::Move, (100., 300.), anchor, (290., 196.), WINDOW);
        out.moved((-500., -500.), WINDOW, (0., 0.));
        assert_eq!(out.anchor.top, Some(0.));
        assert_eq!(out.anchor.left, Some(0.));
    }

    #[test]
    fn a_resize_holds_the_opposite_corner_and_keeps_the_minimum() {
        let anchor = Anchor {
            top: Some(100.),
            left: Some(100.),
            ..Anchor::default()
        };
        let mut drag = Drag::start(
            Operation::SouthEast,
            (500., 400.),
            anchor,
            (400., 300.),
            WINDOW,
        );
        drag.moved((600., 450.), WINDOW, (128., 128.));
        assert_eq!(drag.size, (500., 350.));
        assert_eq!(drag.anchor.top, Some(100.));
        assert_eq!(drag.anchor.left, Some(100.));
        drag.moved((0., 0.), WINDOW, (128., 128.));
        assert_eq!(drag.size, (128., 128.));
        // Resizing west moves the left edge: the right edge is held.
        let mut west = Drag::start(Operation::West, (100., 200.), anchor, (400., 300.), WINDOW);
        west.moved((50., 200.), WINDOW, (0., 0.));
        assert_eq!(west.size.0, 450.);
        assert_eq!(west.anchor.right, Some(780.));
        assert_eq!(west.anchor.left, None);
    }

    #[test]
    fn handles_follow_the_widget_config_and_clamp_at_the_minimum() {
        assert!(handles(false, false).is_empty());
        let y = handles(false, true);
        assert_eq!(
            y.iter().map(|h| h.operation).collect::<Vec<_>>(),
            [Operation::North, Operation::South]
        );
        assert_eq!(y[0].anchor.top, Some(-6.));
        assert_eq!(y[0].height, Some(8.));
        assert_eq!(y[0].width, None);
        let all = handles(true, true);
        assert_eq!(all.len(), 8);
        let se = all
            .iter()
            .find(|h| h.operation == Operation::SouthEast)
            .unwrap();
        assert_eq!(se.cursor, Cursor::NorthWestSouthEast);
        assert_eq!((se.width, se.height), (Some(16.), Some(16.)));
        assert_eq!((se.anchor.bottom, se.anchor.right), (Some(-6.), Some(-6.)));
        let ne = all
            .iter()
            .find(|h| h.operation == Operation::NorthEast)
            .unwrap();
        assert_eq!(ne.cursor, Cursor::NorthEastSouthWest);
        // The text chat's 500x372 at its 350x100 minimum stays; a stored 200 wide grows back to 350.
        assert_eq!(
            requested_size((Some(500.), Some(372.)), (350., 100.)),
            (Some(500.), Some(372.))
        );
        assert_eq!(
            requested_size((Some(200.), None), (350., 100.)),
            (Some(350.), None)
        );
        // A resize to under the minimum keeps the minimum, and the box stays in the window.
        let anchor = Anchor {
            top: Some(600.),
            left: Some(1000.),
            ..Anchor::default()
        };
        let mut drag = Drag::start(
            Operation::SouthEast,
            (1500., 972.),
            anchor,
            (500., 372.),
            WINDOW,
        );
        drag.moved((1280., 720.), WINDOW, (350., 100.));
        // The pointer is held to the window: 220 px less wide would be 280, the minimum keeps 350.
        assert_eq!(drag.size, (350., 120.));
        assert_eq!(drag.anchor.top, Some(600.));
    }
}
