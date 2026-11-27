//! Not in the pinned client: the media viewer opens from the picture that was clicked and closes back into it, as in
//! Discord's native client ("Phoenix", `DECISIONS.md`). The pinned build fades the viewer in over 200 ms with the
//! scrim on the modal spring (`lightbox.rs`).
//!
//! Phoenix's media viewer (`discord-desktop/src/media_viewer/view.rs`) runs one transition, the slot `mv-shared`:
//! 300 ms with curve 5, `cubic-bezier(.4, 0, .2, 1)` (spec at RVA `0x50f4361`). The picture moves and scales from the
//! thumbnail's rectangle in the chat to its place in the viewer, and the scrim darkens with it; closing runs it back.
//! A screen recording of Phoenix (30 fps) agrees: two openings and one closing fit this curve within
//! 0.03 of the way on average (the closing within 0.013). Under reduced motion it snaps, as Phoenix's driver does.

use super::{Css, Ease, key, value_from};
use gpui::{Bounds, Pixels};
use std::cell::RefCell;

/// `mv-shared`: 300 ms, `cubic-bezier(.4, 0, .2, 1)`.
pub const SHARED: Css = Css::new(300., Ease::Bezier(0.4, 0., 0.2, 1.)).full_motion();

#[derive(Clone)]
struct Open {
    /// The viewer item the transition belongs to.
    item: String,
    /// The thumbnail's rectangle in the window when it was clicked.
    source: Bounds<Pixels>,
    /// Separates one opening from the next in the motion store.
    run: u64,
    closing: bool,
}

thread_local! {
    static OPEN: RefCell<Option<Open>> = const { RefCell::new(None) };
    static RUNS: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

/// The viewer opens on `item` from the thumbnail at `source` (none when the thumbnail was not drawn).
pub fn begin(item: &str, source: Option<Bounds<Pixels>>) {
    let run = RUNS.with(|runs| {
        runs.set(runs.get() + 1);
        runs.get()
    });
    OPEN.with(|open| {
        *open.borrow_mut() = source
            .filter(|source| source.size.width > Pixels::ZERO && source.size.height > Pixels::ZERO)
            .map(|source| Open {
                item: item.to_owned(),
                source,
                run,
                closing: false,
            });
    });
}

/// The viewer is asked to close while it shows `item`: true when it closes by running back into the thumbnail (the
/// caller keeps it open until `frame` says it is done), false when it closes at once.
pub fn begin_close(item: &str, reduced: bool) -> bool {
    OPEN.with(|open| {
        let mut open = open.borrow_mut();
        match open.as_mut() {
            Some(state) if !reduced && !state.closing && state.item == item => {
                state.closing = true;
                true
            }
            _ => {
                *open = None;
                false
            }
        }
    })
}

/// Forgets the transition (the viewer closed).
pub fn end() {
    OPEN.with(|open| *open.borrow_mut() = None);
}

/// What the transition shows this frame for `item`: the progress (0 at the thumbnail, 1 in place), the thumbnail's
/// rectangle, and whether the viewer is closing. `None` when no transition belongs to this item.
pub fn frame(item: &str) -> Option<(f32, Bounds<Pixels>, bool)> {
    let state = OPEN.with(|open| open.borrow().clone())?;
    if state.item != item {
        return None;
    }
    let target = if state.closing { 0. } else { 1. };
    let t = value_from(key(&("media-viewer-shared", state.run)), 0., target, SHARED);
    Some((t, state.source, state.closing))
}

/// The picture's pose at progress `t`: its offset from its place and its scale about its centre, for a picture laid
/// out `size` big with its centre at `center`, coming from `source`.
pub fn pose(
    t: f32,
    source: Bounds<Pixels>,
    center: (f32, f32),
    size: (f32, f32),
) -> ((f32, f32), f32) {
    let t = t.clamp(0., 1.);
    let from_center = (
        f32::from(source.origin.x) + f32::from(source.size.width) / 2.,
        f32::from(source.origin.y) + f32::from(source.size.height) / 2.,
    );
    let from_scale = if size.0 > 0. {
        f32::from(source.size.width) / size.0
    } else {
        1.
    };
    let offset = (
        (from_center.0 - center.0) * (1. - t),
        (from_center.1 - center.1) * (1. - t),
    );
    (offset, from_scale + (1. - from_scale) * t)
}

#[cfg(test)]
pub(crate) fn reset_for_tests() {
    OPEN.with(|open| *open.borrow_mut() = None);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::motion::{begin_frame, freeze, reset_for_tests as reset_motion};
    use gpui::{point, px, size};

    fn at(ms: f64, reduced: bool) {
        freeze(Some(ms));
        begin_frame(reduced);
    }

    fn thumb() -> Bounds<Pixels> {
        Bounds {
            origin: point(px(100.), px(500.)),
            size: size(px(200.), px(100.)),
        }
    }

    #[test]
    fn the_curve_is_mv_shared() {
        assert_eq!(SHARED.duration_ms, 300.);
        assert_eq!(SHARED.ease, Ease::Bezier(0.4, 0., 0.2, 1.));
        // The recording's closing, frames after the close: 0.065, 0.289, 0.605, 0.771, 0.888 of the way.
        let fit = [(0, 0.065), (1, 0.289), (2, 0.605), (3, 0.771), (4, 0.888)];
        for (frame, measured) in fit {
            let ms = 49. + frame as f64 * 1000. / 30.;
            let model = SHARED.ease.apply(ms / 300.);
            assert!(
                (model - measured).abs() < 0.03,
                "frame {frame}: {model} vs {measured}"
            );
        }
    }

    #[test]
    fn the_pose_runs_from_the_thumbnail_to_the_middle() {
        let center = (640., 360.);
        let size = (800., 400.);
        let (offset, scale) = pose(0., thumb(), center, size);
        assert_eq!(offset, (200. - 640., 550. - 360.));
        assert_eq!(scale, 0.25);
        let (offset, scale) = pose(1., thumb(), center, size);
        assert_eq!(offset, (0., 0.));
        assert_eq!(scale, 1.);
    }

    #[test]
    fn it_opens_over_300ms_and_closes_back() {
        reset_motion();
        reset_for_tests();
        at(0., false);
        begin("a", Some(thumb()));
        assert_eq!(frame("a").map(|f| f.0), Some(0.));
        at(150., false);
        let middle = frame("a").expect("open").0;
        assert!(middle > 0.5 && middle < 0.9, "{middle}");
        at(300., false);
        assert_eq!(frame("a").map(|f| f.0), Some(1.));
        assert!(begin_close("a", false));
        at(316., false);
        let (t, _, closing) = frame("a").expect("closing");
        assert!(closing && t == 1.);
        at(616., false);
        assert_eq!(frame("a").map(|f| f.0), Some(0.));
        end();
        assert_eq!(frame("a"), None);
    }

    #[test]
    fn without_a_thumbnail_or_under_reduced_motion_nothing_runs() {
        reset_motion();
        reset_for_tests();
        at(0., false);
        begin("a", None);
        assert_eq!(frame("a"), None);
        assert!(!begin_close("a", false));
        begin("b", Some(thumb()));
        assert_eq!(frame("c"), None);
        assert!(!begin_close("b", true));
        assert_eq!(frame("b"), None);
        at(0., true);
        begin("d", Some(thumb()));
        assert_eq!(frame("d").map(|f| f.0), Some(1.));
    }
}
