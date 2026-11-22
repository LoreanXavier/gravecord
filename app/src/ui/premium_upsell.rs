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
//! Resolves active `user-offer` trial (`722178.FY`: `C6i5Jt`/`cR9ifw`/`8FZfNo`/`xzAcST` + `FREE TRIAL` badge)
//! or discount (`bkQ4bH`) on `TIER_2` per module 831318, falling back to `pj0XBN` ("Get Nitro").

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

/// `maybeOpenProfilePreviewModal` of module 596339: a GIF avatar needs `ANIMATED_AVATAR` (the official reads the
/// file's `image/gif` type; the picked file's extension stands in), a banner `PROFILE_PREMIUM_FEATURES`.
pub(super) fn animated_upload_blocked(
    banner: bool,
    file_name: &str,
    premium: dc_state::premium::PremiumType,
) -> bool {
    use dc_state::premium::Feature;
    if banner {
        !premium.has(Feature::ProfilePremiumFeatures)
    } else {
        file_name.to_ascii_lowercase().ends_with(".gif") && !premium.has(Feature::AnimatedAvatar)
    }
}

/// Sticker Pack Nitro Upsell Modal state (module 397313: `TBsJfQ`, `uAfKTe`, `sWnv5M`,
/// `pqHIf7`, `cpT0Cq`, `Gd/XHF`, opened from sticker picker when clicking a Nitro sticker).
#[derive(Clone, Debug)]
pub(crate) struct StickerNitroUpsell {
    pub monthly_price: String,
    pub offer_badge: Option<String>,
}

/// "Reimagine your profile with Nitro" (module 536015): its picture.
pub(super) const TRY_IT_OUT_GRAPHIC: Graphic = Graphic {
    url: "https://cdn.discordapp.com/assets/content/47cc9b4a38d020f9f0bf747920f0a9b0d02109a9eb9a15510e7a08b7fb106350.svg",
    width: 237.,
    height: 192.,
};

/// Resolves the primary CTA label and optional expressive header badge (`FREE TRIAL`) from a
/// `GET /users/@me/billing/user-offer` JSON payload for `tier_sku` (module 831318 / 722178.FY).
pub(super) fn resolve_upsell_offer(
    offer: Option<&serde_json::Value>,
    tier_sku: u64,
) -> (SharedString, Option<&'static str>) {
    let tier_str = tier_sku.to_string();
    if let Some(offer) = offer {
        let trial = &offer["user_trial_offer"]["subscription_trial"];
        if trial["sku_id"].as_str() == Some(tier_str.as_str()) {
            let interval = trial["interval"].as_u64().unwrap_or(1);
            let count = trial["interval_count"].as_u64().unwrap_or(1) as f64;
            let price = "$0";
            let label = match interval {
                // DAY = 3, WEEK = 4, MONTH = 1, YEAR = 2
                3 => dc_intl::format("cR9ifw", &[("days", count.into()), ("price", price.into())]),
                4 => dc_intl::format(
                    "C6i5Jt",
                    &[("weeks", count.into()), ("price", price.into())],
                ),
                2 => dc_intl::format(
                    "xzAcST",
                    &[("years", count.into()), ("price", price.into())],
                ),
                _ => dc_intl::format(
                    "8FZfNo",
                    &[("months", count.into()), ("price", price.into())],
                ),
            };
            return (SharedString::from(label), Some("FREE TRIAL"));
        }
        let discount = &offer["user_discount_offer"]["discount"];
        if let Some(amount) = discount["amount"].as_u64().filter(|&n| n > 0) {
            let matches_tier = discount["plan_ids"]
                .as_array()
                .is_none_or(|plans| plans.is_empty())
                || discount["sku_id"].as_str() == Some(tier_str.as_str());
            if matches_tier {
                let label = dc_intl::format("bkQ4bH", &[("percent", (amount as f64).into())]);
                return (SharedString::from(label), None);
            }
        }
    }
    (SharedString::from(dc_intl::t("pj0XBN")), None)
}

