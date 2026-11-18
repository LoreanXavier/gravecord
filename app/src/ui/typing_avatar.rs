//! The status corner of an avatar while its person types (module 97808: `D` draws it still, `b` springs between the
//! status dot and the typing pill), and the three typing dots (module 264701: the CSS dots of module 128539 under the
//! experiment `2026-07-typing-users-css-dots`, module 345870; else the springing SVG dots of module 863610). The chat's
//! typing line (module 620014 `X`) uses the same two kinds of dots.
//!
//! The avatar's round cut-out is drawn, as everywhere in this client, as a disc of the background colour; the typing
//! cut-out is the same in the wider pill shape.
use super::{color, motion};
use gpui::{AnyElement, IntoElement, div, prelude::*, px};

/// The experiment that picks the CSS dots (module 345870); variation 1 turns them on, the default is off.
pub(super) const CSS_DOTS_EXPERIMENT: &str = "2026-07-typing-users-css-dots";

/// `size`, `status`, `stroke` and `offset` of an avatar size (module 778712 `vC`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Spec {
    pub size: f32,
    pub status: f32,
    pub stroke: f32,
    pub offset: f32,
}

pub(super) fn spec(size: f32) -> Spec {
    let (status, stroke, offset) = match size as u32 {
        16 | 20 => (6., 2., 0.),
        24 => (8., 3., 0.),
        32 => (10., 3., 0.),
        40 | 44 | 48 => (12., 4., 0.),
        56 => (14., 4., 2.),
        72 | 80 => (16., 6., 4.),
        96 => (20., 8., 6.),
        120 => (24., 8., 8.),
        _ => (30., 10., 10.),
    };
    Spec {
        size,
        status,
        stroke,
        offset,
    }
}

/// A box in avatar coordinates.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub radius: f32,
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

/// The avatar's cut-out at `typing` (0 the round status hole, 1 the typing pill): `R` of module 97808,
/// `avatarCutoutX = size - d + (d - status) / 2 - stroke - offset` with `d` the status width (2.5 times the status
/// while typing), the height `status + 2 stroke`, the corner half the round hole's side.
pub(super) fn cutout(spec: Spec, typing: f32) -> Rect {
    let d = spec.status * lerp(1., 2.5, typing);
    Rect {
        x: spec.size - d + (d - spec.status) / 2. - spec.stroke - spec.offset,
        y: spec.size - spec.status - spec.stroke - spec.offset,
        w: d + 2. * spec.stroke,
        h: spec.status + 2. * spec.stroke,
        radius: (spec.status + 2. * spec.stroke) / 2.,
    }
}

/// The coloured status shape: module 935154 `v` (with `topOffset` 2 and `leftOffset` 6, as module 97808 asks) inside
/// the group `b` translates to `(size - 1.75 status - offset, size - 1.5 status - offset)`. Round it is the status
/// square at `x = .75 status`; typing it is `2.5 status` wide from `x = 0`; both `.5 status` down with half-status
/// corners.
pub(super) fn pill(spec: Spec, typing: f32) -> Rect {
    let s = spec.status;
    let group_x = spec.size - 1.75 * s - spec.offset;
    let group_y = spec.size - 1.5 * s - spec.offset;
    Rect {
        x: group_x + lerp(0.75 * s, 0., typing),
        y: group_y + 0.5 * s,
        w: s * lerp(1., 2.5, typing),
        h: s,
        radius: 0.5 * s,
    }
}

/// The centres of the three dots in avatar coordinates and their radius: module 264701 is placed at `x = .15 * 2.5
/// status`, `y = .75 status` of the group with `dotRadius = status / 4`; module 863610 puts dot `e` at
/// `r + r * 2.5 * e`.
pub(super) fn dot_centres(spec: Spec) -> ([(f32, f32); 3], f32) {
    let s = spec.status;
    let r = s / 4.;
    let group_x = spec.size - 1.75 * s - spec.offset;
    let group_y = spec.size - 1.5 * s - spec.offset;
    let left = group_x + 0.15 * 2.5 * s;
    let top = group_y + 0.75 * s;
    let centre = |e: f32| (left + r + r * 2.5 * e, top + r);
    ([centre(0.), centre(1.), centre(2.)], r)
}

/// `A` of module 863610: the triangle wave of period 2 between 0 and 1.
fn triangle(x: f64) -> f64 {
    let t = x.rem_euclid(2.);
    if t > 1. { 2. - t } else { t }
}

/// Piecewise linear over the stops `[0, .4, .8, 1]` (react-spring's `to(range, output)`).
fn ramp(t: f64, low: f64, high: f64) -> f64 {
    if t <= 0.4 {
        low
    } else if t >= 0.8 {
        high
    } else {
        low + (high - low) * (t - 0.4) / 0.4
    }
}

