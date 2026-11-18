//! The premium upsell dialog (module 831318): the design system's dialog with a header graphic (module 772707) over
//! the `nitro-pink` gradient, its graphic, title and subtitle centred, then the secondary call to action beside the
//! white `expressive` Get Nitro button with the Nitro wheel (module 403581) that closes the dialog and opens the
//! purchase flow (783420's `onClick`). Drawn for:
//!
//! * the upload limit (`R` of module 16407, `upload_error.rs`) and HD streaming (module 826789, `go_live.rs`), whose
//!   graphic is a looping video: its space is kept and the video is not drawn;
//! * "Reimagine your profile with Nitro" (module 536015, the profile editor's "Level up your look" link): its CDN
//!   picture in the header's graphic box (`graphic_modal.rs`).
//!
//! GAP: the primary label is always Get Nitro; 831318 names a trial ("Try N months free") or a discount ("Save N% on
//! Nitro") when the account has such an offer, and adds the `free_trial` or `beta` badge, which this client does not
//! read.

use super::graphic_modal::{Graphic, fitted, graphic_box_height};
use super::modal::{self, Tone};
use super::{Root, color, icon};
use crate::parity::Parity;
use crate::ui::motion::Posed as _;
use gpui::{
    AnyElement, Context, FontWeight, IntoElement, SharedString, Window, deferred, div, img,
    prelude::*, px, rgba,
};

/// The header's graphic.
pub(super) enum Header {
    /// A video, not drawn: the 112px it takes in the measured upload upsell are kept.
    Video,
    /// A picture in the graphic box of module 772707.
    Picture(Graphic),
}

type Handler = Box<dyn Fn(&mut Root, &mut Window, &mut Context<Root>)>;