impl Root {
    pub(super) fn premium_upsell(&self, spec: Spec, cx: &mut Context<Self>) -> AnyElement {
        let [close_id, perks_id, get_id] = spec.ids;
        let close = std::rc::Rc::new(spec.close);
        let (close_scrim, close_button) = (close.clone(), close.clone());
        let on_secondary = spec.on_secondary;
        let on_primary = spec.on_primary;
        let (primary_label, offer_badge) = resolve_upsell_offer(
            self.fetched.get("user-offer"),
            dc_state::premium::sku::TIER_2,
        );
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
            .min_w(px(202.))
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
            .child(primary_label)
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
                // Only this dialog closes, not one under it (a picker).
                cx.stop_propagation();
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
                            .when_some(offer_badge, |col, badge| {
                                col.child(
                                    div()
                                        .px(px(8.))
                                        .py(px(2.))
                                        .rounded(px(999.))
                                        .bg(rgba(0xffffffff))
                                        .text_size(px(11.))
                                        .font_weight(FontWeight(700.))
                                        .text_color(rgba(0x000000ff))
                                        .child(badge),
                                )
                            })
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

    /// `G` of module 688796, from `maybeOpenProfilePreviewModal` of module 596339 after the cropper: a GIF avatar
    /// without `ANIMATED_AVATAR`, or a banner without `PROFILE_PREMIUM_FEATURES`, opens the animated upsell instead of
    /// becoming the pending picture. Returns whether it opened.
    pub(super) fn animated_upload_gate(
        &mut self,
        banner: bool,
        file_name: &str,
        cx: &mut Context<Self>,
    ) -> bool {
        let blocked = animated_upload_blocked(banner, file_name, self.premium());
        if blocked {
            self.animated_upload_upsell = Some(banner);
            cx.notify();
        }
        blocked
    }

    /// Module 688796 on the premium upsell dialog: "Bring your profile to life with animated avatars" / "...with an
    /// animated banner", "Access custom profiles, emoji upgrades, and more with Nitro.", View all Nitro perks. Its
    /// graphic (`ANIMATED_AVATAR_MODAL_UPSELL` / `ANIMATED_BANNER_MODAL_UPSELL` of module 49792) is a looping video:
    /// its space is kept and the video is not drawn.
    pub(super) fn animated_upload_upsell(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let banner = self.animated_upload_upsell?;
        Some(self.premium_upsell(
            Spec {
                id: "animated-upload-upsell",
                ids: [
                    "animated-upload-upsell-close",
                    "animated-upload-upsell-perks",
                    "animated-upload-upsell-get",
                ],
                header: Header::Video,
                title: dc_intl::t(if banner { "zXPa2C" } else { "X7tO60" }).into(),
                subtitle: dc_intl::t(if banner { "yQAnf4" } else { "ifCRdB" }).into(),
                secondary: dc_intl::t("PcTCB7").into(),
                // `onSecondaryClick` (`s7` of module 192308 closes every modal), then 87719 `x`: the Nitro page.
                on_secondary: Box::new(|this, _, cx| {
                    this.animated_upload_upsell = None;
                    this.open_nitro(cx);
                }),
                on_primary: Box::new(|this, _, cx| {
                    this.animated_upload_upsell = None;
                    this.open_purchase(false, cx);
                }),
                close: Box::new(|this, _, _| this.animated_upload_upsell = None),
                priority: 3,
            },
            cx,
        ))
    }

    /// Opens the Sticker Pack Nitro Upsell modal (module 397313).
    pub(super) fn open_sticker_nitro_upsell(
        &mut self,
        offer_badge: Option<String>,
        cx: &mut Context<Self>,
    ) {
        self.sticker_upsell = Some(StickerNitroUpsell {
            monthly_price: "$9.99".to_owned(),
            offer_badge,
        });
        cx.notify();
    }

    /// Renders the Sticker Pack Nitro Upsell modal (module 397313: `TBsJfQ`, `uAfKTe`, `sWnv5M`,
    /// `pqHIf7`, `cpT0Cq`, `Gd/XHF`).
    pub(in crate::ui) fn sticker_nitro_upsell_layer(
        &self,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let state = self.sticker_upsell.as_ref()?;
        let intro_text: String = dc_intl::format_runs(
            "TBsJfQ",
            &[("monthlyPrice", state.monthly_price.as_str().into())],
        )
        .into_iter()
        .map(|(s, _)| s)
        .collect();
        let boost_perk = dc_intl::format(
            "sWnv5M",
            &[
                ("numGuildSubscriptions", dc_intl::Arg::Num(2.)),
                ("discountPercent", "30%".into()),
            ],
        );
        let perk_row = |glyph: &'static str, text: String| {
            div()
                .flex()
                .items_center()
                .gap(px(12.))
                .child(
                    div()
                        .flex_none()
                        .size(px(32.))
                        .rounded(px(8.))
                        .bg(color("--background-mod-subtle"))
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(icon(glyph, 18., "--text-strong")),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_size(px(14.))
                        .line_height(px(19.))
                        .font_weight(FontWeight(500.))
                        .text_color(color("--interactive-text-active"))
                        .child(text),
                )
        };
        let cta_label = if state.offer_badge.is_some() {
            dc_intl::t("Gd/XHF")
        } else {
            dc_intl::tr("Get Nitro")
        };
        let dialog = div()
            .id("sticker-pack-premium-upsell-modal")
            .occlude()
            .w(px(440.))
            .rounded(px(12.))
            .bg(color("--modal-background"))
            .border_1()
            .border_color(color("--border-subtle"))
            .overflow_hidden()
            .flex()
            .flex_col()
            .on_click(cx.listener(|_, _, _, cx| cx.stop_propagation()))
            // Gradient hero banner
            .child(
                div()
                    .relative()
                    .h(px(136.))
                    .px(px(24.))
                    .bg(gpui::linear_gradient(
                        135.,
                        gpui::linear_color_stop(rgba(0x5865f2ff), 0.),
                        gpui::linear_color_stop(rgba(0xb845c1ff), 1.),
                    ))
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .gap(px(8.))
                    .children(state.offer_badge.clone().map(|badge| {
                        div()
                            .px(px(10.))
                            .py(px(3.))
                            .rounded_full()
                            .bg(rgba(0x111214aa))
                            .text_size(px(11.))
                            .font_weight(FontWeight(700.))
                            .text_color(color("--white"))
                            .child(badge)
                    }))
                    .child(icon("nitro_wheel", 42., "--white"))
                    .child(
                        div()
                            .id("sticker-upsell-close")
                            .absolute()
                            .top(px(12.))
                            .right(px(12.))
                            .size(px(28.))
                            .rounded_full()
                            .bg(rgba(0x00000044))
                            .hover(|s| s.bg(rgba(0x00000066)))
                            .cursor_pointer()
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(icon("modal_close", 16., "--white"))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.sticker_upsell = None;
                                cx.notify();
                            })),
                    ),
            )
            // Body & 3 perks
            .child(
                div()
                    .p(px(20.))
                    .flex()
                    .flex_col()
                    .gap(px(14.))
                    .child(
                        div()
                            .text_size(px(14.))
                            .line_height(px(20.))
                            .text_color(color("--text-default"))
                            .child(intro_text),
                    )
                    .child(perk_row(
                        "menu_add_reaction",
                        dc_intl::t("uAfKTe").to_owned(),
                    ))
                    .child(perk_row("boost_gem", boost_perk))
                    .child(perk_row("nitro_wheel", dc_intl::t("pqHIf7").to_owned())),
            )
            // Footer
            .child(
                div()
                    .px(px(20.))
                    .py(px(14.))
                    .bg(color("--modal-footer-background"))
                    .border_t_1()
                    .border_color(color("--border-subtle"))
                    .flex()
                    .items_center()
                    .justify_end()
                    .gap(px(8.))
                    .child(
                        div()
                            .id("sticker-upsell-cancel")
                            .h(px(36.))
                            .px(px(16.))
                            .rounded(px(8.))
                            .bg(color("--control-secondary-background-default"))
                            .hover(|s| s.bg(color("--control-secondary-background-hover")))
                            .cursor_pointer()
                            .flex()
                            .items_center()
                            .text_size(px(14.))
                            .font_weight(FontWeight(500.))
                            .text_color(color("--text-strong"))
                            .child(dc_intl::t("cpT0Cq"))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.sticker_upsell = None;
                                cx.notify();
                            })),
                    )
                    .child(
                        div()
                            .id("sticker-upsell-subscribe")
                            .h(px(36.))
                            .px(px(16.))
                            .rounded(px(8.))
                            .bg(color("--status-positive"))
                            .hover(|s| s.opacity(0.9))
                            .cursor_pointer()
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .text_size(px(14.))
                            .font_weight(FontWeight(600.))
                            .text_color(color("--white"))
                            .child(icon("nitro_wheel", 16., "--white"))
                            .child(cta_label)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.sticker_upsell = None;
                                this.open_purchase(false, cx);
                                cx.notify();
                            })),
                    ),
            );
        Some(
            deferred(
                div()
                    .id("sticker-upsell-backdrop")
                    .occlude()
                    .absolute()
                    .inset_0()
                    .bg(rgba(0x000000b3))
                    .flex()
                    .items_center()
                    .justify_center()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.sticker_upsell = None;
                        cx.notify();
                    }))
                    .child(dialog),
            )
            .with_priority(3)
            .into_any_element(),
        )
    }

    /// Upload-size Nitro upsell modal ("This video was shared with Nitro", module 232118:
    /// `5x18pr`, `TaT999` for Basic/Classic or `2op/Su` for Free, `PcTCB7`, `pj0XBN`).
    pub(super) fn nitro_upload_badge_upsell(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.nitro_upload_badge_upsell {
            return None;
        }
        let max_size = "500MB";
        let subtitle = video_upload_upsell_subtitle(self.premium(), max_size);
        Some(self.premium_upsell(
            Spec {
                id: "nitro-upload-badge-upsell",
                ids: [
                    "nitro-upload-badge-close",
                    "nitro-upload-badge-perks",
                    "nitro-upload-badge-get",
                ],
                header: Header::Video,
                title: dc_intl::t("5x18pr").into(),
                subtitle: subtitle.into(),
                secondary: dc_intl::t("PcTCB7").into(),
                on_secondary: Box::new(|this, _, cx| {
                    this.nitro_upload_badge_upsell = false;
                    this.open_nitro(cx);
                }),
                on_primary: Box::new(|this, _, cx| {
                    this.nitro_upload_badge_upsell = false;
                    this.open_purchase(false, cx);
                }),
                close: Box::new(|this, _, cx| {
                    this.nitro_upload_badge_upsell = false;
                    cx.notify();
                }),
                priority: 3,
            },
            cx,
        ))
    }

    #[cfg(feature = "capture")]
    pub(crate) fn script_sticker_upsell(&mut self, arg: &str, cx: &mut Context<Self>) -> String {
        match arg.trim() {
            "video-upload" => {
                self.nitro_upload_badge_upsell = true;
                cx.notify();
                "sticker-upsell: video-upload".to_owned()
            }
            "close" => {
                self.sticker_upsell = None;
                self.nitro_upload_badge_upsell = false;
                cx.notify();
                "sticker-upsell: closed".to_owned()
            }
            "trial" => {
                self.open_sticker_nitro_upsell(Some("1 Month Free".to_owned()), cx);
                "sticker-upsell: trial".to_owned()
            }
            _ => {
                self.open_sticker_nitro_upsell(None, cx);
                "sticker-upsell: default".to_owned()
            }
        }
    }

    /// Module 633772 — Nitro Required Modal: compact 440px dialog (`BNg1ir` "Nitro Required",
    /// 267x120 theme-aware artwork, tier-dependent body `mKA9OJ` for Classic or `sqSlm4` otherwise,
    /// and `P3etAv` "Get Nitro!" primary CTA that opens the Nitro purchase flow).
    #[allow(dead_code)]
    pub(super) fn open_nitro_required_modal(&mut self, cx: &mut Context<Self>) {
        self.nitro_required_modal = true;
        let url = nitro_required_art_url(super::theme() == dc_tokens::Theme::Light);
        self.request_image(url.to_owned(), url.to_owned());
        cx.notify();
    }

    pub(super) fn nitro_required_modal(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.nitro_required_modal {
            return None;
        }
        let premium = self.premium();
        let art_url = nitro_required_art_url(super::theme() == dc_tokens::Theme::Light);
        let art = self.images.get(art_url).cloned();
        let body_text = nitro_required_body(premium);
        let pose = crate::ui::motion::modal_pose("nitro-required-modal");
        let layer = div()
            .id("nitro-required-modal-layer")
            .occlude()
            .absolute()
            .inset_0()
            .bg(pose.dim(rgba(0x000000b8)))
            .flex()
            .items_center()
            .justify_center()
            .on_click(cx.listener(|this, _, _, cx| {
                cx.stop_propagation();
                this.nitro_required_modal = false;
                cx.notify();
            }))
            .child(
                div()
                    .posed(pose)
                    .parity_with_border("nitro-required-modal", 1.)
                    .id("nitro-required-modal")
                    .occlude()
                    .relative()
                    .w(px(440.))
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
                        // Header with centered title + top-right close button
                        div()
                            .relative()
                            .px(px(24.))
                            .pt(px(20.))
                            .pb(px(12.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(
                                div()
                                    .text_size(px(20.))
                                    .line_height(px(24.))
                                    .font_weight(FontWeight(700.))
                                    .text_color(color("--text-strong"))
                                    .child(dc_intl::t("BNg1ir")),
                            )
                            .child(
                                div()
                                    .id("nitro-required-close")
                                    .absolute()
                                    .top(px(16.))
                                    .right(px(16.))
                                    .size(px(32.))
                                    .rounded(px(8.))
                                    .hover(|s| s.bg(color("--background-mod-subtle")))
                                    .cursor_pointer()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .child(icon("close", 20., "--interactive-icon-default"))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.nitro_required_modal = false;
                                        cx.notify();
                                    })),
                            ),
                    )
                    .child(
                        // 267x120 theme-aware hero art + description body
                        div()
                            .px(px(24.))
                            .pb(px(20.))
                            .flex()
                            .flex_col()
                            .items_center()
                            .gap(px(16.))
                            .child(
                                div()
                                    .w(px(267.))
                                    .h(px(120.))
                                    .rounded(px(8.))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .when_some(art, |d, picture| {
                                        d.child(img(picture).w(px(267.)).h(px(120.)))
                                    })
                                    .when(self.images.get(art_url).is_none(), |d| {
                                        d.bg(color("--background-mod-subtle")).child(icon(
                                            "nitro_wheel",
                                            56.,
                                            "--brand-500",
                                        ))
                                    }),
                            )
                            .child(
                                div()
                                    .w_full()
                                    .text_center()
                                    .text_size(px(14.))
                                    .line_height(px(20.))
                                    .text_color(color("--text-default"))
                                    .child(body_text),
                            ),
                    )
                    .child(
                        // Footer with Get Nitro! primary button (`P3etAv`)
                        div()
                            .px(px(24.))
                            .py(px(16.))
                            .border_t_1()
                            .border_color(color("--border-subtle"))
                            .bg(color("--background-base-lower"))
                            .rounded_b(px(12.))
                            .flex()
                            .justify_end()
                            .child(
                                div()
                                    .id("nitro-required-cta")
                                    .h(px(38.))
                                    .px(px(20.))
                                    .rounded(px(8.))
                                    .bg(color("--brand-500"))
                                    .hover(|s| s.bg(color("--brand-560")))
                                    .cursor_pointer()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .gap(px(6.))
                                    .text_size(px(14.))
                                    .font_weight(FontWeight(500.))
                                    .text_color(color("--white"))
                                    .child(icon("nitro_wheel", 18., "--white"))
                                    .child(dc_intl::t("P3etAv"))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.nitro_required_modal = false;
                                        this.open_purchase(false, cx);
                                        cx.notify();
                                    })),
                            ),
                    ),
            );
        Some(deferred(layer).with_priority(3).into_any_element())
    }

    #[cfg(feature = "capture")]
    pub(crate) fn script_nitro_required(&mut self, arg: &str, cx: &mut Context<Self>) -> String {
        if arg.trim() == "close" {
            self.nitro_required_modal = false;
            cx.notify();
            return "nitro-required: closed".to_owned();
        }
        self.open_nitro_required_modal(cx);
        "nitro-required: opened".to_owned()
    }
}