/// One spring dot of module 863610 at `now_ms`: `dotCycle` runs from 2.8 up by 4 every 2400 ms (linear, the
/// spring's `duration` config), dot `e` reads `A(dotCycle - .25 e)` and maps it to a radius of .8 to 1 times its own
/// and an opacity of .3 to 1. A window without focus shows every dot whole. Returns (radius factor, opacity).
pub(super) fn spring_dot(index: usize, now_ms: f64, focused: bool) -> (f32, f32) {
    if !focused {
        return (1., 1.);
    }
    let cycle = 2.8 + now_ms / 600.;
    let wave = triangle(cycle - 0.25 * index as f64);
    (ramp(wave, 0.8, 1.) as f32, ramp(wave, 0.3, 1.) as f32)
}

/// Whether the account is in the CSS dots variation.
pub(super) fn css_dots(experiments: &dc_state::experiments::Assignments) -> bool {
    experiments.variant(CSS_DOTS_EXPERIMENT) == Some(1)
}

fn status_tint(status: &str) -> &'static str {
    match status {
        "dnd" => "--icon-status-dnd",
        "idle" => "--icon-status-idle",
        "invisible" | "offline" => "--icon-status-offline",
        "streaming" => "--platform-twitch",
        _ => "--icon-status-online",
    }
}

/// Three dots of radius `r` laid out from (0, 0) as module 863610 draws them (an SVG `2r * 3 + r` wide and `2r`
/// high), or as module 264701's CSS dots (`2r` discs `2.5 r` apart). `enter` (0..1) is the spring dots'
/// `dotPosition`: they slide out of the middle while they appear.
fn dots(r: f32, tint: &str, css: bool, enter: f32) -> AnyElement {
    // Not in the pinned client: the NoTypingAnimation plugin (Vencord `src/plugins/noTypingAnimation`) draws the dots
    // as an unfocused window does, still and whole, and asks for no animation frames.
    let focused = motion::window_active() && !crate::plugin_on!("NoTypingAnimation");
    let now = motion::now();
    let middle = (2. * r * 3. + r / 4. * 2.) / 2.;
    let mut row = div()
        .relative()
        .w(px(2. * r * 3. + r))
        .h(px(2. * r))
        .opacity(if css { 1. } else { enter.clamp(0., 1.) });
    for index in 0..3 {
        let home = r + r * 2.5 * index as f32;
        let (scale, opacity) = if css {
            let (opacity, scale) = motion::typing_dot(index, focused);
            (scale, opacity)
        } else {
            if focused {
                motion::keep_animating();
            }
            spring_dot(index, now, focused)
        };
        let centre = if css { home } else { lerp(middle, home, enter) };
        let radius = r * scale;
        row = row.child(
            div()
                .absolute()
                .left(px(centre - radius))
                .top(px(r - radius))
                .size(px(radius * 2.))
                .rounded_full()
                .bg(color(tint))
                .opacity(opacity),
        );
    }
    row.into_any_element()
}

/// Module 863610's dots of radius `r`, themed: what the TypingIndicator plugin draws (`plugin_hooks`).
pub(super) fn three_dots(r: f32) -> AnyElement {
    dots(r, "--text-default", false, 1.)
}

/// The chat line's dots (module 620014 `X`): the CSS dots (`.dot_U9Tvvz`, 7px, 2px apart) or module 863610 with
/// `dotRadius` 3.5, themed (`--text-default`).
pub(super) fn chat_dots(css: bool) -> AnyElement {
    if css {
        let focused = motion::window_active() && !crate::plugin_on!("NoTypingAnimation");
        let dot = |index: usize| {
            let (opacity, scale) = motion::typing_dot(index, focused);
            div()
                .size(px(7.))
                .rounded_full()
                .bg(color("--text-default"))
                .opacity(opacity)
                .paint_scale(scale, gpui::point(0.5, 0.5))
        };
        div()
            .flex()
            .items_center()
            .gap(px(2.))
            .child(dot(0))
            .child(dot(1))
            .child(dot(2))
            .into_any_element()
    } else {
        dots(3.5, "--text-default", false, 1.)
    }
}

