//! Not in the pinned client: the ghost a deleted message leaves, taken from Discord's native client ("Phoenix",
//! `DECISIONS.md`). The pinned build drops the row at once.
//!
//! Phoenix's `DeletedGhost` (`discord-desktop-chat/src/chat/row_motion.rs`, `message-delete`) keeps drawing the
//! deleted row for a moment:
//! * its opacity is `1 - p`, where `p` runs a transition of 120 ms with curve 3 (`ease-out`);
//! * its height is the row's measured height times `s`, clipped, and its scale `0.96 + 0.04 * s`, where `s` is a
//!   spring from 1 to 0 (`{tension: 800, friction: 50, mass: 1}`, react-spring's integration) that Phoenix samples
//!   once per millisecond into a table of at most 500 values;
//! * the ghost is gone at the first millisecond whose sample is 0 or less.
//!
//! Under reduced motion Phoenix drops the ghost at once, and so does this.
//!
//! Here the delete event of a message in the open channel is held while its ghost runs, so the row is still drawn
//! from the message store, and the event is applied when the ghost ends (`ui/departure.rs`). A day divider whose only
//! message is leaving goes with it.

use super::{Ease, key};
use dc_state::Id;
use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyElement, InteractiveElement as _, IntoElement, ParentElement, Styled, canvas, div, px,
};
use std::cell::RefCell;
use std::collections::HashMap;

/// `DeletedGhost`'s opacity transition: 120 ms, curve 3 (`ease-out`).
pub const FADE_MS: f64 = 120.;
pub const FADE_EASE: Ease = Ease::EaseOut;
/// The spring of the height and the scale.
pub const TENSION: f64 = 800.;
pub const FRICTION: f64 = 50.;
pub const MASS: f64 = 1.;
/// Phoenix samples the spring this many times, one per millisecond.
const SAMPLES: usize = 500;
/// The scale the ghost shrinks to.
pub const SCALE_TO: f32 = 0.96;
/// A held delete whose row is never drawn is applied after this long (real time).
pub const FALLBACK_MS: u64 = 2000;

thread_local! {
    static SPRING: Vec<f64> = sample_spring();
    static GHOSTS: RefCell<Ghosts> = RefCell::new(Ghosts::default());
}

#[derive(Default)]
struct Ghosts {
    /// The message, the channel, the held delete event and when it was received on the motion clock.
    by_message: HashMap<Id, Ghost>,
    /// The natural height of each ghost part (message row, day divider), measured while it was drawn.
    heights: HashMap<u64, f32>,
}

struct Ghost {
    channel: Id,
    event: serde_json::Value,
    start: f64,
}

/// The spring from 1 to 0, one sample per millisecond (react-spring's `advance`: per millisecond, the force
/// `-tension * 1e-6 * x - friction * 1e-3 * v` over the mass, then the velocity, then the position).
fn sample_spring() -> Vec<f64> {
    let mut table = Vec::with_capacity(SAMPLES + 1);
    let (mut x, mut v) = (1., 0.);
    table.push(x);
    for _ in 0..SAMPLES {
        let a = (-TENSION * 1e-6 * x - FRICTION * 1e-3 * v) / MASS;
        v += a;
        x += v;
        table.push(x);
    }
    table
}

/// The spring's sample `ms` milliseconds after the delete, or `None` once the ghost is gone.
pub fn spring_at(ms: f64) -> Option<f64> {
    let index = ms.max(0.).floor() as usize;
    SPRING.with(|table| table.get(index).copied().filter(|s| *s > 0.))
}

/// How the ghost is drawn `ms` milliseconds after the delete: (height factor, scale, opacity), or `None` once it is
/// gone.
pub fn pose(ms: f64) -> Option<(f32, f32, f32)> {
    let s = spring_at(ms)?.clamp(0., 1.) as f32;
    let p = FADE_EASE.apply((ms / FADE_MS).clamp(0., 1.)) as f32;
    Some((s, SCALE_TO + (1. - SCALE_TO) * s, 1. - p))
}

/// Holds the delete `event` of `message` (in `channel`) while its ghost runs. False when it is already held.
pub fn begin(message: Id, channel: Id, event: serde_json::Value) -> bool {
    let start = super::now();
    GHOSTS.with(|ghosts| {
        let mut ghosts = ghosts.borrow_mut();
        if ghosts.by_message.contains_key(&message) {
            return false;
        }
        ghosts.by_message.insert(
            message,
            Ghost {
                channel,
                event,
                start,
            },
        );
        true
    })
}

/// Whether `message` is leaving.
pub fn is_leaving(message: Id) -> bool {
    GHOSTS.with(|ghosts| ghosts.borrow().by_message.contains_key(&message))
}

/// The pose of the ghost of `message` this frame, or `None` when it is not leaving or its ghost has ended.
pub fn frame(message: Id) -> Option<(f32, f32, f32)> {
    let start = GHOSTS.with(|ghosts| ghosts.borrow().by_message.get(&message).map(|g| g.start))?;
    pose(super::now() - start)
}

/// Ends the ghost of `message`: the held delete event to apply now, if it was still held.
pub fn take(message: Id) -> Option<serde_json::Value> {
    GHOSTS.with(|ghosts| {
        let mut ghosts = ghosts.borrow_mut();
        let ghost = ghosts.by_message.remove(&message)?;
        ghosts.heights.remove(&part_key(message, false));
        ghosts.heights.remove(&part_key(message, true));
        Some(ghost.event)
    })
}

