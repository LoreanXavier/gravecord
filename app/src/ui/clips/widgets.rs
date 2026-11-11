//! The design system pieces the Clips views use: icon-only buttons and text buttons (module 657718, stylesheet
//! F7qvhY: `sm` is 32 px tall, `md` 40; an icon takes 16 px at `sm` and 20 px at `md`, module 27989).
use super::super::{color, icon};
use gpui::{Div, FontWeight, SharedString, Stateful, div, prelude::*, px};

/// The button sizes (`sm`, `md`).
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Size {
    Sm,
    Md,
}

impl Size {
    fn height(self) -> f32 {
        match self {
            // `min-height: 30px` of the children wrapper plus the 1px border each side.
            Size::Sm => 32.,
            Size::Md => 40.,
        }
    }

    fn glyph(self) -> f32 {
        match self {
            Size::Sm => 16.,
            Size::Md => 20.,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Variant {
    Primary,
    Secondary,
}

impl Variant {
    fn family(self) -> &'static str {
        match self {
            Variant::Primary => "primary",
            Variant::Secondary => "secondary",
        }
    }
}

/// `variant: "icon-only"`: a square with the glyph in `--control-icon-only-icon-default`, brightening and filling on
/// hover and press. `selected` keeps the pressed look (a toggled star).
pub(super) fn icon_button(
    id: impl Into<SharedString>,
    glyph: &'static str,
    size: Size,
    selected: bool,
) -> Stateful<Div> {
    let id: SharedString = id.into();
    let side = size.height();
    div()
        .id(id.clone())
        .group(id.clone())
        .flex_none()
        .size(px(side))
        .rounded(px(8.))
        .flex()
        .items_center()
        .justify_center()
        .cursor_pointer()
        .when(selected, |this| {
            this.bg(color("--control-icon-only-background-active"))
        })
        .hover(|style| style.bg(color("--control-icon-only-background-hover")))
        .active(|style| style.bg(color("--control-icon-only-background-active")))
        .child(
            icon(
                glyph,
                size.glyph(),
                if selected {
                    "--control-icon-only-icon-active"
                } else {
                    "--control-icon-only-icon-default"
                },
            )
            .group_hover(id, |style| {
                style.text_color(color("--control-icon-only-icon-hover"))
            }),
        )
}

/// A text button, optionally with a leading glyph, as wide as its box. Text is `body-md/medium` at `md` and
/// `body-sm/medium` at `sm` (module 657718).
pub(super) fn button(
    id: impl Into<SharedString>,
    label: impl Into<SharedString>,
    glyph: Option<&'static str>,
    variant: Variant,
    size: Size,
    full_width: bool,
) -> Stateful<Div> {
    let family = variant.family();
    let token = |part: &str, state: &str| color(&format!("--control-{family}-{part}-{state}"));
    let text = match variant {
        Variant::Secondary => "--control-secondary-text-default",
        _ => "--white",
    };
    let (text_size, line) = match size {
        Size::Sm => (14., 18.),
        Size::Md => (16., 20.),
    };
    let id: SharedString = id.into();
    div()
        .id(id)
        .h(px(size.height()))
        .flex_none()
        .when(full_width, |this| this.w_full())
        .when(!full_width, |this| this.min_w(px(60.)))
        .px(px(if size == Size::Sm { 11. } else { 15. }))
        .rounded(px(8.))
        .border_1()
        .border_color(token("border", "default"))
        .bg(token("background", "default"))
        .flex()
        .items_center()
        .justify_center()
        .gap(px(4.))
        .cursor_pointer()
        .text_size(px(text_size))
        .line_height(px(line))
        .font_weight(FontWeight(500.))
        .text_color(if variant == Variant::Secondary {
            color(text)
        } else {
            color("--white")
        })
        .hover(|style| {
            style
                .bg(token("background", "hover"))
                .border_color(token("border", "hover"))
        })
        .active(|style| {
            style
                .bg(token("background", "active"))
                .border_color(token("border", "active"))
        })
        .children(glyph.map(|glyph| {
            icon(
                glyph,
                size.glyph(),
                if variant == Variant::Secondary {
                    "--control-secondary-text-default"
                } else {
                    "--white"
                },
            )
        }))
        .child(label.into())
}
