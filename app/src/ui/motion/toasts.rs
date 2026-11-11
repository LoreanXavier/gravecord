//! Toast enter and leave (module 616667 with 435871, stylesheet 765531).
//!
//! `.toast` runs `animation: 300ms linear both` (`--toast-animation-duration-ms: 300`). `.enter` is
//! `toast-slide-in` under `.full-motion` (from opacity 0 and `translateY(-100%)`, +100% for a bottom toast to
//! opacity 1 and no offset), otherwise `toast-fade-in` (opacity only). `.leave` plays the same keyframes backwards
//! and the toast is removed when the animation ends. `.delayed` (`animation-delay: --toast-queue-enter-delay-ms`, 400 ms) holds the entering toast at its first keyframe
//! while the replaced one leaves (module 616667: `leaving` keeps the old toast, `isDelayed` is set when a toast was shown).

use std::cell::RefCell;
use std::time::Instant;

/// `--toast-animation-duration-ms`.
pub const DURATION_MS: f64 = 300.;

/// `--toast-queue-enter-delay-ms`.
pub const QUEUE_ENTER_DELAY_MS: f64 = 400.;

/// Height of a one-line toast (24 padding + 2 border + 20 line), what `translateY(-100%)` moves by.
pub const HEIGHT: f32 = 46.;

/// One toast to draw: message, success, opacity and the vertical offset in px.
pub type Drawn = (String, bool, f32, f32);

#[derive(Clone)]
struct Held {
    message: String,
    success: bool,
    id: Instant,
}

#[derive(Default)]
struct State {
    entry: Option<(Held, f64, f64)>,
    leaving: Option<(Held, f64)>,
}

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State::default());
}

/// Opacity and the vertical offset in px for a top toast at `progress` (0 hidden, 1 shown).
pub fn pose(progress: f32, reduced: bool) -> (f32, f32) {
    let progress = progress.clamp(0., 1.);
    let offset = if reduced {
        0.
    } else {
        -(1. - progress) * HEIGHT
    };
    (progress, offset)
}

fn linear(elapsed: f64) -> f32 {
    (elapsed / DURATION_MS).clamp(0., 1.) as f32
}

/// The toasts to draw this frame, the leaving one first, from the toast that is wanted (its message, kind and
/// identity). A changed identity retires the shown toast (it plays its leave) and starts the new one, after the
/// queue delay when one was shown.
pub fn frames(current: Option<(&str, bool, Instant)>) -> Vec<Drawn> {
    let now = super::now();
    let reduced = super::reduced();
    STATE.with(|state| {
        let mut state = state.borrow_mut();
        let wanted = current.map(|(message, success, id)| Held {
            message: message.to_owned(),
            success,
            id,
        });
        let same = match (&state.entry, &wanted) {
            (Some((held, ..)), Some(wanted)) => held.id == wanted.id,
            (None, None) => true,
            _ => false,
        };
        if !same {
            let delayed = state.entry.is_some();
            if let Some((held, ..)) = state.entry.take() {
                state.leaving = Some((held, now));
            }
            state.entry =
                wanted.map(|held| (held, now, if delayed { QUEUE_ENTER_DELAY_MS } else { 0. }));
        }
        let mut out = Vec::new();
        if let Some((held, since)) = state.leaving.clone() {
            let progress = 1. - linear(now - since);
            if progress <= 0. {
                state.leaving = None;
            } else {
                let (opacity, offset) = pose(progress, reduced);
                out.push((held.message, held.success, opacity, offset));
            }
        }
        if let Some((held, since, delay)) = state.entry.clone() {
            let (opacity, offset) = pose(linear(now - since - delay), reduced);
            out.push((held.message, held.success, opacity, offset));
        }
        let moving = state.leaving.is_some()
            || state
                .entry
                .as_ref()
                .is_some_and(|(_, since, delay)| now - since - delay < DURATION_MS);
        if moving {
            super::keep_animating();
        }
        out
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::motion::{begin_frame, freeze, reset_for_tests};

    fn at(ms: f64, current: Option<(&str, bool, Instant)>) -> Vec<Drawn> {
        freeze(Some(ms));
        begin_frame(false);
        frames(current)
    }

    #[test]
    fn slides_down_from_its_own_height_and_fades() {
        assert_eq!(pose(0., false), (0., -HEIGHT));
        assert_eq!(pose(1., false), (1., 0.));
        assert_eq!(pose(0.5, false), (0.5, -HEIGHT / 2.));
    }

    #[test]
    fn reduced_motion_only_fades() {
        assert_eq!(pose(0.5, true), (0.5, 0.));
    }

    #[test]
    fn enter_takes_300ms_and_the_leave_is_the_reverse() {
        reset_for_tests();
        let id = Instant::now();
        assert_eq!(at(0., Some(("hi", false, id)))[0].2, 0.);
        let half = at(150., Some(("hi", false, id)));
        assert!((half[0].2 - 0.5).abs() < 0.01);
        assert_eq!(at(400., Some(("hi", false, id)))[0].2, 1.);
        assert_eq!(at(500., None).len(), 1);
        assert_eq!(at(650., None)[0].2, 0.5);
        assert!(at(900., None).is_empty());
    }

    #[test]
    fn a_replacing_toast_waits_400ms_while_the_old_one_leaves() {
        STATE.with(|state| *state.borrow_mut() = State::default());
        reset_for_tests();
        let first = Instant::now();
        let second = first + std::time::Duration::from_millis(1);
        at(0., Some(("a", false, first)));
        at(500., Some(("a", false, first)));
        let drawn = at(600., Some(("b", false, second)));
        assert_eq!(drawn.len(), 2);
        assert_eq!(drawn[0].0, "a");
        assert_eq!(drawn[1].2, 0.);
        // 300 ms in: the old one is gone, the new one has not begun (its delay is 400 ms).
        let drawn = at(900., Some(("b", false, second)));
        assert_eq!(drawn.len(), 1);
        assert_eq!(drawn[0].2, 0.);
        let drawn = at(1150., Some(("b", false, second)));
        assert!((drawn[0].2 - 0.5).abs() < 0.01, "{}", drawn[0].2);
    }
}
