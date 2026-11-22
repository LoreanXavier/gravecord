//! The Shop page behind the Shop row of the Home sidebar (the router's `/shop`). The header with its hover menus, the
//! search field, the wishlist button and the Orbs balance live here, together with the answers the pages read; the cards
//! are in `cards`, the Featured and Orbs Shop pages in `featured`, the Browse page in `browse` and the product modal in
//! `modal`. Official modules followed: 971458 (the header and its menus), 685491 (the search field), 253782 (the tab
//! page), 821816 (Browse), 987421 (the card), 110676 (the blocks).
pub(crate) mod art_cache;
mod browse;
pub(in crate::ui) mod cards;
pub(crate) mod checkout_review;
pub(in crate::ui) mod collections;
pub(super) mod data;
mod editor_wishlist;
mod featured;
pub(in crate::ui) mod game_shop;
pub(crate) mod gift_cta;
mod modal;

use super::discover::{gutter_scrollbar, text};
use super::{Root, color, editor_style, icon};
use crate::ui::motion::Transitions as _;
use crate::{
    editor::{Editor, EditorEvent},
    parity::Parity,
};
use data::{Category, Filters, Home, Product, Sort};
use gpui::{
    AnyElement, Context, Entity, Focusable, IntoElement, ScrollHandle, SharedString, Subscription,
    Window, deferred, div, img, point, prelude::*, px, relative,
};
use std::collections::{HashMap, HashSet};

pub(super) const FETCH_PREFIX: &str = "shop:";
/// The width of the scroller's gutter and the page's left edge in the 1280 window.
const GUTTER: f32 = 8.;
const HEADER: f32 = 49.;
/// The feed's first count and its step (`useState(12)` and `e => e + 12` of module 253782) and the count at which it
/// stops growing and the end-of-feed block shows.
pub(super) const FEED_FIRST: usize = 12;
pub(super) const FEED_MAX: usize = 36;

/// The tabs of the header (`G2` of module 758836) the page can show.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum Tab {
    #[default]
    Featured,
    Browse,
    Orbs,
}

/// The hover menus of the header and the balance popout.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Menu {
    Browse,
    GameShops,
    More,
    #[allow(dead_code)]
    Balance,
}

#[derive(Default)]
pub(super) struct State {
    pub open: bool,
    pub tab: Tab,
    /// Which header entry the pointer is on, and which menu it is on; a menu stays open while either holds.
    /// The menu that is open, and the menu the pointer is in (the header entry or its popout). A menu opens and
    /// closes 100 ms after the pointer arrives or leaves (`useHoverDelay(100, 100)` of module 971458); the
    /// balance popout opens by click.
    pub menu_open: Option<Menu>,
    pub pointer_in: Option<Menu>,
    /// The Game Shops menu: the storefronts that list themselves in the navigation and their applications.
    pub game_shops: Vec<data::GameShop>,
    pub storefront_apps: Vec<String>,
    /// The wishlist page behind the heart button of the header.
    pub wishlist_open: bool,
    /// The wishlist's skus in the order the server listed them.
    pub wishlist_order: Vec<String>,
    pub scroll: ScrollHandle,
    /// The Game Shop page in view: `/shop?tab=game-shops&applicationId=..` (module 980780).
    pub game_shop: Option<game_shop::View>,
    /// The storefronts read for the Game Shop pages, by application id.
    pub storefronts: HashMap<String, game_shop::Storefront>,
    pub search: Option<Entity<Editor>>,
    pub events: Option<Subscription>,
    pub home: Home,
    pub orbs: Home,
    pub collections: Vec<Category>,
    pub collections_total: u64,
    pub products: HashMap<String, Product>,
    pub balance: Option<i64>,
    /// Skus on the account's wishlist: read when the Shop opens, changed by the hearts.
    pub wishlist: HashSet<String>,
    /// Friend user IDs who have wishlisted a given SKU (module 532423 avatar pile badge).
    pub wishlisted_by: HashMap<String, Vec<dc_state::Id>>,
    /// Module 124557: guard preventing overlapping wishlist writes.
    pub wishlist_busy: bool,
    /// Module 38884: "Added to your Wishlist!" NUX modal (`Some(sku)` when open).
    pub wishlist_nux_sku: Option<String>,
    /// Module 38884: whether the first-add NUX modal has already been shown this session.
    pub wishlist_nux_seen: bool,
    pub filters: Filters,
    pub sort: Sort,
    /// Whether the shopper picked a sort: until then the filters choose it.
    pub sort_chosen: bool,
    pub filters_open: bool,
    pub sort_open: bool,
    /// The search answer of the Browse page: sku ids and whether more pages exist.
    pub results: Option<(Vec<String>, bool)>,
    pub feed: Vec<String>,
    pub modal: Option<String>,
    /// The slide of a bundle the modal previews (`selectedBundleSlide`).
    pub bundle_slide: usize,
    pub shelves: HashMap<String, ScrollHandle>,
    pub notice: Option<String>,
    pub checkout: Option<modal::Checkout>,
    /// The sort of the "Find your style" feed, its select being open, and how many times Shuffle was pressed.
    /// The feed's chosen sort; `None` is the block's default ("For You" when it has recommendations).
    pub feed_sort: Option<Sort>,
    pub feed_menu: bool,
    pub shuffle: u32,
    /// The collection page being shown on the Browse tab (a category's sku id).
    pub collection: Option<String>,
    /// The id of the account's wishlist, learned from the answer to an add.
    pub wishlist_id: Option<String>,
    /// The id of the order an Orbs claim created.
    pub order: Option<String>,
    /// The card the pointer is over (an effect preview plays while it is).
    pub hover_card: Option<String>,
    /// The scroll of the product modal's body.
    pub modal_scroll: ScrollHandle,
    pub catalog_pages: u32,
    pub results_key: String,
    /// When the Shop was last opened, and when each of its pictures was asked for and first drawn (milliseconds
    /// after asking, milliseconds after opening): the loading times the harness reports.
    pub opened_at: Option<std::time::Instant>,
    pub img_asked: HashMap<String, std::time::Instant>,
    pub img_seen: HashMap<String, (u64, u64)>,
    /// How many cards the "Find your style" feed shows (`numVisibleItems` of module 253782: 12, then 12 more each
    /// time the page is scrolled to within 200px of its end, up to 36).
    pub feed_visible: usize,
    /// Width over height of the pictures drawn with a cover fit, by url.
    pub img_aspect: HashMap<String, f32>,
    /// The skus each product batch asked for, by request key: a sku the answer leaves out is absent, and its card is
    /// not drawn (`state: "error"` of module 682301 makes the card return nothing).
    pub sku_batches: HashMap<String, Vec<String>>,
    pub absent: HashSet<String>,
    /// A sideways shelf scroll in progress: from, to, and when it started on the motion clock.
    pub shelf_glide: HashMap<String, (f32, f32, f64, Option<f32>)>,
    /// Module 599062: forced/observed Shop load error message (for error state rendering & retry).
    pub load_error: Option<String>,
    /// Module 783481: Social Layer Storefront coupon purchase complete modal.
    pub coupon_complete: Option<game_shop::CouponCompleteModal>,
    /// Module 787844: in-flight Orbs virtual-currency coupon redeem metadata.
    pub coupon_redeeming: Option<game_shop::PendingCouponRedeem>,
    /// Module 784054: selected product variant index by parent SKU id.
    pub variant_selections: HashMap<String, usize>,
    /// Module 784054: currently hovered product variant swatch `(parent_sku_id, variant_index)`.
    pub variant_hover: Option<(String, usize)>,
}

