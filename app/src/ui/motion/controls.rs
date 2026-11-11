//! Controls. The switch follows module 489718 (`Switch`): a react-spring value `state` runs from 0 to 1 (1 to 0)
//! over 300 ms, 200 ms under reduced motion, on the default linear easing (`animate-always`); the thumb, the
//! background and the border are interpolated off it with the ranges below. The thumb stretches while it travels
//! (`rect` x, y, width, height and rx), unless motion is reduced, when it only slides.

use super::{Css, Ease, css, key, mix, value};
use gpui::Rgba;

/// `D`, `x`, `H` and `I` of module 489718.
const COLOR_RANGE: [f64; 4] = [0., 0.1, 0.9, 1.];
const SLIDE_RANGE: [f64; 6] = [0., 0.1, 0.4, 0.6, 0.9, 1.];
const SHAPE_ON: [f64; 6] = [0., 0.3, 0.5, 0.8, 0.9, 1.];
const SHAPE_OFF: [f64; 6] = [0., 0.1, 0.2, 0.5, 0.7, 1.];

/// react-spring's `interpolate({range, output})` with the default `extend` (the ends are held for colours, and for
/// numbers in this module's uses, which stay inside their ranges).
pub fn interpolate(t: f64, range: &[f64], output: &[f64]) -> f64 {
    debug_assert_eq!(range.len(), output.len());
    if t <= range[0] {
        return output[0];
    }
    for i in 1..range.len() {
        if t <= range[i] {
            let span = range[i] - range[i - 1];
            let at = if span == 0. {
                1.
            } else {
                (t - range[i - 1]) / span
            };
            return output[i - 1] + (output[i] - output[i - 1]) * at;
        }
    }
    output[output.len() - 1]
}

/// The same for colours, premultiplied.
pub fn interpolate_color(t: f64, range: &[f64], output: &[Rgba]) -> Rgba {
    if t <= range[0] {
        return output[0];
    }
    for i in 1..range.len() {
        if t <= range[i] {
            let span = range[i] - range[i - 1];
            let at = if span == 0. {
                1.
            } else {
                (t - range[i - 1]) / span
            };
            return mix(output[i - 1], output[i], at as f32);
        }
    }
    output[output.len() - 1]
}

/// What the switch shows at `state` (0 off, 1 on).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Thumb {
    /// How far along its travel the thumb is, 0 to 1 (official `left` of 1 to 24).
    pub travel: f64,
    /// The rect inside the 24px thumb box: x, y, width, height, rx. At rest 4, 4, 16, 16, 8.
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    pub radius: f64,
}

/// The thumb at `state` while the switch is being turned `on` (or off), with motion `reduced` or not.
pub fn thumb(state: f64, on: bool, reduced: bool) -> Thumb {
    let left = if reduced {
        interpolate(state, &COLOR_RANGE, &[1., 1., 24., 24.])
    } else {
        interpolate(state, &SLIDE_RANGE, &[1., 1., 1., 24., 24., 24.])
    };
    let travel = (left - 1.) / 23.;
    if reduced {
        return Thumb {
            travel,
            x: 4.,
            y: 4.,
            width: 16.,
            height: 16.,
            radius: 8.,
        };
    }
    let shape = |output: [f64; 6]| {
        if on {
            let mut reversed = output;
            reversed.reverse();
            interpolate(state, &SHAPE_ON, &reversed)
        } else {
            interpolate(state, &SHAPE_OFF, &output)
        }
    };
    Thumb {
        travel,
        x: shape([4., 4., 2.5, 1., 2.5, 4.]),
        y: shape([4., 7., 2.5, 7., 2.5, 4.]),
        width: shape([16., 16., 19., 22., 19., 16.]),
        height: shape([16., 10., 19., 10., 19., 16.]),
        radius: shape([8., 5., 9.5, 5., 9.5, 8.]),
    }
}

/// The switch's background: its default colour to the active colour at .9 and the selected colour at 1 (module
/// 489718, `backgroundColor` over `D`).
pub fn switch_background(state: f64, off: Rgba, active: Rgba, on: Rgba) -> Rgba {
    interpolate_color(state, &COLOR_RANGE, &[off, off, active, on])
}

