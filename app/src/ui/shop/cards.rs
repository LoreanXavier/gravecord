//! The product card (`productCardContainer_UGbKo5`, module 987421): a 246px square with the preview, a footer gradient
//! and, on hover, the Buy and gift buttons in place of the price line.

use super::data::{self, Product};
use super::{Root, color, icon, text};
use crate::parity::Parity;
use gpui::{
    AnyElement, Context, FontWeight, IntoElement, ObjectFit, SharedString, Window, div, img,
    linear_color_stop, linear_gradient, prelude::*, px,
};

/// The profile mock the effect cards sit on (`preview_lG0NAh` in the capture).
pub(super) const PROFILE_MOCK: &str = "https://cdn.discordapp.com/assets/content/3e995a411944311d4d318143819ce2d6623bb74c0fc162d6726e0377c3c31a4f.png";

/// `https://cdn.discordapp.com/media/v1/collectibles-shop/<id>/static`: the still art of an item or a product.
pub(super) fn item_static(id: &str) -> String {
    format!("https://cdn.discordapp.com/media/v1/collectibles-shop/{id}/static")
}

impl Root {
    pub(super) fn shop_product(&self, sku: &str) -> Option<Product> {
        self.shop.products.get(sku).cloned()
    }

    /// Whether the account has Nitro (the Nitro price list applies).
    pub(super) fn shop_has_nitro(&self) -> bool {
        self.stores.account.premium_type == Some(2)
    }