/// Origin of the Shop load error view (module 599062 `A`: `SHOP_PAGE = 0`, `GIFT_MODAL = 1`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(dead_code)]
pub(super) enum ShopErrorOrigin {
    ShopPage,
    GiftModal,
}

impl Root {
    /// The Shop is the Home content while its flag is set and nothing else has taken the content area.
    pub(super) fn shop_active(&self) -> bool {
        self.shop.open
            && self.guild.is_none()
            && self.channel.is_none()
            && !self.nitro_open
            && !self.discover.open
            && !self.quests_active()
    }

    pub(super) fn open_shop(&mut self, cx: &mut Context<Self>) {
        if self.shop_active() {
            return;
        }
        let from = self.route();
        self.history.visit(
            from,
            super::history::Route {
                guild: None,
                channel: None,
                favorites: false,
                nitro: false,
                guild_guide: false,
                channel_browser: false,
                discover: None,
                quests: false,
                shop: true,
                message_requests: false,
                library: false,
            },
        );
        self.set_shop_open(cx);
    }

    /// The part of `open_shop` a history replay also runs.
    pub(super) fn set_shop_open(&mut self, cx: &mut Context<Self>) {
        self.favorites_view = false;
        self.shop.open = true;
        self.guild = None;
        self.channel = None;
        self.nitro_open = false;
        self.guild_guide_open = false;
        self.channel_browser_open = false;
        self.discover.open = false;
        self.quests.open = false;
        self.shop_enter(cx);
    }

    pub(super) fn shop_enter(&mut self, cx: &mut Context<Self>) {
        self.shop_search_editor(cx);
        self.shop.scroll.set_offset(point(px(0.), px(0.)));
        self.shop.opened_at = Some(std::time::Instant::now());
        self.shop.feed_visible = FEED_FIRST;
        tracing::debug!(
            cached = self.fetched.contains_key("shop:home"),
            "shop opened"
        );
        self.shop_request("shop:home", data::shop_home_route("home"));
        self.shop_request("shop:orbs", data::shop_home_route("orbs"));
        self.shop_request("shop:balance", data::BALANCE_ROUTE.to_owned());
        self.shop_request("shop:storefronts", data::STOREFRONT_CONFIG_ROUTE.to_owned());
        if let Some(user) = self.stores.current_user.as_ref().map(|user| user.id) {
            self.shop_request("shop:profile", data::own_profile_route(user));
        }
        cx.notify();
    }

    pub(super) fn shop_request(&mut self, key: &str, route: String) {
        if self.requested.insert(key.to_owned()) {
            let _ = self.requests.send(crate::net::Request::Fetch {
                key: key.to_owned(),
                route,
            });
        }
    }

    pub(super) fn shop_set_tab(&mut self, tab: Tab, cx: &mut Context<Self>) {
        self.shop.tab = tab;
        self.shop.game_shop = None;
        self.shop.wishlist_open = false;
        self.shop.menu_open = None;
        self.shop.pointer_in = None;
        self.shop.modal = None;
        self.shop.scroll.set_offset(point(px(0.), px(0.)));
        if tab == Tab::Browse {
            self.shop_browse_enter();
        }
        cx.notify();
    }

    /// The bytes behind a shop image, asking for them on the first draw.
    pub(super) fn shop_img(&mut self, url: &str) -> Option<std::sync::Arc<gpui::Image>> {
        let key = format!("shop:img:{url}");
        // Through `request_image`, so a download that failed is asked for again (`image_retry`). Sent straight to the
        // network before, a failed or timed-out picture stayed blank for the rest of the session.
        if !self.requested.contains(&key) {
            self.shop
                .img_asked
                .insert(key.clone(), std::time::Instant::now());
        }
        self.request_image(key.clone(), url.to_owned());
        let image = self.images.get(&key).cloned();
        if image.is_some() && !self.shop.img_seen.contains_key(&key) {
            let after = self
                .shop
                .img_asked
                .get(&key)
                .map_or(0, |asked| asked.elapsed().as_millis() as u64);
            let since_open = self
                .shop
                .opened_at
                .map_or(0, |at| at.elapsed().as_millis() as u64);
            tracing::debug!(after, since_open, "shop picture drawn");
            self.shop.img_seen.insert(key, (after, since_open));
        }
        image
    }

    /// The pictures the Shop asked for, drawn and still missing, with the times from asking to the first draw.
    #[cfg(feature = "capture")]
    pub(super) fn shop_image_report(&self) -> String {
        let asked = self.shop.img_asked.len();
        let mut waits: Vec<u64> = self
            .shop
            .img_seen
            .values()
            .map(|(after, _)| *after)
            .collect();
        waits.sort_unstable();
        let last = self
            .shop
            .img_seen
            .values()
            .map(|(_, since_open)| *since_open)
            .max()
            .unwrap_or(0);
        let missing: Vec<&String> = self
            .shop
            .img_asked
            .keys()
            .filter(|key| !self.shop.img_seen.contains_key(*key))
            .collect();
        let median = waits.get(waits.len() / 2).copied().unwrap_or(0);
        let max = waits.last().copied().unwrap_or(0);
        let opened = self
            .shop
            .opened_at
            .map_or(0, |at| at.elapsed().as_millis() as u64);
        let mut out = format!(
            "shop pictures: asked {asked}, drawn {}, missing {}, ask-to-draw median {median} ms max {max} ms, last drawn {last} ms after opening, open for {opened} ms",
            waits.len(),
            missing.len()
        );
        for key in missing.iter().take(12) {
            out.push_str(&format!(
                "\n  missing {}",
                key.trim_start_matches("shop:img:")
            ));
        }
        // Every drawn picture with its wait, in the order they arrived.
        let mut drawn: Vec<(&String, &(u64, u64))> = self.shop.img_seen.iter().collect();
        drawn.sort_by_key(|(_, (_, since_open))| *since_open);
        for (key, (after, since_open)) in drawn {
            let url = key.trim_start_matches("shop:img:");
            let tail = url.rsplit('/').take(2).collect::<Vec<_>>().join("<");
            let bytes = self.images.get(key).map_or(0, |image| image.bytes().len());
            out.push_str(&format!(
                "\n  drawn {after} ms after asking, {since_open} ms after opening, {bytes} bytes: {tail}"
            ));
        }
        out
    }

    /// `shop_img` for a picture drawn in a box of `width` x `height` logical pixels: it is decoded at that size times the
    /// window's scale factor (with a quarter more) and no larger, which the cover fit of every caller needs.
    pub(super) fn shop_img_box(
        &mut self,
        url: &str,
        width: f32,
        height: f32,
    ) -> Option<std::sync::Arc<gpui::Image>> {
        let key = format!("shop:img:{url}");
        if !self.requested.contains(&key)
            && let Ok(mut boxes) = crate::net::SHOP_FIT.lock()
        {
            boxes
                .get_or_insert_with(Default::default)
                .insert(key, (width, height));
        }
        self.shop_img(url)
    }

