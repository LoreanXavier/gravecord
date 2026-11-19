//! The design system's dialog with a header graphic (module 772707 `k`: `size: "md"`, `paddingSize: "lg"`, the header
//! of module 20742 `V6` over the purple gradient of module 315629, the body of module 430993 and the action bar of
//! module 696208 with `actionsFullWidth`). The geometry is the one measured for the voice channel status dialog
//! (`design/dom/ch-vstatus/modal-*`, which is this component): 480 wide, the 478 wide header gradient, the graphic in
//! `headerGraphicContainer_dvN-MP` (288 wide, `aspect-ratio-16/9_sYj23M`, its picture `max-height:100%;max-width:100%`;
//! at most `15vh` tall in a window up to 720 high, hidden below 431), 20px above and 16px below it, then the
//! `heading-xl/semibold` title and the `body-md/normal` `text-subtle` subtitle 8px apart, centred.

use super::{Root, channel_status, color, icon, modal};
use gpui::{
    AnyElement, Context, FontWeight, IntoElement, Rgba, deferred, div, img, prelude::*, px, rgba,
};

/// A picture of the header: its address and its natural size (the `width` and `height` of the SVG).
#[derive(Clone, Copy)]
pub(super) struct Graphic {
    pub url: &'static str,
    pub width: f32,
    pub height: f32,
}

pub(super) struct Spec {
    pub id: &'static str,
    pub graphic: Option<Graphic>,
    pub title: gpui::SharedString,
    pub subtitle: Option<AnyElement>,
    pub body: Option<AnyElement>,
    /// The action bar, each button taking its share of the width.
    pub buttons: Vec<modal::Button>,
    /// The close button and the scrim (`onClose` of the dialog).
    pub close: modal::Handler,
}

/// The dialog's `size`, its header's `gradientColor` and the graphic's `aspectRatio` (module 772707 `k`, header of
/// module 20742).
#[derive(Clone, Copy)]
pub(super) struct Look {
    /// `md` (480) or `sm` (400).
    pub width: f32,
    pub gradient: (&'static str, &'static str),
    /// The graphic box's width over its height (`aspect-ratio-16/9`, `aspect-ratio-6/4`, ...).
    pub aspect: f32,
}

impl Look {
    /// The defaults of module 772707: `md`, `purple`, 16/9.
    pub(super) const DEFAULT: Look = Look {
        width: channel_status::MODAL_WIDTH,
        gradient: (
            "--expressive-gradient-purple-start",
            "--expressive-gradient-purple-end",
        ),
        aspect: 16. / 9.,
    };
}

/// The graphic box's height in a window this high, or `None` when it is hidden (`max-height:430px`).
pub(super) fn graphic_box_height(window_height: f32) -> Option<f32> {
    graphic_box_height_at(window_height, 16. / 9.)
}

/// As `graphic_box_height`, for a graphic box of this aspect ratio.
pub(super) fn graphic_box_height_at(window_height: f32, aspect: f32) -> Option<f32> {
    if window_height <= 430. {
        return None;
    }
    let full: f32 = 288. / aspect;
    Some(if window_height <= 720. {
        full.min(window_height * 0.15)
    } else {
        full
    })
}

/// The picture's size inside a box `box_height` tall: shrunk to fit 288 by `box_height`, never grown.
pub(super) fn fitted(graphic: Graphic, box_height: f32) -> (f32, f32) {
    let scale = (288. / graphic.width)
        .min(box_height / graphic.height)
        .min(1.);
    (graphic.width * scale, graphic.height * scale)
}

impl Root {
    /// Asks for the header picture of a dialog about to open.
    pub(super) fn request_graphic(&mut self, graphic: Graphic) {
        self.request_image(graphic.url.to_owned(), graphic.url.to_owned());
    }

    pub(super) fn graphic_modal(&self, spec: Spec, cx: &mut Context<Self>) -> AnyElement {
        self.graphic_modal_with(spec, Look::DEFAULT, cx)
    }

