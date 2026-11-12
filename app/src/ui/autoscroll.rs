//! Middle-button autoscroll. The official client is a Chromium page, and on Windows Chromium scrolls what is under
//! the pointer when the middle button is pressed there: the further the pointer is moved from the point of the
//! press, the faster. The rules are those of Blink's `autoscroll_controller.cc`:
//!
//! - nothing moves within 15 px of the press point, on each axis by itself (`kNoMiddleClickAutoscrollRadius`);
//! - the speed on an axis is `|distance| ^ 2.2 * 0.000008` pixels a millisecond, toward the pointer (`kExponent`,
//!   `kMultiplier`);
//! - a press released after the pointer left the 15 px (`kMiddleClickHolding`) ends it at the release; a press
//!   released in place (`kMiddleClickToggled`) leaves it on until the next press of any button, a key or the wheel.
//!
//! Chromium's panning cursors and the mark it draws at the press point are not drawn here.

use gpui::{
    DispatchPhase, IntoElement, KeyDownEvent, MouseButton, MouseDownEvent, MouseMoveEvent,
    MouseUpEvent, Pixels, PlatformInput, Point, ScrollDelta, ScrollEvent, ScrollSource, Styled,
    TouchPhase, point, px,
};
use std::cell::RefCell;
use std::rc::Rc;
use std::time::Instant;

/// `kNoMiddleClickAutoscrollRadius`.
const RADIUS: f32 = 15.;
/// `kExponent`.
const EXPONENT: f32 = 2.2;
/// `kMultiplier`, without its sign: pixels a millisecond.
const MULTIPLIER: f32 = 0.000_008;
/// The longest step one frame scrolls for, so a stalled frame does not jump.
const LONGEST_STEP_MS: f32 = 50.;

pub(super) struct Autoscroll {
    origin: Point<Pixels>,
    pointer: Point<Pixels>,
    /// The pointer left the radius while the button was down: the release ends it.
    holding: bool,
    /// The button was released in place: it stays on until the next press, key or wheel turn.
    toggled: bool,
    last: Instant,
    /// The step this module is sending now, which its own wheel listener must not take for the user's.
    sending: bool,
}

pub(super) type Shared = Rc<RefCell<Option<Autoscroll>>>;

/// The speed on one axis for a pointer `distance` px from the press point, in px a millisecond; its sign is the
/// wheel's (negative scrolls toward the end of the content, which is where a pointer below the press point points).
pub(super) fn velocity(distance: f32) -> f32 {
    if distance.abs() <= RADIUS {
        return 0.;
    }
    -distance.signum() * distance.abs().powf(EXPONENT) * MULTIPLIER
}

/// A zero-size element that listens on the whole window and sends the scroll steps while autoscroll is on.
pub(super) fn layer(state: Shared) -> impl IntoElement {
    gpui::canvas(
        |_, _, _| (),
        move |_, (), window, _| {
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
                } else if event.button == MouseButton::Middle {
                    *down.borrow_mut() = Some(Autoscroll {
                        origin: event.position,
                        pointer: event.position,
                        holding: false,
                        toggled: false,
                        last: Instant::now(),
                        sending: false,
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
            window.on_mouse_event(move |event: &MouseMoveEvent, phase, _, _| {
                if phase != DispatchPhase::Capture {
                    return;
                }
                if let Some(auto) = moved.borrow_mut().as_mut() {
                    if auto.sending {
                        return;
                    }
                    auto.pointer = event.position;
                    let away = event.position - auto.origin;
                    if !auto.toggled
                        && (f32::from(away.x).abs() > RADIUS || f32::from(away.y).abs() > RADIUS)
                    {
                        auto.holding = true;
                    }
                }
            });
            let wheel = state.clone();
            window.on_mouse_event(move |_: &ScrollEvent, phase, _, _| {
                if phase != DispatchPhase::Capture {
                    return;
                }
                let mut state = wheel.borrow_mut();
                if state.as_ref().is_some_and(|auto| !auto.sending) {
                    *state = None;
                }
            });
            let key = state.clone();
            window.on_key_event(move |_: &KeyDownEvent, phase, _, _| {
                if phase == DispatchPhase::Capture {
                    *key.borrow_mut() = None;
                }
            });

            // One step a frame while it is on.
            let step = {
                let mut state = state.borrow_mut();
                let Some(auto) = state.as_mut() else {
                    return;
                };
                let now = Instant::now();
                let elapsed =
                    (now.duration_since(auto.last).as_secs_f32() * 1000.).min(LONGEST_STEP_MS);
                auto.last = now;
                let away = auto.pointer - auto.origin;
                let delta = point(
                    px(velocity(f32::from(away.x)) * elapsed),
                    px(velocity(f32::from(away.y)) * elapsed),
                );
                (auto.origin, delta)
            };
            window.request_animation_frame();
            let (origin, delta) = step;
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

    #[test]
    fn nothing_moves_inside_the_radius() {
        assert_eq!(velocity(0.), 0.);
        assert_eq!(velocity(15.), 0.);
        assert_eq!(velocity(-15.), 0.);
        assert!(velocity(16.) < 0.);
        assert!(velocity(-16.) > 0.);
    }

    #[test]
    fn the_speed_is_the_distance_to_the_power_of_2_2() {
        // 100 px: 100^2.2 * 0.000008 = 0.2010 px a millisecond, toward the pointer.
        assert!((velocity(100.) + 0.201).abs() < 0.001);
        assert!((velocity(-100.) - 0.201).abs() < 0.001);
        // 300 px: 2.253 px a millisecond.
        assert!((velocity(300.) + 2.253).abs() < 0.01);
    }
}