    /// Asks for the products that no answer has delivered yet (`requestProducts` of module 295586: batches of 100).
    pub(super) fn shop_need_products(&mut self, skus: &[String]) {
        // Skus a batch already asked for (answered or still out) are not asked for again.
        let asked: HashSet<&String> = self.shop.sku_batches.values().flatten().collect();
        let mut seen = HashSet::new();
        let missing: Vec<String> = skus
            .iter()
            .filter(|sku| {
                !self.shop.products.contains_key(*sku)
                    && !self.shop.absent.contains(*sku)
                    && !asked.contains(sku)
                    && seen.insert(*sku)
            })
            .cloned()
            .collect();
        for chunk in missing.chunks(100) {
            let key = format!("shop:skus:{}:{}", chunk[0], chunk.len());
            self.shop.sku_batches.insert(key.clone(), chunk.to_vec());
            self.shop_request(&key, data::products_route(chunk));
        }
    }

    pub(super) fn shop_fetched(&mut self, key: &str, cx: &mut Context<Self>) {
        let Some(body) = self.fetched.get(key).cloned() else {
            return;
        };
        if key.starts_with("shop:usenow:") {
            self.shop_use_now_answered(key, &body, cx);
            return;
        }
        if key.starts_with("shop:order:") {
            self.shop_order_fetched(key, &body, cx);
            return;
        }
        if body["fetch_failed"] == true {
            // A failed batch may be asked for again.
            self.shop.sku_batches.remove(key);
            self.shop_forget(key);
            cx.notify();
            return;
        }
        if key.starts_with("shop:gameshop:") {
            self.shop_gameshop_fetched(key, &body);
            cx.notify();
            return;
        }
        let remember = |this: &mut Root, products: &[Product]| {
            for product in products {
                this.shop
                    .products
                    .insert(product.sku_id.clone(), product.clone());
            }
        };
        match key {
            "shop:wishlist" => {
                // `{body: {id, wishlist_items: [{sku: {id}}]}}` (module 808247's answer to an add or a remove).
                self.shop_take_wishlist(&body["body"]);
            }
            "shop:home" | "shop:orbs" => {
                self.shop_seed_effects(&body);
                let home = data::parse_home(&body);
                for category in &home.categories {
                    remember(self, &category.products);
                }
                let mut wanted = Vec::new();
                for block in &home.blocks {
                    match block {
                        data::Block::Feed {
                            popular,
                            recommended,
                        } if key == "shop:home" => {
                            wanted.extend(popular.iter().take(24).cloned());
                            wanted.extend(recommended.iter().take(24).cloned());
                        }
                        data::Block::Feed { popular, .. } => {
                            wanted.extend(popular.iter().take(12).cloned());
                        }
                        data::Block::Shelf { skus, .. } => {
                            wanted.extend(skus.iter().cloned());
                        }
                        data::Block::Hero(hero) => {
                            wanted.extend(hero.skus.iter().cloned());
                        }
                        _ => {}
                    }
                }
                if key == "shop:home" {
                    self.shop.home = home;
                } else {
                    self.shop.orbs = home;
                }
                self.shop_need_products(&wanted);
            }
            "shop:balance" => {
                self.shop.balance = body["balance"].as_i64();
            }
            "shop:storefronts" => {
                self.shop.storefront_apps = data::storefront_nav_apps(&body);
                if !self.shop.storefront_apps.is_empty() {
                    let route = data::applications_route(&self.shop.storefront_apps);
                    self.shop_request("shop:apps", route);
                }
            }
            "shop:apps" => {
                self.shop.game_shops =
                    data::parse_game_shops(&self.shop.storefront_apps.clone(), &body);
            }
            "shop:profile" => {
                // The wishlist ids are the keys of `wishlist_settings`; the first one is read (`getFirstWishlistId`).
                if let Some(id) = data::wishlist_ids(&body).into_iter().next() {
                    self.shop.wishlist_id = Some(id.clone());
                    self.shop_request("shop:wishlistread", data::wishlist_route(&id));
                }
            }
            "shop:wishlistread" => {
                self.shop_take_wishlist(&body);
            }
            _ if key.starts_with("shop:collections:") => {
                self.shop_seed_effects(&body);
                let (categories, total) = data::parse_collections(&body);
                for category in &categories {
                    remember(self, &category.products);
                }
                self.shop.collections.extend(categories);
                self.shop.collections_total = total;
            }
            _ if key.starts_with("shop:search:") => {
                let (skus, more) = data::parse_search(&body);
                self.shop_need_products(&skus);
                if key == self.shop.results_key {
                    self.shop.results = Some((skus, more));
                }
            }
            _ if key.starts_with("shop:skus:") => {
                let products = data::parse_products(&body);
                remember(self, &products);
                self.shop_seed_effects(&body);
                let mut touched_wishlist = false;
                for product in &products {
                    if self.wishlist_prefetch_skus.contains(&product.sku_id)
                        || self.shop.wishlist.contains(&product.sku_id)
                    {
                        self.shop_prefetch_product_art(product, 78.);
                        touched_wishlist = true;
                    }
                }
                if touched_wishlist {
                    self.persist_wishlist_cache();
                }
                if let Some(asked) = self.shop.sku_batches.get(key) {
                    let absent = data::absent_skus(asked, &products);
                    self.shop.absent.extend(absent);
                }
                tracing::debug!(
                    products = products.len(),
                    since_open = self
                        .shop
                        .opened_at
                        .map_or(0, |at| at.elapsed().as_millis() as u64),
                    "shop products answered"
                );
            }
            _ if key.starts_with("shop:feed:") => {
                let (skus, _) = data::parse_search(&body);
                self.shop_need_products(&skus);
                self.shop.feed = skus;
            }
            _ if key.starts_with("shop:product:") => {
                self.shop_seed_effects(&body);
                if let Some(product) = data::parse_product(&body) {
                    remember(self, &[product]);
                }
            }
            _ => {}
        }
        cx.notify();
    }

    /// Seeds `ProfileEffectStore` from any shop, storefront, or wishlist payload with embedded effect layers.
    pub(in crate::ui) fn shop_seed_effects(&mut self, body: &serde_json::Value) {
        let effects = data::parse_profile_effects(body);
        if effects.is_empty() {
            return;
        }
        let mut cache = self.profile_effects.borrow_mut();
        for (sku, effect) in effects {
            cache.store.insert_for(sku, effect);
        }
    }

