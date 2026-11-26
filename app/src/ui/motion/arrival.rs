//! Not in the pinned client: the arrival of a message this account sends, taken from Discord's native client
//! ("Phoenix", owner decision in `DECISIONS.md`). The pinned build shows a new message at once.
//!
//! Phoenix wraps the row in `MessageArrival` (`discord-desktop-features-ui/src/features_ui/message_arrival.rs`). It
//! keeps three slots per row: `grow` and `fade`, each run from 0 to 1 by a transition of 300 ms with the curve its
//! motion layer numbers 4 (`ease-in-out`, `cubic-bezier(.42, 0, .58, 1)`), and `height`, the row's measured height.
//! The row is drawn `height * grow` tall with its content at the top and not clipped, at opacity `fade`. In a list
//! held at its bottom this pushes the rows above up by the new row's height over the same 300 ms, which is what the
//! recording of a send shows. Under reduced motion Phoenix swaps the transition for an instant one; here the
//! transition is `.full_motion()`, which snaps.
//!
//! The run starts when the row of the local send is first drawn (the pending row, before the server confirms it), and
//! only for a send made while the list was at its bottom. Rows that are not arriving are drawn exactly as before:
//! nothing wraps them.

use super::{Css, Ease, key, value_from};
use dc_state::Id;
use gpui::{AnyElement, IntoElement, ParentElement, Styled, canvas, div, px};
use std::cell::RefCell;
use std::collections::HashMap;

/// `MessageArrival`'s `grow` and `fade`: 300 ms, curve 4 of Phoenix's motion layer (`ease-in-out`).
pub const ARRIVAL: Css = Css::new(300., Ease::EaseInOut).full_motion();

/// How long a recorded arrival is kept. A run takes 300 ms; one whose row stopped being drawn (a channel switch) or
/// never was is forgotten after this, so it cannot replay later.
const FORGET_MS: f64 = 2000.;

#[derive(Clone, Copy)]
struct Arrival {
    /// The motion store's key: the first id of the row, kept when the server's id replaces it.
    key: u64,
    /// When the send was recorded, on the motion clock.
    recorded: f64,
}

#[derive(Default)]
struct Arrivals {
    by_message: HashMap<Id, Arrival>,
    /// The natural height of each arriving row, measured on the previous frame.
    heights: HashMap<u64, f32>,
}

thread_local! {
    static ARRIVALS: RefCell<Arrivals> = RefCell::new(Arrivals::default());
}

fn with<R>(f: impl FnOnce(&mut Arrivals) -> R) -> R {
    ARRIVALS.with(|arrivals| f(&mut arrivals.borrow_mut()))
}

/// Records that the row of `message` (a local send) arrives now.
pub fn begin(message: Id) {
    let now = super::now();
    with(|arrivals| {
        arrivals
            .by_message
            .retain(|_, arrival| now - arrival.recorded < FORGET_MS);
        let live: Vec<u64> = arrivals.by_message.values().map(|a| a.key).collect();
        arrivals.heights.retain(|key, _| live.contains(key));
        arrivals.by_message.insert(
            message,
            Arrival {
                key: key(&("message-arrival", message)),
                recorded: now,
            },
        );
    });
}

/// The server's message `to` took the place of the local row `from`: the run goes on under the new id.
pub fn replaced(from: Id, to: Id) {
    with(|arrivals| {
        if let Some(arrival) = arrivals.by_message.remove(&from) {
            arrivals.by_message.insert(to, arrival);
        }
    });
}

/// Whether `message` has a run going (or recorded and not drawn yet).
pub fn is_arriving(message: Id) -> bool {
    with(|arrivals| arrivals.by_message.contains_key(&message))
}

/// The progress of the row of `message` this frame, from 0 to 1 and eased, or `None` when the row is not arriving.
/// A finished run is forgotten, so the row is drawn plain from then on.
pub fn progress(message: Id) -> Option<f32> {
    let arrival = with(|arrivals| arrivals.by_message.get(&message).copied())?;
    let t = value_from(arrival.key, 0., 1., ARRIVAL);
    if t >= 1. {
        with(|arrivals| {
            arrivals.by_message.remove(&message);
            arrivals.heights.remove(&arrival.key);
        });
        return None;
    }
    Some(t)
}

/// `MessageArrival`'s pose at progress `t`: the share of the row's height that is laid out, and its opacity.
pub fn pose(t: f32) -> (f32, f32) {
    let t = t.clamp(0., 1.);
    (t, t)
}

/// The height the row of `message` takes in the list at progress `t`, from its natural height (unknown before its
/// first frame, when `t` is 0 anyway).
pub fn laid_out_height(natural: Option<f32>, t: f32) -> f32 {
    natural.unwrap_or(0.) * pose(t).0
}

/// Draws `row` (the row of `message`) at progress `t`: its box `height * t` tall, the content at the top of it and
/// not clipped, at opacity `t`. The content is measured on every frame for the next one.
pub fn wrap(message: Id, t: f32, row: AnyElement) -> AnyElement {
    let Some(arrival_key) = with(|arrivals| arrivals.by_message.get(&message).map(|a| a.key))
    else {
        return row;
    };
    let natural = with(|arrivals| arrivals.heights.get(&arrival_key).copied());
    let (_, opacity) = pose(t);
    div()
        .w_full()
        .flex()
        .flex_col()
        .h(px(laid_out_height(natural, t)))
        .child(
            div()
                .relative()
                .w_full()
                .flex_none()
                .opacity(opacity)
                .child(row)
                .child(
                    canvas(
                        move |bounds, _, _| {
                            let height = f32::from(bounds.size.height);
                            with(|arrivals| arrivals.heights.insert(arrival_key, height));
                        },
                        |_, _, _, _| {},
                    )
                    .absolute()
                    .inset_0(),
                ),
        )
        .into_any_element()
}

