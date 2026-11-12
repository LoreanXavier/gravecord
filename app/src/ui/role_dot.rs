//! The role dot of "Role Colors: Next to names" (`roleDot`, module 315886 and `188259` CSS): a 20 px rounded square on
//! `--background-mod-normal` holding three circles of the SVG: a 12 px one in `--text-strong`, the same one in the role
//! colour at 40% opacity, and a 10 px one in the role colour.

use super::color;
use gpui::{Div, ParentElement, Styled, div, px, rgb, rgba};

/// The 12 px dot for the role colour `role` (0xRRGGBB), to sit in the 20 px square.
pub(super) fn dot(role: u32) -> Div {
    let circle = |size: f32, inset: f32| {
        div()
            .absolute()
            .left(px(inset))
            .top(px(inset))
            .size(px(size))
            .rounded_full()
    };
    div()
        .relative()
        .size(px(12.))
        .child(circle(12., 0.).bg(color("--text-strong")))
        .child(circle(12., 0.).bg(rgba((role << 8) | 0x66)))
        .child(circle(10., 1.).bg(rgb(role)))
}

/// The same dot for a role with a gradient (module 545442 `u`: the two circles' fill is `url(#dotGradient-…)`, the
/// gradient across each circle's own box).
pub(super) fn dot_gradient(colors: &dc_state::RoleColors) -> Div {
    let circle = |size: f32, inset: f32| {
        div()
            .absolute()
            .left(px(inset))
            .top(px(inset))
            .size(px(size))
            .rounded_full()
    };
    div()
        .relative()
        .size(px(12.))
        .child(circle(12., 0.).bg(super::color("--text-strong")))
        .child(
            circle(12., 0.)
                .child(gpui::img(super::role_gradient::circle(colors, 12, 0.4)).size(px(12.))),
        )
        .child(
            circle(10., 1.)
                .child(gpui::img(super::role_gradient::circle(colors, 10, 1.)).size(px(10.))),
        )
}