/// Theme-aware hero illustration for module 633772 (267x120 SVG asset on Discord CDN).
pub(super) fn nitro_required_art_url(light: bool) -> &'static str {
    if light {
        "https://discord.com/assets/4290a31884b6c920.svg"
    } else {
        "https://discord.com/assets/711e1f277360806b.svg"
    }
}

/// Tier-dependent body copy for module 633772 (`mKA9OJ` for Classic, `sqSlm4` otherwise).
pub(super) fn nitro_required_body(premium: dc_state::premium::PremiumType) -> String {
    if premium == dc_state::premium::PremiumType::Classic {
        dc_intl::t("mKA9OJ").to_owned()
    } else {
        dc_intl::t("sqSlm4").to_owned()
    }
}

/// Subtitle selector for module 232118 (`TaT999` for Basic/Classic vs `2op/Su` for Free).
pub(super) fn video_upload_upsell_subtitle(
    premium: dc_state::premium::PremiumType,
    max_size: &str,
) -> String {
    use dc_intl::Arg;
    use dc_state::premium::PremiumType;
    use std::borrow::Cow;
    match premium {
        PremiumType::Basic | PremiumType::Classic => dc_intl::format(
            "TaT999",
            &[
                ("currentMaxSize", Arg::Str(Cow::Borrowed("50MB"))),
                ("maxSize", Arg::Str(Cow::Borrowed(max_size))),
            ],
        ),
        _ => dc_intl::format("2op/Su", &[("maxSize", Arg::Str(Cow::Borrowed(max_size)))]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dc_state::premium::PremiumType;

    #[test]
    fn video_upload_upsell_232118_copy() {
        let free = video_upload_upsell_subtitle(PremiumType::None, "500MB");
        assert!(free.contains("500MB"));
        let basic = video_upload_upsell_subtitle(PremiumType::Basic, "500MB");
        assert!(basic.contains("50MB") && basic.contains("500MB"));
        for k in ["5x18pr", "TaT999", "2op/Su", "PcTCB7", "pj0XBN"] {
            assert!(dc_intl::has_key(k), "missing 232118 key {k}");
        }
    }

    #[test]
    fn a_gif_avatar_needs_animated_avatars_and_a_banner_needs_nitro() {
        assert!(animated_upload_blocked(false, "cat.GIF", PremiumType::None));
        assert!(animated_upload_blocked(
            false,
            "cat.gif",
            PremiumType::Basic
        ));
        assert!(!animated_upload_blocked(
            false,
            "cat.gif",
            PremiumType::Classic
        ));
        assert!(!animated_upload_blocked(
            false,
            "cat.png",
            PremiumType::None
        ));
        assert!(animated_upload_blocked(
            true,
            "wide.png",
            PremiumType::Classic
        ));
        assert!(!animated_upload_blocked(
            true,
            "wide.gif",
            PremiumType::Nitro
        ));
    }

    #[test]
    fn resolve_upsell_offer_picks_trial_discount_or_default() {
        let tier = dc_state::premium::sku::TIER_2;
        let (label, badge) = resolve_upsell_offer(None, tier);
        assert_eq!(label.as_ref(), "Get Nitro");
        assert_eq!(badge, None);

        let trial = serde_json::json!({
            "user_trial_offer": {
                "subscription_trial": {
                    "sku_id": tier.to_string(),
                    "interval": 1,
                    "interval_count": 1
                }
            }
        });
        let (t_label, t_badge) = resolve_upsell_offer(Some(&trial), tier);
        assert!(t_label.contains("1 month"), "got {t_label}");
        assert_eq!(t_badge, Some("FREE TRIAL"));

        let disc = serde_json::json!({
            "user_discount_offer": {
                "discount": { "amount": 30, "sku_id": tier.to_string() }
            }
        });
        let (d_label, d_badge) = resolve_upsell_offer(Some(&disc), tier);
        assert!(d_label.contains("30%"), "got {d_label}");
        assert_eq!(d_badge, None);
    }

    #[test]
    fn nitro_required_modal_633772_copy_and_assets() {
        for k in ["BNg1ir", "mKA9OJ", "sqSlm4", "P3etAv"] {
            assert!(dc_intl::has_key(k), "missing 633772 key {k}");
        }
        let classic = nitro_required_body(PremiumType::Classic);
        let free = nitro_required_body(PremiumType::None);
        assert_ne!(classic, free);
        assert!(nitro_required_art_url(false).contains("711e1f277360806b.svg"));
        assert!(nitro_required_art_url(true).contains("4290a31884b6c920.svg"));
    }
}
