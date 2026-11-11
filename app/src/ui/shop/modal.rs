//! The product modal (`modalRoot_fEFP8r`): the information rail at the left with the price and the Buy and gift buttons,
//! and the preview at the right (a profile card and a chat line wearing the item). Buy, gift and "Buy with Orbs" stop at
//! the checkout summary: no purchase request, payment data or Orbs spend is ever sent from here. Official modules
//! followed: 987421 (the buttons), 44120 (the checkout), 786953 (the modal).

use super::cards::{item_static, orbs_badge};
use super::data::{self, Product};
use super::{Root, color, icon, text};
use crate::parity::Parity;
use gpui::{
    AnyElement, Context, IntoElement, ObjectFit, SharedString, Window, deferred, div, img,
    prelude::*, px,
};

/// The checkout the Buy and gift buttons open.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Checkout {
    pub sku: String,
    pub gift: bool,
    /// The Orbs "Review" step instead of the money checkout.
    pub orbs: bool,
    pub stage: ClaimStage,
    /// The critical notice at the top of the Orbs review (`orb-checkout-error-notice`).
    pub error: Option<&'static str>,
    /// Entitlement reads made since the order was signed.
    pub reads: usize,
    /// Whether the granted entitlement is already used up (Nitro credits start at once).
    pub consumed: bool,
}

/// Where an Orbs claim is: the review, the three orders calls in flight, the end.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ClaimStage {
    Review,
    Working,
    Done,
}

impl Root {
    pub(super) fn shop_open_product(&mut self, sku: &str, cx: &mut Context<Self>) {
        self.shop.modal = Some(sku.to_owned());
        self.shop.bundle_slide = 0;
        self.shop.checkout = None;
        self.shop
            .modal_scroll
            .set_offset(gpui::point(px(0.), px(0.)));
        if !self.shop.products.contains_key(sku) {
            self.shop_request(&format!("shop:product:{sku}"), data::product_route(sku));
        }
        cx.notify();
    }

    pub(super) fn shop_close_modal(&mut self, cx: &mut Context<Self>) {
        if self.shop.checkout.is_some() {
            self.shop.checkout = None;
        } else {
            self.shop.modal = None;
        }
        cx.notify();
    }

    /// "Buy for ..." and the gift button: opens the checkout summary. An unverified account would get the verify modal
    /// first in the official client; that check needs the account's flags, so here the summary opens directly.
    pub(super) fn shop_buy(&mut self, sku: &str, gift: bool, cx: &mut Context<Self>) {
        if !gift && self.shop_claimable(sku) {
            self.shop_send_keyed("shop:claim", data::claim_premium(sku));
            cx.notify();
            return;
        }
        let orbs = !gift
            && self
                .shop_product(sku)
                .is_some_and(|product| product.orbs_exclusive());
        self.shop.checkout = Some(Checkout {
            sku: sku.to_owned(),
            gift,
            orbs,
            stage: ClaimStage::Review,
            error: None,
            reads: 0,
            consumed: false,
        });
        if self.shop.modal.is_none() {
            self.shop.modal = Some(sku.to_owned());
        }
        cx.notify();
    }

    /// The heart: adds the sku to the wishlist or takes it off. The account write goes through the request path that a
    /// harness run refuses; the request shapes are in `data` and tested there.
    pub(super) fn shop_toggle_wishlist(&mut self, sku: &str, cx: &mut Context<Self>) {
        let write = if self.shop.wishlist.remove(sku) {
            match self.shop.wishlist_id.clone() {
                Some(id) => data::wishlist_remove(&id, sku),
                None => {
                    cx.notify();
                    return;
                }
            }
        } else {
            self.shop.wishlist.insert(sku.to_owned());
            data::wishlist_add(sku)
        };
        self.shop_send(write);
        cx.notify();
    }

    /// A Nitro subscriber's free claim ("Add to your collection"): a Nitro-tier product whose Nitro price is zero.
    pub(super) fn shop_claimable(&self, sku: &str) -> bool {
        self.shop_has_nitro()
            && self.shop_product(sku).is_some_and(|product| {
                product.premium_type.is_some()
                    && product
                        .fiat_price(true)
                        .is_some_and(|price| price.amount == 0)
            })
    }

    /// The Orbs button state of a product against the current balance.
    pub(super) fn shop_orb_state(&self, product: &Product) -> data::OrbButton {
        data::OrbButton::check(
            product.price(data::PRICE_DEFAULT, Some(data::ORB)),
            self.shop.balance,
        )
    }

    /// "Claim with Orbs": the orders API chain of the official checkout. Create the order, sign it, read the
    /// entitlements, then refetch the balance. The button is inert unless the balance covers the price.
    fn shop_claim_orbs(&mut self, cx: &mut Context<Self>) {
        let Some(checkout) = self.shop.checkout.clone() else {
            return;
        };
        let Some(product) = self.shop_product(&checkout.sku) else {
            return;
        };
        if checkout.stage != ClaimStage::Review
            || self.shop_orb_state(&product) != data::OrbButton::Ready
        {
            return;
        }
        if let Some(open) = self.shop.checkout.as_mut() {
            open.stage = ClaimStage::Working;
            open.error = None;
            open.reads = 0;
        }
        self.shop_send_keyed("shop:order:create", data::orb_order_create(&checkout.sku));
        cx.notify();
    }

    /// The answers of the claim chain: create, sign, then the entitlement reads (`redeemVirtualCurrency` of module
    /// 715054). A refused signing or any failed step puts the official notice on the review; an order with no
    /// entitlement after the last wait puts the "taking longer" notice.
    pub(super) fn shop_order_fetched(
        &mut self,
        key: &str,
        body: &serde_json::Value,
        cx: &mut Context<Self>,
    ) {
        let reads = self.shop.checkout.as_ref().map_or(0, |open| open.reads);
        match data::claim_next(key, body, reads) {
            data::ClaimNext::Refused => {
                if let Some(open) = self.shop.checkout.as_mut() {
                    open.stage = ClaimStage::Review;
                }
            }
            data::ClaimNext::Sign { order, revision } => {
                self.shop_send_keyed("shop:order:sign", data::order_sign(&order, revision));
                self.shop.order = Some(order);
            }
            data::ClaimNext::Entitlements { attempt } => self.shop_read_entitlements(attempt, cx),
            data::ClaimNext::Done { consumed } => {
                if let Some(open) = self.shop.checkout.as_mut() {
                    open.stage = ClaimStage::Done;
                    open.consumed = consumed;
                }
                let _ = self.requests.send(crate::net::Request::Fetch {
                    key: "shop:balance".to_owned(),
                    route: data::BALANCE_ROUTE.to_owned(),
                });
            }
            data::ClaimNext::Fail(text) => {
                if let Some(open) = self.shop.checkout.as_mut() {
                    open.stage = ClaimStage::Review;
                    open.error = Some(text);
                }
            }
            data::ClaimNext::Ignore => {}
        }
        cx.notify();
    }

