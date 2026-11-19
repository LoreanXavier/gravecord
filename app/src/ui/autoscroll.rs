//! Middle-button autoscroll. The official client is a Chromium page, and on Windows Chromium scrolls what is under
//! the pointer when the middle button is pressed there: the further the pointer is moved from the point of the
//! press, the faster. The rules are those of Blink's `autoscroll_controller.cc`:
//!
//! - nothing moves within 15 px of the press point, on each axis by itself (`kNoMiddleClickAutoscrollRadius`);
//! - the velocity on an axis is `|distance| ^ 2.2 * 0.000008`, toward the pointer (`kExponent`, `kMultiplier`). The
//!   browser turns it into a fling on a `FixedVelocityCurve` (`ui/events/gestures/fixed_velocity_curve.cc`), whose
//!   offset is `velocity * seconds * 5000` (`kConstantMultiplier`): 5 times the velocity in pixels a millisecond;
//! - a press released after the pointer left the 15 px (`kMiddleClickHolding`) ends it at the release; a press
//!   released in place (`kMiddleClickToggled`) leaves it on until the next press of any button, a key or the wheel.
//!
//! The pointer is one of Chromium's panning cursors while it is on (`MiddleClickAutoscrollCursor`, see [`cursor`]):
//! the round mark with arrows at rest (its centre, the hot spot, is where the button went down) and a direction arrow
//! once the pointer is out of the 15 px. Blink paints nothing at the press point; that round cursor is the only
//! mark, and it moves with the pointer like Chromium's. gpui draws the cursors from Chromium's own images
//! (`ui/resources/cursors/pan_*.cur`, in the gpui patch).
//!
//! Which axes scroll: Blink walks up from the box under the press for one that scrolls vertically and one that scrolls
//! horizontally (`overflow` `scroll` or `auto` with content past the edge; `hidden` does not count). Discord's
//! scrollers are made by module 751591, whose default "vertical" orientation is `overflowY: scroll|auto, overflowX:
//! hidden`, so the scrollers here (chat, channel, DM and member lists, settings pages, popouts) scroll vertically only:
//! the steps carry no horizontal part (a vertical gpui scroller would turn it into a vertical one) and the cursors are
//! the vertical set.

use gpui::{
    CursorStyle, DispatchPhase, IntoElement, KeyDownEvent, MouseButton, MouseDownEvent,
    MouseMoveEvent, MouseUpEvent, Pixels, PlatformInput, Point, ScrollDelta, ScrollEvent,
    ScrollSource, Styled, TouchPhase, point, px,
};
use std::cell::RefCell;
use std::rc::Rc;
use std::time::Instant;

/// `kNoMiddleClickAutoscrollRadius`.
const RADIUS: f32 = 15.;
/// `kExponent`.
const EXPONENT: f32 = 2.2;
/// `kMultiplier`, without its sign.
const MULTIPLIER: f32 = 0.000_008;
/// `FixedVelocityCurve`'s `kConstantMultiplier` (5000 a second), per millisecond.
const CURVE_PER_MS: f32 = 5.;
/// The longest step one frame scrolls for, so a stalled frame does not jump.
const LONGEST_STEP_MS: f32 = 50.;

/// The axes the scroller under the press scrolls on (Blink's `vertical_autoscroll_layout_box_` and
/// `horizontal_autoscroll_layout_box_`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Axes {
    pub vertical: bool,
    pub horizontal: bool,
}

/// Every scroller here is Discord's default vertical one (module 751591): see the module notes.
const SCROLLER_AXES: Axes = Axes {
    vertical: true,
    horizontal: false,
};

pub(super) struct Autoscroll {
    origin: Point<Pixels>,
    pointer: Point<Pixels>,
    axes: Axes,
    /// The pointer left the radius while the button was down: the release ends it.
    holding: bool,
    /// The button was released in place: it stays on until the next press, key or wheel turn.
    toggled: bool,
    last: Instant,
    /// The step this module is sending now, which its own wheel listener must not take for the user's.
    sending: bool,
    /// The cursor the last frame showed, for the harness.
    cursor: CursorStyle,
}