    /// Warms `shop_img` / `shop_img_box` / effect art for a product likely to be rendered soon (e.g. wishlists).
    pub(in crate::ui) fn shop_prefetch_product_art(&mut self, product: &Product, size: f32) {
        let item = product.items.first();
        match product.kind {
            data::KIND_BUNDLE => {
                if let Some(bg) = &product.bg_static {
                    self.shop_img_box(bg, 242., 242.);
                }
                if let Some(fg) = &product.fg_static {
                    self.shop_img_box(fg, size, size);
                } else {
                    let scale = size / 242.;
                    let area_height = 186. * scale;
                    let box_width = area_height * 1.1;
                    let three = product
                        .items
                        .iter()
                        .any(|i| i.kind == data::KIND_PROFILE_EFFECT)
                        && product
                            .items
                            .iter()
                            .any(|i| i.kind == data::KIND_AVATAR_DECORATION)
                        && product.items.iter().any(|i| i.kind == data::KIND_NAMEPLATE);
                    let avatar = if three { 72. } else { 80. } * scale;
                    for item in &product.items {
                        match item.kind {
                            data::KIND_PROFILE_EFFECT => {
                                let height = area_height * 0.8;
                                let width = height * if three { 0.8 } else { 0.86 };
                                self.shop_img_box(cards::PROFILE_MOCK, width, height);
                                let still = item
                                    .thumbnail
                                    .clone()
                                    .unwrap_or_else(|| cards::item_static(&item.sku_id));
                                self.shop_img_box(&still, width, height);
                                if let Ok(sku) = item.sku_id.parse::<u64>() {
                                    self.profile_effect_prefetch_sku(sku);
                                }
                            }
                            data::KIND_AVATAR_DECORATION => {
                                let ring = avatar * 1.2;
                                self.shop_img_box(&cards::item_static(&item.sku_id), ring, ring);
                            }
                            data::KIND_NAMEPLATE => {
                                let width = box_width * 0.96;
                                self.shop_img_box(
                                    &cards::item_static(&item.sku_id),
                                    width,
                                    42. * scale,
                                );
                            }
                            _ => {}
                        }
                    }
                    for sub in &product.bundled {
                        self.shop_prefetch_product_art(sub, size);
                    }
                }
            }
            data::KIND_EXTERNAL => {
                if let Some(url) = data::external_art_url(&product.sku_id) {
                    self.shop_img_box(&url, size, 252.);
                }
            }
            data::KIND_AVATAR_DECORATION => {
                let id = item.map_or(product.sku_id.as_str(), |item| item.sku_id.as_str());
                let box_side = 144. * (size / 242.);
                self.shop_img_box(&cards::item_static(id), box_side, box_side);
            }
            data::KIND_NAMEPLATE => {
                let id = item.map_or(product.sku_id.as_str(), |item| item.sku_id.as_str());
                self.shop_img(&cards::item_static(id));
            }
            data::KIND_PROFILE_EFFECT | data::KIND_PROFILE_FRAME => {
                self.shop_img_box(cards::PROFILE_MOCK, size, size);
                if let Some(item) = item {
                    if let Some(thumbnail) = &item.thumbnail {
                        self.shop_img(thumbnail);
                    }
                    for layer in &item.layers {
                        let url = format!(
                            "https://cdn.discordapp.com/media/v1/collectibles-shop/{}/{layer}/static",
                            item.sku_id
                        );
                        self.shop_img(&url);
                    }
                }
                if product.kind == data::KIND_PROFILE_EFFECT
                    && let Ok(sku) = product.sku_id.parse::<u64>()
                {
                    self.profile_effect_prefetch_sku(sku);
                }
            }
            _ => {
                self.shop_img(&cards::item_static(&product.sku_id));
            }
        }
    }

    /// The wishlist an answer carries: its id, its skus, and the products behind them.
    fn shop_take_wishlist(&mut self, list: &serde_json::Value) {
        if let Some(id) = list["id"].as_str() {
            self.shop.wishlist_id = Some(id.to_owned());
        }
        self.shop_seed_effects(list);
        for product in data::parse_wishlist_products(list) {
            self.shop
                .products
                .insert(product.sku_id.clone(), product.clone());
            self.shop_prefetch_product_art(&product, 78.);
        }
        if let Some(items) = list["wishlist_items"].as_array() {
            let skus: Vec<String> = items
                .iter()
                .filter_map(|item| {
                    item["sku"]["id"]
                        .as_str()
                        .or_else(|| item["sku_id"].as_str())
                        .map(str::to_owned)
                })
                .collect();
            self.wishlist_prefetch_skus.extend(skus.iter().cloned());
            self.shop.wishlist = skus.iter().cloned().collect();
            self.shop.wishlist_order = skus.clone();
            for sku in &skus {
                if let Some(product) = self.shop.products.get(sku).cloned() {
                    self.shop_prefetch_product_art(&product, 78.);
                }
            }
            self.shop_need_products(&skus);
            self.editor_wishlist_need(&skus);
            if let Some(me) = self.stores.current_user.as_ref().map(|user| user.id) {
                self.fetched
                    .insert(format!("profile-wishlist:{me}"), list.clone());
                if let Some(wid) = list["id"].as_str() {
                    self.wishlist_ids_cache.insert(me, wid.to_owned());
                }
            }
            self.persist_wishlist_cache();
        }
    }

    pub(super) fn shop_is_wishlisted(&self, sku: &str) -> bool {
        self.shop.wishlist.contains(sku)
    }

    pub(super) fn shop_wishlist_toggle(&mut self, sku: &str, cx: &mut Context<Self>) {
        self.shop_toggle_wishlist(sku, cx);
    }

    pub(super) fn shop_open_gift(&mut self, sku: &str, cx: &mut Context<Self>) {
        self.shop_buy(sku, true, cx);
    }

    /// Asks for a read again after an error.
    fn shop_forget(&mut self, key: &str) {
        self.requested.remove(key);
        self.fetched.remove(key);
    }

    /// Writes the raw answers the page parses to `design/parity/shop-dump-<name>.json` (a harness step); the shapes are
    /// noted beside each parser in `data`.
    #[cfg(feature = "capture")]
    pub fn shop_dump(&self) -> String {
        let mut written = Vec::new();
        for (key, body) in &self.fetched {
            if let Some(name) = key.strip_prefix("shop:") {
                let name = name.replace(':', "-");
                let path = format!("design/parity/shop-dump-{name}.json");
                let text = serde_json::to_string_pretty(body).unwrap_or_default();
                if std::fs::write(&path, text).is_ok() {
                    written.push(name);
                }
            }
        }
        written.sort();
        format!(
            "shop dump: {}\n{}",
            written.join(", "),
            self.shop_image_report()
        )
    }

    /// A harness step: opens the product modal and the Orbs checkout for a sku in memory, in a chosen state. Nothing is
    /// sent: `review` is the review step; `done` and `done-consumed` the collected modal; `owned`, `funds`, `bundle`,
    /// `expired`, `late` and `generic` the review with the notice a failed claim shows.
    #[cfg(feature = "capture")]
    pub fn script_shop_checkout(&mut self, words: &[&str], cx: &mut Context<Self>) -> String {
        let Some(sku) = words.first() else {
            return "shop checkout: no sku".to_owned();
        };
        let state = words.get(1).copied().unwrap_or("review");
        self.shop_open_product(sku, cx);
        if state == "open" {
            return format!("shop checkout: {sku} open");
        }
        let error = match state {
            "owned" => Some("Item already owned."),
            "funds" => Some("Not enough Orbs."),
            "bundle" => Some("You already own one or more items in this bundle."),
            "expired" => Some("Your checkout session expired. Reopen checkout to continue."),
            "late" => Some(data::ENTITLEMENTS_LATE),
            "generic" => Some(data::GENERIC_CLAIM_ERROR),
            _ => None,
        };
        let stage = if state.starts_with("done") {
            modal::ClaimStage::Done
        } else {
            modal::ClaimStage::Review
        };
        self.shop.checkout = Some(modal::Checkout {
            sku: (*sku).to_owned(),
            gift: false,
            orbs: true,
            stage,
            error,
            reads: 0,
            consumed: state == "done-consumed",
        });
        cx.notify();
        format!("shop checkout: {sku} {state}")
    }

    #[cfg(feature = "capture")]
    pub(crate) fn script_shop_error(&mut self, arg: &str, cx: &mut Context<Self>) -> String {
        if arg.trim() == "clear" {
            self.shop.load_error = None;
            cx.notify();
            return "shop-error: cleared".to_owned();
        }
        self.set_shop_open(cx);
        self.shop.load_error = Some(if arg.trim().is_empty() {
            "HTTP 503 Service Unavailable (collectibles-categories)".to_owned()
        } else {
            arg.trim().to_owned()
        });
        cx.notify();
        "shop-error: opened".to_owned()
    }