    /// The card's art by product kind: bundle backdrop and cut-out, avatar with its decoration, the nameplate strip on a
    /// mock member row, or the profile mock for an effect.
    ///
    /// `brand` paints the avatar behind a decoration in the brand color (the modal) instead of the card's gray;
    /// an effect is drawn by the shared profile-effect renderer (`hovering`: the pointer is over the card).
    pub(super) fn shop_preview(
        &mut self,
        product: &Product,
        size: f32,
        brand: bool,
        hovering: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let item = product.items.first();
        let deco = move |this: &mut Root, id: &str, avatar: f32, ring: f32| -> AnyElement {
            let art = this.shop_img_box(&item_static(id), avatar.max(ring), avatar.max(ring));
            div()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .child(
                    div()
                        .relative()
                        .size(px(ring))
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(
                            div()
                                .size(px(avatar))
                                .rounded_full()
                                .bg(if brand {
                                    color("--background-brand")
                                } else {
                                    color("--background-mod-strong")
                                })
                                .flex()
                                .items_center()
                                .justify_center()
                                .child(icon(
                                    "sb_discord",
                                    avatar * 0.55,
                                    if brand { "--white" } else { "--text-muted" },
                                )),
                        )
                        .children(art.map(|art| {
                            img(art)
                                .absolute()
                                .top_0()
                                .left_0()
                                .size(px(ring))
                                .object_fit(ObjectFit::Contain)
                        })),
                )
                .into_any_element()
        };
        match product.kind {
            data::KIND_BUNDLE => self.shop_bundle_preview(product, size, hovering, brand),
            data::KIND_EXTERNAL => {
                let url = data::external_art_url(&product.sku_id);
                let badge = product.sku_id == data::ORB_BADGE;
                let art = url.and_then(|url| self.shop_img_box(&url, size, 252.));
                // `externalProductWrapper`: 252 high and centred, inside the card's bottom padding of 56.
                div()
                    .size_full()
                    .pb(px(56.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(
                        div()
                            .relative()
                            .flex_none()
                            .w_full()
                            .h(px(252.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .children(art.map(|art| {
                                if badge {
                                    img(art)
                                        .flex_none()
                                        .size(px(120.))
                                        .object_fit(ObjectFit::Contain)
                                        .into_any_element()
                                } else {
                                    img(art)
                                        .absolute()
                                        .top_0()
                                        .left_0()
                                        .size_full()
                                        .object_fit(ObjectFit::Contain)
                                        .into_any_element()
                                }
                            })),
                    )
                    .into_any_element()
            }
            data::KIND_AVATAR_DECORATION => {
                let id = item.map_or(product.sku_id.clone(), |item| item.sku_id.clone());
                if brand {
                    deco(self, &id, size * 0.46, size * 0.55)
                } else {
                    // `SIZE_120` with its 144px decoration, centred in the preview area above the 56px foot
                    // (`productPreviewContainer_UGbKo5.hasBottomContent`), at the scale of a 242px card.
                    let scale = size / 242.;
                    div()
                        .size_full()
                        .px(px(8. * scale))
                        .pb(px(56. * scale))
                        .child(deco(self, &id, 120. * scale, 144. * scale))
                        .into_any_element()
                }
            }
            data::KIND_NAMEPLATE => {
                let id = item.map_or(product.sku_id.clone(), |item| item.sku_id.clone());
                let art = self.shop_img(&item_static(&id));
                div()
                    .size_full()
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .gap(px(8.))
                    .child(skeleton_row(0.5))
                    .child(
                        div()
                            .relative()
                            .w(px(size - 34.))
                            .h(px((size - 34.) * 0.1875))
                            .rounded(px(4.))
                            .overflow_hidden()
                            .children(
                                art.map(|art| img(art).size_full().object_fit(ObjectFit::Cover)),
                            )
                            .child(
                                div()
                                    .absolute()
                                    .left(px(8.))
                                    .top_0()
                                    .bottom_0()
                                    .flex()
                                    .items_center()
                                    .gap(px(8.))
                                    .child(
                                        div()
                                            .size(px(24.))
                                            .rounded_full()
                                            .bg(color("--background-mod-strong")),
                                    )
                                    .child(skeleton_bar(80.)),
                            ),
                    )
                    .child(skeleton_row(0.35))
                    .into_any_element()
            }
            data::KIND_PROFILE_EFFECT | data::KIND_PROFILE_FRAME => {
                // `preview_lG0NAh`: the profile mock covering the card from its top.
                let mock = self.shop_cover_at(Some(PROFILE_MOCK), size, size, (0.5, 0.));
                let mut urls: Vec<String> = Vec::new();
                if let Some(item) = item {
                    if let Some(thumbnail) = &item.thumbnail {
                        urls.push(thumbnail.clone());
                    }
                    for layer in &item.layers {
                        urls.push(format!(
                            "https://cdn.discordapp.com/media/v1/collectibles-shop/{}/{layer}/static",
                            item.sku_id
                        ));
                    }
                }
                let effect = (product.kind == data::KIND_PROFILE_EFFECT)
                    .then(|| product.sku_id.parse::<u64>().ok())
                    .flatten()
                    .and_then(|sku| {
                        let surface = crate::ui::profile_effects::Surface::shop_preview(
                            size, hovering, false,
                        );
                        self.profile_effect_layer(
                            &format!("shop-{}", product.sku_id),
                            sku,
                            surface,
                            window,
                            cx,
                        )
                    });
                let layers: Vec<_> = if effect.is_some() {
                    Vec::new()
                } else {
                    urls.iter().filter_map(|url| self.shop_img(url)).collect()
                };
                div()
                    .relative()
                    .size_full()
                    .overflow_hidden()
                    .children(mock)
                    .children(effect)
                    .children(layers.into_iter().map(|art| {
                        img(art)
                            .absolute()
                            .top_0()
                            .left_0()
                            .size_full()
                            .object_fit(ObjectFit::Cover)
                    }))
                    .into_any_element()
            }
            _ => {
                let art = self.shop_img(&item_static(&product.sku_id));
                div()
                    .size_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .children(
                        art.map(|art| img(art).size(px(size * 0.5)).object_fit(ObjectFit::Contain)),
                    )
                    .into_any_element()
            }
        }
    }

    /// A bundle's preview (module 842385). With `fg_static` the art covers the card from its foot
    /// (`preview_4ZmCSI`: `object-fit: cover; object-position: bottom`), the animated art in its place and 8% larger
    /// while hovered. Without it the bundle's own items are laid out (`container_4ZmCSI`): the profile effect at the
    /// left, the avatar decoration at the right and, with all three kinds, the nameplate across the foot.
    fn shop_bundle_preview(
        &mut self,
        product: &Product,
        size: f32,
        hovering: bool,
        brand: bool,
    ) -> AnyElement {
        if brand {
            // The product modal: backdrop and foreground over the whole preview.
            let bg = self.shop_cover_at(product.bg_static.as_deref(), size, size, (0.5, 0.5));
            let fg = self.shop_cover_at(product.fg_static.as_deref(), size, size, (0.5, 1.));
            return div()
                .relative()
                .size_full()
                .overflow_hidden()
                .children(bg)
                .children(fg)
                .into_any_element();
        }
        if let Some(fg_static) = product.fg_static.clone() {
            let animated = product
                .fg_animated
                .as_deref()
                .filter(|_| hovering && !crate::ui::motion::reduced());
            let grow = if hovering { 1.08 } else { 1. };
            let drawn = size * grow;
            let art = animated
                .and_then(|url| self.shop_cover_at(Some(url), drawn, drawn, (0.5, 1.)))
                .or_else(|| self.shop_cover_at(Some(&fg_static), drawn, drawn, (0.5, 1.)));
            return div()
                .relative()
                .size_full()
                .overflow_hidden()
                .child(
                    div()
                        .absolute()
                        .left(px((size - drawn) / 2.))
                        .top(px((size - drawn) / 2.))
                        .size(px(drawn))
                        .children(art),
                )
                .into_any_element();
        }
        let effect = product
            .items
            .iter()
            .find(|item| item.kind == data::KIND_PROFILE_EFFECT)
            .cloned();
        let decoration = product
            .items
            .iter()
            .find(|item| item.kind == data::KIND_AVATAR_DECORATION)
            .cloned();
        let nameplate = product
            .items
            .iter()
            .find(|item| item.kind == data::KIND_NAMEPLATE)
            .cloned();
        let three = effect.is_some() && decoration.is_some() && nameplate.is_some();
        // The preview area above the card's 56px foot, and the 1.1 box inside it.
        let scale = size / 242.;
        let area_height = 186. * scale;
        let box_width = area_height * 1.1;
        let effect_art = effect.as_ref().map(|item| {
            let height = area_height * 0.8;
            let width = height * if three { 0.8 } else { 0.86 };
            let mock = self.shop_cover_at(Some(PROFILE_MOCK), width, height, (0.5, 0.));
            let still = item
                .thumbnail
                .clone()
                .unwrap_or_else(|| item_static(&item.sku_id));
            let art = self.shop_cover_at(Some(&still), width, height, (0.5, 0.));
            div()
                .absolute()
                .left(px(box_width * 0.06))
                .top(px(area_height * 0.1))
                .w(px(width))
                .h(px(height))
                .rounded(px(8.))
                .overflow_hidden()
                .children(mock)
                .children(art)
        });
        let avatar = if three { 72. } else { 80. } * scale;
        let decoration_art = decoration.as_ref().map(|item| {
            let ring = avatar * 1.2;
            let art = self.shop_img_box(&item_static(&item.sku_id), ring, ring);
            div()
                .absolute()
                .right(px(box_width * 0.06))
                .top(px(area_height * if three { 0.12 } else { 0.22 }))
                .size(px(ring))
                .flex()
                .items_center()
                .justify_center()
                .child(
                    div()
                        .size(px(avatar))
                        .rounded_full()
                        .bg(gpui::rgb(0x0c0e0e))
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(icon("sb_discord", avatar * 0.55, "--text-muted")),
                )
                .children(art.map(|art| {
                    img(art)
                        .absolute()
                        .top_0()
                        .left_0()
                        .size(px(ring))
                        .object_fit(ObjectFit::Contain)
                }))
        });
        let nameplate_art = nameplate.filter(|_| three).map(|item| {
            let width = box_width * 0.96;
            let art = self.shop_cover_at(
                Some(&item_static(&item.sku_id)),
                width,
                42. * scale,
                (0.5, 0.5),
            );
            div()
                .absolute()
                .left(px(box_width * 0.02))
                .bottom(px(area_height * 0.14))
                .w(px(width))
                .h(px(42. * scale))
                .rounded(px(8.))
                .overflow_hidden()
                .bg(color("--background-base-low"))
                .children(art)
                .child(
                    div()
                        .absolute()
                        .left(px(8.))
                        .top_0()
                        .bottom_0()
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .child(
                            div()
                                .size(px(24.))
                                .rounded_full()
                                .bg(color("--background-mod-strong")),
                        )
                        .child(skeleton_bar(80.)),
                )
        });
        div()
            .size_full()
            .pb(px(56. * scale))
            .flex()
            .items_center()
            .justify_center()
            .child(
                div()
                    .relative()
                    .w(px(box_width))
                    .h(px(area_height))
                    .children(effect_art)
                    .children(nameplate_art)
                    .children(decoration_art),
            )
            .into_any_element()
    }

    /// The limited-time pill's text (module 268959): the days its collection has left, "Last day to buy" on the last.
    fn shop_limited_text(&self, product: &Product) -> Option<String> {
        let category = self
            .shop
            .home
            .categories
            .iter()
            .chain(self.shop.orbs.categories.iter())
            .chain(self.shop.collections.iter())
            .find(|category| category.sku_id == product.category_sku_id)?;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .ok()?
            .as_secs() as i64;
        let days = data::days_left(category.unpublished_at.as_deref()?, now)?;
        Some(if days > 1 {
            dc_intl::t!("Io7ozn", days = days).to_uppercase()
        } else {
            dc_intl::tr("Last day to buy").to_uppercase()
        })
    }

    /// One card. `width` is the outer width (246 on shelves, the column width in grids); the height is 246.
    pub(super) fn shop_card(
        &mut self,
        sku: &str,
        width: Option<f32>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        self.shop_card_with(sku, width, false, window, cx)
    }

    /// `shop_card`; `hide_bundle_background` leaves out a bundle's `bg_static` (the feed's
    /// `hideStaticBundleBackgroundAsset`).
    pub(super) fn shop_card_with(
        &mut self,
        sku: &str,
        width: Option<f32>,
        hide_bundle_background: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        // `tR` of module 987421: a product still loading is a skeleton card; one the answer left out is no card.
        if self.shop_product(sku).is_none() {
            if self.shop.absent.contains(sku) {
                return div().into_any_element();
            }
            // The page asks for its missing products in one batch (`shop_featured`); a card does not ask alone.
            return super::featured::skeleton_card(0, width, window);
        }
        let id = SharedString::from(format!("shop-card-{sku}"));
        let outer = div()
            .id(id.clone())
            .group(id.clone())
            .parity("shop-card")
            .relative()
            .flex_none()
            .h(px(246.))
            .rounded(px(8.))
            .border(px(2.))
            // `border-muted`, `border-subtle` while hovered (`hovered_UGbKo5`).
            .border_color(color("--border-muted"))
            .hover(|card| card.border_color(color("--border-subtle")))
            .bg(color("--background-base-low"))
            .overflow_hidden()
            .on_hover({
                let sku = sku.to_owned();
                cx.listener(move |this, inside: &bool, _, cx| {
                    let now = if *inside { Some(sku.clone()) } else { None };
                    let mine = this.shop.hover_card.as_deref() == Some(sku.as_str());
                    if (*inside || mine) && this.shop.hover_card != now {
                        this.shop.hover_card = now;
                        cx.notify();
                    }
                })
            });
        let outer = match width {
            Some(width) => outer.w(px(width)),
            None => outer.w_full(),
        };
        let Some(product) = self.shop_product(sku) else {
            return outer.into_any_element();
        };
        let nitro = self.shop_has_nitro();
        let orbs_only = product.orbs_exclusive();
        let discount = product.bundle_discount(nitro);
        let price = product
            .fiat_price(nitro)
            .or_else(|| product.price(data::PRICE_DEFAULT, Some(data::ORB)))
            .cloned();
        let limited = self.shop_limited_text(&product);
        let hovering = self.shop.hover_card.as_deref() == Some(sku);
        let preview = self.shop_preview(&product, 242., false, hovering, window, cx);
        // `tb` of module 987421: a bundle with a static foreground does not lift on hover.
        let static_bundle = product.kind == data::KIND_BUNDLE && product.fg_static.is_some();
        let lift = hovering && !static_bundle;
        // `tg`: the bundle's own backdrop behind the preview, except in the feed.
        let backdrop = (product.kind == data::KIND_BUNDLE && !hide_bundle_background)
            .then(|| self.shop_cover_at(product.bg_static.as_deref(), 242., 242., (0.5, 0.5)))
            .flatten();
        let sku_owned = sku.to_owned();
        let wished = self.shop.wishlist.contains(sku);
        let name_row = |product: &Product| -> AnyElement {
            let row = text(14., 20., 500., "--text-strong").truncate();
            match product.piece_count() {
                Some(count) => {
                    let prefix = format!("{count}pc");
                    let bold = gpui::HighlightStyle {
                        font_weight: Some(FontWeight(700.)),
                        ..Default::default()
                    };
                    let shown = format!("{prefix} {}", product.name);
                    row.child(
                        gpui::StyledText::new(shown).with_highlights([(0..prefix.len(), bold)]),
                    )
                    .into_any_element()
                }
                None => row.child(product.name.clone()).into_any_element(),
            }
        };
        // `_` of module 140877: a free product reads "Free", a Nitro product "Included with Nitro".
        let price_note = if product
            .price(data::PRICE_DEFAULT, None)
            .is_some_and(|price| price.amount == 0)
        {
            Some(dc_intl::tr("Free"))
        } else if product.premium_type.is_some() {
            Some(dc_intl::tr("Included with Nitro"))
        } else {
            None
        };
        let price = if price_note.is_some() { None } else { price };
        let price_row = price.as_ref().map(|price| {
            let is_orb = price.currency == data::ORB;
            div()
                .flex()
                .items_center()
                .gap(px(4.))
                .when(is_orb, |row| {
                    row.child(icon("shop_orb_18", 18., "--text-default"))
                })
                .child(text(14., 20., 600., "--text-default").child(price.text()))
                .when_some(discount, |row, (_, percent)| {
                    row.child(
                        text(14., 20., 600., "--text-feedback-positive")
                            .child(format!("(-{percent}%)")),
                    )
                })
        });
        let actions = self.shop_card_actions(&product, orbs_only, cx);
        let heart = {
            let wished_sku = sku_owned.clone();
            div()
                .id(SharedString::from(format!("shop-card-heart-{sku}")))
                .absolute()
                .top(px(12.))
                .right(px(12.))
                .size(px(40.))
                .rounded(px(8.))
                .backdrop_blur(px(10.))
                .bg(color("--control-overlay-secondary-background-default"))
                .hover(|heart| heart.bg(color("--control-overlay-secondary-background-hover")))
                .border_1()
                .border_color(color("--border-muted"))
                .cursor_pointer()
                .flex()
                .items_center()
                .justify_center()
                .invisible()
                .group_hover(id.clone(), |heart| heart.visible())
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.shop_toggle_wishlist(&wished_sku, cx);
                }))
                .child(icon(
                    "shop_heart_card",
                    20.,
                    if wished {
                        "--text-feedback-critical"
                    } else {
                        "--white"
                    },
                ))
        };
        outer
            .cursor_pointer()
            .on_click(cx.listener(move |this, _, _, cx| {
                this.shop_open_product(&sku_owned, cx);
            }))
            .children(backdrop.map(|art| div().absolute().inset_0().child(art)))
            .child(
                div()
                    .absolute()
                    .inset_0()
                    .when(lift, |wrap| wrap.top(px(-6.)).bottom(px(6.)))
                    .child(preview),
            )
            .child(footer_gradient(static_bundle))
            .child(
                div()
                    .absolute()
                    .left_0()
                    .right_0()
                    .bottom_0()
                    .px(px(12.))
                    .pb(px(12.))
                    .flex()
                    .flex_col()
                    .gap(px(4.))
                    .group_hover(id.clone(), |row| row.invisible())
                    .child(name_row(&product))
                    .child(
                        div()
                            .h(px(20.))
                            .flex()
                            .items_center()
                            .children(price_row)
                            .children(
                                price_note
                                    .map(|note| text(16., 20., 600., "--text-strong").child(note)),
                            ),
                    ),
            )
            .child(
                div()
                    .absolute()
                    .left_0()
                    .right_0()
                    .bottom_0()
                    .px(px(12.))
                    .pb(px(12.))
                    .flex()
                    .flex_col()
                    .gap(px(7.))
                    .invisible()
                    .group_hover(id.clone(), |row| row.visible())
                    .child(name_row(&product))
                    .child(
                        div()
                            .h(px(40.))
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .children(actions),
                    ),
            )
            // The badge order of module 268959: limited time, (new), Orbs exclusive, Nitro exclusive.
            .child(match (limited, orbs_only, product.premium_type.is_some()) {
                (Some(text), _, _) => text_badge(&text),
                (None, true, _) => orbs_badge(),
                (None, false, true) => text_badge(&dc_intl::tr("Nitro exclusive").to_uppercase()),
                _ => div().into_any_element(),
            })
            .child(heart)
            .into_any_element()
    }

    /// The hover buttons (`getPurchaseButton` of module 987421): "Buy for <price>" with a gift button, or "View Details"
    /// for Orbs-only items.
    fn shop_card_actions(
        &mut self,
        product: &Product,
        orbs_only: bool,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let sku = product.sku_id.clone();
        let nitro = self.shop_has_nitro();
        let label = match product.fiat_price(nitro) {
            Some(price) if !orbs_only => format!("Buy for {}", price.text_exact()),
            _ => "View Details".to_owned(),
        };
        let buy = {
            let sku = sku.clone();
            div()
                .id(SharedString::from(format!("shop-card-buy-{sku}")))
                .h(px(40.))
                .flex_1()
                .rounded(px(8.))
                .bg(color("--background-brand"))
                .flex()
                .items_center()
                .justify_center()
                .cursor_pointer()
                .hover(|button| button.bg(color("--brand-560")))
                .on_click(cx.listener(move |this, _, _, cx| {
                    cx.stop_propagation();
                    if orbs_only {
                        this.shop_open_product(&sku, cx);
                    } else {
                        this.shop_buy(&sku, false, cx);
                    }
                }))
                .child(
                    text(14., 18., 500., "--white")
                        .font_weight(FontWeight(500.))
                        .child(label),
                )
                .into_any_element()
        };
        let mut out = vec![buy];
        if !orbs_only {
            let sku = sku.clone();
            let tip = format!("shop-card-gift-tip-{sku}");
            let button = div()
                .id(SharedString::from(format!("shop-card-gift-{sku}")))
                .aria_label(gift_button_label(&product.name))
                .size(px(40.))
                .flex_none()
                .rounded(px(8.))
                .bg(color("--background-brand"))
                .flex()
                .items_center()
                .justify_center()
                .cursor_pointer()
                .hover(|button| button.bg(color("--brand-560")))
                .on_click(cx.listener(move |this, _, _, cx| {
                    cx.stop_propagation();
                    this.shop_buy(&sku, true, cx);
                }))
                .child(icon("shop_gift", 20., "--white"));
            // Module 878112: "Send a gift" over the button.
            out.push(
                self.with_tip(
                    tip,
                    crate::ui::tooltip::Tip::text(
                        dc_intl::t("JCFN/y"),
                        crate::ui::tooltip::Side::Top,
                    ),
                    button,
                    cx,
                )
                .into_any_element(),
            );
        }
        out
    }
}

/// The gift button's accessible name (module 878112, `5UYoSr`): "Send a gift, {product}".
pub(in crate::ui) fn gift_button_label(product: &str) -> String {
    dc_intl::t!(
        "5UYoSr",
        ctaLabel = dc_intl::t("JCFN/y"),
        productLabel = product
    )
    .to_string()
}

/// `footerGradient_UGbKo5` (dark theme): 146 tall from `--background-base-lowest` at the foot, 80% at 30%, clear at
/// the top; 100 tall and in eight steps under a static bundle (`staticBundleFooterGradient_UGbKo5`).
fn footer_gradient(static_bundle: bool) -> AnyElement {
    let base = color("--background-base-lowest");
    let at = |alpha: f32| gpui::Rgba { a: alpha, ..base };
    // Stops from the foot up: (share of the height, opacity).
    let stops: &[(f32, f32)] = if static_bundle {
        &[
            (0., 0.92),
            (0.15, 0.9),
            (0.3, 0.85),
            (0.42, 0.72),
            (0.5, 0.58),
            (0.58, 0.38),
            (0.66, 0.15),
            (0.74, 0.),
            (1., 0.),
        ]
    } else {
        &[(0., 1.), (0.3, 0.8), (1., 0.)]
    };
    let height = if static_bundle { 100. } else { 146. };
    let mut gradient = div().absolute().left_0().right_0().bottom_0().h(px(height));
    for pair in stops.windows(2) {
        let ((low_at, low), (high_at, high)) = (pair[0], pair[1]);
        gradient = gradient.child(
            div()
                .absolute()
                .left_0()
                .right_0()
                .bottom(px(height * low_at))
                .h(px(height * (high_at - low_at)))
                .bg(linear_gradient(
                    180.,
                    linear_color_stop(at(high), 0.),
                    linear_color_stop(at(low), 1.),
                )),
        );
    }
    gradient.into_any_element()
}

/// A white pill with black text at the card's top left, as the Orbs pill without its mark.
fn text_badge(label: &str) -> AnyElement {
    div()
        .absolute()
        .top(px(12.))
        .left(px(12.))
        .h(px(16.))
        .px(px(4.))
        .rounded(px(8.))
        .bg(color("--white"))
        .flex()
        .items_center()
        .child(text(12., 16., 600., "--black").child(label.to_owned()))
        .into_any_element()
}

/// The white "ORBS EXCLUSIVE" pill at the card's top left.
pub(super) fn orbs_badge() -> AnyElement {
    div()
        .absolute()
        .top(px(12.))
        .left(px(12.))
        .h(px(16.))
        .px(px(8.))
        .rounded(px(8.))
        .bg(color("--white"))
        .flex()
        .items_center()
        .gap(px(4.))
        .child(icon("shop_orb_12", 12., "--black"))
        .child(text(12., 16., 600., "--black").child("ORBS EXCLUSIVE"))
        .into_any_element()
}

fn skeleton_bar(width: f32) -> gpui::Div {
    div()
        .w(px(width))
        .h(px(8.))
        .rounded(px(4.))
        .bg(color("--background-mod-strong"))
}

fn skeleton_row(opacity: f32) -> gpui::Div {
    div()
        .flex()
        .items_center()
        .gap(px(8.))
        .opacity(opacity)
        .child(
            div()
                .size(px(24.))
                .rounded_full()
                .bg(color("--background-mod-strong")),
        )
        .child(skeleton_bar(110.))
}

#[cfg(test)]
mod tests {
    /// Module 878112: the gift button is named by its call to action and the product.
    #[test]
    fn the_gift_button_names_the_product() {
        assert_eq!(
            super::gift_button_label("Burnice White"),
            "Send a gift, Burnice White"
        );
    }
}