pub(super) type Shared = Rc<RefCell<Option<Autoscroll>>>;

thread_local! {
    static KEY_INTERCEPTOR: RefCell<Option<gpui::Subscription>> = const { RefCell::new(None) };
}

/// Blink's velocity on one axis for a pointer `distance` px from the press point (`kMultiplier` is negative: a
/// pointer below the press point gives a negative velocity, which scrolls toward the end of the content).
pub(super) fn velocity(distance: f32) -> f32 {
    if distance.abs() <= RADIUS {
        return 0.;
    }
    -distance.signum() * distance.abs().powf(EXPONENT) * MULTIPLIER
}

/// What the fling scrolls in a millisecond for that velocity, in gpui's wheel sign (negative scrolls toward the end).
pub(super) fn pixels_per_ms(distance: f32) -> f32 {
    velocity(distance) * CURVE_PER_MS
}

/// Blink's `MiddleClickAutoscrollCursor` for the pointer `away` from the press point.
pub(super) fn cursor(away: Point<f32>, axes: Axes) -> CursorStyle {
    let (vx, vy) = (velocity(away.x), velocity(away.y));
    let east = vx < 0.;
    let west = vx > 0.;
    let north = vy > 0.;
    let south = vy < 0.;
    if north && axes.vertical {
        if axes.horizontal {
            if east {
                return CursorStyle::PanNorthEast;
            }
            if west {
                return CursorStyle::PanNorthWest;
            }
        }
        return CursorStyle::PanNorth;
    }
    if south && axes.vertical {
        if axes.horizontal {
            if east {
                return CursorStyle::PanSouthEast;
            }
            if west {
                return CursorStyle::PanSouthWest;
            }
        }
        return CursorStyle::PanSouth;
    }
    if east && axes.horizontal {
        return CursorStyle::PanEast;
    }
    if west && axes.horizontal {
        return CursorStyle::PanWest;
    }
    match (axes.vertical, axes.horizontal) {
        (true, false) => CursorStyle::PanMiddleVertical,
        (false, true) => CursorStyle::PanMiddleHorizontal,
        _ => CursorStyle::PanMiddle,
    }
}

/// The step for `elapsed` ms with the pointer `away` from the press point, on the axes that scroll.
fn step(away: Point<f32>, axes: Axes, elapsed: f32) -> Point<f32> {
    let along = |on: bool, distance: f32| {
        if on {
            pixels_per_ms(distance) * elapsed
        } else {
            0.
        }
    };
    point(along(axes.horizontal, away.x), along(axes.vertical, away.y))
}

fn offset(auto: &Autoscroll) -> Point<f32> {
    let away = auto.pointer - auto.origin;
    point(f32::from(away.x), f32::from(away.y))
}

/// For the harness: `off`, or the press point, the pointer, the mode and the cursor.
#[cfg(feature = "capture")]
fn state_line(state: &Shared) -> String {
    match state.borrow().as_ref() {
        None => "autoscroll: off".into(),
        Some(auto) => format!(
            "autoscroll: on origin={:.0},{:.0} pointer={:.0},{:.0} mode={} cursor={:?}",
            f32::from(auto.origin.x),
            f32::from(auto.origin.y),
            f32::from(auto.pointer.x),
            f32::from(auto.pointer.y),
            if auto.toggled {
                "toggled"
            } else if auto.holding {
                "holding"
            } else {
                "initial"
            },
            auto.cursor,
        ),
    }
}

impl super::Root {
    /// Harness `autoscroll state`: the cursor is not in a frame, so it is read here.
    #[cfg(feature = "capture")]
    pub(crate) fn autoscroll_state_line(&self) -> String {
        state_line(&self.autoscroll)
    }
}