    #[cfg(feature = "capture")]
    pub(super) fn shop_search_editors(&self) -> Vec<Entity<Editor>> {
        if self.shop_active() {
            self.shop.search.iter().cloned().collect()
        } else {
            Vec::new()
        }
    }

    // ---- the search field ----

    fn shop_search_editor(&mut self, cx: &mut Context<Self>) {
        if self.shop.search.is_some() {
            return;
        }
        let editor = cx.new(|cx| {
            let mut style = editor_style();
            style.size = 14.;
            style.line_height = 18.;
            style.text = super::hsla("--input-text-default");
            style.caret = super::hsla("--input-text-default");
            style.placeholder = super::hsla("--input-placeholder-text-default");
            let mut editor = Editor::new(style, cx);
            editor.set_placeholder("Search the Shop".into());
            editor.set_reports_all_edits(true);
            editor
        });
        let events = cx.subscribe(&editor, |this, editor, event: &EditorEvent, cx| {
            match event {
                EditorEvent::Edited => {}
                EditorEvent::Cancel => {
                    editor.update(cx, |editor, _| editor.set_text(""));
                    this.shop_search("", cx);
                }
                // Enter sends the text to the catalog (`ee` of module 685491).
                EditorEvent::Submit(text) => this.shop_search(text.trim(), cx),
            }
            cx.notify();
        });
        self.shop.search = Some(editor);
        self.shop.events = Some(events);
    }

    /// Lets the filters pick the sort while the shopper has not (`userHasSelectedSort`).
    pub(super) fn shop_default_sort(&mut self) {
        if !self.shop.sort_chosen {
            self.shop.sort = self.shop.filters.default_sort();
        }
    }

    /// Runs a search on the Browse page, or clears it.
    pub(super) fn shop_search(&mut self, text: &str, cx: &mut Context<Self>) {
        self.shop.filters.search = text.chars().take(256).collect();
        self.shop_default_sort();
        if self.shop.tab != Tab::Browse {
            self.shop_set_tab(Tab::Browse, cx);
        } else {
            self.shop_browse_enter();
        }
        cx.notify();
    }

    // ---- the page ----

