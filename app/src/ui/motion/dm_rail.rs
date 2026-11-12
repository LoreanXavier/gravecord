//! The direct message items of the server rail enter and leave (`tC` in module 396655, a react-spring `Controller`
//! per item with `getRailItemSpringConfig`): `height` and `opacity` run for 150 ms (`duration: 150`, spring
//! settings that a duration replaces by a linear timing), `scale` is a spring `{friction: 28, tension: 600}`. An
//! item enters from 0 to 1 on all three and leaves back to 0. The items present when the rail first shows
//! (`componentWillAppear`) and every change while the window is unfocused (`immediate: !isFocused()`) skip it.

use super::{
    Css, Drive, Ease, SpringConfig, enter_between, key, settled, spring_from, value_from, with,
};
use std::cell::RefCell;
use std::collections::HashSet;

/// `{friction: 28, tension: 600}`.
pub const SCALE: SpringConfig = SpringConfig::new(600., 28.);

/// `height` and `opacity`: `duration: 150`.
pub const FADE: Css = Css::new(150., Ease::Linear);

/// How an item is drawn this frame: the fraction of its height, its opacity and its scale.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pose {
    pub height: f32,
    pub opacity: f32,
    pub scale: f32,
}

impl Pose {
    pub const SHOWN: Pose = Pose {
        height: 1.,
        opacity: 1.,
        scale: 1.,
    };
}

#[derive(Default)]
struct State {
    started: bool,
    order: Vec<u64>,
    immediate: HashSet<u64>,
    leaving: HashSet<u64>,
}

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State::default());
}

fn keys(id: u64) -> (u64, u64) {
    (key(&("dm-rail-fade", id)), key(&("dm-rail-scale", id)))
}

/// The items to draw, in order, with whether each is leaving and its pose. `ids` are the items the rail holds now.
pub fn step(ids: &[u64]) -> Vec<(u64, bool, Pose)> {
    let focused = super::window_active();
    STATE.with(|state| {
        let mut state = state.borrow_mut();
        if !state.started && !ids.is_empty() {
            state.started = true;
            state.immediate.extend(ids.iter().copied());
        }
        // Items that went away start their leave where they stood.
        let previous = state.order.clone();
        for id in &previous {
            if !ids.contains(id) && !state.leaving.contains(id) && state.started {
                if focused {
                    state.leaving.insert(*id);
                }
                state.immediate.remove(id);
            }
        }
        state.leaving.retain(|id| !ids.contains(id));
        // Display order: the current items, each leaving one back at its old place.
        let mut display: Vec<u64> = ids.to_vec();
        for (index, id) in previous.iter().enumerate() {
            if state.leaving.contains(id) && !display.contains(id) {
                display.insert(index.min(display.len()), *id);
            }
        }
        let mut out = Vec::new();
        let mut done = Vec::new();
        for id in display {
            let (fade, scale) = keys(id);
            if state.leaving.contains(&id) {
                let height = value_from(fade, 1., 0., FADE);
                let spring = spring_from(scale, 1., 0., SCALE);
                if height <= 0.0001 && settled(&[fade, scale]) {
                    done.push(id);
                    continue;
                }
                out.push((
                    id,
                    true,
                    Pose {
                        height,
                        opacity: height,
                        scale: spring,
                    },
                ));
            } else if state.immediate.contains(&id) || !focused {
                // An item that came while the window was away is there when the window is back: it does not enter
                // again at every return.
                state.immediate.insert(id);
                out.push((id, false, Pose::SHOWN));
            } else {
                let fraction = enter_between(fade, 0., 1., Drive::Css(FADE));
                let spring = enter_between(scale, 0., 1., Drive::Spring(SCALE));
                out.push((
                    id,
                    false,
                    Pose {
                        height: fraction,
                        opacity: fraction,
                        scale: spring,
                    },
                ));
            }
        }
        for id in done {
            state.leaving.remove(&id);
            let (fade, scale) = keys(id);
            with(|motion| {
                motion.tweens.remove(&fade);
                motion.springs.remove(&scale);
            });
        }
        state.order = out.iter().map(|(id, _, _)| *id).collect();
        let held = state.order.clone();
        state.immediate.retain(|id| held.contains(id));
        out
    })
}

#[cfg(test)]
pub(crate) fn reset() {
    STATE.with(|state| *state.borrow_mut() = State::default());
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::motion::{begin_frame, freeze, reset_for_tests};

    fn at(ms: f64, ids: &[u64]) -> Vec<(u64, bool, Pose)> {
        freeze(Some(ms));
        begin_frame(false);
        step(ids)
    }

    #[test]
    fn the_first_items_appear_at_once_and_a_new_one_grows_in() {
        reset_for_tests();
        reset();
        assert_eq!(at(0., &[1])[0].2, Pose::SHOWN);
        at(100., &[1]);
        let first = at(200., &[1, 2]);
        assert_eq!(first[1].2.height, 0.);
        let mid = at(275., &[1, 2]);
        assert!((mid[1].2.height - 0.5).abs() < 0.05, "{:?}", mid[1]);
        assert!(mid[1].2.scale > 0.5, "{:?}", mid[1]);
        let end = at(700., &[1, 2]);
        assert_eq!(end[1].2, Pose::SHOWN);
    }

    #[test]
    fn a_removed_item_shrinks_away_where_it_stood() {
        reset_for_tests();
        reset();
        at(0., &[1, 2, 3]);
        at(100., &[1, 2, 3]);
        let out = at(116., &[1, 3]);
        assert_eq!(out.len(), 3);
        assert_eq!(out[1].0, 2);
        assert!(out[1].1);
        let mid = at(191., &[1, 3]);
        assert!(
            mid[1].1 && (mid[1].2.height - 0.5).abs() < 0.12,
            "{:?}",
            mid[1]
        );
        let gone = at(800., &[1, 3]);
        assert_eq!(gone.len(), 2);
    }
}