    /// The dialog in another size, gradient or graphic shape.
    pub(super) fn graphic_modal_with(
        &self,
        spec: Spec,
        look: Look,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let pose = super::motion::modal_pose(spec.id);
        let close = std::rc::Rc::new(spec.close);
        let close_button = close.clone();
        let close_scrim = close.clone();
        let gradient = channel_status::header_gradient(color(look.gradient.0), color(look.gradient.1));
        let box_height = spec
            .graphic
            .and_then(|_| graphic_box_height_at(self.viewport.1, look.aspect));
        let picture = spec.graphic.zip(box_height).map(|(graphic, box_height)| {
            let (width, height) = fitted(graphic, box_height);
            div()
                .w(px(288.))
                .h(px(box_height))
                .flex()
                .items_center()
                .justify_center()
                .children(
                    self.images
                        .get(graphic.url)
                        .cloned()
                        .map(|art| img(art).w(px(width)).h(px(height))),
                )
        });
        let header = div()
            .relative()
            .flex_none()
            .w(px(look.width - 2.))
            .child(
                img(gradient)
                    .absolute()
                    .top(px(0.))
                    .left(px(0.))
                    .size_full(),
            )
            .child(
                div()
                    .relative()
                    .pt(px(16.))
                    .px(px(32.))
                    .pb(px(16.))
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(px(8.))
                    .text_center()
                    .children(picture.map(|picture| {
                        div()
                            .w_full()
                            .pt(px(20.))
                            .pb(px(16.))
                            .flex()
                            .justify_center()
                            .child(picture)
                    }))
                    .child(
                        div()
                            .w_full()
                            .text_size(px(24.))
                            .line_height(px(32.))
                            .font_weight(FontWeight(600.))
                            .text_color(color("--text-strong"))
                            .child(spec.title),
                    )
                    .children(spec.subtitle.map(|subtitle| {
                        div()
                            .w_full()
                            .text_size(px(16.))
                            .line_height(px(20.))
                            .text_color(color("--text-subtle"))
                            .child(subtitle)
                    })),
            );
        let buttons: Vec<AnyElement> = spec
            .buttons
            .into_iter()
            .map(|button| modal::footer_button(button, cx))
            .collect();
        let panel = div()
            .id(spec.id)
            .opacity(pose.opacity)
            .paint_scale(pose.scale, gpui::point(0.5, 0.5))
            .occlude()
            .relative()
            .w(px(look.width))
            .pt(px(8.))
            .pb(px(16.))
            .rounded(px(12.))
            .border_1()
            .border_color(color("--border-subtle"))
            .bg(color("--background-surface-high"))
            .shadow(vec![gpui::BoxShadow {
                color: gpui::hsla(0., 0., 0., 0.24),
                offset: gpui::point(px(0.), px(12.)),
                blur_radius: px(24.),
                spread_radius: px(0.),
                inset: false,
            }])
            .flex()
            .flex_col()
            .child(header)
            // `bodySpacerTop_dvN-MP` and `bodySpacerBottom_dvN-MP` around the body, 16px each.
            .children(
                spec.body
                    .map(|body| div().flex_none().py(px(16.)).px(px(32.)).child(body)),
            )
            .when(!buttons.is_empty(), |panel| {
                panel.child(
                    div()
                        .flex_none()
                        .px(px(32.))
                        .py(px(16.))
                        .flex()
                        .gap(px(8.))
                        .children(buttons),
                )
            })
            .child(
                super::keyboard::inset_ring_when_focused(
                    div().id(gpui::SharedString::from(format!("{}-close", spec.id))),
                )
                .focusable()
                .tab_index(0)
                .absolute()
                .top(px(8.))
                .right(px(8.))
                .size(px(38.))
                .rounded(px(8.))
                .cursor_pointer()
                .flex()
                .items_center()
                .justify_center()
                .child(icon("modal_close", 20., "--text-strong"))
                .on_click(cx.listener(move |this, _, window, cx| {
                    close_button(this, window, cx);
                    cx.notify();
                })),
            );
        deferred(
            div()
                .id(gpui::SharedString::from(format!("{}-layer", spec.id)))
                .occlude()
                .absolute()
                .inset_0()
                .bg(Rgba {
                    a: f32::from(0xb8u8) / 255. * pose.scrim,
                    ..rgba(0x000000ff)
                })
                .flex()
                .items_center()
                .justify_center()
                .on_click(cx.listener(move |this, _, window, cx| {
                    close_scrim(this, window, cx);
                    cx.notify();
                }))
                .child(panel),
        )
        .with_priority(1)
        .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_graphic_box_follows_the_window_height() {
        assert_eq!(graphic_box_height(900.), Some(162.));
        let near = |height: f32, want: f32| {
            graphic_box_height(height).is_some_and(|got| (got - want).abs() < 0.01)
        };
        assert!(near(720., 108.));
        assert!(near(431., 431. * 0.15));
        assert_eq!(graphic_box_height(430.), None);
    }

    #[test]
    fn a_picture_shrinks_to_its_box_and_never_grows() {
        let wide = Graphic {
            url: "",
            width: 288.,
            height: 162.,
        };
        assert_eq!(fitted(wide, 162.), (288., 162.));
        assert_eq!(fitted(wide, 108.), (192., 108.));
        let teapot = Graphic {
            url: "",
            width: 248.,
            height: 153.,
        };
        // The measured channel status picture: 175 by 108 in a 720 high window.
        let (width, height) = fitted(teapot, 108.);
        assert_eq!(height, 108.);
        assert!((width - 175.06).abs() < 0.01);
    }
}