    /// One read of the signed order's entitlements, after the wait that belongs to the attempt.
    fn shop_read_entitlements(&mut self, attempt: usize, cx: &mut Context<Self>) {
        let Some(id) = self.shop.order.clone() else {
            return;
        };
        if let Some(open) = self.shop.checkout.as_mut() {
            open.reads = attempt;
        }
        let delay = attempt
            .checked_sub(1)
            .and_then(|at| data::ENTITLEMENT_DELAYS_MS.get(at))
            .copied()
            .unwrap_or(0);
        let requests = self.requests.clone();
        cx.spawn(async move |_, cx| {
            if delay > 0 {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(delay))
                    .await;
            }
            let _ = requests.send(crate::net::Request::Fetch {
                key: "shop:order:entitlements".to_owned(),
                route: data::order_entitlements_route(&id),
            });
        })
        .detach();
    }

    fn shop_send_keyed(&mut self, key: &str, write: data::Write) {
        let method = match write.method {
            "POST" => reqwest::Method::POST,
            "DELETE" => reqwest::Method::DELETE,
            "PUT" => reqwest::Method::PUT,
            _ => reqwest::Method::PATCH,
        };
        let _ = self.requests.send(crate::net::Request::Rest {
            key: Some(key.to_owned()),
            method,
            route: write.route,
            body: write.body,
        });
    }

    /// Sends a `data::Write` through the REST request path.
    fn shop_send(&mut self, write: data::Write) {
        let method = match write.method {
            "POST" => reqwest::Method::POST,
            "DELETE" => reqwest::Method::DELETE,
            "PUT" => reqwest::Method::PUT,
            _ => reqwest::Method::PATCH,
        };
        let _ = self.requests.send(crate::net::Request::Rest {
            key: Some("shop:wishlist".to_owned()),
            method,
            route: write.route,
            body: write.body,
        });
    }

    pub(in crate::ui) fn shop_modal(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let sku = self.shop.modal.clone()?;
        let product = self.shop_product(&sku);
        let height = (f32::from(window.viewport_size().height) - 112.).clamp(300., 608.);
        let body: AnyElement = match product {
            Some(product) => self.shop_modal_body(&product, window, cx),
            None => div().into_any_element(),
        };
        let checkout = self.shop_checkout(window, cx);
        let modal = div()
            .parity("shop-modal")
            .occlude()
            .relative()
            .w(px(960.))
            .h(px(height))
            .rounded(px(12.))
            .overflow_hidden()
            .bg(color("--modal-background"))
            .flex()
            .child(body);
        Some(
            deferred(
                div()
                    .id("shop-modal-layer")
                    .occlude()
                    .absolute()
                    .inset_0()
                    .bg(color("--background-scrim"))
                    .flex()
                    .items_center()
                    .justify_center()
                    .on_click(cx.listener(|this, _, _, cx| this.shop_close_modal(cx)))
                    .child(modal)
                    .children(checkout),
            )
            .with_priority(3)
            .into_any_element(),
        )
    }

    fn shop_modal_body(
        &mut self,
        product: &Product,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let nitro = self.shop_has_nitro();
        let orbs_only = product.orbs_exclusive();
        let sku = product.sku_id.clone();
        let slide = self
            .shop
            .bundle_slide
            .min(product.items.len().saturating_sub(1));
        let view = product.slide(slide);
        let kind = view.kind;
        let category = self
            .shop
            .home
            .categories
            .iter()
            .chain(self.shop.orbs.categories.iter())
            .chain(self.shop.collections.iter())
            .find(|category| category.sku_id == product.category_sku_id)
            .cloned();
        let banner = category
            .as_ref()
            .filter(|_| !orbs_only)
            .and_then(|category| category.pdp_bg_url.clone())
            .and_then(|url| self.shop_img(&url));
        let collection = category.map(|category| category.name);
        let wished = self.shop.wishlist.contains(&sku);
        let price = product
            .fiat_price(nitro)
            .or_else(|| product.price(data::PRICE_DEFAULT, Some(data::ORB)))
            .cloned();
        let nitro_price = (!nitro && !orbs_only)
            .then(|| product.fiat_price(true).cloned())
            .flatten();
        let left_preview = self.shop_preview(&view, 330., true, true, window, cx);
        let claimable = self.shop_claimable(&sku);
        let orb_state = self.shop_orb_state(product);
        let orb_amount = price.as_ref().map(|price| price.amount).unwrap_or_default();
        let buy_label = match (&price, orbs_only) {
            (Some(_), false) if claimable => "Add to your collection".to_owned(),
            (Some(price), false) => format!("Buy for {}", price.text()),
            (Some(_), true) => "Redeem".to_owned(),
            _ => "Unavailable".to_owned(),
        };
        let buy = {
            let sku = sku.clone();
            let blocked = orbs_only && orb_state != data::OrbButton::Ready;
            let button = div()
                .id("shop-modal-buy")
                .parity("shop-modal-buy")
                .h(px(40.))
                .w(px(if orbs_only { 282. } else { 234. }))
                .rounded(px(8.))
                .bg(color("--background-brand"))
                .flex()
                .items_center()
                .justify_center()
                .gap(px(4.))
                .when(blocked, |button| button.opacity(0.5).cursor_not_allowed())
                .when(!blocked, |button| {
                    button
                        .cursor_pointer()
                        .hover(|button| button.bg(color("--brand-560")))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            cx.stop_propagation();
                            this.shop_buy(&sku, false, cx);
                        }))
                })
                .child(text(16., 20., 500., "--white").child(buy_label))
                .when(orbs_only, |button| {
                    button
                        .child(icon("shop_orb_18", 18., "--white"))
                        .child(text(16., 20., 500., "--white").child(orb_amount.to_string()))
                });
            let tip = orb_state.modal_tip().filter(|_| orbs_only);
            match tip {
                Some(tip) => self
                    .with_tip(
                        "shop-modal-buy-tip",
                        crate::ui::tooltip::Tip::text(tip, crate::ui::tooltip::Side::Top),
                        button,
                        cx,
                    )
                    .into_any_element(),
                None => button.into_any_element(),
            }
        };
        let gift = {
            let sku = sku.clone();
            div()
                .id("shop-modal-gift")
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
                .child(icon("shop_gift_modal", 20., "--white"))
        };
        let pricing = div()
            .flex()
            .flex_col()
            .gap(px(13.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .h(px(20.))
                    .gap(px(4.))
                    .when(orbs_only, |row| {
                        let dim = orb_state != data::OrbButton::Ready;
                        row.w_full()
                            .justify_between()
                            .child(
                                text(12., 16., 500., "--text-default")
                                    .when(dim, |label| label.opacity(0.5))
                                    .child(if orb_state == data::OrbButton::NotRedeemable {
                                        "Not available"
                                    } else {
                                        "Using Orbs"
                                    }),
                            )
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(4.))
                                    .when(dim, |price| price.opacity(0.5))
                                    .child(icon("shop_orb_18", 20., "--text-default"))
                                    .child(
                                        text(16., 20., 600., "--text-default")
                                            .child(orb_amount.to_string()),
                                    ),
                            )
                    })
                    .when(!orbs_only, |row| {
                        row.child(
                            text(16., 20., 600., "--text-strong").child(
                                price.as_ref().map(|price| price.text()).unwrap_or_default(),
                            ),
                        )
                    }),
            )
            .children(nitro_price.map(|nitro_price| {
                div()
                    .mt(px(-8.))
                    .flex()
                    .items_center()
                    .gap(px(4.))
                    .child(icon("shop_nitro_16", 16., "--interactive-text-default"))
                    .child(
                        text(12., 16., 500., "--interactive-text-default")
                            .child(format!("{} with Nitro.", nitro_price.text())),
                    )
                    .child(
                        text(12., 16., 600., "--interactive-text-default")
                            .id("shop-modal-subscribe")
                            .underline()
                            .cursor_pointer()
                            .on_click(cx.listener(|this, _, _, cx| this.open_purchase(false, cx)))
                            .child("Subscribe now"),
                    )
            }))
            .child(
                div()
                    .flex()
                    .gap(px(8.))
                    .when(price.is_some(), |row| row.child(buy))
                    .when(!orbs_only && price.is_some(), |row| row.child(gift)),
            );
        let bundle_strip = (product.kind == data::KIND_BUNDLE && !product.items.is_empty())
            .then(|| self.shop_bundle_strip(product, slide, cx));
        let left = div()
            .parity("shop-modal-left")
            .relative()
            .flex_none()
            .w(px(330.))
            .h_full()
            .p(px(24.))
            .flex()
            .flex_col()
            .justify_between()
            .children(orbs_only.then(|| {
                div()
                    .absolute()
                    .top(px(0.))
                    .left(px(4.))
                    .child(orbs_badge())
            }))
            .child(
                div()
                    .flex_1()
                    .min_h(px(0.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .overflow_hidden()
                    .child(
                        div()
                            .size(px(260.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(left_preview),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(24.))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(4.))
                            .child(
                                text(24., 32., 600., "--text-strong").child(product.name.clone()),
                            )
                            .child(
                                text(14., 18., 400., "--text-default")
                                    .child(self.shop_summary(product)),
                            )
                            .children(bundle_strip),
                    )
                    .child(pricing),
            );
        let close = |cx: &mut Context<Self>| {
            div()
                .id("shop-modal-close")
                .size(px(32.))
                .rounded(px(8.))
                .bg(color("--background-base-lower"))
                .flex()
                .items_center()
                .justify_center()
                .cursor_pointer()
                .on_click(cx.listener(|this, _, _, cx| {
                    cx.stop_propagation();
                    this.shop.modal = None;
                    this.shop.checkout = None;
                    cx.notify();
                }))
                .child(icon("shop_close_16", 16., "--text-strong"))
        };
        let round = |id: &'static str, name: &'static str, tint: &'static str| {
            div()
                .id(id)
                .size(px(32.))
                .rounded(px(8.))
                .bg(color("--background-base-lower"))
                .flex()
                .items_center()
                .justify_center()
                .cursor_pointer()
                .child(icon(name, 16., tint))
        };
        let heart = {
            let sku = sku.clone();
            round(
                "shop-modal-heart",
                "shop_heart_16",
                if wished {
                    "--text-feedback-critical"
                } else {
                    "--text-strong"
                },
            )
            .on_click(cx.listener(move |this, _, _, cx| {
                cx.stop_propagation();
                this.shop_toggle_wishlist(&sku, cx);
            }))
        };
        // The share button copies the item's link and says so in a toast (module 760429: `Link Copied!`). The official
        // button opens the forward picker with this copy as one of its actions; that picker is in BLOCKED.md.
        let share_sku = sku.clone();
        let share = round("shop-modal-share", "shop_share", "--text-strong").on_click(cx.listener(
            move |this, _, _, cx| {
                cx.stop_propagation();
                let link = data::share_link(&share_sku, this.shop.tab == super::Tab::Orbs);
                cx.write_to_clipboard(gpui::ClipboardItem::new_string(link));
                this.show_success_toast("Link Copied!".to_owned(), cx);
            },
        ));
        let user = self
            .stores
            .current_user
            .as_ref()
            .map(|user| user.username.clone())
            .unwrap_or_default();
        let user_name = user.clone();
        let deco_art = (kind == data::KIND_AVATAR_DECORATION)
            .then(|| {
                let id = view
                    .items
                    .first()
                    .map_or(view.sku_id.clone(), |item| item.sku_id.clone());
                self.shop_img(&item_static(&id))
            })
            .flatten();
        let avatar = |size: f32, art: Option<std::sync::Arc<gpui::Image>>| {
            div()
                .relative()
                .size(px(size * 1.2))
                .flex_none()
                .flex()
                .items_center()
                .justify_center()
                .child(
                    div()
                        .size(px(size))
                        .rounded_full()
                        .bg(color("--background-brand"))
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(icon("sb_discord", size * 0.6, "--white")),
                )
                .children(art.map(|art| {
                    img(art)
                        .absolute()
                        .top_0()
                        .left_0()
                        .size(px(size * 1.2))
                        .object_fit(ObjectFit::Contain)
                }))
        };
        let ring = |inner: AnyElement| {
            div()
                .size(px(92.))
                .rounded_full()
                .bg(color("--background-surface-high"))
                .flex()
                .items_center()
                .justify_center()
                .child(inner)
        };
        let status = div()
            .absolute()
            .left(px(66.))
            .top(px(66.))
            .size(px(24.))
            .rounded_full()
            .bg(color("--background-surface-high"))
            .flex()
            .items_center()
            .justify_center()
            .child(
                div()
                    .size(px(16.))
                    .rounded_full()
                    .bg(color("--status-danger"))
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(
                        div()
                            .w(px(8.))
                            .h(px(2.))
                            .rounded(px(1.))
                            .bg(color("--white")),
                    ),
            );
        let orb_badge_shown = product.sku_id == data::ORB_BADGE;
        // The preview card lists the account's own badges (the profile read of the Shop), then the Orbs badge when the
        // product is that badge (`badge_IqWmhA`: 20px images in a 22px pill, 3px padding and a 2px gap).
        let badge_urls = data::profile_badge_urls(
            self.fetched
                .get("shop:profile")
                .unwrap_or(&serde_json::Value::Null),
        );
        let mut badge_art: Vec<std::sync::Arc<gpui::Image>> = badge_urls
            .iter()
            .filter_map(|url| self.shop_img(url))
            .collect();
        if orb_badge_shown {
            badge_art.extend(self.shop_img(data::ORB_BADGE_PNG));
        }
        let frame_art: Vec<_> = if kind == data::KIND_PROFILE_FRAME {
            view.items
                .first()
                .map(|item| {
                    item.layers
                        .iter()
                        .map(|layer| {
                            format!(
                                "https://cdn.discordapp.com/media/v1/collectibles-shop/{}/{layer}/static",
                                item.sku_id
                            )
                        })
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default()
                .iter()
                .filter_map(|url| self.shop_img(url))
                .collect()
        } else {
            Vec::new()
        };
        let effect = (kind == data::KIND_PROFILE_EFFECT)
            .then(|| view.sku_id.parse::<u64>().ok())
            .flatten()
            .and_then(|sku| {
                let surface = crate::ui::profile_effects::Surface::shop_preview(302., true, true);
                self.profile_effect_layer("shop-modal", sku, surface, window, cx)
            });
        let profile = div()
            .relative()
            .flex_none()
            .w(px(302.))
            .h(px(224.))
            .rounded(px(8.))
            .overflow_hidden()
            .border_1()
            .border_color(color("--border-subtle"))
            .bg(color("--background-surface-high"))
            .child(div().h(px(105.)).w_full().bg(gpui::rgb(0x5c64f4)))
            .child(
                div()
                    .absolute()
                    .left(px(9.))
                    .top(px(54.))
                    .child(ring(avatar(80., deco_art.clone()).into_any_element()))
                    .child(status),
            )
            .child(
                div()
                    .absolute()
                    .left(px(15.))
                    .top(px(161.))
                    .flex()
                    .flex_col()
                    .child(text(20., 24., 700., "--text-strong").child(user.clone()))
                    .child(div().h(px(3.)))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .child(text(14., 18., 500., "--text-strong").child(user.clone()))
                            .children((!badge_art.is_empty()).then(|| {
                                div()
                                    .h(px(22.))
                                    .px(px(3.))
                                    .rounded(px(6.))
                                    .bg(color("--background-mod-subtle"))
                                    .flex()
                                    .items_center()
                                    .gap(px(2.))
                                    .children(badge_art.into_iter().map(|art| {
                                        img(art).size(px(20.)).object_fit(ObjectFit::Contain)
                                    }))
                            })),
                    ),
            )
            .children(effect)
            .children(frame_art.into_iter().map(|art| {
                img(art)
                    .absolute()
                    .top_0()
                    .left_0()
                    .size_full()
                    .object_fit(ObjectFit::Cover)
            }));
        let chat_shown = kind == data::KIND_AVATAR_DECORATION;
        let chat = chat_shown.then(|| {
            div()
                .relative()
                .top(px(-38.))
                .ml(px(42.))
                .flex_none()
                .w(px(322.))
                .h(px(132.))
                .rounded(px(8.))
                .bg(color("--background-base-low"))
                .p(px(16.))
                .flex()
                .flex_col()
                .gap(px(8.))
                .child(
                    div()
                        .flex()
                        .gap(px(12.))
                        .child(avatar(32., deco_art))
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .child(
                                    div()
                                        .flex()
                                        .gap(px(8.))
                                        .items_baseline()
                                        .child(text(16., 22., 500., "--text-strong").child(user))
                                        .child(
                                            text(12., 16., 500., "--text-muted").child("1:18 PM"),
                                        ),
                                )
                                .child(
                                    text(16., 22., 400., "--text-default")
                                        .child("Look at my beautiful decoration"),
                                ),
                        ),
                )
                .child(
                    div()
                        .h(px(40.))
                        .rounded(px(8.))
                        .bg(color("--background-mod-subtle"))
                        .px(px(8.))
                        .flex()
                        .items_center()
                        .justify_between()
                        .child(icon("shop_mock_plus", 24., "--interactive-text-default"))
                        .child(icon("shop_mock_emoji", 24., "--interactive-text-default")),
                )
        });
        let external = product.kind == data::KIND_EXTERNAL;
        let centre = if external && !orb_badge_shown && data::is_fractional_nitro(&product.sku_id) {
            self.shop_nitro_perks().into_any_element()
        } else if external && !orb_badge_shown {
            self.shop_external_preview(product).into_any_element()
        } else if kind == data::KIND_NAMEPLATE {
            self.shop_nameplate_pane(&view, &user_name)
                .into_any_element()
        } else {
            div()
                .flex()
                .flex_col()
                .w(px(if chat_shown { 344. } else { 302. }))
                .child(profile)
                .children(chat)
                .into_any_element()
        };
        let right = div()
            .parity("shop-modal-preview")
            .relative()
            .flex_1()
            .min_w(px(0.))
            .h_full()
            .overflow_hidden()
            .bg(color("--background-surface-highest"))
            .children(banner.map(|banner| {
                // The preview container paints the art `cover` over its whole 650px; the art is blurred by the
                // server and ends in the flat surface colour at 294px, so the top 294px of it show.
                div()
                    .absolute()
                    .top_0()
                    .left_0()
                    .w_full()
                    .h(px(294.))
                    .overflow_hidden()
                    .child(
                        img(banner)
                            .absolute()
                            .top_0()
                            .left_0()
                            .w_full()
                            .h(px(650.))
                            .object_fit(ObjectFit::Cover),
                    )
            }))
            .child(
                div()
                    .absolute()
                    .top(px(16.))
                    .left(px(16.))
                    .h(px(32.))
                    .px(px(12.))
                    .rounded(px(8.))
                    .bg(color("--background-base-lower"))
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .child(icon("shop_collection_16", 16., "--text-strong"))
                    .child(
                        text(14., 18., 600., "--text-strong")
                            .child(collection.unwrap_or_else(|| "Shop".to_owned())),
                    ),
            )
            .child(
                div()
                    .absolute()
                    .top(px(16.))
                    .right(px(16.))
                    .flex()
                    .gap(px(10.))
                    .child(heart)
                    .child(share)
                    .child(close(cx)),
            )
            .child(
                div()
                    .size_full()
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .child(centre),
            );
        let content = div()
            .w(px(945.))
            .h(px(650.))
            .flex()
            .child(left)
            .child(right);
        let handle = self.shop.modal_scroll.clone();
        div()
            .relative()
            .size_full()
            .child(
                div()
                    .id("shop-modal-scroll")
                    .size_full()
                    .overflow_y_scroll()
                    .track_scroll(&handle)
                    .child(content),
            )
            .child(classic_scrollbar(handle))
            .into_any_element()
    }

    /// The bundle's contents (`bundleContents_Sy_obt`): a row of 48px thumbnails with the selected one outlined, then the
    /// selected item's name and type.
    fn shop_bundle_strip(
        &mut self,
        product: &Product,
        selected: usize,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let mut row = div().flex().flex_row().flex_wrap().gap(px(10.));
        for (at, item) in product.items.iter().enumerate() {
            let inset = matches!(
                item.kind,
                data::KIND_AVATAR_DECORATION | data::KIND_PROFILE_FRAME
            );
            let url = match (&item.thumbnail, item.kind) {
                (Some(thumb), data::KIND_PROFILE_EFFECT) => thumb.clone(),
                _ => item_static(&item.sku_id),
            };
            let art = self.shop_img(&url);
            let on = at == selected;
            row = row.child(
                div()
                    .id(SharedString::from(format!("shop-bundle-item-{at}")))
                    .relative()
                    .size(px(48.))
                    .flex_none()
                    .rounded(px(8.))
                    .overflow_hidden()
                    .bg(color("--background-mod-subtle"))
                    .flex()
                    .items_center()
                    .justify_center()
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        cx.stop_propagation();
                        this.shop.bundle_slide = at;
                        cx.notify();
                    }))
                    .children(art.map(|art| {
                        img(art)
                            .size(px(48. * if inset { 0.75 } else { 0.93 }))
                            .object_fit(ObjectFit::Contain)
                    }))
                    .child(
                        div()
                            .absolute()
                            .inset_0()
                            .rounded(px(8.))
                            .border_2()
                            .border_color(if on {
                                color("--border-focus")
                            } else {
                                gpui::rgba(0x00000000)
                            }),
                    ),
            );
        }
        let active = product.items.get(selected);
        let name = product
            .bundled
            .get(selected)
            .map(|part| part.name.clone())
            .or_else(|| active.map(|item| item.label.clone()))
            .unwrap_or_default();
        let kind = active.and_then(|item| data::item_type_label(item.kind));
        div()
            .parity("shop-bundle-contents")
            .flex()
            .flex_col()
            .gap(px(8.))
            .pt(px(12.))
            .child(row)
            .child(
                div()
                    .flex()
                    .items_baseline()
                    .min_w(px(0.))
                    .child(text(14., 18., 500., "--text-default").child(name))
                    .children(kind.map(|kind| {
                        text(14., 18., 500., "--text-muted").child(format!(" - {kind}"))
                    })),
            )
            .into_any_element()
    }

    /// The nameplate's right pane (`eB` of module 355735): a member list of five other members with the shopper's own
    /// row, wearing the nameplate, among them.
    fn shop_nameplate_pane(&mut self, view: &Product, user: &str) -> impl IntoElement {
        let plate = view
            .items
            .first()
            .map_or(view.sku_id.clone(), |item| item.sku_id.clone());
        let art = self.shop_img(&item_static(&plate));
        let member = |name: &str, tint: &'static str| {
            div()
                .h(px(42.))
                .px(px(8.))
                .flex()
                .items_center()
                .gap(px(12.))
                .child(
                    div()
                        .size(px(32.))
                        .flex_none()
                        .rounded_full()
                        .bg(color("--background-mod-strong")),
                )
                .child(text(16., 20., 500., tint).child(name.to_owned()))
        };
        let group = |label: &str| {
            div()
                .mt(px(24.))
                .mb(px(4.))
                .pl(px(16.))
                .child(text(12., 16., 500., "--channels-default").child(label.to_owned()))
        };
        let own = div()
            .relative()
            .h(px(42.))
            .mx(px(8.))
            .rounded(px(8.))
            .overflow_hidden()
            .children(art.map(|art| {
                img(art)
                    .absolute()
                    .top_0()
                    .left_0()
                    .size_full()
                    .object_fit(ObjectFit::Cover)
            }))
            .child(
                div()
                    .absolute()
                    .inset_0()
                    .px(px(8.))
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .child(
                        div()
                            .size(px(32.))
                            .flex_none()
                            .rounded_full()
                            .bg(color("--background-brand"))
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(icon("sb_discord", 20., "--white")),
                    )
                    .child(text(16., 20., 500., "--text-strong").child(user.to_owned())),
            );
        div()
            .parity("shop-nameplate-pane")
            .w(px(240.))
            .pb(px(8.))
            .rounded(px(8.))
            .bg(color("--background-surface-high"))
            .child(group("Admins - 3"))
            .child(member("Mallow", "--role-sky-blue"))
            .child(member("Phibi", "--role-magenta"))
            .child(own)
            .child(member("Locke", "--role-magenta"))
            .child(group("Online - 12"))
            .child(member("Boom", "--role-light-green"))
            .child(member("Cherry", "--role-light-green"))
    }

    /// The Nitro credit's right pane (`eW` of module 355735): the Nitro artwork, the wordmark and the perks.
    fn shop_nitro_perks(&mut self) -> impl IntoElement {
        let header = self.shop_img(
            "https://cdn.discordapp.com/assets/content/81644099db353c3ea7cefa334f8d298d06b768a68a349de663d2151fd01eddc7.png",
        );
        div()
            .parity("shop-nitro-perks")
            .p(px(24.))
            .rounded(px(16.))
            .bg(gpui::linear_gradient(
                180.,
                gpui::linear_color_stop(gpui::rgb(0x000000), 0.),
                gpui::linear_color_stop(gpui::rgb(0x36266d), 1.),
            ))
            .flex()
            .flex_col()
            .child(
                div()
                    .w(px(211.))
                    .h(px(157.))
                    .mb(px(24.))
                    .children(header.map(|art| img(art).size_full().object_fit(ObjectFit::Cover))),
            )
            .child(icon("shop_nitro_wordmark", 106., "--white"))
            .child(div().mt(px(16.)).flex().flex_col().gap(px(10.)).children(
                data::NITRO_PERKS.iter().map(|perk| {
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .child(icon("checkbox_check", 16., "--white"))
                        .child(text(14., 18., 500., "--text-overlay-light").child(*perk))
                }),
            ))
    }

    /// The right pane of an external product that has no mock (`externalProductProfilePreview`): its art, 350px wide.
    fn shop_external_preview(&mut self, product: &Product) -> impl IntoElement {
        let art = data::external_art_url(&product.sku_id).and_then(|url| self.shop_img(&url));
        div()
            .w(px(350.))
            .h(px(252.))
            .flex()
            .items_center()
            .justify_center()
            .children(art.map(|art| img(art).size_full().object_fit(ObjectFit::Contain)))
    }

    /// The summary line (`Z` of module 355735): see `data::product_summary`.
    fn shop_summary(&self, product: &Product) -> String {
        data::product_summary(product)
    }

    /// The checkout summary (`payment-modal` / `gift-payment-modal`): what is bought, for whom and the total. The
    /// payment step is not reached from here: nothing is sent.
    fn shop_checkout(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let checkout = self.shop.checkout.clone()?;
        let product = self.shop_product(&checkout.sku)?;
        let nitro = self.shop_has_nitro();
        let total = product
            .fiat_price(nitro)
            .or_else(|| product.price(data::PRICE_DEFAULT, Some(data::ORB)))
            .map(|price| price.text())
            .unwrap_or_default();
        if checkout.orbs {
            if checkout.stage == ClaimStage::Done {
                return Some(self.shop_collected(&product, checkout.consumed, window, cx));
            }
            return Some(self.shop_orb_checkout(&product, checkout.stage, window, cx));
        }
        let title = if checkout.gift {
            "Send a Gift"
        } else {
            "Checkout"
        };
        Some(
            div()
                .id("shop-checkout")
                .parity("shop-checkout")
                .occlude()
                .absolute()
                .w(px(480.))
                .rounded(px(8.))
                .bg(color("--modal-background"))
                .flex()
                .flex_col()
                .on_click(|_, _, cx| cx.stop_propagation())
                .child(
                    div()
                        .px(px(24.))
                        .pt(px(24.))
                        .pb(px(16.))
                        .child(text(24., 32., 600., "--text-strong").child(title)),
                )
                .child(
                    div()
                        .px(px(24.))
                        .py(px(16.))
                        .flex()
                        .justify_between()
                        .child(text(16., 20., 500., "--text-default").child(product.name))
                        .child(text(16., 20., 600., "--text-strong").child(total)),
                )
                .child(
                    div()
                        .px(px(24.))
                        .py(px(16.))
                        .flex()
                        .justify_between()
                        .bg(color("--background-base-low"))
                        .child(
                            div()
                                .id(SharedString::from("shop-checkout-back"))
                                .cursor_pointer()
                                .on_click(cx.listener(|this, _, _, cx| {
                                    cx.stop_propagation();
                                    this.shop.checkout = None;
                                    cx.notify();
                                }))
                                .child(text(14., 18., 600., "--text-strong").child("Back")),
                        )
                        .child(
                            div()
                                .h(px(32.))
                                .px(px(16.))
                                .rounded(px(8.))
                                .bg(color("--background-brand"))
                                .opacity(0.5)
                                .flex()
                                .items_center()
                                .child(text(14., 18., 500., "--white").child("Add Payment Method")),
                        ),
                )
                .into_any_element(),
        )
    }

    /// The Orbs "Review" step of the unified checkout (`ORB_CHECKOUT`, module 141393 `sc`): the "Checkout" header, the
    /// critical notice of a failed claim, the item with its Orbs price, the "Payment method" field holding the
    /// "Orbs Balance" row, the legal text and the "Claim with Orbs" button. Without enough Orbs the button is
    /// disabled and its tooltip says "Not enough Orbs.".
    fn shop_orb_checkout(
        &mut self,
        product: &Product,
        stage: ClaimStage,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let state = self.shop_orb_state(product);
        let error = self.shop.checkout.as_ref().and_then(|open| open.error);
        let price = product
            .price(data::PRICE_DEFAULT, Some(data::ORB))
            .map(|price| price.amount)
            .unwrap_or_default();
        let balance = self.shop.balance.unwrap_or_default();
        let blocked = state != data::OrbButton::Ready;
        let working = stage == ClaimStage::Working;
        let art = match data::external_art_url(&product.sku_id) {
            Some(url) => {
                let art = self.shop_img(&url);
                div()
                    .size_full()
                    .children(art.map(|art| img(art).size_full().object_fit(ObjectFit::Contain)))
                    .into_any_element()
            }
            None => self.shop_preview(product, 64., false, false, window, cx),
        };
        let summary = self.shop_summary(product);
        let notice = error.map(|message| {
            div()
                .flex()
                .items_start()
                .gap(px(8.))
                .p(px(12.))
                .rounded(px(4.))
                .bg(color("--background-feedback-mod-critical"))
                .border_1()
                .border_color(color("--inlinenotice-border-critical"))
                .child(icon("shop_circle_error", 16., "--icon-feedback-critical"))
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.))
                        .child(text(14., 18., 400., "--text-strong").child(message)),
                )
        });
        let claim = div()
            .id("shop-orb-claim")
            .parity("shop-orb-claim")
            .h(px(40.))
            .px(px(16.))
            .rounded(px(8.))
            .bg(color("--background-brand"))
            .flex()
            .items_center()
            .justify_center()
            .when(blocked || working, |button| {
                button.opacity(0.5).cursor_not_allowed()
            })
            .when(!blocked && !working, |button| {
                button
                    .cursor_pointer()
                    .hover(|button| button.bg(color("--brand-560")))
                    .on_click(cx.listener(|this, _, _, cx| {
                        cx.stop_propagation();
                        this.shop_claim_orbs(cx);
                    }))
            })
            .child(text(16., 20., 500., "--white").child("Claim with Orbs"));
        let claim = match state.checkout_tip().filter(|_| !working) {
            Some(tip) => self
                .with_tip(
                    "shop-orb-claim-tip",
                    crate::ui::tooltip::Tip::text(tip, crate::ui::tooltip::Side::Top),
                    claim,
                    cx,
                )
                .into_any_element(),
            None => claim.into_any_element(),
        };
        let close = div()
            .id("shop-checkout-close")
            .size(px(24.))
            .flex()
            .items_center()
            .justify_center()
            .cursor_pointer()
            .on_click(cx.listener(|this, _, _, cx| {
                cx.stop_propagation();
                this.shop.checkout = None;
                cx.notify();
            }))
            .child(icon("close", 24., "--interactive-icon-default"));
        let item = div()
            .flex()
            .items_center()
            .gap(px(12.))
            .child(
                div()
                    .relative()
                    .flex_none()
                    .size(px(64.))
                    .rounded(px(8.))
                    .overflow_hidden()
                    .bg(color("--background-mod-subtle"))
                    .child(div().absolute().inset_0().child(art)),
            )
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .flex()
                    .flex_col()
                    .child(text(16., 22., 400., "--text-default").child(product.name.clone()))
                    .child(text(14., 18., 400., "--text-muted").child(summary)),
            )
            .child(
                div()
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap(px(4.))
                    .child(icon("shop_orb_18", 16., "--text-default"))
                    .child(text(16., 22., 500., "--text-default").child(price.to_string())),
            );
        let method = div()
            .flex()
            .flex_col()
            .gap(px(8.))
            .child(text(14., 18., 600., "--text-strong").child("Payment method"))
            .child(
                div()
                    .px(px(12.))
                    .py(px(18.))
                    .rounded(px(8.))
                    .bg(color("--background-mod-subtle"))
                    .border_1()
                    .border_color(color("--border-subtle"))
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(text(16., 20., 400., "--text-default").child("Orbs Balance"))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(4.))
                            .child(icon("shop_orb_18", 16., "--icon-subtle"))
                            .child(
                                text(16., 20., 400., "--text-default").child(balance.to_string()),
                            ),
                    ),
            );
        let panel = div()
            .id("shop-checkout")
            .parity("shop-checkout")
            .occlude()
            .w(px(480.))
            .rounded(px(8.))
            .bg(color("--modal-background"))
            .flex()
            .flex_col()
            .pt(px(8.))
            .pb(px(16.))
            .on_click(|_, _, cx| cx.stop_propagation())
            .child(
                div()
                    .px(px(24.))
                    .pt(px(16.))
                    .pb(px(8.))
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(text(20., 24., 600., "--text-strong").child("Checkout"))
                    .child(close),
            )
            .child(
                div()
                    .px(px(24.))
                    .flex()
                    .flex_col()
                    .gap(px(24.))
                    .children(notice)
                    .child(item)
                    .child(method)
                    .child(text(12., 16., 400., "--text-muted").child(data::ORB_LEGAL)),
            )
            .child(
                div()
                    .px(px(24.))
                    .pt(px(16.))
                    .pb(px(8.))
                    .flex()
                    .justify_end()
                    .child(claim),
            );
        div()
            .id("shop-checkout-layer")
            .occlude()
            .absolute()
            .inset_0()
            .bg(color("--background-scrim"))
            .flex()
            .items_center()
            .justify_center()
            .on_click(cx.listener(|this, _, _, cx| {
                cx.stop_propagation();
                this.shop.checkout = None;
                cx.notify();
            }))
            .child(panel)
            .into_any_element()
    }

    /// What a button of the celebration modal does (`aG` of module 184264): every button but "Use Now" closes the
    /// modal first, then goes where its label says.
    fn shop_collected_act(
        &mut self,
        action: data::CollectedAction,
        product: &Product,
        cx: &mut Context<Self>,
    ) {
        use data::CollectedAction::*;
        if action == UseNow {
            // The writes go out one after the other; the toast and the close wait for the last answer.
            let writes = data::use_now_writes(product);
            let last = writes.len().saturating_sub(1);
            for (index, write) in writes.into_iter().enumerate() {
                let _ = self.requests.send(crate::net::Request::Rest {
                    key: (index == last)
                        .then(|| format!("{}usenow:{}", super::FETCH_PREFIX, product.sku_id)),
                    method: write.method,
                    route: write.route,
                    body: Some(write.body),
                });
            }
            return;
        }
        self.shop.checkout = None;
        self.shop.modal = None;
        match action {
            Close | UseNow => {}
            EarnMoreOrbs => self.open_quests(cx),
            ViewBadge => {
                if self.status_popout.is_none() {
                    self.toggle_status_popout(cx);
                }
            }
            ExplorePerks => self.open_nitro(cx),
            ViewCredits => self.open_settings_page("subscriptions", cx),
            EditProfile => self.open_profile_editor(cx),
        }
        cx.notify();
    }

    /// The answer to "Use Now": the toast of module 450481 and the modal closing, or the modal closing alone.
    pub(super) fn shop_use_now_answered(
        &mut self,
        key: &str,
        body: &serde_json::Value,
        cx: &mut Context<Self>,
    ) {
        let sku = key
            .trim_start_matches(super::FETCH_PREFIX)
            .trim_start_matches("usenow:");
        if body["ok"] == true
            && let Some(product) = self.shop.products.get(sku)
        {
            let text = data::use_now_toast(product);
            self.show_success_toast(text.to_owned(), cx);
        }
        self.shop.checkout = None;
        self.shop.modal = None;
        cx.notify();
    }

    /// The celebration modal after a claim (`COLLECTIBLES_COLLECTED_MODAL`, modules 393870 and 328064): the
    /// item, a title, the line under it and the buttons. The product modal closes behind it.
    fn shop_collected(
        &mut self,
        product: &Product,
        consumed: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let copy = data::collected_copy(product, consumed);
        let shown_product = product.clone();
        let button = |id: &'static str,
                      label: &'static str,
                      primary: bool,
                      action: data::CollectedAction,
                      cx: &mut Context<Self>| {
            let product = shown_product.clone();
            let (base, hover) = if primary {
                ("--background-brand", "--brand-560")
            } else {
                (
                    "--control-secondary-background-default",
                    "--control-secondary-background-hover",
                )
            };
            div()
                .id(id)
                .h(px(40.))
                .px(px(16.))
                .rounded(px(8.))
                .flex()
                .items_center()
                .justify_center()
                .cursor_pointer()
                .bg(color(base))
                .hover(move |button| button.bg(color(hover)))
                .on_click(cx.listener(move |this, _, _, cx| {
                    cx.stop_propagation();
                    this.shop_collected_act(action, &product, cx);
                }))
                .child(text(16., 20., 500., "--white").child(label))
        };
        let actions: Vec<AnyElement> = copy
            .buttons
            .iter()
            .map(|(label, primary, action)| {
                let id = if *primary {
                    "shop-collected-primary"
                } else {
                    "shop-collected-secondary"
                };
                button(id, label, *primary, *action, cx).into_any_element()
            })
            .collect();
        let wide = matches!(
            product.kind,
            data::KIND_NAMEPLATE | data::KIND_AVATAR_DECORATION | data::KIND_BUNDLE
        );
        let height = if wide { 162. } else { 192. };
        let art = self.shop_preview(product, 288., false, false, window, cx);
        let panel = div()
            .id("shop-collected")
            .parity("shop-collected")
            .occlude()
            .w(px(480.))
            .rounded(px(8.))
            .overflow_hidden()
            .bg(color("--modal-background"))
            .flex()
            .flex_col()
            .pt(px(8.))
            .pb(px(16.))
            .on_click(|_, _, cx| cx.stop_propagation())
            .child(
                div()
                    .relative()
                    .px(px(32.))
                    .pt(px(16.))
                    .pb(px(16.))
                    .flex()
                    .flex_col()
                    .items_center()
                    .child(div().absolute().top_0().left_0().right_0().h_full().bg(
                        gpui::linear_gradient(
                            90.,
                            gpui::linear_color_stop(gpui::rgba(0xa157f273), 0.),
                            gpui::linear_color_stop(gpui::rgba(0x3d168a73), 1.),
                        ),
                    ))
                    .child(div().absolute().top_0().left_0().right_0().h_full().bg(
                        gpui::linear_gradient(
                            180.,
                            gpui::linear_color_stop(gpui::rgba(0x00000000), 0.),
                            gpui::linear_color_stop(color("--modal-background"), 1.),
                        ),
                    ))
                    .child(
                        div()
                            .relative()
                            .pt(px(20.))
                            .pb(px(16.))
                            .w_full()
                            .flex()
                            .justify_center()
                            .child(
                                div()
                                    .relative()
                                    .w(px(288.))
                                    .h(px(height))
                                    .overflow_hidden()
                                    .child(div().absolute().inset_0().child(art)),
                            ),
                    )
                    .child(
                        div()
                            .relative()
                            .text_center()
                            .child(text(24., 30., 600., "--text-strong").child(copy.title)),
                    )
                    .child(
                        div()
                            .relative()
                            .mt(px(8.))
                            .text_center()
                            .child(text(16., 20., 400., "--text-subtle").child(copy.subtitle)),
                    ),
            )
            .child(
                div()
                    .px(px(32.))
                    .pt(px(16.))
                    .pb(px(16.))
                    .flex()
                    .justify_end()
                    .gap(px(8.))
                    .children(actions),
            );
        div()
            .id("shop-collected-layer")
            .occlude()
            .absolute()
            .inset_0()
            .bg(color("--background-scrim"))
            .flex()
            .items_center()
            .justify_center()
            .child(panel)
            .into_any_element()
    }
}

