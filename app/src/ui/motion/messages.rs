//! Message motion. The hover bar of a message (`popover_DpHsW6`, stylesheet 28636, module 194085) has no entrance of
//! its own: the message component mounts it while the row is hovered and `.userMessageButtons` shows it with no
//! transition. A new message has none either (`chat-row-enter` belongs to the Conjure chat, module 986109, not to the
//! channel's message list). What moves is the icon inside a hover bar button: `.hoverBarButton:hover .icon` is
//! `scale(1.1)`, `:active` `scale(1)`, and `.full-motion .icon` is `transition: transform .4s cubic-bezier(.16, 1, .3, 1)`.

use super::{Css, Ease, hovered, key, value, window_active};
use gpui::{AnyElement, InteractiveElement as _, IntoElement, ParentElement, Styled, div, px};

/// `.full-motion .icon_DpHsW6`.
const ICON: Css = Css::new(400., Ease::Bezier(0.16, 1., 0.3, 1.)).full_motion();

/// The key a hover bar button's hover probe is stored under.
pub(in crate::ui) fn hover_key(button: &str) -> u64 {
    key(&("hover-bar-button", button))
}

thread_local! {
    static PRESSED: std::cell::Cell<Option<u64>> = const { std::cell::Cell::new(None) };
}

/// Whether the primary button is held on this hover bar button (`:active`). A press that was let go elsewhere is
/// dropped once the pointer has left the button.
fn pressed(button: &str) -> bool {
    let key = hover_key(button);
    let held = PRESSED.with(|cell| cell.get()) == Some(key);
    if held && !hovered(key) {
        PRESSED.with(|cell| cell.set(None));
        return false;
    }
    held
}

/// The scale of the icon of a hover bar button this frame: 1 at rest, `1.1` while the pointer is over the button
/// (`:hover`), 1 again while it is pressed (`:active`), all under the same 0.4 s transition.
pub(in crate::ui) fn icon_scale(button: &str) -> f32 {
    let target = if hovered(hover_key(button)) && !pressed(button) {
        1.1
    } else {
        1.
    };
    value(key(&("hover-bar-scale", button)), target, ICON)
}

/// Tracks the press of a hover bar button; an absolute overlay that lets every event through.
pub(in crate::ui) fn press_probe(button: &str) -> AnyElement {
    let key = hover_key(button);
    div()
        .absolute()
        .inset_0()
        .on_mouse_down(gpui::MouseButton::Left, move |_, _, _| {
            PRESSED.with(|cell| cell.set(Some(key)));
        })
        .on_mouse_up(gpui::MouseButton::Left, move |_, _, _| {
            PRESSED.with(|cell| cell.set(None));
        })
        .into_any_element()
}

/// A 20px box holding the button's icon or emoji, scaled as the stylesheet scales it.
pub(in crate::ui) fn icon_box(button: &str, child: impl IntoElement) -> AnyElement {
    div()
        .size(px(20.))
        .flex_none()
        .paint_scale(icon_scale(button), gpui::point(0.5, 0.5))
        .child(child)
        .into_any_element()
}

#[cfg(test)]
mod press_tests {
    use super::*;
    use crate::ui::motion::{begin_frame, freeze, reset_for_tests, set_hover};