#[cfg(test)]
pub(crate) fn reset_for_tests() {
    with(|arrivals| *arrivals = Arrivals::default());
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::motion::{begin_frame, freeze, reset_for_tests as reset_motion};

    fn frame(at: f64, reduced: bool) {
        freeze(Some(at));
        begin_frame(reduced);
    }

    #[test]
    fn the_curve_is_ease_in_out_over_300ms() {
        // Phoenix's curve 4: cubic-bezier(.42, 0, .58, 1), symmetric about its middle.
        let ease = |x: f64| ARRIVAL.ease.apply(x);
        assert_eq!(ARRIVAL.duration_ms, 300.);
        assert_eq!(ease(0.), 0.);
        assert_eq!(ease(1.), 1.);
        assert!((ease(0.5) - 0.5).abs() < 1e-6);
        for x in [0.1, 0.25, 0.4] {
            assert!((ease(x) + ease(1. - x) - 1.).abs() < 1e-6, "{x}");
        }
        // Slow at both ends: below the line before the middle.
        assert!(ease(0.1) < 0.1 && ease(0.25) < 0.25);
        assert!((ease(0.25) - 0.129).abs() < 0.002, "{}", ease(0.25));
    }

    #[test]
    fn the_pose_grows_the_height_and_fades_in_together() {
        assert_eq!(pose(0.), (0., 0.));
        assert_eq!(pose(0.5), (0.5, 0.5));
        assert_eq!(pose(1.), (1., 1.));
        assert_eq!(pose(2.), (1., 1.));
        assert_eq!(laid_out_height(None, 0.3), 0.);
        assert_eq!(laid_out_height(Some(26.), 0.5), 13.);
    }

    /// The video of Phoenix (30 fps) put the rows above the new 26px row at these offsets, in px still to travel, on
    /// consecutive frames: 26, 24, 20, 15, 10, then 4, 1, 0 after a repeated frame. The run is placed 35 ms before the
    /// first of them, the best fit of the start.
    #[test]
    fn the_timeline_matches_the_recorded_send_within_a_pixel_and_a_half() {
        reset_motion();
        reset_for_tests();
        frame(0., false);
        begin(7);
        assert_eq!(progress(7), Some(0.));
        let measured = [
            (0, 26.),
            (1, 24.),
            (2, 20.),
            (3, 15.),
            (4, 10.),
            (6, 4.),
            (7, 1.),
            (8, 0.),
        ];
        for (frame_ix, left) in measured {
            let at = 35. + frame_ix as f64 * 1000. / 30.;
            frame(at, false);
            let t = progress(7).unwrap_or(1.);
            let travelled = 26. * (1. - pose(t).0);
            assert!(
                (travelled - left).abs() <= 1.5,
                "frame {frame_ix}: {travelled} vs {left}"
            );
        }
    }

    #[test]
    fn the_run_ends_after_300ms_and_the_row_is_plain_again() {
        reset_motion();
        reset_for_tests();
        frame(0., false);
        begin(1);
        assert_eq!(progress(1), Some(0.));
        frame(150., false);
        let middle = progress(1).expect("running");
        assert!((middle - 0.5).abs() < 0.01, "{middle}");
        frame(300., false);
        assert_eq!(progress(1), None);
        assert!(!is_arriving(1));
        frame(316., false);
        assert_eq!(progress(1), None);
    }

    #[test]
    fn the_confirmed_message_keeps_the_run_of_the_local_row() {
        reset_motion();
        reset_for_tests();
        frame(0., false);
        begin(10);
        let _ = progress(10);
        frame(100., false);
        let before = progress(10).expect("running");
        replaced(10, 99);
        assert!(!is_arriving(10));
        let after = progress(99).expect("still running");
        assert_eq!(before, after);
        frame(200., false);
        assert!(progress(99).expect("running") > after);
    }

    #[test]
    fn reduced_motion_shows_the_row_at_once() {
        reset_motion();
        reset_for_tests();
        frame(0., true);
        begin(5);
        assert_eq!(progress(5), None);
        assert!(!is_arriving(5));
    }

    #[test]
    fn rows_that_never_arrived_are_not_touched() {
        reset_motion();
        reset_for_tests();
        frame(0., false);
        assert_eq!(progress(3), None);
        assert!(!is_arriving(3));
    }

    #[test]
    fn a_send_whose_row_never_shows_is_forgotten() {
        reset_motion();
        reset_for_tests();
        frame(0., false);
        begin(1);
        frame(FORGET_MS + 1., false);
        begin(2);
        assert!(!is_arriving(1));
        assert!(is_arriving(2));
    }

    #[test]
    fn a_run_left_halfway_is_forgotten_and_does_not_replay() {
        reset_motion();
        reset_for_tests();
        frame(0., false);
        begin(1);
        let _ = progress(1);
        frame(100., false);
        assert!(progress(1).is_some_and(|t| t > 0. && t < 1.));
        // The channel changes: the row is not drawn again until much later.
        frame(FORGET_MS + 200., false);
        begin(2);
        assert!(!is_arriving(1));
        assert_eq!(progress(1), None);
    }
}
