//! Zoom of a focused stream (modules 40056 and 555148 `M`): the wheel zooms between 1 and 5 about the pointer, by
//! 1.1 to the power of the wheel's hundredths held to one step; while zoomed, a drag pans, held so the picture
//! never leaves the tile, and a press that moved does not count as a click on the tile. The zoom belongs to the
//! focused tile and starts over when another one is focused.
//!
//! Not built: the zoom slider of the tile's controls, and the 0.15s ease of a zoom step (a step lands at once).

use gpui::{Bounds, Pixels};
use std::cell::Cell;
use std::rc::Rc;

pub(super) const MIN: f32 = 1.;
pub(super) const MAX: f32 = 5.;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Zoom {
    pub level: f32,
    /// How far the picture's centre is from the tile's, in pixels.
    pub pan: (f32, f32),
}

impl Default for Zoom {
    fn default() -> Self {
        Self {
            level: MIN,
            pan: (0., 0.),
        }
    }
}

/// `clampPanOffset`: half of what the zoom adds on each axis.
pub(super) fn clamp_pan(pan: (f32, f32), level: f32, size: (f32, f32)) -> (f32, f32) {
    let x = size.0 * (level - 1.) / 2.;
    let y = size.1 * (level - 1.) / 2.;
    (pan.0.clamp(-x, x), pan.1.clamp(-y, y))
}

/// `doZoom`: the level held to its range, and the point `at` (from the tile's centre) kept under the pointer.
pub(super) fn zoom_to(zoom: Zoom, wanted: f32, at: (f32, f32), size: (f32, f32)) -> Zoom {
    let level = wanted.clamp(MIN, MAX);
    if level == zoom.level {
        return zoom;
    }
    let by = level / zoom.level;
    let pan = (
        (zoom.pan.0 - at.0) * by + at.0,
        (zoom.pan.1 - at.1) * by + at.1,
    );
    Zoom {
        level,
        pan: clamp_pan(pan, level, size),
    }
}

/// The level a wheel movement asks for; `delta_y` as a browser reports it (down is positive, in pixels).
pub(super) fn wheel_level(level: f32, delta_y: f32) -> f32 {
    level * 1.1f32.powf((-delta_y / 100.).clamp(-1., 1.))
}

#[derive(Default)]
pub(super) struct State {
    /// Whose stream the zoom is of.
    owner: Option<u64>,
    pub zoom: Zoom,
    /// A pan in progress: where the pointer went down and the pan then.
    pub drag: Option<((f32, f32), (f32, f32))>,
    /// The press moved the picture: the click that ends it is not the tile's.
    moved: bool,
    /// Where the zoomed tile was painted.
    pub bounds: Rc<Cell<Bounds<Pixels>>>,
}

impl State {
    /// The zoom follows the focused stream and starts over with another one.
    pub fn follow(&mut self, owner: Option<u64>) {
        if self.owner != owner {
            self.owner = owner;
            self.zoom = Zoom::default();
            self.drag = None;
            self.moved = false;
        }
    }

    pub fn of(&self, owner: u64) -> Zoom {
        if self.owner == Some(owner) {
            self.zoom
        } else {
            Zoom::default()
        }
    }

    fn size(&self) -> (f32, f32) {
        let bounds = self.bounds.get();
        (f32::from(bounds.size.width), f32::from(bounds.size.height))
    }

    /// The pointer from the tile's centre.
    fn off_centre(&self, x: Pixels, y: Pixels) -> (f32, f32) {
        let bounds = self.bounds.get();
        let centre = bounds.center();
        (f32::from(x - centre.x), f32::from(y - centre.y))
    }

    pub fn wheel(&mut self, x: Pixels, y: Pixels, delta_y: f32) {
        let at = self.off_centre(x, y);
        let wanted = wheel_level(self.zoom.level, delta_y);
        self.zoom = zoom_to(self.zoom, wanted, at, self.size());
    }

    pub fn press(&mut self, x: Pixels, y: Pixels) {
        self.moved = false;
        if self.zoom.level > MIN {
            self.drag = Some(((f32::from(x), f32::from(y)), self.zoom.pan));
        }
    }

    pub fn drag_to(&mut self, x: Pixels, y: Pixels) {
        let Some((from, pan)) = self.drag else {
            return;
        };
        let moved = (f32::from(x) - from.0, f32::from(y) - from.1);
        if moved.0.abs() > 0.01 || moved.1.abs() > 0.01 {
            self.moved = true;
        }
        self.zoom.pan = clamp_pan(
            (pan.0 + moved.0, pan.1 + moved.1),
            self.zoom.level,
            self.size(),
        );
    }

    pub fn release(&mut self) {
        self.drag = None;
    }

    /// Whether the press that just ended panned; asked once by the click.
    pub fn take_moved(&mut self) -> bool {
        std::mem::take(&mut self.moved)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SIZE: (f32, f32) = (800., 450.);

    #[test]
    fn a_wheel_step_is_a_tenth_and_no_more() {
        assert!((wheel_level(1., -100.) - 1.1).abs() < 1e-5);
        assert!((wheel_level(1., -900.) - 1.1).abs() < 1e-5);
        assert!((wheel_level(2.2, 100.) - 2.).abs() < 1e-5);
    }

    #[test]
    fn the_level_stays_between_one_and_five() {
        let zoom = zoom_to(Zoom::default(), 9., (0., 0.), SIZE);
        assert_eq!(zoom.level, 5.);
        assert_eq!(zoom_to(zoom, 0.2, (0., 0.), SIZE).level, 1.);
    }

    #[test]
    fn the_point_under_the_pointer_stays_there() {
        // A point 100px right of the centre: after doubling, the picture's centre is 100px to the left.
        let zoom = zoom_to(Zoom::default(), 2., (100., 0.), SIZE);
        assert_eq!(zoom.pan, (-100., 0.));
    }

    #[test]
    fn the_picture_never_leaves_the_tile() {
        assert_eq!(clamp_pan((900., -900.), 2., SIZE), (400., -225.));
        assert_eq!(clamp_pan((30., 30.), 1., SIZE), (0., 0.));
    }

    #[test]
    fn another_stream_starts_at_one() {
        let mut state = State::default();
        state.follow(Some(1));
        state.zoom.level = 3.;
        assert_eq!(state.of(1).level, 3.);
        assert_eq!(state.of(2).level, 1.);
        state.follow(Some(2));
        assert_eq!(state.zoom, Zoom::default());
    }
}