    /// Module 599062 (`h`): Shop Load Error View (`i5SQ74` "Well, this is awkward.", `LuhKs2` / `+2QBZX`,
    /// staff-only debug message when `flags & 1 != 0`, and primary `+hivLW` "Reload" button).
    pub(super) fn shop_load_error_view(
        &self,
        origin: ShopErrorOrigin,
        error_message: Option<&str>,
        retry_key: &'static str,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let light = super::theme() == dc_tokens::Theme::Light;
        let art_url = if light {
            "https://discord.com/assets/9afc0a2d5f56c719.svg"
        } else {
            "https://discord.com/assets/fe8bf3ee09628502.svg"
        };
        let art = self.images.get(art_url).cloned();
        let desc = match origin {
            ShopErrorOrigin::GiftModal => dc_intl::t("+2QBZX"),
            ShopErrorOrigin::ShopPage => dc_intl::t("LuhKs2"),
        };
        let is_staff = self
            .stores
            .current_user
            .as_ref()
            .is_some_and(|u| u.flags & 1 != 0);
        let debug_line = (is_staff || cfg!(feature = "capture"))
            .then_some(error_message)
            .flatten()
            .filter(|m| !m.is_empty())
            .map(|m| format!("staff-only debug: {m}"));
        let mut container = div()
            .id("shop-load-error")
            .w_full()
            .flex()
            .flex_col()
            .items_center()
            .justify_center();
        container = match origin {
            ShopErrorOrigin::ShopPage => container.mt(px(80.)).min_h(px(300.)),
            ShopErrorOrigin::GiftModal => container.p(px(24.)),
        };
        container
            .child(match art {
                Some(image) => img(image)
                    .h(px(140.))
                    .object_fit(gpui::ObjectFit::Contain)
                    .into_any_element(),
                None => icon("storefront_store", 56., "--icon-muted").into_any_element(),
            })
            .child(
                div()
                    .mt(px(24.))
                    .mb(px(12.))
                    .text_size(px(24.))
                    .line_height(px(30.))
                    .font_weight(gpui::FontWeight(600.))
                    .text_color(color("--text-strong"))
                    .child(dc_intl::t("i5SQ74")),
            )
            .child(
                div()
                    .max_w(px(420.))
                    .text_center()
                    .text_size(px(16.))
                    .line_height(px(20.))
                    .text_color(color("--text-default"))
                    .child(desc),
            )
            .when_some(debug_line, |col, line| {
                col.child(
                    div()
                        .mt(px(8.))
                        .text_size(px(12.))
                        .text_color(color("--text-muted"))
                        .child(line),
                )
            })
            .child(
                div()
                    .id("shop-load-error-reload")
                    .mt(px(24.))
                    .h(px(38.))
                    .px(px(20.))
                    .rounded(px(8.))
                    .bg(color("--control-primary-background-default"))
                    .hover(|s| s.bg(color("--control-primary-background-hover")))
                    .cursor_pointer()
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_size(px(14.))
                    .font_weight(gpui::FontWeight(600.))
                    .text_color(color("--white"))
                    .child(dc_intl::t("+hivLW"))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.shop.load_error = None;
                        this.shop_forget(retry_key);
                        this.fetch_failures.remove(retry_key);
                        this.open_shop(cx);
                    })),
            )
            .into_any_element()
    }

    pub(super) fn shop_page(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let in_game_shop = self.shop.game_shop.is_some() && !self.shop.wishlist_open;
        let header = if in_game_shop {
            self.shop_game_header(cx)
        } else {
            self.shop_header(window, cx)
        };
        let scroll = self.shop.scroll.clone();
        let active_key = match self.shop.tab {
            Tab::Featured => "shop:home",
            Tab::Orbs => "shop:orbs",
            Tab::Browse => "shop:collections:0",
        };
        let has_load_error = self.shop.load_error.is_some()
            || (self.fetch_failures.contains_key(active_key)
                && !self.fetched.contains_key(active_key));
        let body = if has_load_error {
            let msg = self.shop.load_error.clone();
            self.shop_load_error_view(ShopErrorOrigin::ShopPage, msg.as_deref(), active_key, cx)
        } else {
            match self.shop.tab {
                _ if self.shop.wishlist_open => self.shop_wishlist_page(window, cx),
                _ if in_game_shop => self.shop_game_body(window, cx),
                Tab::Featured => self.shop_featured(false, window, cx),
                Tab::Orbs => self.shop_featured(true, window, cx),
                Tab::Browse => self.shop_browse(window, cx),
            }
        };
        let scroller = div()
            .id("shop-scroll")
            .size_full()
            .overflow_y_scroll()
            .track_scroll(&scroll)
            .pr(px(GUTTER))
            .child(body);
        let notice = self.shop.notice.clone().map(|message| {
            div()
                .absolute()
                .bottom(px(16.))
                .left_0()
                .right_0()
                .flex()
                .justify_center()
                .child(
                    text(14., 18., 500., "--text-strong")
                        .px(px(16.))
                        .py(px(10.))
                        .rounded(px(8.))
                        .bg(color("--background-surface-higher"))
                        .child(message),
                )
        });
        div()
            .flex_1()
            .min_w(px(0.))
            .h_full()
            .pt(px(1.))
            .child(
                div()
                    .parity("shop-page")
                    .relative()
                    .size_full()
                    .flex()
                    .flex_col()
                    .bg(color("--background-base-lower"))
                    .child(header)
                    .child(
                        div()
                            .parity("shop-view")
                            .relative()
                            .flex_1()
                            .min_h(px(0.))
                            .child(scroller)
                            .child(gutter_scrollbar(scroll)),
                    )
                    .children(notice),
            )
            .into_any_element()
    }

    // ---- the header ----

    /// `headerBar_ViLURN`: 49px tall (the 1px rule included), the shop mark, the tabs, and the search, wishlist and Orbs
    /// balance at the right.
    fn shop_header(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let open = self.shop.menu_open;
        let (visible, overflow) = self.shop_nav_split(window);
        let mut nav = div()
            .parity("shop-nav")
            .h(px(48.))
            .ml(px(44.))
            .flex()
            .items_center()
            .gap(px(32.));
        for option in &visible {
            nav = nav.child(self.shop_nav_item(
                option.id,
                option.label,
                option.tab.is_some_and(|tab| tab == self.shop.tab),
                option.menu,
                cx,
            ));
        }
        if !overflow.is_empty() {
            nav = nav.child(self.shop_nav_item("more", "More", false, Some(Menu::More), cx));
        }
        let search = self.shop_search_field(window, cx);
        let wishlist = self.shop_wishlist_button(cx);
        let balance = self.shop_balance_pill(cx);
        let menu = open.and_then(|menu| self.shop_menu(window, menu, cx));
        div()
            .parity("shop-header")
            .relative()
            .flex_none()
            .w_full()
            .h(px(HEADER))
            .pl(px(16.))
            .pr(px(12.))
            .flex()
            .items_center()
            .child(
                // The shop mark is a button that goes to the Featured tab (`onClick: () => s(G2.HOME)` of module 685491).
                div()
                    .id("shop-mark")
                    .size(px(24.))
                    .flex_none()
                    .cursor_pointer()
                    .on_click(cx.listener(|this, _, _, cx| this.shop_set_tab(Tab::Featured, cx)))
                    .child(icon("shop_header", 24., "--text-strong")),
            )
            .child(nav)
            .child(div().flex_1())
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(search)
                    .child(wishlist)
                    .child(balance),
            )
            .child(
                div()
                    .absolute()
                    .left_0()
                    .right_0()
                    .bottom_0()
                    .h(px(1.))
                    .bg(color("--background-mod-subtle")),
            )
            .children(menu)
            .into_any_element()
    }

    fn shop_nav_item(
        &mut self,
        id: &'static str,
        label: &'static str,
        active: bool,
        menu: Option<Menu>,
        cx: &mut Context<Self>,
    ) -> gpui::Stateful<gpui::Div> {
        let open = menu.is_some() && self.shop.menu_open == menu;
        let tab = match id {
            "featured" => Some(Tab::Featured),
            "browse" => Some(Tab::Browse),
            "orbs" => Some(Tab::Orbs),
            _ => None,
        };
        let beta = id == "game-servers";
        // Game Servers leads to a tab that is not built (BLOCKED.md): it neither takes a click nor underlines on hover.
        let live = tab.is_some() || menu.is_some();
        div()
            .id(SharedString::from(format!("shop-nav-{id}")))
            .group(SharedString::from(format!("shop-nav-{id}")))
            .relative()
            .flex_none()
            .h_full()
            .flex()
            .items_center()
            .when(live, |item| item.cursor_pointer())
            .child(
                text(16., 20., 500., "--text-default")
                    .flex()
                    .items_center()
                    .child(label),
            )
            .when(beta, |item| item.child(div().w(px(8.))).child(beta_badge()))
            .when(menu.is_some(), |item| {
                item.child(
                    div()
                        .w(px(20.))
                        .flex()
                        .justify_end()
                        .items_center()
                        .child(icon(
                            if open { "shop_caret_up" } else { "shop_caret" },
                            12.,
                            "--text-default",
                        )),
                )
            })
            .child(
                div()
                    .absolute()
                    .left_0()
                    .right_0()
                    .bottom_0()
                    .h(px(2.))
                    .when(active, |bar| bar.bg(color("--text-strong")))
                    .when(!active && live, |bar| {
                        bar.group_hover(SharedString::from(format!("shop-nav-{id}")), |bar| {
                            bar.bg(color("--background-brand"))
                        })
                    }),
            )
            .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                if let Some(menu) = menu {
                    this.shop_pointer(menu, *hovered, cx);
                }
            }))
            .on_click(cx.listener(move |this, _, _, cx| {
                if let Some(tab) = tab {
                    this.shop_set_tab(tab, cx);
                }
            }))
    }

    /// `searchBar`: 182 by 32 with the 12px magnifier in a 22px box.
    fn shop_search_field(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let Some(editor) = self.shop.search.clone() else {
            return div().into_any_element();
        };
        let focused = editor.read(cx).focus_handle(cx).is_focused(window);
        let filled = !editor.read(cx).text().is_empty();
        let border = if focused { 2. } else { 1. };
        div()
            .parity_with_border("shop-search", border)
            .flex_none()
            .w(px(182.))
            .h(px(32.))
            .border(px(border))
            .border_color(color(if focused {
                "--input-border-active"
            } else {
                "--input-border-default"
            }))
            .rule(crate::ui::motion::Rule::TextField)
            .rounded(px(8.))
            .bg(color("--input-background-default"))
            .pl(px(4. - border))
            .pr(px(5. - border))
            .flex()
            .items_center()
            .gap(px(4.))
            .child(
                div()
                    .size(px(22.))
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(icon("shop_search", 12., "--interactive-text-default")),
            )
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .h(px(18.))
                    .child(editor.clone()),
            )
            .when(filled, |row| {
                row.child(
                    div()
                        .id("shop-search-clear")
                        .size(px(22.))
                        .flex_none()
                        .rounded(px(4.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .cursor_pointer()
                        .on_click(cx.listener(|this, _, _, cx| {
                            if let Some(editor) = this.shop.search.clone() {
                                editor.update(cx, |editor, _| editor.set_text(""));
                            }
                            this.shop_search("", cx);
                        }))
                        .child(icon("discover_clear", 12., "--interactive-text-default")),
                )
            })
            .into_any_element()
    }

    /// The 32px wishlist button: it opens the current user's `FullProfileModal` on `Tab::Wishlist`.
    fn shop_wishlist_button(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let button = div()
            .id("shop-wishlist")
            .parity("shop-wishlist")
            .size(px(32.))
            .flex_none()
            .rounded(px(8.))
            .bg(color("--background-mod-subtle"))
            .flex()
            .items_center()
            .justify_center()
            .cursor_pointer()
            .hover(|button| button.bg(color("--background-mod-strong")))
            .on_click(cx.listener(|this, _, window, cx| this.shop_open_wishlist(window, cx)))
            .child(icon("shop_heart", 16., "--text-default"));
        self.with_tip(
            "shop-wishlist",
            super::tooltip::Tip::text(dc_intl::t!("7lZ31J"), super::tooltip::Side::Bottom),
            button,
            cx,
        )
        .into_any_element()
    }

    /// The balance pill: the Orbs mark and the amount, with the popout of module 986630 on hover.
    fn shop_balance_pill(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let amount = self.shop.balance.unwrap_or(0);
        div()
            .id("shop-balance")
            .parity("shop-balance")
            .flex_none()
            .h(px(32.))
            .px(px(12.))
            .rounded(px(8.))
            .bg(color("--background-mod-subtle"))
            .flex()
            .items_center()
            .gap(px(2.))
            .cursor_pointer()
            .hover(|pill| pill.bg(color("--background-mod-strong")))
            .on_click(cx.listener(|this, _, _, cx| {
                this.shop.menu_open = None;
                this.toggle_orb_wallet(cx);
            }))
            .child(icon("shop_orb_18", 20., "--text-default"))
            .child(text(16., 20., 600., "--text-default").child(amount.to_string()))
            .into_any_element()
    }

    /// The header options in order (`useCollectiblesShopNavigationMenuOptions` of module 685491): Featured, Browse, the
    /// Orbs Shop, the Game Shops menu once a storefront lists itself, and Game Servers.
    fn shop_nav_options(&self) -> Vec<NavOption> {
        let mut options = vec![
            NavOption::page("featured", "Featured", Tab::Featured, 62.17),
            NavOption::menu("browse", "Browse", Menu::Browse, 72.95, Some(Tab::Browse)),
            NavOption::page("orbs", "Orbs Shop", Tab::Orbs, 71.97),
        ];
        if !self.shop.game_shops.is_empty() {
            options.push(NavOption::menu(
                "game-shops",
                "Game Shops",
                Menu::GameShops,
                108.23,
                None,
            ));
        }
        options.push(NavOption {
            id: "game-servers",
            label: "Game Servers",
            tab: None,
            menu: None,
            width: 148.70,
        });
        options
    }

    /// The options that stay in the row and the ones that moved into More: the row is as wide as the window minus
    /// the sidebars and the toolbar (513.5 at 1280), the gap between options 32.
    fn shop_nav_split(&self, window: &Window) -> (Vec<NavOption>, Vec<NavOption>) {
        let options = self.shop_nav_options();
        let row = f32::from(window.viewport_size().width) - 766.5;
        let widths: Vec<f32> = options.iter().map(|option| option.width).collect();
        let last = data::last_visible(&widths, NAV_MORE_WIDTH, 32., row);
        if last + 1 >= options.len() {
            return (options, Vec::new());
        }
        let overflow = options[last + 1..].to_vec();
        (options[..=last].to_vec(), overflow)
    }

    /// The pointer arrives in or leaves a menu's entry or popout: 100 ms later the menu opens or closes if the
    /// pointer is still there or still gone.
    fn shop_pointer(&mut self, menu: Menu, inside: bool, cx: &mut Context<Self>) {
        if inside {
            self.shop.pointer_in = Some(menu);
        } else if self.shop.pointer_in == Some(menu) {
            self.shop.pointer_in = None;
        }
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(std::time::Duration::from_millis(100))
                .await;
            let _ = this.update(cx, |this, cx| {
                if inside {
                    if this.shop.pointer_in == Some(menu) && this.shop.menu_open != Some(menu) {
                        this.shop.menu_open = Some(menu);
                        cx.notify();
                    }
                } else if this.shop.pointer_in.is_none() && this.shop.menu_open == Some(menu) {
                    this.shop.menu_open = None;
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// The popout under a header entry: 4px below it, left-aligned with it.
    fn shop_menu(
        &mut self,
        window: &Window,
        menu: Menu,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        // What a row shows and does: its label, the icon in front of it, a beta badge, the tab it opens.
        type Row = (String, Option<String>, bool, Option<Tab>, Option<String>);
        let (left, entries): (f32, Vec<Row>) = match menu {
            Menu::Browse => (
                179.,
                [
                    "Avatar Decorations",
                    "Nameplates",
                    "Profile Effects",
                    "Profile Frames",
                    "Bundles",
                    "Collabs",
                    "Shop All",
                ]
                .into_iter()
                .map(|label| (label.to_owned(), None, false, Some(Tab::Browse), None))
                .collect(),
            ),
            Menu::GameShops => {
                let games = self.shop.game_shops.clone();
                (
                    self.shop_nav_left(window, "game-shops"),
                    games
                        .into_iter()
                        .map(|game| (game.name, game.icon, false, None, Some(game.id)))
                        .collect(),
                )
            }
            Menu::More => {
                let (_, overflow) = self.shop_nav_split(window);
                (
                    self.shop_nav_left(window, "more"),
                    overflow
                        .into_iter()
                        .map(|option| {
                            (
                                option.label.to_owned(),
                                None,
                                option.id == "game-servers",
                                option.tab,
                                None,
                            )
                        })
                        .collect(),
                )
            }
            Menu::Balance => return Some(self.shop_balance_popout(cx)),
        };
        let rows: Vec<AnyElement> = entries
            .into_iter()
            .map(|(label, icon_url, beta, tab, game)| {
                let art = icon_url.and_then(|url| self.shop_img_box(&url, 32., 32.));
                let shown = label.clone();
                div()
                    .id(SharedString::from(format!("shop-menu-{label}")))
                    .h(px(34.))
                    .px(px(8.))
                    .rounded(px(2.))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    // A Game Shops row opens that game's storefront page; Game Servers has its own tab, which is not
                    // built (BLOCKED.md), so that row neither highlights nor takes a click.
                    .when(tab.is_some() || game.is_some(), |row| {
                        row.cursor_pointer()
                            .hover(|row| row.bg(color("--background-mod-subtle")))
                            .on_click(cx.listener(move |this, _, _, cx| match &game {
                                Some(app) => {
                                    this.shop.menu_open = None;
                                    this.shop.pointer_in = None;
                                    this.shop_open_game_shop(app, 0, cx);
                                }
                                None => this.shop_menu_choose(menu, &label, tab, cx),
                            }))
                    })
                    .children(art.map(|art| {
                        img(art)
                            .size(px(18.))
                            .flex_none()
                            .rounded(px(4.))
                            .object_fit(gpui::ObjectFit::Cover)
                    }))
                    .child(text(14., 18., 600., "--text-strong").child(shown))
                    .when(beta, |row| row.child(beta_badge()))
                    .into_any_element()
            })
            .collect();
        Some(
            deferred(
                div()
                    .id("shop-menu")
                    .absolute()
                    .top(px(HEADER - 2.))
                    .left(px(left))
                    .pt(px(4.))
                    .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                        this.shop_pointer(menu, *hovered, cx);
                    }))
                    .child(
                        div()
                            .parity("shop-menu")
                            .occlude()
                            .min_w(px(150.))
                            .p(px(8.))
                            .rounded(px(8.))
                            .bg(color("--background-surface-high"))
                            .border(px(1.))
                            .border_color(color("--border-subtle"))
                            .flex()
                            .flex_col()
                            .children(rows),
                    ),
            )
            .with_priority(2)
            .into_any_element(),
        )
    }

    /// Where a header entry starts inside the header: the shop mark, the 44px margin, the options before it and
    /// their gaps.
    fn shop_nav_left(&self, window: &Window, id: &str) -> f32 {
        let (visible, _) = self.shop_nav_split(window);
        let mut left = 16. + 24. + 44.;
        for option in &visible {
            if option.id == id {
                return left;
            }
            left += option.width + 32.;
        }
        // The More button follows the last visible option.
        left
    }

    fn shop_menu_choose(
        &mut self,
        menu: Menu,
        label: &str,
        tab: Option<Tab>,
        cx: &mut Context<Self>,
    ) {
        self.shop.menu_open = None;
        self.shop.pointer_in = None;
        if menu == Menu::Browse {
            let kind = match label {
                "Avatar Decorations" => Some("AVATAR_DECORATION"),
                "Nameplates" => Some("NAMEPLATE"),
                "Profile Effects" => Some("PROFILE_EFFECT"),
                "Profile Frames" => Some("PROFILE_FRAME"),
                "Bundles" => Some("BUNDLE"),
                _ => None,
            };
            self.shop.filters = Filters::default();
            if let Some(kind) = kind {
                self.shop.filters.item_types.push(kind.to_owned());
            }
            self.shop.filters.collabs = label == "Collabs";
            self.shop_default_sort();
        }
        if let Some(tab) = tab {
            self.shop_set_tab(tab, cx);
        }
        cx.notify();
    }

    /// The balance popout: what Orbs are, and the button to Quest Home.
    fn shop_balance_popout(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let amount = self.shop.balance.unwrap_or(0);
        deferred(
            div()
                .absolute()
                .top(px(HEADER - 6.))
                .right(px(12.))
                .pt(px(4.))
                .child(
                    div()
                        .parity("shop-balance-popout")
                        .occlude()
                        .w(px(280.))
                        .p(px(16.))
                        .rounded(px(8.))
                        .bg(color("--background-surface-high"))
                        .border(px(1.))
                        .border_color(color("--border-subtle"))
                        .flex()
                        .flex_col()
                        .gap(px(12.))
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(8.))
                                .child(icon("shop_orb_18", 20., "--text-strong"))
                                .child(
                                    text(16., 20., 600., "--text-strong")
                                        .child(format!("{amount} Orbs")),
                                ),
                        )
                        .child(
                            text(14., 18., 400., "--text-default").child(
                                "Complete Quests to earn Orbs, then spend them in the Shop.",
                            ),
                        )
                        .child(
                            div()
                                .id("shop-balance-quests")
                                .h(px(32.))
                                .rounded(px(8.))
                                .bg(color("--background-brand"))
                                .flex()
                                .items_center()
                                .justify_center()
                                .cursor_pointer()
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.shop.menu_open = None;
                                    this.open_quests(cx);
                                }))
                                .child(text(14., 18., 500., "--white").child("Open Quest Home")),
                        ),
                ),
        )
        .with_priority(2)
        .into_any_element()
    }

    /// The heart button opens `FullProfileModal` for the current user directly on `Tab::Wishlist` (module 685491).
    fn shop_open_wishlist(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.shop.wishlist_open = false;
        self.shop.menu_open = None;
        self.shop.pointer_in = None;
        self.shop.modal = None;
        let order = self.shop.wishlist_order.clone();
        self.shop_need_products(&order);
        self.editor_wishlist_need(&order);
        if let Some(me_id) = self.stores.current_user.as_ref().map(|user| user.id) {
            self.open_full_profile_tab(me_id, super::full_profile::Tab::Wishlist, window, cx);
        } else {
            self.shop.wishlist_open = true;
            self.shop.scroll.set_offset(point(px(0.), px(0.)));
            cx.notify();
        }
    }

    // ---- shared drawing helpers ----

    /// A catalog banner: `object-fit: cover; object-position: left` (`catalogBannerImage` of module 296329). An
    /// image wider than its box keeps its left edge and its height; any other is covered from the centre.
    pub(super) fn shop_cover_left(
        &mut self,
        url: Option<&str>,
        box_aspect: f32,
    ) -> Option<AnyElement> {
        let image = self.shop_img_box(url?, 865., 865. / box_aspect)?;
        let aspect = image::ImageReader::new(std::io::Cursor::new(image.bytes()))
            .with_guessed_format()
            .ok()
            .and_then(|reader| reader.into_dimensions().ok())
            .filter(|(_, height)| *height > 0)
            .map(|(width, height)| width as f32 / height as f32);
        match aspect {
            Some(aspect) if aspect > box_aspect => Some(
                div()
                    .absolute()
                    .top_0()
                    .left_0()
                    .h_full()
                    .w(relative(aspect / box_aspect))
                    .child(img(image).size_full().object_fit(gpui::ObjectFit::Fill))
                    .into_any_element(),
            ),
            _ => Some(
                img(image)
                    .size_full()
                    .object_fit(gpui::ObjectFit::Cover)
                    .into_any_element(),
            ),
        }
    }

    /// The width over height of a picture, read from its header once.
    pub(super) fn shop_img_aspect(
        &mut self,
        url: &str,
    ) -> Option<(f32, std::sync::Arc<gpui::Image>)> {
        let image = self.shop_img(url)?;
        if let Some(aspect) = self.shop.img_aspect.get(url) {
            return Some((*aspect, image));
        }
        let aspect = image::ImageReader::new(std::io::Cursor::new(image.bytes()))
            .with_guessed_format()
            .ok()
            .and_then(|reader| reader.into_dimensions().ok())
            .filter(|(width, height)| *width > 0 && *height > 0)
            .map(|(width, height)| width as f32 / height as f32)?;
        self.shop.img_aspect.insert(url.to_owned(), aspect);
        Some((aspect, image))
    }

    /// CSS `background-size: cover` (or `object-fit: cover`) with a `background-position` of `anchor` (0 left/top, 0.5
    /// centre, 1 right/bottom), in a box of `width` x `height`: gpui's cover fit always centres, which cut the left
    /// of art the official anchors left (the Dead by Daylight tile lost its logo).
    pub(super) fn shop_cover_at(
        &mut self,
        url: Option<&str>,
        width: f32,
        height: f32,
        anchor: (f32, f32),
    ) -> Option<AnyElement> {
        let url = url?;
        // The decode box: the picture is kept at the size it is drawn at (see `shop_img_box`).
        self.shop_img_box(url, width, height);
        let (aspect, image) = self.shop_img_aspect(url)?;
        let (left, top, drawn_width, drawn_height) =
            data::cover_rect(width, height, aspect, anchor);
        Some(
            div()
                .absolute()
                .left(px(left))
                .top(px(top))
                .w(px(drawn_width))
                .h(px(drawn_height))
                .child(img(image).size_full().object_fit(gpui::ObjectFit::Fill))
                .into_any_element(),
        )
    }

    /// A remote image filling its box, or the box alone while it loads.
    pub(super) fn shop_cover(&mut self, url: Option<&str>) -> Option<AnyElement> {
        let image = self.shop_img(url?)?;
        Some(
            img(image)
                .size_full()
                .object_fit(gpui::ObjectFit::Cover)
                .into_any_element(),
        )
    }
}

/// The width of the More button in the header row (measured in the official header).
const NAV_MORE_WIDTH: f32 = 58.09;

/// One option of the header row: where it goes, its menu if it has one and its measured width.
#[derive(Clone, Debug)]
struct NavOption {
    id: &'static str,
    label: &'static str,
    tab: Option<Tab>,
    menu: Option<Menu>,
    width: f32,
}

impl NavOption {
    fn page(id: &'static str, label: &'static str, tab: Tab, width: f32) -> Self {
        Self {
            id,
            label,
            tab: Some(tab),
            menu: None,
            width,
        }
    }

    fn menu(
        id: &'static str,
        label: &'static str,
        menu: Menu,
        width: f32,
        tab: Option<Tab>,
    ) -> Self {
        Self {
            id,
            label,
            tab,
            menu: Some(menu),
            width,
        }
    }
}

/// The BETA badge of the Game Servers option (`BadgeType.BETA`, the secondary variant).
fn beta_badge() -> impl IntoElement {
    div()
        .h(px(16.))
        .px(px(6.))
        .rounded(px(8.))
        .bg(color("--background-mod-strong"))
        .flex()
        .items_center()
        .child(text(10., 12., 700., "--text-default").child("BETA"))
}