/// The platform's classic scrollbar the official modal shows (`overflow-y: scroll` with a light color scheme): a
/// 15px track, 17px arrow buttons and a 9px thumb.
fn classic_scrollbar(handle: gpui::ScrollHandle) -> impl IntoElement {
    use gpui::{Bounds, canvas, fill, point, size};
    canvas(
        |_, _, _| {},
        move |bounds, _, window, _| {
            let track = grey(252);
            let ink = grey(139);
            window.paint_quad(fill(bounds, track));
            let max = handle.max_offset().y;
            let viewport = bounds.size.height;
            let arrow = px(17.);
            let span = viewport - arrow * 2.;
            let center = bounds.left() + px(7.5);
            for (row, width) in [1., 1., 3., 3., 5., 5., 7.].into_iter().enumerate() {
                let up = Bounds::new(
                    point(center - px(width / 2.), bounds.top() + px(5. + row as f32)),
                    size(px(width), px(1.)),
                );
                let down = Bounds::new(
                    point(
                        center - px(width / 2.),
                        bounds.bottom() - px(5. + row as f32 + 1.),
                    ),
                    size(px(width), px(1.)),
                );
                window.paint_quad(fill(up, ink));
                window.paint_quad(fill(down, ink));
            }
            let thumb = if max > px(0.) {
                (span * (viewport / (viewport + max))).max(px(20.))
            } else {
                span
            };
            let scrolled = if max > px(0.) {
                -handle.offset().y / max
            } else {
                0.
            };
            let top = bounds.top() + arrow + (span - thumb) * scrolled;
            let area = Bounds::new(point(bounds.left() + px(3.), top), size(px(9.), thumb));
            window.paint_quad(fill(area, ink));
        },
    )
    .absolute()
    .top_0()
    .right_0()
    .w(px(15.))
    .h_full()
}

fn grey(level: u8) -> gpui::Rgba {
    let value = f32::from(level) / 255.;
    gpui::Rgba {
        r: value,
        g: value,
        b: value,
        a: 1.,
    }
}
