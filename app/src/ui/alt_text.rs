//! The ALT button on a picture and the popout it opens. With "Show image descriptions" on (`viewImageDescriptions`,
//! module 253932), a picture whose sender wrote a description wears a small ALT button (module 383055 `t_`/`tg`);
//! pressing it opens the "Image Description (Alt Text)" popout (`tf`). Sizes are from the `mediaMosaicAltText*`
//! classes of CSS module 959760.

use super::{Root, color};
use dc_state::Id;
use gpui::{
    AnyElement, BoxShadow, Context, FontWeight, IntoElement, MouseButton, SharedString, div, point,
    prelude::*, px,
};
use std::cell::Cell;
use std::time::{Duration, Instant};

thread_local! {
    /// The attachment whose description popout is open.
    static OPEN: Cell<Option<Id>> = const { Cell::new(None) };
    /// When a press outside the popout last closed it. The same press may land on the ALT button, whose handler
    /// would then reopen what the press just closed.
    static CLOSED_AT: Cell<Option<Instant>> = const { Cell::new(None) };
}

pub(super) fn open() -> Option<Id> {
    OPEN.with(Cell::get)
}

/// Closes the popout. True when one was open.
pub(super) fn close() -> bool {
    OPEN.with(|open| open.replace(None)).is_some()
}

fn popout_width(text: &str) -> f32 {
    (text.chars().count() as f32 * 8. + 26.).clamp(200., 360.)
}

fn toggle(id: Id) {
    let just_closed = CLOSED_AT
        .with(Cell::get)
        .is_some_and(|at| at.elapsed() < Duration::from_millis(100));
    if just_closed {
        return;
    }
    OPEN.with(|open| {
        open.set(if open.get() == Some(id) {
            None
        } else {
            Some(id)
        })
    });
}

impl Root {
    /// The ALT button of one attachment, drawn over the picture's bottom left corner, or nothing when the setting
    /// is off or the attachment has no description.
    pub(super) fn alt_text_overlay(
        &self,
        attachment: &dc_state::Attachment,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let text = attachment.description.as_deref()?;
        if !self.pref_on("chat_inline_media_image_descriptions", false) {
            return None;
        }
        let id = attachment.id;
        let button = div()
            .id(SharedString::from(format!("alt-{id}")))
            .px(px(8.))
            .py(px(7.))
            .rounded(px(4.))
            // `.mediaMosaicAltText`: rgba(0,0,0,.6), .8 on hover, white 14px semibold on 18px lines.
            .bg(gpui::rgba(0x0000_0099))
            .hover(|style| style.bg(gpui::rgba(0x0000_00cc)))
            .text_color(gpui::white())
            .text_size(px(14.))
            .line_height(px(18.))
            .font_weight(FontWeight(600.))
            .cursor_pointer()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |_, _, _, cx| {
                    toggle(id);
                    cx.stop_propagation();
                    cx.notify();
                }),
            )
            .on_mouse_up(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child("ALT");
        let popout = (open() == Some(id)).then(|| {
            div()
                .id(SharedString::from(format!("alt-popout-{id}")))
                .absolute()
                .left(px(0.))
                .bottom(px(36.))
                // An absolute box keeps the button's width, so the popout's own width is set: the text's
                // length at 16px, from a floor to the popout's 360px.
                .w(px(popout_width(text)))
                .flex()
                .flex_col()
                .px(px(12.))
                .py(px(13.5))
                .rounded(px(8.))
                .bg(color("--background-surface-high"))
                .text_color(color("--text-default"))
                .shadow(vec![BoxShadow {
                    color: gpui::rgba(0x0000_0040).into(),
                    offset: point(px(0.), px(8.)),
                    blur_radius: px(16.),
                    spread_radius: px(0.),
                    inset: false,
                }])
                .on_mouse_down_out(cx.listener(|_, _, _, cx| {
                    if close() {
                        CLOSED_AT.with(|at| at.set(Some(Instant::now())));
                        cx.notify();
                    }
                }))
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .child(
                    div()
                        .mb(px(9.5))
                        .text_size(px(14.))
                        .line_height(px(18.))
                        .font_weight(FontWeight(500.))
                        .child("Image Description (Alt Text)"),
                )
                .child(
                    div()
                        .text_size(px(16.))
                        .line_height(px(20.))
                        .child(text.to_owned()),
                )
        });
        Some(
            div()
                .absolute()
                .left(px(4.5))
                .bottom(px(4.))
                .child(button)
                .children(popout)
                .into_any_element(),
        )
    }
}