pub(super) struct Spec {
    pub id: &'static str,
    /// The element ids of the close button, the secondary and the primary button.
    pub ids: [&'static str; 3],
    pub header: Header,
    pub title: SharedString,
    pub subtitle: SharedString,
    /// `secondaryCTA`.
    pub secondary: SharedString,
    pub on_secondary: Handler,
    /// What Get Nitro does after the dialog closes.
    pub on_primary: Handler,
    /// The close button and the scrim.
    pub close: Handler,
    /// The layer's draw priority.
    pub priority: usize,
}

/// "Reimagine your profile with Nitro" (module 536015): its picture.
pub(super) const TRY_IT_OUT_GRAPHIC: Graphic = Graphic {
    url: "https://cdn.discordapp.com/assets/content/47cc9b4a38d020f9f0bf747920f0a9b0d02109a9eb9a15510e7a08b7fb106350.svg",
    width: 237.,
    height: 192.,
};

impl Root {
    pub(super) fn premium_upsell(&self, spec: Spec, cx: &mut Context<Self>) -> AnyElement {
        let [close_id, perks_id, get_id] = spec.ids;
        let close = std::rc::Rc::new(spec.close);
        let (close_scrim, close_button) = (close.clone(), close.clone());
        let on_secondary = spec.on_secondary;
        let on_primary = spec.on_primary;
        let perks = modal::footer_button(
            modal::button(
                perks_id,
                spec.secondary,
                Tone::Secondary,
                move |this, window, cx| on_secondary(this, window, cx),
            ),
            cx,
        );
        let get_nitro = div()
            .id(get_id)
            // `expressiveWrapper_F7qvhY` is 202 wide beside the 204 of the secondary button.
            .flex_none()
            .w(px(202.))
            .h(px(40.))
            .px(px(16.))
            .rounded(px(8.))
            .bg(rgba(0xffffffff))
            .cursor_pointer()
            .flex()
            .items_center()
            .justify_center()
            .gap(px(4.))
            .text_size(px(16.))
            .line_height(px(20.))
            .font_weight(FontWeight(500.))
            .text_color(rgba(0x000000ff))
            .child(icon("nitro_wheel", 20., "--black"))
            .child(dc_intl::tr("Get Nitro"))
            .on_click(cx.listener(move |this, _, window, cx| {
                on_primary(this, window, cx);
                cx.notify();
            }));
        let graphic = match spec.header {
            Header::Video => div().h(px(112.)).w_full().into_any_element(),
            Header::Picture(graphic) => {
                let picture = graphic_box_height(self.viewport.1).map(|box_height| {
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
                div()
                    .w_full()
                    .pt(px(20.))
                    .pb(px(16.))
                    .flex()
                    .justify_center()
                    .children(picture)
                    .into_any_element()
            }
        };
        let pose = crate::ui::motion::modal_pose(spec.id);
        let layer = div()
            .id(SharedString::from(format!("{}-layer", spec.id)))
            .occlude()
            .absolute()
            .inset_0()
            .bg(pose.dim(rgba(0x000000b8)))
            .flex()
            .items_center()
            .justify_center()
            .on_click(cx.listener(move |this, _, window, cx| {
                close_scrim(this, window, cx);
                cx.notify();
            }))
            .child(
                div()
                    .posed(pose)
                    .parity_with_border(spec.id, 1.)
                    .id(spec.id)
                    .occlude()
                    .relative()
                    .overflow_hidden()
                    .w(px(480.))
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
                    .on_click(|_, _, cx| cx.stop_propagation())
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .absolute()
                            .top_0()
                            .left_0()
                            .w(px(478.))
                            .h(px(241.))
                            .child(
                                img(super::upload_error::header_glow(478, 241))
                                    .w(px(478.))
                                    .h(px(241.)),
                            ),
                    )
                    .child(
                        div()
                            .relative()
                            .px(px(32.))
                            .pt(px(16.))
                            .pb(px(16.))
                            .flex()
                            .flex_col()
                            .items_center()
                            .gap(px(8.))
                            .child(graphic)
                            .child(
                                div()
                                    .text_size(px(24.))
                                    .line_height(px(32.))
                                    .font_weight(FontWeight(600.))
                                    // The header is centred: a title on two lines is too.
                                    .text_center()
                                    .text_color(color("--text-strong"))
                                    .child(spec.title),
                            )
                            .child(
                                div()
                                    .text_size(px(16.))
                                    .line_height(px(20.))
                                    .text_center()
                                    .text_color(color("--text-subtle"))
                                    .child(spec.subtitle),
                            )
                            .child(
                                div()
                                    .id(close_id)
                                    .absolute()
                                    .top(px(0.))
                                    .right(px(8.))
                                    .size(px(38.))
                                    .rounded(px(8.))
                                    .cursor_pointer()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .child(icon("modal_close", 20., "--text-strong"))
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        cx.stop_propagation();
                                        close_button(this, window, cx);
                                        cx.notify();
                                    })),
                            ),
                    )
                    .child(
                        div()
                            .relative()
                            .px(px(32.))
                            .pt(px(16.))
                            .flex()
                            .gap(px(8.))
                            .child(perks)
                            .child(get_nitro),
                    ),
            );
        deferred(layer)
            .with_priority(spec.priority)
            .into_any_element()
    }

    /// Opens "Reimagine your profile with Nitro" (module 536015 `K`).
    #[cfg_attr(not(feature = "capture"), allow(dead_code))]
    pub(super) fn open_try_it_out_upsell(&mut self, cx: &mut Context<Self>) {
        self.request_graphic(TRY_IT_OUT_GRAPHIC);
        self.try_it_out_upsell = true;
        cx.notify();
    }

    /// Module 536015's dialog, while open.
    pub(super) fn try_it_out_upsell(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.try_it_out_upsell {
            return None;
        }
        Some(self.premium_upsell(
            Spec {
                id: "try-it-out-upsell",
                ids: [
                    "try-it-out-upsell-close",
                    "try-it-out-upsell-perks",
                    "try-it-out-upsell-get",
                ],
                header: Header::Picture(TRY_IT_OUT_GRAPHIC),
                title: dc_intl::t("reVc0M").into(),
                subtitle: dc_intl::t("KckEj7").into(),
                secondary: dc_intl::t("PcTCB7").into(),
                // 87719 `x`: the dialog closes and the Nitro page opens.
                on_secondary: Box::new(|this, _, cx| {
                    this.try_it_out_upsell = false;
                    this.open_nitro(cx);
                }),
                on_primary: Box::new(|this, _, cx| {
                    this.try_it_out_upsell = false;
                    this.open_purchase(false, cx);
                }),
                close: Box::new(|this, _, _| this.try_it_out_upsell = false),
                priority: 3,
            },
            cx,
        ))
    }
}