    #[test]
    fn pressing_returns_the_icon_to_scale_1_over_the_same_transition() {
        reset_for_tests();
        freeze(Some(0.));
        begin_frame(false);
        set_hover(hover_key("p"), true);
        let _ = icon_scale("p");
        freeze(Some(1000.));
        begin_frame(false);
        assert_eq!(icon_scale("p"), 1.1);
        PRESSED.with(|cell| cell.set(Some(hover_key("p"))));
        freeze(Some(1001.));
        begin_frame(false);
        let _ = icon_scale("p");
        freeze(Some(1201.));
        begin_frame(false);
        let middle = icon_scale("p");
        assert!(middle < 1.1 && middle > 1.0, "{middle}");
        freeze(Some(1700.));
        begin_frame(false);
        assert_eq!(icon_scale("p"), 1.);
        PRESSED.with(|cell| cell.set(None));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::motion::{begin_frame, freeze, reset_for_tests, set_hover};

    #[test]
    fn the_icon_grows_to_1_1_over_400ms_and_snaps_when_motion_is_reduced() {
        reset_for_tests();
        freeze(Some(0.));
        begin_frame(false);
        assert_eq!(icon_scale("a"), 1.);
        set_hover(hover_key("a"), true);
        freeze(Some(1.));
        begin_frame(false);
        let _ = icon_scale("a");
        freeze(Some(201.));
        begin_frame(false);
        let middle = icon_scale("a");
        let expected = 1. + 0.1 * Ease::Bezier(0.16, 1., 0.3, 1.).apply(0.5) as f32;
        assert!((middle - expected).abs() < 1e-4, "{middle} {expected}");
        assert!(middle > 1.09, "the curve is front loaded: {middle}");
        freeze(Some(600.));
        begin_frame(false);
        assert_eq!(icon_scale("a"), 1.1);
        // Reduced motion: the transition is gone, the :hover scale still applies.
        reset_for_tests();
        freeze(Some(0.));
        begin_frame(true);
        let _ = icon_scale("b");
        set_hover(hover_key("b"), true);
        freeze(Some(1.));
        begin_frame(true);
        assert_eq!(icon_scale("b"), 1.1);
    }
}

/// The speaking ring of a voice participant (module 556525, hook `y.v`). While speaking the avatar carries
/// `box-shadow: 0 0 0 {n}px C, inset 0 0 0 {r}px C, inset 0 0 0 {a}px var(--background-base-lower)` with
/// `C = var(--status-speaking)`, and `transition: box-shadow 50ms ease-out` while the window has focus (not gated
/// by reduced motion). Without a measured volume (what the voice events here give) `n = 0`, `r = 2`, `a = 3`.
pub const SPEAKING: Css = Css::new(50., Ease::EaseOut);

/// The widths in px of the inner ring (`r`) and of the base-lower ring (`a`) at this frame, for one participant.
pub fn speaking_rings(who: u64, speaking: bool) -> (f32, f32) {
    let target = if speaking { 1. } else { 0. };
    let t = if window_active() {
        value(key(&("speaking", who)), target, SPEAKING)
    } else {
        target
    };
    (2. * t, 3. * t)
}

#[cfg(test)]
mod speaking_tests {
    use super::*;
    use crate::ui::motion::{begin_frame, freeze, reset_for_tests};

    #[test]
    fn the_ring_grows_to_2_and_3_px_in_50ms() {
        reset_for_tests();
        freeze(Some(0.));
        begin_frame(false);
        assert_eq!(speaking_rings(1, false), (0., 0.));
        freeze(Some(16.));
        begin_frame(false);
        assert_eq!(speaking_rings(1, true), (0., 0.));
        freeze(Some(41.));
        begin_frame(false);
        let mid = speaking_rings(1, true);
        assert!(mid.0 > 0. && mid.0 < 2., "{mid:?}");
        freeze(Some(80.));
        begin_frame(false);
        assert_eq!(speaking_rings(1, true), (2., 3.));
    }
}

/// The status message under a form field (`statusMessageContainer_VhFsFg`, module 715965 and stylesheet 448400):
/// `.full-motion` plays `slideDown_VhFsFg`, 0.3 s `ease-in-out`, from opacity 0 and `translateY(-4px)` to rest. Its
/// `height: 0` to `auto` step cannot interpolate, so the height does not move.
pub const STATUS_SLIDE: Css = Css::new(300., Ease::EaseInOut).full_motion();

/// Opacity and the vertical offset in px of a status message at progress `t` (0 to 1).
pub fn status_pose(t: f32) -> (f32, f32) {
    (t, -4. * (1. - t))
}

/// Wraps a field's status message so that it slides in when it appears. `what` identifies the message.
pub fn status_message(what: &str, child: impl IntoElement) -> AnyElement {
    let t = super::enter(
        key(&("status-message", what)),
        super::Drive::Css(STATUS_SLIDE),
    );
    let (opacity, offset) = status_pose(t);
    div()
        .relative()
        .top(px(offset))
        .opacity(opacity)
        .child(child)
        .into_any_element()
}

#[cfg(test)]
mod status_tests {
    use super::*;
    use crate::ui::motion::{begin_frame, freeze, reset_for_tests};

    #[test]
    fn the_status_message_slides_down_4px_while_fading_in() {
        assert_eq!(status_pose(0.), (0., -4.));
        assert_eq!(status_pose(1.), (1., 0.));
        reset_for_tests();
        freeze(Some(0.));
        begin_frame(false);
        let t0 = super::super::enter(key("m"), super::super::Drive::Css(STATUS_SLIDE));
        assert_eq!(t0, 0.);
        freeze(Some(150.));
        begin_frame(false);
        let mid = super::super::enter(key("m"), super::super::Drive::Css(STATUS_SLIDE));
        assert!((mid - 0.5).abs() < 0.02, "{mid}");
        freeze(Some(400.));
        begin_frame(false);
        assert_eq!(
            super::super::enter(key("m"), super::super::Drive::Css(STATUS_SLIDE)),
            1.
        );
    }

    #[test]
    fn reduced_motion_shows_it_at_once() {
        reset_for_tests();
        freeze(Some(0.));
        begin_frame(true);
        assert_eq!(
            super::super::enter(key("r"), super::super::Drive::Css(STATUS_SLIDE)),
            1.
        );
    }
}