/// A zero-size element that listens on the whole window, sets the panning cursor and sends the scroll steps while
/// autoscroll is on.
pub(super) fn layer(state: Shared) -> impl IntoElement {
    gpui::canvas(
        |_, _, _| (),
        move |_, (), window, cx| {
            let down = state.clone();
            window.on_mouse_event(move |event: &MouseDownEvent, phase, window, cx| {
                if phase != DispatchPhase::Capture {
                    return;
                }
                let on = down.borrow().is_some();
                if on {
                    // Any press ends it and does nothing else.
                    *down.borrow_mut() = None;
                    cx.stop_propagation();
                    window.refresh();
                } else if event.button == MouseButton::Middle
                    && !crate::inline::middle_press_on_link(event.position)
                {
                    *down.borrow_mut() = Some(Autoscroll {
                        origin: event.position,
                        pointer: event.position,
                        axes: SCROLLER_AXES,
                        holding: false,
                        toggled: false,
                        last: Instant::now(),
                        sending: false,
                        cursor: cursor(point(0., 0.), SCROLLER_AXES),
                    });
                    window.refresh();
                }
            });
            let up = state.clone();
            window.on_mouse_event(move |event: &MouseUpEvent, phase, window, _| {
                if phase != DispatchPhase::Capture || event.button != MouseButton::Middle {
                    return;
                }
                let mut state = up.borrow_mut();
                let Some(auto) = state.as_mut() else {
                    return;
                };
                if auto.holding {
                    *state = None;
                    window.refresh();
                } else {
                    auto.toggled = true;
                }
            });
            let moved = state.clone();
            window.on_mouse_event(move |event: &MouseMoveEvent, phase, window, _| {
                if phase != DispatchPhase::Capture {
                    return;
                }
                if let Some(auto) = moved.borrow_mut().as_mut() {
                    if auto.sending {
                        return;
                    }
                    auto.pointer = event.position;
                    let away = offset(auto);
                    // Blink: the first change of velocity turns a press still held into `kMiddleClickHolding`.
                    if !auto.toggled && (away.x.abs() > RADIUS || away.y.abs() > RADIUS) {
                        auto.holding = true;
                    }
                    window.refresh();
                }
            });
            let wheel = state.clone();
            window.on_mouse_event(move |_: &ScrollEvent, phase, window, _| {
                if phase != DispatchPhase::Capture {
                    return;
                }
                crate::inline::forget_link_under_pointer();
                let mut state = wheel.borrow_mut();
                if state.as_ref().is_some_and(|auto| !auto.sending) {
                    *state = None;
                    window.refresh();
                }
            });
            // Blink's `KeyboardEventManager`: a key down ends it and is swallowed. A listener here only hears keys
            // whose focus path passes this element, so the window's keystroke interceptor, which hears them all
            // before anything else, does it; it is registered once.
            KEY_INTERCEPTOR.with(|interceptor| {
                if interceptor.borrow().is_some() {
                    return;
                }
                let key = state.clone();
                *interceptor.borrow_mut() = Some(cx.intercept_keystrokes(move |_, window, cx| {
                    if key.borrow_mut().take().is_some() {
                        cx.stop_propagation();
                        window.refresh();
                    }
                }));
            });
            let key = state.clone();
            window.on_key_event(move |_: &KeyDownEvent, phase, window, cx| {
                if phase == DispatchPhase::Capture && key.borrow_mut().take().is_some() {
                    cx.stop_propagation();
                    window.refresh();
                }
            });

            // The cursor, and one step a frame while it is on.
            let (origin, delta) = {
                let mut state = state.borrow_mut();
                let Some(auto) = state.as_mut() else {
                    return;
                };
                let away = offset(auto);
                auto.cursor = cursor(away, auto.axes);
                window.set_window_cursor_style(auto.cursor);
                let now = Instant::now();
                let elapsed =
                    (now.duration_since(auto.last).as_secs_f32() * 1000.).min(LONGEST_STEP_MS);
                auto.last = now;
                let delta = step(away, auto.axes, elapsed);
                (auto.origin, point(px(delta.x), px(delta.y)))
            };
            window.request_animation_frame();
            if delta.x == px(0.) && delta.y == px(0.) {
                return;
            }
            let state = state.clone();
            window.on_next_frame(move |window, cx| {
                match state.borrow_mut().as_mut() {
                    Some(auto) => auto.sending = true,
                    None => return,
                }
                window.dispatch_event(
                    PlatformInput::Scroll(ScrollEvent {
                        position: origin,
                        delta: ScrollDelta::Pixels(delta),
                        modifiers: gpui::Modifiers::default(),
                        touch_phase: TouchPhase::Moved,
                        source: ScrollSource::Pointer,
                    }),
                    cx,
                );
                if let Some(auto) = state.borrow_mut().as_mut() {
                    auto.sending = false;
                }
            });
        },
    )
    .absolute()
    .size_0()
}