/// The status corner of an avatar of `size` px: the status dot, or the typing pill with its dots, springing between
/// the two (`{tension: 1200, friction: 70}`, module 97808 `g`) and the dots sliding in (`{tension: 900, friction:
/// 50}`, module 863610). `ring` is the colour the cut-out is drawn in, `key` names this avatar for its springs.
/// Children are absolutely placed in the avatar's own box (the pill reaches past its right edge).
pub(super) fn status_corner(
    status: &str,
    typing: bool,
    ring: &str,
    size: f32,
    key: u64,
    css: bool,
) -> AnyElement {
    let spec = spec(size);
    let target = if typing { 1. } else { 0. };
    // A window without focus moves at once (`animate-never` while `document.hasFocus()` is false).
    let morph = if motion::window_active() {
        motion::spring_from(
            motion::key(&("typing-morph", key)),
            0.,
            target,
            motion::SpringConfig::new(1200., 70.),
        )
    } else {
        target
    };
    let enter = if motion::window_active() {
        motion::spring_from(
            motion::key(&("typing-dots", key)),
            0.,
            target,
            motion::SpringConfig::new(900., 50.),
        )
    } else {
        target
    };
    let morph = morph.max(0.);
    if morph <= 0.001 && !typing {
        return if spec.status >= 12. {
            super::status_dot_40(status, ring).into_any_element()
        } else {
            super::status_dot(status, ring).into_any_element()
        };
    }
    let hole = cutout(spec, morph);
    let shape = pill(spec, morph);
    let (centres, r) = dot_centres(spec);
    let show_dots = if css { typing } else { enter > 0.001 || typing };
    div()
        .absolute()
        .left_0()
        .top_0()
        .size(px(size))
        .child(
            div()
                .absolute()
                .left(px(hole.x))
                .top(px(hole.y))
                .w(px(hole.w))
                .h(px(hole.h))
                .rounded(px(hole.radius))
                .bg(color(ring)),
        )
        .child(
            div()
                .absolute()
                .left(px(shape.x))
                .top(px(shape.y))
                .w(px(shape.w))
                .h(px(shape.h))
                .rounded(px(shape.radius))
                .bg(color(status_tint(status))),
        )
        .when(show_dots, |corner| {
            corner.child(
                div()
                    .absolute()
                    .left(px(centres[0].0 - r))
                    .top(px(centres[0].1 - r))
                    .child(dots(r, "--white", css, enter)),
            )
        })
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_32px_cutout_widens_from_the_status_hole_to_the_typing_pill() {
        let spec = spec(32.);
        // Round: the 16px hole whose centre is the dot's (27, 27).
        assert_eq!(
            cutout(spec, 0.),
            Rect {
                x: 19.,
                y: 19.,
                w: 16.,
                h: 16.,
                radius: 8.
            }
        );
        // Typing: 2.5 times the status wide plus the stroke on each side, centred on the same point.
        assert_eq!(
            cutout(spec, 1.),
            Rect {
                x: 11.5,
                y: 19.,
                w: 31.,
                h: 16.,
                radius: 8.
            }
        );
        assert_eq!(pill(spec, 0.).x, 22.);
        assert_eq!(pill(spec, 1.).x, 14.5);
        assert_eq!(pill(spec, 1.).w, 25.);
        assert_eq!(pill(spec, 1.).y, 22.);
    }

    #[test]
    fn the_dots_sit_in_the_middle_of_the_pill() {
        let (centres, r) = dot_centres(spec(32.));
        assert_eq!(r, 2.5);
        assert_eq!(centres, [(20.75, 27.), (27., 27.), (33.25, 27.)]);
        // The pill runs from 14.5 to 39.5: the middle dot is its centre.
        let shape = pill(spec(32.), 1.);
        assert_eq!(shape.x + shape.w / 2., centres[1].0);
    }

    #[test]
    fn spring_dots_pulse_in_turn_and_rest_whole_without_focus() {
        // At 0 ms the cycle is 2.8: the first dot reads A(.8) = .8, full size; the third A(.3), small and faint.
        let (radius, opacity) = spring_dot(0, 0., true);
        assert!((radius - 1.).abs() < 1e-6 && (opacity - 1.).abs() < 1e-6);
        let (radius, opacity) = spring_dot(2, 0., true);
        assert!((radius - 0.8).abs() < 1e-6 && (opacity - 0.3).abs() < 1e-6);
        // The wave repeats every 1200 ms.
        assert_eq!(spring_dot(1, 250., true), spring_dot(1, 1450., true));
        assert_eq!(spring_dot(1, 250., false), (1., 1.));
    }

    #[test]
    fn the_css_dots_come_only_with_variation_one() {
        let ready = |variant: i64| {
            serde_json::json!({"assignments": {"1": {"9": {"assignments": [
                [dc_state::experiments::hash(CSS_DOTS_EXPERIMENT), variant, 0, 1]
            ]}}}})
        };
        assert!(css_dots(&dc_state::experiments::Assignments::from_ready(
            &ready(1),
            "9"
        )));
        assert!(!css_dots(&dc_state::experiments::Assignments::from_ready(
            &ready(0),
            "9"
        )));
        assert!(!css_dots(&dc_state::experiments::Assignments::default()));
    }
}