/// The switch's `state` for the switch named `id`, run over 300 ms (200 reduced), linear.
pub fn switch_state(id: &str, on: bool, reduced: bool) -> f64 {
    let spec: Css = css(if reduced { 200. } else { 300. }, Ease::Linear);
    f64::from(value(key(&("switch", id)), if on { 1. } else { 0. }, spec))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interpolation_holds_the_ends_and_runs_straight_between_stops() {
        let range = [0., 0.1, 0.4, 0.6, 0.9, 1.];
        let output = [1., 1., 1., 24., 24., 24.];
        assert_eq!(interpolate(0.05, &range, &output), 1.);
        assert_eq!(interpolate(0.5, &range, &output), 12.5);
        assert_eq!(interpolate(0.95, &range, &output), 24.);
    }

    #[test]
    fn the_thumb_stretches_while_it_travels_and_is_a_circle_at_rest() {
        for on in [false, true] {
            for state in [0., 1.] {
                let thumb = thumb(state, on, false);
                assert_eq!(
                    (thumb.x, thumb.y, thumb.width, thumb.height, thumb.radius),
                    (4., 4., 16., 16., 8.)
                );
            }
        }
        // Turning on, half way: the widest point of the stretch, 22 by 10.
        let mid = thumb(0.5, true, false);
        assert_eq!((mid.width, mid.height, mid.x), (22., 10., 1.));
        assert!((mid.travel - (12.5 - 1.) / 23.).abs() < 1e-9);
        // Turning off, half way: the same shape, from the other table.
        let mid = thumb(0.5, false, false);
        assert_eq!((mid.width, mid.height), (22., 10.));
    }

    #[test]
    fn reduced_motion_only_slides_the_thumb() {
        let mid = thumb(0.5, true, true);
        assert_eq!((mid.width, mid.height, mid.radius), (16., 16., 8.));
        assert!((mid.travel - 0.5).abs() < 1e-9);
        assert_eq!(thumb(0.05, true, true).travel, 0.);
        assert_eq!(thumb(0.95, true, true).travel, 1.);
    }

    #[test]
    fn the_state_runs_for_300_ms_and_200_reduced() {
        super::super::reset_for_tests();
        super::super::freeze(Some(0.));
        super::super::begin_frame(false);
        switch_state("a", false, false);
        switch_state("b", false, true);
        super::super::freeze(Some(10.));
        super::super::begin_frame(false);
        switch_state("a", true, false);
        switch_state("b", true, true);
        super::super::freeze(Some(160.));
        super::super::begin_frame(false);
        assert!((switch_state("a", true, false) - 0.5).abs() < 1e-6);
        assert!((switch_state("b", true, true) - 0.75).abs() < 1e-6);
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Radio and checkbox: the CSS keyframe animations of stylesheet 384209
// ---------------------------------------------------------------------------------------------------------------

use super::{keyframes, toggled};

/// What an animated part shows: opacity and uniform scale.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Look {
    pub opacity: f32,
    pub scale: f32,
}

const fn look(opacity: f32, scale: f32) -> Look {
    Look { opacity, scale }
}

fn run(
    id: u64,
    on: bool,
    in_ms: f64,
    out_ms: f64,
    rest: (Look, Look),
    frame: impl Fn(bool, f32) -> Look,
) -> Look {
    let within = if on { in_ms } else { out_ms };
    match toggled(id, on, within) {
        Some(ms) => frame(on, (ms / within) as f32),
        None => {
            if on {
                rest.0
            } else {
                rest.1
            }
        }
    }
}

const FILL_EASE: Ease = Ease::Bezier(0.33, 0., 0.67, 1.);

/// `outerRadioFill_oJuRbG`: `fillIn` 225 ms `cubic-bezier(.33,0,.67,1)` forwards, `fillOut` .16 s both.
pub fn radio_fill(id: u64, selected: bool) -> Look {
    run(
        id,
        selected,
        225.,
        160.,
        (look(1., 1.), look(0., 0.)),
        |selected, t| {
            if selected {
                look(
                    keyframes(&[(0., 0.), (0.01, 1.), (1., 1.)], FILL_EASE, t),
                    keyframes(
                        &[(0., 0.), (0.35, 0.33), (0.4, 1.15), (1., 1.)],
                        FILL_EASE,
                        t,
                    ),
                )
            } else {
                look(
                    keyframes(&[(0., 1.), (0.99, 1.), (1., 0.)], FILL_EASE, t),
                    keyframes(&[(0., 1.), (1., 0.)], FILL_EASE, t),
                )
            }
        },
    )
}

/// `innerDotRadio_oJuRbG`: `dotIn` 333 ms `cubic-bezier(.26,.21,.67,1)` forwards, `dotOut` .25 s
/// `cubic-bezier(.33,0,.25,1)` both.
pub fn radio_dot(id: u64, selected: bool) -> Look {
    run(
        id,
        selected,
        333.,
        250.,
        (look(1., 1.), look(0., 0.)),
        |selected, t| {
            if selected {
                look(
                    1.,
                    keyframes(
                        &[(0., 0.), (0.4, 1.2), (0.75, 1.), (1., 1.)],
                        Ease::Bezier(0.26, 0.21, 0.67, 1.),
                        t,
                    ),
                )
            } else {
                let ease = Ease::Bezier(0.33, 0., 0.25, 1.);
                look(
                    keyframes(&[(0., 1.), (0.99, 1.), (1., 0.)], ease, t),
                    keyframes(&[(0., 1.), (0.4, 1.), (1., 0.)], ease, t),
                )
            }
        },
    )
}

/// `checkStroke_vTUVyc`: `checkDraw` .3 s `cubic-bezier(.65,0,.83,.83)` both, `checkUndraw` .12 s
/// `cubic-bezier(.65,0,.83,.84)` backwards.
pub fn check_stroke(id: u64, checked: bool) -> Look {
    run(
        id,
        checked,
        300.,
        120.,
        (look(1., 1.), look(0., 1.)),
        |checked, t| {
            if checked {
                let ease = Ease::Bezier(0.65, 0., 0.83, 0.83);
                look(
                    keyframes(&[(0., 0.), (0.25, 0.), (0.26, 1.), (1., 1.)], ease, t),
                    keyframes(
                        &[(0., 0.7), (0.25, 0.7), (0.26, 0.7), (0.75, 1.1), (1., 1.)],
                        ease,
                        t,
                    ),
                )
            } else {
                let ease = Ease::Bezier(0.65, 0., 0.83, 0.84);
                look(
                    keyframes(&[(0., 1.), (0.25, 1.), (0.26, 0.), (1., 0.)], ease, t),
                    keyframes(&[(0., 1.), (0.25, 0.7), (0.26, 0.7), (1., 0.7)], ease, t),
                )
            }
        },
    )
}

/// `dot_vTUVyc`, the 2.2 px circle at the centre: `dotGrow` .3 s forwards, `dotShrink` .12 s backwards.
pub fn check_dot(id: u64, checked: bool) -> Look {
    run(
        id,
        checked,
        300.,
        120.,
        (look(0., 1.), look(0., 1.)),
        |checked, t| {
            if checked {
                let ease = Ease::Bezier(0.65, 0., 0.83, 0.83);
                look(
                    keyframes(&[(0., 1.), (0.25, 1.), (0.26, 0.), (1., 0.)], ease, t),
                    keyframes(&[(0., 1.), (0.25, 3.), (1., 3.)], ease, t),
                )
            } else {
                let ease = Ease::Bezier(0.65, 0., 0.83, 0.84);
                look(
                    keyframes(&[(0., 0.), (0.25, 0.), (0.26, 1.), (1., 1.)], ease, t),
                    keyframes(&[(0., 2.), (0.25, 2.), (0.26, 2.), (1., 1.)], ease, t),
                )
            }
        },
    )
}

/// The marks inside a checkbox indicator (`checkboxIndicator_vTUVyc`), absolutely placed in its 18 px padding box:
/// the 2.2 px dot that bursts when it gets checked, and the check, which draws in and out. `check` is the check
/// icon; `tint` colours the dot (`--checkbox-icon-active`). Place it inside an element that has an id.
pub fn checkbox_marks(checked: bool, check: impl gpui::IntoElement, tint: gpui::Hsla) -> gpui::Div {
    use gpui::{InteractiveElement as _, ParentElement as _, Styled as _, div, px};
    div()
        .absolute()
        .left(px(0.))
        .top(px(0.))
        .size(px(18.))
        .child(
            div()
                .id("check-dot")
                .absolute()
                .left(px(9. - 0.99))
                .top(px(9. - 0.99))
                .size(px(1.98))
                .rounded_full()
                .bg(tint)
                .style_transition(move |style, id, _, _| {
                    style.opacity = Some(check_dot(id, checked).opacity);
                })
                .paint_scale_with(move |id, _| {
                    (check_dot(id, checked).scale, gpui::point(0.5, 0.5))
                }),
        )
        .child(
            div()
                .id("check-stroke")
                .absolute()
                .left(px(0.))
                .top(px(0.))
                .child(check)
                .style_transition(move |style, id, _, _| {
                    style.opacity = Some(check_stroke(id, checked).opacity);
                })
                .paint_scale_with(move |id, _| {
                    (check_stroke(id, checked).scale, gpui::point(0.5, 0.5))
                }),
        )
}