#[cfg(test)]
mod tests {
    use super::*;

    const BOTH: Axes = Axes {
        vertical: true,
        horizontal: true,
    };
    const ACROSS: Axes = Axes {
        vertical: false,
        horizontal: true,
    };

    #[test]
    fn nothing_moves_inside_the_radius() {
        assert_eq!(velocity(0.), 0.);
        assert_eq!(velocity(15.), 0.);
        assert_eq!(velocity(-15.), 0.);
        assert!(velocity(16.) < 0.);
        assert!(velocity(-16.) > 0.);
    }

    #[test]
    fn the_speed_is_the_distance_to_the_power_of_2_2_times_the_curve() {
        // 100 px: 100^2.2 * 0.000008 = 0.2010, which the fixed velocity curve makes 1.005 px a millisecond.
        assert!((velocity(100.) + 0.201).abs() < 0.001);
        assert!((pixels_per_ms(100.) + 1.005).abs() < 0.005);
        assert!((pixels_per_ms(-100.) - 1.005).abs() < 0.005);
        // 300 px: 2.253 * 5 = 11.26 px a millisecond.
        assert!((pixels_per_ms(300.) + 11.26).abs() < 0.05);
    }

    #[test]
    fn a_vertical_scroller_takes_no_horizontal_step() {
        let moved = step(point(200., 100.), SCROLLER_AXES, 10.);
        assert_eq!(moved.x, 0.);
        assert!(moved.y < 0.);
        let both = step(point(200., 100.), BOTH, 10.);
        assert!(both.x < 0. && both.y < 0.);
    }

    #[test]
    fn the_cursor_at_rest_names_the_axes() {
        let rest = point(10., -15.);
        assert_eq!(cursor(rest, SCROLLER_AXES), CursorStyle::PanMiddleVertical);
        assert_eq!(cursor(rest, ACROSS), CursorStyle::PanMiddleHorizontal);
        assert_eq!(cursor(rest, BOTH), CursorStyle::PanMiddle);
    }

    #[test]
    fn the_cursor_points_where_the_pointer_went() {
        // Screen y grows downward: a pointer above the press scrolls toward the top (north).
        assert_eq!(cursor(point(0., -40.), BOTH), CursorStyle::PanNorth);
        assert_eq!(cursor(point(0., 40.), BOTH), CursorStyle::PanSouth);
        assert_eq!(cursor(point(40., 0.), BOTH), CursorStyle::PanEast);
        assert_eq!(cursor(point(-40., 0.), BOTH), CursorStyle::PanWest);
        assert_eq!(cursor(point(40., -40.), BOTH), CursorStyle::PanNorthEast);
        assert_eq!(cursor(point(-40., -40.), BOTH), CursorStyle::PanNorthWest);
        assert_eq!(cursor(point(40., 40.), BOTH), CursorStyle::PanSouthEast);
        assert_eq!(cursor(point(-40., 40.), BOTH), CursorStyle::PanSouthWest);
    }

    #[test]
    fn the_cursor_ignores_an_axis_that_does_not_scroll() {
        // A vertical scroller: a sideways pointer keeps the round mark, a diagonal one shows only north or south.
        assert_eq!(
            cursor(point(60., 0.), SCROLLER_AXES),
            CursorStyle::PanMiddleVertical
        );
        assert_eq!(
            cursor(point(60., 60.), SCROLLER_AXES),
            CursorStyle::PanSouth
        );
        // A horizontal one: a pointer below shows east or west only.
        assert_eq!(cursor(point(60., 60.), ACROSS), CursorStyle::PanEast);
        assert_eq!(
            cursor(point(0., 60.), ACROSS),
            CursorStyle::PanMiddleHorizontal
        );
    }
}