/// The ghosts held for channels other than `channel` (the open one): their events are applied at once.
pub fn other_channels(channel: Option<Id>) -> Vec<Id> {
    GHOSTS.with(|ghosts| {
        ghosts
            .borrow()
            .by_message
            .iter()
            .filter(|(_, ghost)| Some(ghost.channel) != channel)
            .map(|(id, _)| *id)
            .collect()
    })
}

fn part_key(message: Id, divider: bool) -> u64 {
    key(&("message-ghost", message, divider))
}

/// Draws `row` (the message row of `message`, or its day divider) as the ghost pose: the box `height * s` tall and
/// clipping, the content at its top, scaled about its centre and faded. Before the part's height is known (the
/// ghost's first frame) the box keeps its natural height.
pub fn wrap(
    message: Id,
    divider: bool,
    (s, scale, opacity): (f32, f32, f32),
    row: AnyElement,
) -> AnyElement {
    let part = part_key(message, divider);
    let natural = GHOSTS.with(|ghosts| ghosts.borrow().heights.get(&part).copied());
    let content = div()
        .relative()
        .w_full()
        .flex_none()
        .opacity(opacity)
        .paint_scale(scale, gpui::point(0.5, 0.5))
        .child(row)
        .child(
            canvas(
                move |bounds, _, _| {
                    let height = f32::from(bounds.size.height);
                    GHOSTS.with(|ghosts| ghosts.borrow_mut().heights.insert(part, height));
                },
                |_, _, _, _| {},
            )
            .absolute()
            .inset_0(),
        );
    div()
        .w_full()
        .flex()
        .flex_col()
        .overflow_hidden()
        .when_some(natural, |this, natural| this.h(px(natural * s)))
        .child(content)
        .into_any_element()
}

#[cfg(test)]
pub(crate) fn reset_for_tests() {
    GHOSTS.with(|ghosts| *ghosts.borrow_mut() = Ghosts::default());
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::motion::{begin_frame, freeze, reset_for_tests as reset_motion};

    #[test]
    fn the_spring_leaves_1_and_reaches_0_after_about_210ms() {
        assert_eq!(spring_at(0.), Some(1.));
        let gone = (0..600)
            .find(|ms| spring_at(*ms as f64).is_none())
            .expect("ends");
        assert!((200..=220).contains(&gone), "{gone}");
        // Monotonic until it is gone (the spring is a little under critical damping).
        let mut last = 2.;
        for ms in 0..gone {
            let s = spring_at(ms as f64).expect("running");
            assert!(s < last || ms == 0, "{ms}");
            last = s;
        }
    }

    #[test]
    fn the_spring_is_react_springs_integration() {
        // Two steps by hand: a = -800e-6 * 1 = -0.0008; v = -0.0008; x = 0.9992; then
        // a = -800e-6 * 0.9992 - 50e-3 * -0.0008 = -0.00075936; v = -0.00155936; x = 0.99764064.
        assert!((spring_at(1.).unwrap() - 0.9992).abs() < 1e-12);
        assert!((spring_at(2.).unwrap() - 0.99764064).abs() < 1e-12);
    }

    #[test]
    fn the_pose_fades_over_120ms_and_shrinks_to_0_96() {
        let (s, scale, opacity) = pose(0.).unwrap();
        assert_eq!((s, scale, opacity), (1., 1., 1.));
        let (_, _, half) = pose(60.).unwrap();
        let expected = 1. - Ease::EaseOut.apply(0.5) as f32;
        assert!((half - expected).abs() < 1e-6, "{half}");
        let (s, scale, opacity) = pose(150.).unwrap();
        assert_eq!(opacity, 0.);
        assert!(s > 0. && s < 0.2, "{s}");
        assert!((scale - (0.96 + 0.04 * s)).abs() < 1e-6);
        assert_eq!(pose(400.), None);
    }

    /// The recording of a delete in Phoenix (30 fps): the ghost's text contrast on the frames after the delete was
    /// 0.63, 0.22 and 0.02; the opacity curve gives the same within 0.05 for a delete 0.5 ms after the last full frame.
    #[test]
    fn the_fade_matches_the_recording() {
        for (frame, measured) in [(1., 0.63), (2., 0.219), (3., 0.016)] {
            let ms = frame * 1000. / 30. - 0.5;
            let (_, _, opacity) = pose(ms).unwrap_or((0., 0., 0.));
            assert!(
                (opacity - measured).abs() < 0.05,
                "frame {frame}: {opacity} vs {measured}"
            );
        }
    }

    #[test]
    fn a_held_delete_is_given_back_once() {
        reset_motion();
        reset_for_tests();
        freeze(Some(0.));
        begin_frame(false);
        let event = serde_json::json!({"id": "5", "channel_id": "9"});
        assert!(begin(5, 9, event.clone()));
        assert!(!begin(5, 9, event.clone()));
        assert!(is_leaving(5));
        freeze(Some(100.));
        begin_frame(false);
        assert!(frame(5).is_some());
        freeze(Some(400.));
        begin_frame(false);
        assert_eq!(frame(5), None);
        assert_eq!(take(5), Some(event));
        assert_eq!(take(5), None);
        assert!(!is_leaving(5));
    }

    #[test]
    fn ghosts_of_other_channels_are_listed() {
        reset_motion();
        reset_for_tests();
        begin(1, 10, serde_json::Value::Null);
        begin(2, 20, serde_json::Value::Null);
        assert_eq!(other_channels(Some(10)), vec![2]);
    }
}
