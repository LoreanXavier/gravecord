//! The Game Shop page: `/shop?tab=game-shops&applicationId=<id>` (module 980780 around the storefront of module 953331).
//! It reads `GET /partner-sdk/applications/{id}/application-storefront` (module 733391 `p`, parsed by module 871123
//! `sq`): the pages with their section lists, the store listings behind the sku ids, and the pricing map. The cards are
//! module 869099, the header module 980780, the page background module 953331 `es`.
//!
//! Not built (BLOCKED.md): the sku modal a card opens (module 317560), the purchase and gift flows behind the hover
//! buttons, the wishlist tooltip, the leaderboard, promotion banners and the hover animation of the cards.
use super::{HEADER, Menu, Root, Tab};
use crate::parity::Parity;
use crate::ui::discover::text;
use crate::ui::{color, icon};
use gpui::{
    AnyElement, Context, IntoElement, ObjectFit, SharedString, Window, div, img, linear_color_stop,
    linear_gradient, point, prelude::*, px, relative, rgba,
};
use serde_json::Value;
use std::collections::HashMap;

/// The page in view: the application and which of the storefront's pages is selected.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::ui) struct View {
    pub app: String,
    pub page: usize,
}

/// The 24px of padding around the page and the gap between cards (`container_wWMd_F`, `cardGrid_F4LOYz`).
const PAD: f32 = 24.;
/// The left edge of the page area (rail and sidebar) and the scroller's gutter.
const PAGE_LEFT: f32 = 375.;
const GUTTER: f32 = 8.;
/// `minmax(246px, 1fr)` of the card grid.
const MIN_CARD: f32 = 246.;
const SMALL_HEIGHT: f32 = 246.;
/// The window's title bar: the page starts under it, and the header under that.
const TITLE_BAR: f32 = 33.;
/// The card's border, the corner button's inset and its size.
const BORDER: f32 = 2.;
const CORNER: f32 = 8.;

/// A store listing as the cards need it.
#[derive(Clone, Debug, Default, PartialEq)]
pub(in crate::ui) struct Listing {
    pub id: String,
    pub name: String,
    pub card_image: Option<String>,
    pub card_background: Option<String>,
    /// The first carousel item's label icon (`primaryIconAsset` of module 871123 `Cv`).
    pub label_icon: Option<String>,
    pub price: Option<String>,
    /// Module 787844: optional Orbs coupon price (`discord_orb` entry in `storefront_pricing`).
    pub orb_price: Option<u64>,
    /// Module 783481: optional coupon discount percent (`discount_percent` / `coupon_discount_percent`).
    pub discount_percent: Option<u32>,
}

/// Surface that opened the coupon flow (module 783481 & 787844).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::ui) enum CouponSurface {
    Storefront,
    ProductDetails,
}

/// Module 783481: Social Layer Storefront coupon purchase complete modal state.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::ui) struct CouponCompleteModal {
    pub application_id: String,
    pub application_name: String,
    pub sku_id: String,
    pub discount_percent: Option<u32>,
    pub surface: CouponSurface,
}

/// Module 787844: in-flight `POST /virtual-currency/skus/{skuId}/redeem` state.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::ui) struct PendingCouponRedeem {
    pub application_id: String,
    pub application_name: String,
    pub sku_id: String,
    pub discount_percent: Option<u32>,
    pub orb_price: u64,
    pub surface: CouponSurface,
}

/// Module 783481: title for coupon purchase complete modal (`NWcb7k` with discount or `Xtf9wP` fallback).
pub(in crate::ui) fn coupon_complete_title(discount_percent: Option<u32>) -> String {
    if let Some(pct) = discount_percent {
        dc_intl::format(
            "NWcb7k",
            &[("discountPercent", dc_intl::Arg::Num(pct as f64))],
        )
    } else {
        dc_intl::t("Xtf9wP").to_owned()
    }
}

/// Module 783481: subtitle `"Explore the {applicationName} Shop and treat yourself."` (`tm2lfr`).
pub(in crate::ui) fn coupon_complete_subtitle(application_name: &str) -> String {
    dc_intl::format("tm2lfr", &[("applicationName", application_name.into())])
}

/// Module 787844: dual-mode Orbs coupon CTA label + whether balance suffices to spend Orbs.
pub(in crate::ui) fn orb_coupon_cta_label(
    balance: u64,
    orb_price: u64,
    surface: CouponSurface,
) -> (String, bool) {
    if balance < orb_price {
        (dc_intl::t("SymzJC").to_owned(), false)
    } else {
        let label = match surface {
            CouponSurface::Storefront => dc_intl::t("BxjHiu").to_owned(),
            CouponSurface::ProductDetails => dc_intl::format(
                "lTwxHh",
                &[("orbAmount", dc_intl::Arg::Num(orb_price as f64))],
            ),
        };
        (label, true)
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub(in crate::ui) struct Section {
    pub title: String,
    pub skus: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub(in crate::ui) struct Page {
    pub title: String,
    pub skus: Vec<String>,
    pub sections: Vec<Section>,
    /// `leaderboard.background_image_asset_id`: the page background.
    pub background: Option<String>,
}

/// Module 849134 (`SocialLayerStorefrontPopover`): graphic source variants (`asset`, `video`, `sku`).
#[derive(Clone, Debug, PartialEq, Eq)]
#[allow(dead_code)]
pub(crate) enum GameShopGraphicSource {
    Asset {
        url: String,
    },
    Video {
        url: String,
    },
    Sku {
        image_url: String,
        background_image_url: Option<String>,
        alt_text: &'static str,
    },
}

/// Module 849134 (`SocialLayerStorefrontPopover`): Guild Sidebar Game Shop promo popover state.
#[derive(Clone, Debug, PartialEq, Eq)]
#[allow(dead_code)]
pub(crate) struct GameShopPopover {
    pub guild_id: dc_state::Id,
    pub app_id: String,
    pub title: String,
    pub body: String,
    pub action_label: String,
    pub graphic: GameShopGraphicSource,
}

/// Module 849134 / 616740: build an application-drop Game Shop popover (`7PvvS9`, `9J4h1a`, `ulQB+t`, `RzWDqY`).
/// Returns `None` if `asset_id` is empty (official `616740` guard).
#[allow(dead_code)]
pub(crate) fn application_announcement_popover(
    guild_id: dc_state::Id,
    app_id: &str,
    app_name: &str,
    asset_id: &str,
    bg_asset_id: Option<&str>,
) -> Option<GameShopPopover> {
    if asset_id.trim().is_empty() {
        return None;
    }
    Some(GameShopPopover {
        guild_id,
        app_id: app_id.to_owned(),
        title: dc_intl::t("7PvvS9").to_owned(),
        body: dc_intl::format(
            "9J4h1a",
            &[("applicationName", dc_intl::Arg::Str(app_name.into()))],
        ),
        action_label: dc_intl::t("RzWDqY").to_owned(),
        graphic: GameShopGraphicSource::Sku {
            image_url: asset_url(app_id, asset_id, 256),
            background_image_url: bg_asset_id
                .filter(|s| !s.trim().is_empty())
                .map(|bg| asset_url(app_id, bg, 256)),
            alt_text: dc_intl::t("ulQB+t"),
        },
    })
}

/// Module 849134 / 616740: build a Discord storefront announcement popover (`video` or `asset`).
/// Returns `None` if both video and asset URLs are empty.
#[allow(dead_code)]
pub(crate) fn discord_announcement_popover(
    guild_id: dc_state::Id,
    app_id: &str,
    title: &str,
    body: &str,
    cta: Option<&str>,
    video_url: Option<&str>,
    asset_url_opt: Option<&str>,
) -> Option<GameShopPopover> {
    let graphic = if let Some(v) = video_url.filter(|s| !s.trim().is_empty()) {
        GameShopGraphicSource::Video { url: v.to_owned() }
    } else {
        let a = asset_url_opt.filter(|s| !s.trim().is_empty())?;
        GameShopGraphicSource::Asset { url: a.to_owned() }
    };
    Some(GameShopPopover {
        guild_id,
        app_id: app_id.to_owned(),
        title: title.to_owned(),
        body: body.to_owned(),
        action_label: cta
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| dc_intl::t("RzWDqY"))
            .to_owned(),
        graphic,
    })
}

/// Module 962299 (`SocialLayerStorefrontPromotionBanner`): active sticky promo banner data for a Game Shop.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(in crate::ui) struct StorefrontPromotion {
    pub text: String,
    /// Unix timestamp (seconds) when the promotion expires, if bounded.
    pub ends_at_secs: Option<u64>,
    /// `"nitro"` triggers `nitro-pink` gradient + `nitro_wheel` icon even in the generic branch.
    pub flavor: String,
    /// True when the viewer needs Nitro / an upgrade (`906383.ad` -> Branch 1 `pj0XBN` "Get Nitro").
    pub needs_nitro: bool,
    /// Optional redeemable Orbs coupon SKU attached to the promotion (Branch 2 -> `787844`).
    pub coupon_sku_id: Option<String>,
    pub coupon_orb_price: Option<u64>,
    pub coupon_discount_percent: Option<u32>,
    pub coupon_earned_or_consumed: bool,
}

/// Module 962299: resolved banner branch (`NitroUpsell` -> `OrbsCoupon` -> `Info`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::ui) enum PromoBranch {
    NitroUpsell,
    OrbsCoupon,
    Info,
}

impl StorefrontPromotion {
    pub(in crate::ui) fn is_expired(&self, now_secs: u64) -> bool {
        self.ends_at_secs.is_some_and(|end| now_secs >= end)
    }

    pub(in crate::ui) fn branch(&self) -> PromoBranch {
        if self.needs_nitro {
            PromoBranch::NitroUpsell
        } else if self.coupon_sku_id.is_some()
            && self.coupon_orb_price.is_some()
            && !self.coupon_earned_or_consumed
        {
            PromoBranch::OrbsCoupon
        } else {
            PromoBranch::Info
        }
    }

    pub(in crate::ui) fn is_nitro_pink(&self) -> bool {
        matches!(
            self.branch(),
            PromoBranch::NitroUpsell | PromoBranch::OrbsCoupon
        ) || self.flavor.eq_ignore_ascii_case("nitro")
    }
}

/// Module 962299 / 647474 (`BXpdIg`): formats countdown badge (`"{days} days left"` or `HH:MM:SS`).
pub(in crate::ui) fn promo_countdown_label(remaining_secs: u64) -> String {
    if remaining_secs >= 86_400 {
        let days = (remaining_secs / 86_400).max(1);
        let formatted = dc_intl::format("BXpdIg", &[("days", (days as f64).into())]);
        if !formatted.trim().is_empty() {
            return formatted;
        }
        return format!("{days} days left");
    }
    let h = remaining_secs / 3600;
    let m = (remaining_secs % 3600) / 60;
    let s = remaining_secs % 60;
    format!("{h:02}:{m:02}:{s:02}")
}

#[derive(Clone, Debug, Default, PartialEq)]
pub(in crate::ui) struct Storefront {
    pub app: String,
    pub pages: Vec<Page>,
    pub listings: HashMap<String, Listing>,
    pub promotion: Option<StorefrontPromotion>,
}

/// `GET /partner-sdk/applications/{id}/application-storefront` (`SOCIAL_LAYER_STOREFRONT_BY_APPLICATION_ID`).
pub(in crate::ui) fn storefront_route(app: &str) -> String {
    format!("/partner-sdk/applications/{app}/application-storefront")
}

pub(in crate::ui) fn storefront_key(app: &str) -> String {
    format!("shop:gameshop:{app}")
}

/// Module 371794 `YE`: `https://cdn.discordapp.com/app-assets/{app}/store/{asset}.webp?size={n}`.
pub(in crate::ui) fn asset_url(app: &str, asset: &str, size: u32) -> String {
    format!("https://cdn.discordapp.com/app-assets/{app}/store/{asset}.webp?size={size}")
}

/// The price as `Intl.NumberFormat("en-US", {style: "currency"})` shows it: a symbol for the few currencies en-US has
/// one for, else the code and a no-break space.
pub(in crate::ui) fn format_price(amount: i64, currency: &str, exponent: u32) -> String {
    let divisor = 10i64.pow(exponent);
    let (whole, fraction) = (amount / divisor, amount % divisor);
    let grouped = group_thousands(whole);
    let number = if exponent == 0 {
        grouped
    } else {
        format!("{grouped}.{fraction:0width$}", width = exponent as usize)
    };
    match currency.to_lowercase().as_str() {
        "usd" => format!("${number}"),
        "eur" => format!("\u{20ac}{number}"),
        "gbp" => format!("\u{a3}{number}"),
        other => format!("{}\u{a0}{number}", other.to_uppercase()),
    }
}

/// `1234567` as `1,234,567`.
fn group_thousands(number: i64) -> String {
    let digits = number.abs().to_string();
    let mut out = String::new();
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            out.push(',');
        }
        out.push(digit);
    }
    if number < 0 {
        out.insert(0, '-');
    }
    out
}

/// The price a card shows: the pricing map's first fiat entry for the viewer (`getSkuPricing`), else the listing's own
/// price.
fn listing_price(listing: &Value, pricing: &Value) -> Option<String> {
    let sku = &listing["sku"];
    let id = sku["id"].as_str()?;
    let mapped = pricing["sku_price_map"][id]["pricing_result_id"]
        .as_str()
        .and_then(|result| pricing["pricing_result_id_map"][result].as_object())
        .and_then(|by_gateway| by_gateway.get("0").or_else(|| by_gateway.values().next()))
        .and_then(|entry| entry["user_price"].as_array())
        .and_then(|prices| {
            prices
                .iter()
                .find(|price| price["currency"].as_str() != Some("discord_orb"))
        })
        .and_then(|price| {
            Some((
                price["amount"].as_i64()?,
                price["currency"].as_str()?.to_owned(),
                price["exponent"].as_u64()? as u32,
            ))
        });
    let (amount, currency, exponent) = mapped.or_else(|| {
        let price = &sku["price"];
        Some((
            price["amount"].as_i64()?,
            price["currency"].as_str()?.to_owned(),
            price["currency_exponent"].as_u64()? as u32,
        ))
    })?;
    Some(format_price(amount, &currency, exponent))
}

/// Module 787844: extract optional `discord_orb` price from `storefront_pricing`.
fn listing_orb_price(listing: &Value, pricing: &Value) -> Option<u64> {
    let sku = &listing["sku"];
    let id = sku["id"].as_str()?;
    pricing["sku_price_map"][id]["pricing_result_id"]
        .as_str()
        .and_then(|result| pricing["pricing_result_id_map"][result].as_object())
        .and_then(|by_gateway| by_gateway.get("0").or_else(|| by_gateway.values().next()))
        .and_then(|entry| entry["user_price"].as_array())
        .and_then(|prices| {
            prices
                .iter()
                .find(|price| price["currency"].as_str() == Some("discord_orb"))
        })
        .and_then(|price| price["amount"].as_u64())
}

fn strings(value: &Value) -> Vec<String> {
    value
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|id| id.as_str().map(str::to_owned))
        .collect()
}

/// Module 871123 `sq` and the listings behind it.
pub(super) fn parse_storefront(body: &Value) -> Option<Storefront> {
    let app = body["application_id"].as_str()?.to_owned();
    let pages = body["pages"]
        .as_array()?
        .iter()
        .map(|page| Page {
            title: page["title"].as_str().unwrap_or_default().to_owned(),
            skus: strings(&page["sku_ids"]),
            sections: page["sections"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|section| Section {
                    title: section["title"].as_str().unwrap_or_default().to_owned(),
                    skus: strings(&section["sku_ids"]),
                })
                .collect(),
            background: page["leaderboard"]["background_image_asset_id"]
                .as_str()
                .map(str::to_owned),
        })
        .collect();
    let listings = body["store_listings"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|listing| {
            let sku = &listing["sku"];
            let id = sku["id"].as_str()?.to_owned();
            let social = &sku["tenant_metadata"]["social_layer"];
            Some((
                id.clone(),
                Listing {
                    id,
                    name: sku["name"].as_str().unwrap_or_default().to_owned(),
                    // `cardImageAssetId ?? thumbnailAssetId` (module 871123 `fq`).
                    card_image: social["card_image_asset_id"]
                        .as_str()
                        .or_else(|| sku["thumbnail_asset_id"].as_str())
                        .map(str::to_owned),
                    card_background: social["card_background_image_asset_id"]
                        .as_str()
                        .map(str::to_owned),
                    label_icon: social["carousel_items"][0]["label_icon_asset_id"]
                        .as_str()
                        .map(str::to_owned),
                    price: listing_price(listing, &body["storefront_pricing"]),
                    orb_price: listing_orb_price(listing, &body["storefront_pricing"]),
                    discount_percent: social["discount_percent"]
                        .as_u64()
                        .or_else(|| sku["discount_percent"].as_u64())
                        .map(|n| n as u32),
                },
            ))
        })
        .collect();
    let promotion = body
        .get("promotion")
        .filter(|v| v.is_object())
        .or_else(|| body["promotions"].as_array().and_then(|a| a.first()))
        .and_then(|p| {
            let text = p["header_text"]
                .as_str()
                .or_else(|| p["text"].as_str())?
                .trim()
                .to_owned();
            if text.is_empty() {
                return None;
            }
            let coupon = &p["coupon"];
            Some(StorefrontPromotion {
                text,
                ends_at_secs: p["ends_at_secs"].as_u64(),
                flavor: p["flavor"].as_str().unwrap_or_default().to_owned(),
                needs_nitro: p["needs_nitro"].as_bool().unwrap_or(false),
                coupon_sku_id: coupon["sku_id"].as_str().map(str::to_owned),
                coupon_orb_price: coupon["orb_price"].as_u64(),
                coupon_discount_percent: coupon["discount_percent"].as_u64().map(|n| n as u32),
                coupon_earned_or_consumed: coupon["earned"].as_bool().unwrap_or(false)
                    || coupon["consumed"].as_bool().unwrap_or(false),
            })
        });
    Some(Storefront {
        app,
        pages,
        listings,
        promotion,
    })
}

/// Module 225647: an all-capitals title shows with each word capitalised ("NEW" is "New").
pub(super) fn display_title(title: &str) -> String {
    if title != title.to_uppercase() {
        return title.to_owned();
    }
    let mut out = String::new();
    let mut start = true;
    for letter in title.chars() {
        if letter.is_alphabetic() {
            if start {
                out.extend(letter.to_uppercase());
            } else {
                out.extend(letter.to_lowercase());
            }
            start = false;
        } else {
            start = !(letter == '\'' || letter == '\u{2019}');
            out.push(letter);
        }
    }
    out
}

/// Columns of `repeat(auto-fill, minmax(246px, 1fr))` with a 24px gap.
pub(super) fn grid_columns(width: f32) -> usize {
    (((width + PAD) / (MIN_CARD + PAD)).floor() as usize).max(1)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Variant {
    Small,
    FullWidth,
}

impl Root {
    /// An entry of the Game Shops menu (or a page title of the header): the page opens and its storefront is read.
    pub(in crate::ui) fn shop_open_game_shop(
        &mut self,
        app: &str,
        page: usize,
        cx: &mut Context<Self>,
    ) {
        self.shop.wishlist_open = false;
        self.shop.menu_open = None;
        self.shop.pointer_in = None;
        self.shop.modal = None;
        self.shop.tab = Tab::Featured;
        self.shop.game_shop = Some(View {
            app: app.to_owned(),
            page,
        });
        self.shop.scroll.set_offset(point(px(0.), px(0.)));
        self.shop_request(&storefront_key(app), storefront_route(app));
        cx.notify();
    }

    /// The storefront answered.
    pub(super) fn shop_gameshop_fetched(&mut self, key: &str, body: &Value) {
        let Some(app) = key.strip_prefix("shop:gameshop:") else {
            return;
        };
        if let Some(storefront) = parse_storefront(body) {
            self.shop.storefronts.insert(app.to_owned(), storefront);
        }
    }

    /// The loaded storefront of the page in view.
    fn game_shop_data(&self) -> Option<&Storefront> {
        let view = self.shop.game_shop.as_ref()?;
        self.shop.storefronts.get(&view.app)
    }

    /// The width of the page's content: the area right of the sidebars, minus the gutter and the 24px either side.
    fn game_shop_width(window: &Window) -> f32 {
        f32::from(window.viewport_size().width) - PAGE_LEFT - GUTTER - 2. * PAD
    }

    /// The header of the page (`et` of module 980780): the shop button, a divider, the game menu, the page titles and
    /// the heart, Orbs balance and Learn More buttons.
    pub(super) fn shop_game_header(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let view = self.shop.game_shop.clone();
        let current = view.as_ref().map(|view| view.app.clone());
        let game = current
            .as_ref()
            .and_then(|app| self.shop.game_shops.iter().find(|game| &game.id == app))
            .cloned();
        let titles: Vec<String> = self
            .game_shop_data()
            .map(|data| data.pages.iter().map(|page| page.title.clone()).collect())
            .unwrap_or_default();
        let selected = view.as_ref().map(|view| view.page).unwrap_or(0);
        let logo = game
            .as_ref()
            .and_then(|game| game.icon.clone())
            .and_then(|url| self.shop_img(&url));
        let open = self.shop.menu_open == Some(Menu::GameShops);
        let menu = if open { self.shop_game_menu(cx) } else { None };
        let heart = self.shop_wishlist_button(cx);
        let balance = self.shop_balance_pill(cx);
        let info = div()
            .id("shop-game-info")
            .parity("shop-game-info")
            .size(px(32.))
            .flex_none()
            .rounded(px(8.))
            .bg(color("--background-mod-subtle"))
            .flex()
            .items_center()
            .justify_center()
            .cursor_pointer()
            .hover(|button| button.bg(color("--background-mod-strong")))
            .on_click(cx.listener(|_, _, _, cx| {
                // `getArticleURL(SOCIAL_LAYER_STOREFRONT)`: the support article about game shops.
                cx.open_url("https://support.discord.com/hc/en-us/articles/40102783004311");
            }))
            .child(icon("shop_info", 16., "--text-default"));
        div()
            .parity("shop-header")
            .relative()
            .flex_none()
            .w_full()
            .h(px(HEADER))
            .pl(px(16.))
            .pr(px(12.))
            .pb(px(1.))
            .flex()
            .items_center()
            .child(
                div()
                    .id("shop-mark")
                    .size(px(24.))
                    .flex_none()
                    .cursor_pointer()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.shop.game_shop = None;
                        this.shop_set_tab(Tab::Featured, cx)
                    }))
                    .child(icon("shop_header", 24., "--text-strong")),
            )
            .child(
                div()
                    .flex_none()
                    .ml(px(24.))
                    .w(px(1.))
                    .h(px(32.))
                    .bg(color("--border-strong")),
            )
            .child(
                div()
                    .id("shop-game-trigger")
                    .parity("shop-game-trigger")
                    .flex_none()
                    .ml(px(24.))
                    .h(px(48.))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .cursor_pointer()
                    .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
                        this.shop_pointer(Menu::GameShops, *hovered, cx);
                    }))
                    .child(
                        div()
                            .flex_none()
                            .size(px(24.))
                            .rounded(px(4.))
                            .overflow_hidden()
                            .bg(color("--background-mod-subtle"))
                            .children(
                                logo.map(|logo| img(logo).size_full().object_fit(ObjectFit::Cover)),
                            ),
                    )
                    .child(
                        text(16., 20., 500., "--text-default")
                            .child(game.map(|game| game.name).unwrap_or_default()),
                    )
                    .child(icon(
                        if open { "shop_caret_up" } else { "shop_caret" },
                        16.,
                        "--text-default",
                    )),
            )
            .when(titles.len() > 1, |header| {
                header.child(
                    div()
                        .flex_none()
                        .ml(px(24.))
                        .h(px(48.))
                        .flex()
                        .items_center()
                        .gap(px(24.))
                        .children(titles.iter().enumerate().map(|(index, title)| {
                            let app = current.clone().unwrap_or_default();
                            div()
                                .id(SharedString::from(format!("shop-game-page-{index}")))
                                .h_full()
                                .flex()
                                .items_center()
                                .pt(px(1.))
                                .cursor_pointer()
                                .border_b_1()
                                .border_color(if index == selected {
                                    color("--icon-strong")
                                } else {
                                    gpui::rgba(0x00000000)
                                })
                                .hover(|title| title.border_color(color("--background-brand")))
                                .child(
                                    text(14., 18., 500., "--text-default")
                                        .child(display_title(title)),
                                )
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.shop_open_game_shop(&app, index, cx);
                                }))
                        })),
                )
            })
            .child(div().flex_1())
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(heart)
                    .child(balance)
                    .child(info),
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

    /// The game menu (`X` of module 980780): every shop that lists itself, the current one in the brand colour with a
    /// check, opening under the trigger.
    fn shop_game_menu(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let games = self.shop.game_shops.clone();
        let current = self.shop.game_shop.as_ref().map(|view| view.app.clone());
        let rows: Vec<AnyElement> = games
            .into_iter()
            .map(|game| {
                let art = game.icon.clone().and_then(|url| self.shop_img(&url));
                let is_current = current.as_deref() == Some(game.id.as_str());
                let app = game.id.clone();
                div()
                    .id(SharedString::from(format!("shop-game-menu-{}", game.id)))
                    .h(px(34.))
                    .px(px(8.))
                    .rounded(px(2.))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .cursor_pointer()
                    .hover(|row| row.bg(color("--background-mod-subtle")))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.shop_open_game_shop(&app, 0, cx);
                    }))
                    .children(art.map(|art| {
                        img(art)
                            .size(px(18.))
                            .flex_none()
                            .rounded(px(4.))
                            .object_fit(ObjectFit::Cover)
                    }))
                    .child(
                        text(
                            14.,
                            18.,
                            600.,
                            if is_current {
                                "--text-brand"
                            } else {
                                "--text-strong"
                            },
                        )
                        .flex_1()
                        .child(game.name),
                    )
                    .when(is_current, |row| {
                        row.child(icon("discover_check", 16., "--text-brand"))
                    })
                    .into_any_element()
            })
            .collect();
        Some(
            gpui::deferred(
                div()
                    .id("shop-menu")
                    .absolute()
                    .top(px(HEADER - 2.))
                    .left(px(89.))
                    .pt(px(4.))
                    .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
                        this.shop_pointer(Menu::GameShops, *hovered, cx);
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

    /// The body of the page: the background, then the featured card and the sections.
    pub(super) fn shop_game_body(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(view) = self.shop.game_shop.clone() else {
            return div().into_any_element();
        };
        let Some(store) = self.game_shop_data().cloned() else {
            // The storefront has not arrived (or failed): the page shows an empty body, as the loading spinner does.
            return div().size_full().into_any_element();
        };
        let page = store
            .pages
            .get(view.page)
            .or_else(|| store.pages.first())
            .cloned()
            .unwrap_or_default();
        let width = Self::game_shop_width(window);
        let scrolled = -f32::from(self.shop.scroll.offset().y);
        let viewport = f32::from(window.viewport_size().height) - TITLE_BAR - HEADER;
        let mut body = div()
            .relative()
            .w_full()
            .pl(px(PAD))
            .pr(px(PAD))
            .pt(px(PAD))
            .pb(px(PAD))
            .flex()
            .flex_col();
        // Module 962299 (`SocialLayerStorefrontPromotionBanner`): sticky promo banner above content.
        if let Some(promo) = store.promotion.as_ref() {
            let now_secs = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_secs());
            if !promo.is_expired(now_secs) {
                body = body.child(self.shop_game_promo_banner(&store.app, promo, now_secs, cx));
            }
        }
        let mut top = PAD;
        // `single-sku-wide-header`: a page with one sku opens with that card at the full width.
        let single = page.skus.len() == 1;
        let hero = single && page.background.is_some();
        if single {
            let height = width * 516. / 1056.;
            let card = self.game_shop_card(
                &store,
                &page.skus[0],
                Variant::FullWidth,
                width,
                height,
                top,
                scrolled,
                viewport,
                cx,
            );
            body = body.child(div().w_full().child(card));
            top += height;
        } else if !page.skus.is_empty() {
            let (row, used) =
                self.game_shop_grid(&store, &page.skus, width, top + PAD, scrolled, viewport, cx);
            body = body.child(row);
            top += used + PAD;
        }
        for section in &page.sections {
            if section.skus.is_empty() {
                continue;
            }
            let titled = !section.title.is_empty();
            let margin = if titled { 12. } else { PAD };
            let mut block = div().w_full().flex().flex_col();
            if titled {
                top += 48.;
                block = block.child(
                    text(20., 24., 600., "--text-strong")
                        .mt(px(48.))
                        .truncate()
                        .child(section.title.clone()),
                );
                top += 24.;
            }
            top += margin;
            let (grid, used) =
                self.game_shop_grid(&store, &section.skus, width, top, scrolled, viewport, cx);
            top += used;
            block = block.child(div().w_full().mt(px(margin)).child(grid));
            body = body.child(block);
        }
        let background = page
            .background
            .clone()
            .filter(|_| hero)
            .and_then(|asset| self.shop_img(&asset_url(&store.app, &asset, 2048)));
        div()
            .parity("shop-game-body")
            .relative()
            .w_full()
            .bg(color("--background-base-lower"))
            .children(background.map(|image| {
                // `backgroundImageHero_TgTH47`: 350 tall, fading out with its mask. The mask is drawn as the page
                // colour over the picture in three steps of the same alpha curve.
                let base = |alpha: u8| rgba(0x12121400 | alpha as u32);
                div()
                    .absolute()
                    .top_0()
                    .left_0()
                    .right_0()
                    .h(px(350.))
                    .overflow_hidden()
                    .child(img(image).size_full().object_fit(ObjectFit::Cover))
                    .child(
                        div()
                            .absolute()
                            .top_0()
                            .left_0()
                            .right_0()
                            .h(px(210.))
                            .bg(base(0x66)),
                    )
                    .child(
                        div()
                            .absolute()
                            .top(px(210.))
                            .left_0()
                            .right_0()
                            .h(px(70.))
                            .bg(linear_gradient(
                                180.,
                                linear_color_stop(base(0x66), 0.),
                                linear_color_stop(base(0x99), 1.),
                            )),
                    )
                    .child(
                        div()
                            .absolute()
                            .top(px(280.))
                            .left_0()
                            .right_0()
                            .h(px(70.))
                            .bg(linear_gradient(
                                180.,
                                linear_color_stop(base(0x99), 0.),
                                linear_color_stop(base(0xff), 1.),
                            )),
                    )
            }))
            .child(body)
            .into_any_element()
    }

    /// A grid of small cards; returns it with its height.
    #[allow(clippy::too_many_arguments)]
    fn game_shop_grid(
        &mut self,
        store: &Storefront,
        skus: &[String],
        width: f32,
        top: f32,
        scrolled: f32,
        viewport: f32,
        cx: &mut Context<Self>,
    ) -> (AnyElement, f32) {
        let columns = grid_columns(width);
        let card_width = (width - PAD * (columns as f32 - 1.)) / columns as f32;
        let rows = skus.len().div_ceil(columns);
        let mut cards = Vec::new();
        for (index, sku) in skus.iter().enumerate() {
            let row = index / columns;
            let card_top = top + row as f32 * (SMALL_HEIGHT + PAD);
            cards.push(
                div()
                    .w(px(card_width))
                    .h(px(SMALL_HEIGHT))
                    .flex_none()
                    .child(self.game_shop_card(
                        store,
                        sku,
                        Variant::Small,
                        card_width,
                        SMALL_HEIGHT,
                        card_top,
                        scrolled,
                        viewport,
                        cx,
                    ))
                    .into_any_element(),
            );
        }
        let height = rows as f32 * SMALL_HEIGHT + rows.saturating_sub(1) as f32 * PAD;
        (
            div()
                .parity("shop-game-grid")
                .w_full()
                .flex()
                .flex_wrap()
                .gap(px(PAD))
                .children(cards)
                .into_any_element(),
            height,
        )
    }

    /// `card_RGwqSL`: the picture over its blurred background, a gradient, the item label, the name with its icon and
    /// the price, and the wishlist heart in the corner. A card still outside the view shows the placeholder.
    #[allow(clippy::too_many_arguments)]
    fn game_shop_card(
        &mut self,
        store: &Storefront,
        sku: &str,
        variant: Variant,
        width: f32,
        height: f32,
        top: f32,
        scrolled: f32,
        viewport: f32,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(listing) = store.listings.get(sku).cloned() else {
            return div().into_any_element();
        };
        let wide = variant == Variant::FullWidth;
        // The card loads its pictures once it comes into view (`IntersectionObserver`, threshold 0).
        let seen = top < scrolled + viewport && top + height > scrolled;
        let (fg_size, bg_size) = if wide { (2048, 2048) } else { (512, 1024) };
        let foreground = listing
            .card_image
            .as_ref()
            .filter(|_| seen)
            .and_then(|asset| self.shop_img(&asset_url(&store.app, asset, fg_size)));
        let background = listing
            .card_background
            .as_ref()
            .filter(|_| seen)
            .and_then(|asset| self.shop_img(&asset_url(&store.app, asset, bg_size)));
        let shown = seen
            && (listing.card_image.is_none() || foreground.is_some())
            && (listing.card_background.is_none() || background.is_some());
        let label_icon = listing
            .label_icon
            .as_ref()
            .filter(|_| shown)
            .and_then(|asset| self.shop_img(&asset_url(&store.app, asset, 512)));
        let wished = self.shop.wishlist.contains(sku);
        let id = sku.to_owned();
        let heart_id = sku.to_owned();
        let share_app_id = store.app.clone();
        let share_app_name = store
            .pages
            .first()
            .map(|p| p.title.clone())
            .filter(|t| !t.is_empty())
            .unwrap_or_else(|| store.app.clone());
        let share_sku_id = sku.to_owned();
        let share_sku_name = listing.name.clone();
        let share_img_url = listing
            .card_image
            .as_ref()
            .map(|asset| asset_url(&store.app, asset, 512));
        let group = SharedString::from(format!("shop-game-card-{sku}"));
        let inner_h = height - 2. * BORDER;
        let inner_w = width - 2. * BORDER;
        let card = div()
            .id(SharedString::from(format!("shop-game-card-{sku}")))
            .group(group.clone())
            .parity_with_border(
                if wide {
                    "shop-game-card-wide"
                } else {
                    "shop-game-card"
                },
                2.,
            )
            .relative()
            .w_full()
            .h(px(height))
            .overflow_hidden()
            .rounded(px(8.))
            .border(px(BORDER))
            .border_color(color("--border-subtle"))
            .bg(if shown {
                color("--background-surface-high")
            } else {
                color("--background-base-low")
            });
        let mut card = card;
        if shown {
            // The blurred background is drawn 10% larger than the card and the picture covers the card.
            card = card
                .children(background.map(|image| {
                    div()
                        .absolute()
                        .top(px(-0.05 * inner_h))
                        .left(px(-0.05 * inner_w))
                        .w(px(inner_w * 1.1))
                        .h(px(inner_h * 1.1))
                        .child(img(image).size_full().object_fit(ObjectFit::Cover))
                }))
                .children(foreground.map(|image| {
                    div()
                        .absolute()
                        .top_0()
                        .left_0()
                        .size_full()
                        .child(img(image).size_full().object_fit(ObjectFit::Cover))
                }));
            // `bottomGradient_RGwqSL`: an element as tall as the card that starts a quarter down.
            let black = |alpha: f32| gpui::hsla(0., 0., 0., alpha);
            let ramp = |from: f32, to: f32, low: f32, high: f32| {
                div()
                    .absolute()
                    .left_0()
                    .right_0()
                    .top(px(from * inner_h))
                    .h(px((to - from) * inner_h))
                    .bg(linear_gradient(
                        180.,
                        linear_color_stop(black(low), 0.),
                        linear_color_stop(black(high), 1.),
                    ))
            };
            card = if wide {
                card.child(ramp(0.35, 0.85, 0., 0.6))
                    .child(ramp(0.85, 1., 0.6, 0.75))
            } else {
                card.child(ramp(0.45, 0.95, 0., 0.9)).child(
                    div()
                        .absolute()
                        .left_0()
                        .right_0()
                        .top(px(0.95 * inner_h))
                        .bottom_0()
                        .bg(black(0.9)),
                )
            };
        }
        let pill = |label: &'static str| {
            let item = text(14., 18., 500., "--white").child(label);
            if wide {
                item.px(px(0.)).rounded(px(4.)).bg(rgba(0x00000047))
            } else {
                item
            }
        };
        let details = if shown {
            div()
                .absolute()
                .left_0()
                .right_0()
                .bottom_0()
                .p(px(16.))
                .flex()
                .flex_col()
                .gap(px(4.))
                .child(
                    div()
                        .relative()
                        .h(px(20.))
                        .flex()
                        .items_center()
                        .gap(px(4.))
                        .child(
                            div()
                                .absolute()
                                .left_0()
                                .bottom(px(20. + 4.))
                                .child(pill("In-game item")),
                        )
                        .children(label_icon.map(|image| {
                            img(image)
                                .flex_none()
                                .size(px(20.))
                                .object_fit(ObjectFit::Contain)
                        }))
                        .child(
                            text(14., 20., 500., "--white")
                                .min_w(px(0.))
                                .truncate()
                                .when(wide, |name| name.rounded(px(4.)).bg(rgba(0x00000047)))
                                .child(listing.name.clone()),
                        ),
                )
                .child(
                    div()
                        .h(px(32.))
                        .flex()
                        .items_center()
                        .justify_between()
                        .gap(px(8.))
                        .child(
                            text(14., 20., 600., "--white")
                                .truncate()
                                .child(listing.price.clone().unwrap_or_default()),
                        )
                        .children(listing.orb_price.map(|orb_price| {
                            self.game_shop_orb_coupon_cta(
                                sku.to_owned(),
                                share_app_id.clone(),
                                share_app_name.clone(),
                                orb_price,
                                listing.discount_percent,
                                CouponSurface::Storefront,
                                cx,
                            )
                        })),
                )
        } else {
            // `placeholderText_RGwqSL`: two 24px bars at a width that varies from card to card.
            let seed = sku.bytes().fold(0u32, |sum, byte| {
                sum.wrapping_mul(31).wrapping_add(byte as u32)
            });
            let bar = |salt: u32| {
                div()
                    .h(px(24.))
                    .w(relative(
                        0.2 + ((seed.wrapping_add(salt) % 60) as f32) / 100.,
                    ))
                    .rounded(px(4.))
                    .bg(color("--icon-muted"))
                    .opacity(0.1)
            };
            div()
                .absolute()
                .left_0()
                .right_0()
                .bottom_0()
                .p(px(16.))
                .flex()
                .flex_col()
                .gap(px(4.))
                .child(bar(1))
                .child(bar(7))
        };
        card.child(details)
            .child(
                // Module 763375 (`869099` share button): opens single-SKU ForwardModal (`345938.d`).
                div()
                    .id(SharedString::from(format!("shop-game-share-{sku}")))
                    .absolute()
                    .top(px(CORNER))
                    .right(px(CORNER + 48.))
                    .size(px(40.))
                    .rounded(px(8.))
                    .border_1()
                    .border_color(color("--border-muted"))
                    .bg(rgba(0x0000008f))
                    .flex()
                    .items_center()
                    .justify_center()
                    .cursor_pointer()
                    .invisible()
                    .group_hover(group.clone(), |btn| btn.visible())
                    .on_click(cx.listener(move |this, _, window, cx| {
                        cx.stop_propagation();
                        this.open_share_storefront(
                            super::super::forward::ShareStorefront {
                                application_id: share_app_id.clone(),
                                application_name: share_app_name.clone(),
                                guild_id: this.guild,
                                items: vec![super::super::forward::ShareStorefrontItem {
                                    sku_id: share_sku_id.clone(),
                                    slug: None,
                                    name: share_sku_name.clone(),
                                    image_url: share_img_url.clone(),
                                }],
                            },
                            window,
                            cx,
                        );
                    }))
                    .child(icon("menu_forward", 20., "--primary-100")),
            )
            .child(
                div()
                    .id(SharedString::from(format!("shop-game-heart-{sku}")))
                    .absolute()
                    .top(px(CORNER))
                    .right(px(CORNER))
                    .size(px(40.))
                    .rounded(px(8.))
                    .border_1()
                    .border_color(color("--border-muted"))
                    .bg(rgba(0x0000008f))
                    .flex()
                    .items_center()
                    .justify_center()
                    .cursor_pointer()
                    // The corner buttons show while the card is hovered (`cardButtonContainerHovered_RGwqSL`).
                    .invisible()
                    .group_hover(group.clone(), |heart| heart.visible())
                    .on_click(cx.listener(move |this, _, _, cx| {
                        cx.stop_propagation();
                        this.shop_toggle_wishlist(&heart_id, cx);
                    }))
                    .child(icon(
                        "shop_heart_card",
                        20.,
                        if wished {
                            "--text-feedback-critical"
                        } else {
                            "--primary-100"
                        },
                    )),
            )
            .on_mouse_down(
                gpui::MouseButton::Right,
                cx.listener(move |this, event: &gpui::MouseDownEvent, _, cx| {
                    // Module 10680 (`SocialLayerStorefrontCard` `onContextMenu` in 869099): Copy SKU ID in dev mode.
                    if this.developer_mode() {
                        cx.stop_propagation();
                        this.context.game_shop_sku = Some(id.clone());
                        this.open_context_menu(
                            super::super::context_menu::Target::GameShopSku,
                            event.position,
                            cx,
                        );
                    }
                }),
            )
            .on_click(cx.listener(move |_, _, _, _| {
                // A card opens the sku modal of module 317560, which is not built (BLOCKED.md).
            }))
            .into_any_element()
    }

    /// Module 962299 (`SocialLayerStorefrontPromotionBanner`, CSS `279673`):
    /// sticky storefront promo banner (`min-h: 32px`, `nitro-pink` gradient or surface pill) with
    /// countdown badge (`BXpdIg`) and contextual CTA (`pj0XBN` "Get Nitro" or `787844` Orbs coupon).
    pub(in crate::ui) fn shop_game_promo_banner(
        &self,
        app_id: &str,
        promo: &StorefrontPromotion,
        now_secs: u64,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let branch = promo.branch();
        let nitro_pink = promo.is_nitro_pink();
        let countdown = promo
            .ends_at_secs
            .filter(|&end| end > now_secs)
            .map(|end| promo_countdown_label(end - now_secs));
        let cta: Option<AnyElement> = match branch {
            PromoBranch::NitroUpsell => Some(
                div()
                    .id("game-shop-promo-get-nitro")
                    .h(px(32.))
                    .px(px(12.))
                    .rounded(px(8.))
                    .bg(color("--white"))
                    .hover(|s| s.opacity(0.9))
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .cursor_pointer()
                    .child(icon("nitro_wheel", 16., "--text-strong"))
                    .child(
                        div()
                            .text_size(px(13.))
                            .line_height(px(16.))
                            .font_weight(gpui::FontWeight(600.))
                            .text_color(rgba(0x111214ff))
                            .child(dc_intl::t("pj0XBN")),
                    )
                    .on_click(cx.listener(|this, _, _, cx| {
                        cx.stop_propagation();
                        this.open_nitro(cx);
                    }))
                    .into_any_element(),
            ),
            PromoBranch::OrbsCoupon => promo.coupon_sku_id.clone().zip(promo.coupon_orb_price).map(
                |(sku_id, orb_price)| {
                    self.game_shop_orb_coupon_cta(
                        sku_id,
                        app_id.to_owned(),
                        app_id.to_owned(),
                        orb_price,
                        promo.coupon_discount_percent,
                        CouponSurface::Storefront,
                        cx,
                    )
                },
            ),
            PromoBranch::Info => None,
        };
        let mut banner = div()
            .id("game-shop-promo-banner")
            .w_full()
            .min_h(px(32.))
            .mb(px(16.))
            .px(px(16.))
            .py(px(10.))
            .rounded(px(8.))
            .flex()
            .items_center()
            .justify_between()
            .gap(px(10.));
        banner = if nitro_pink {
            banner.bg(gpui::linear_gradient(
                90.,
                gpui::linear_color_stop(rgba(0xb845c1ff), 0.),
                gpui::linear_color_stop(rgba(0xe060a8ff), 1.),
            ))
        } else {
            banner
                .bg(color("--background-surface-high"))
                .border_1()
                .border_color(color("--border-subtle"))
        };
        banner
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(icon(
                        if nitro_pink {
                            "nitro_wheel"
                        } else {
                            "shop_info"
                        },
                        16.,
                        if nitro_pink {
                            "--white"
                        } else {
                            "--icon-strong"
                        },
                    ))
                    .child(
                        div()
                            .text_size(px(14.))
                            .line_height(px(18.))
                            .font_weight(gpui::FontWeight(500.))
                            .text_color(if nitro_pink {
                                color("--white")
                            } else {
                                color("--text-strong")
                            })
                            .child(promo.text.clone()),
                    ),
            )
            .child(
                div()
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .children(countdown.map(|label| {
                        div()
                            .px(px(8.))
                            .py(px(3.))
                            .rounded(px(12.))
                            .bg(rgba(0x00000044))
                            .text_size(px(12.))
                            .line_height(px(16.))
                            .font_weight(gpui::FontWeight(600.))
                            .text_color(color("--white"))
                            .child(label)
                    }))
                    .children(cta),
            )
            .into_any_element()
    }

    /// Module 787844 (`Earn Orbs` / `Spend Orbs`): dual-mode Orbs coupon CTA button (`sm` 32px, `shop_orb_18`).
    #[allow(clippy::too_many_arguments)]
    pub(in crate::ui) fn game_shop_orb_coupon_cta(
        &self,
        sku_id: String,
        application_id: String,
        application_name: String,
        orb_price: u64,
        discount_percent: Option<u32>,
        surface: CouponSurface,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let balance = self
            .orb_wallet
            .balance
            .or_else(|| self.shop.balance.map(|b| b.max(0) as u64))
            .unwrap_or(0);
        let (label, can_spend) = orb_coupon_cta_label(balance, orb_price, surface);
        let redeeming = self
            .shop
            .coupon_redeeming
            .as_ref()
            .is_some_and(|p| p.sku_id == sku_id);
        div()
            .id(SharedString::from(format!("game-shop-orb-cta-{sku_id}")))
            .h(px(32.))
            .px(px(12.))
            .rounded(px(8.))
            .bg(if can_spend {
                color("--background-brand")
            } else {
                color("--background-mod-strong")
            })
            .hover(|s| {
                s.bg(if can_spend {
                    color("--brand-560")
                } else {
                    color("--background-surface-highest")
                })
            })
            .flex()
            .items_center()
            .gap(px(6.))
            .cursor_pointer()
            .when(redeeming, |s| s.opacity(0.6))
            .child(icon("shop_orb_18", 16., "--white"))
            .child(
                div()
                    .text_size(px(13.))
                    .line_height(px(16.))
                    .font_weight(gpui::FontWeight(600.))
                    .text_color(color("--white"))
                    .child(label),
            )
            .on_click(cx.listener(move |this, _, _, cx| {
                cx.stop_propagation();
                if redeeming {
                    return;
                }
                if !can_spend {
                    this.open_quests(cx);
                } else {
                    this.shop.coupon_redeeming = Some(PendingCouponRedeem {
                        application_id: application_id.clone(),
                        application_name: application_name.clone(),
                        sku_id: sku_id.clone(),
                        discount_percent,
                        orb_price,
                        surface,
                    });
                    let _ = this.requests.send(
                        super::super::orb_wallet::redeem_virtual_currency_sku_request(&sku_id),
                    );
                    cx.notify();
                }
            }))
            .into_any_element()
    }

    /// Module 783481: Social Layer Storefront Coupon Purchase Complete Modal (`480px`, blue expressive header).
    pub(in crate::ui) fn coupon_complete_layer(
        &self,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let modal = self.shop.coupon_complete.as_ref()?.clone();
        let title = coupon_complete_title(modal.discount_percent);
        let subtitle = coupon_complete_subtitle(&modal.application_name);
        let app_id = modal.application_id.clone();
        let card = div()
            .id("coupon-complete-modal")
            .w(px(480.))
            .rounded(px(12.))
            .overflow_hidden()
            .border_1()
            .border_color(color("--border-subtle"))
            .bg(color("--background-surface-high"))
            .flex()
            .flex_col()
            .on_click(|_, _, cx| cx.stop_propagation())
            .child(
                div()
                    .relative()
                    .w_full()
                    .h(px(172.))
                    .bg(linear_gradient(
                        135.,
                        linear_color_stop(rgba(0x5865f2ff), 0.),
                        linear_color_stop(rgba(0x2d7d9aff), 1.),
                    ))
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(
                        div()
                            .size(px(84.))
                            .rounded_full()
                            .bg(rgba(0xffffff24))
                            .border_2()
                            .border_color(rgba(0xffffff55))
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(icon("shop_orb_18", 44., "--white")),
                    )
                    .child(
                        div()
                            .id("coupon-complete-close")
                            .absolute()
                            .top(px(12.))
                            .right(px(12.))
                            .size(px(30.))
                            .rounded_full()
                            .bg(rgba(0x00000044))
                            .hover(|s| s.bg(rgba(0x00000066)))
                            .cursor_pointer()
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(icon("modal_close", 16., "--white"))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.shop.coupon_complete = None;
                                cx.notify();
                            })),
                    ),
            )
            .child(
                div()
                    .p(px(24.))
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(px(10.))
                    .child(
                        div()
                            .text_size(px(20.))
                            .line_height(px(26.))
                            .font_weight(gpui::FontWeight(700.))
                            .text_color(color("--text-strong"))
                            .text_center()
                            .child(title),
                    )
                    .child(
                        div()
                            .text_size(px(14.))
                            .line_height(px(20.))
                            .text_color(color("--text-subtle"))
                            .text_center()
                            .child(subtitle),
                    ),
            )
            .child(
                div()
                    .px(px(24.))
                    .pb(px(24.))
                    .flex()
                    .items_center()
                    .justify_end()
                    .gap(px(8.))
                    .when(modal.surface == CouponSurface::ProductDetails, |row| {
                        let back_app = app_id.clone();
                        row.child(
                            div()
                                .id("coupon-complete-back-to-shop")
                                .h(px(38.))
                                .px(px(16.))
                                .rounded(px(8.))
                                .bg(color("--background-mod-subtle"))
                                .hover(|s| s.bg(color("--background-mod-strong")))
                                .flex()
                                .items_center()
                                .justify_center()
                                .cursor_pointer()
                                .child(
                                    div()
                                        .text_size(px(14.))
                                        .font_weight(gpui::FontWeight(500.))
                                        .text_color(color("--text-strong"))
                                        .child(dc_intl::t("itMa3c")),
                                )
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.shop.coupon_complete = None;
                                    this.shop_open_game_shop(&back_app, 0, cx);
                                })),
                        )
                    })
                    .child(
                        div()
                            .id("coupon-complete-primary")
                            .h(px(38.))
                            .px(px(20.))
                            .rounded(px(8.))
                            .bg(color("--background-brand"))
                            .hover(|s| s.bg(color("--brand-560")))
                            .flex()
                            .items_center()
                            .justify_center()
                            .cursor_pointer()
                            .child(
                                div()
                                    .text_size(px(14.))
                                    .font_weight(gpui::FontWeight(600.))
                                    .text_color(color("--white"))
                                    .child(match modal.surface {
                                        CouponSurface::Storefront => dc_intl::t("NX+WJN"),
                                        CouponSurface::ProductDetails => dc_intl::t("3PatSz"),
                                    }),
                            )
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.shop.coupon_complete = None;
                                cx.notify();
                            })),
                    ),
            );
        Some(
            gpui::deferred(
                div()
                    .id("coupon-complete-backdrop")
                    .absolute()
                    .inset_0()
                    .bg(rgba(0x000000b3))
                    .flex()
                    .items_center()
                    .justify_center()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.shop.coupon_complete = None;
                        cx.notify();
                    }))
                    .child(card),
            )
            .with_priority(3)
            .into_any_element(),
        )
    }

    /// Capture-harness opener for module 783481 (`coupon-complete` / `coupon-complete pdp`).
    #[cfg(feature = "capture")]
    pub(crate) fn script_coupon_complete(&mut self, arg: &str, cx: &mut Context<Self>) -> String {
        let mode = arg.trim();
        if mode == "close" {
            self.shop.coupon_complete = None;
            cx.notify();
            return "coupon-complete: closed".to_owned();
        }
        let pdp = mode == "pdp";
        self.shop.coupon_complete = Some(CouponCompleteModal {
            application_id: "1346069614634864772".to_owned(),
            application_name: "Marvel Rivals".to_owned(),
            sku_id: "1346069614634864999".to_owned(),
            discount_percent: Some(25),
            surface: if pdp {
                CouponSurface::ProductDetails
            } else {
                CouponSurface::Storefront
            },
        });
        cx.notify();
        format!("coupon-complete: {mode}")
    }

    /// Module 849134 / 616740 (`ia`): Guild Channel List `"Game Shop"` sidebar row (`vyaWs7`).
    /// Shown when an active storefront promo popover targets the selected guild (or a storefront
    /// app is linked to the guild).
    pub(in crate::ui) fn guild_game_shop_row(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let guild_id = self.guild?;
        let pop = self
            .shop
            .game_shop_popover
            .as_ref()
            .filter(|p| p.guild_id == guild_id);
        let app_id = pop.map(|p| p.app_id.clone())?;
        let app_for_hover = app_id.clone();
        let has_new_badge = pop.is_some();
        Some(
            div()
                .id(SharedString::from(format!("game-shop-{guild_id}")))
                .mx(px(8.))
                .h(px(34.))
                .px(px(8.))
                .rounded(px(8.))
                .cursor_pointer()
                .hover(|s| s.bg(color("--background-mod-subtle")))
                .flex()
                .items_center()
                .justify_between()
                .gap(px(8.))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .child(icon("shop", 18., "--interactive-icon-default"))
                        .child(
                            div()
                                .text_size(px(15.))
                                .line_height(px(20.))
                                .font_weight(gpui::FontWeight(500.))
                                .text_color(color("--channels-default"))
                                .child(dc_intl::t("vyaWs7")),
                        ),
                )
                .when(has_new_badge, |row| {
                    row.child(
                        div()
                            .px(px(6.))
                            .h(px(16.))
                            .rounded(px(8.))
                            .bg(color("--control-primary-background-default"))
                            .flex()
                            .items_center()
                            .text_size(px(10.))
                            .font_weight(gpui::FontWeight(700.))
                            .text_color(color("--white"))
                            .child("NEW"),
                    )
                })
                .on_mouse_down(
                    gpui::MouseButton::Left,
                    cx.listener(move |this, _, _, _| {
                        this.shop_request(
                            &storefront_key(&app_for_hover),
                            storefront_route(&app_for_hover),
                        );
                    }),
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.shop.game_shop_popover = None;
                    this.shop.open = true;
                    this.shop_open_game_shop(&app_id, 0, cx);
                }))
                .into_any_element(),
        )
    }

    /// Module 849134 (`SocialLayerStorefrontPopover`): floating purple-gradient coachmark anchored
    /// to the guild sidebar's `"Game Shop"` row (`size="lg"`, `gradientColor="purple"`, `NEW` badge,
    /// `6/4` graphic preview `ulQB+t`, and `"Check it out"` `RzWDqY` CTA).
    pub(in crate::ui) fn game_shop_popover_layer(
        &self,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let pop = self.shop.game_shop_popover.as_ref()?;
        if self.guild.is_some_and(|g| g != pop.guild_id)
            || self.member_view.is_some()
            || self.join_request_modal.is_some()
            || self.template_accept.is_some()
            || self.shop_checkout_review_modal.is_some()
            || self.shop.open
        {
            return None;
        }
        let app_id = pop.app_id.clone();
        let app_prefetch = pop.app_id.clone();
        let graphic_box: AnyElement = match &pop.graphic {
            GameShopGraphicSource::Sku {
                image_url,
                background_image_url,
                alt_text,
            } => {
                let fg_img = self.images.get(image_url).cloned();
                let bg_img = background_image_url
                    .as_ref()
                    .and_then(|u| self.images.get(u))
                    .cloned();
                div()
                    .w(px(288.))
                    .h(px(192.))
                    .rounded(px(8.))
                    .overflow_hidden()
                    .relative()
                    .bg(rgba(0x2b1d4cff))
                    .border_1()
                    .border_color(rgba(0xffffff24))
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .gap(px(8.))
                    .when_some(bg_img, |b, img_data| {
                        b.child(
                            gpui::img(img_data)
                                .absolute()
                                .inset_0()
                                .size_full()
                                .object_fit(gpui::ObjectFit::Cover),
                        )
                    })
                    .child(if let Some(img_data) = fg_img {
                        gpui::img(img_data)
                            .h_full()
                            .object_fit(gpui::ObjectFit::Contain)
                            .into_any_element()
                    } else {
                        div()
                            .flex()
                            .flex_col()
                            .items_center()
                            .gap(px(8.))
                            .px(px(16.))
                            .child(icon("shop", 36., "--white"))
                            .child(
                                div()
                                    .text_center()
                                    .text_size(px(12.))
                                    .line_height(px(16.))
                                    .text_color(rgba(0xffffffcc))
                                    .child(*alt_text),
                            )
                            .into_any_element()
                    })
                    .into_any_element()
            }
            GameShopGraphicSource::Asset { url } => {
                let asset_img = self.images.get(url).cloned();
                div()
                    .w(px(288.))
                    .h(px(192.))
                    .rounded(px(8.))
                    .overflow_hidden()
                    .bg(rgba(0x2b1d4cff))
                    .border_1()
                    .border_color(rgba(0xffffff24))
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(if let Some(img_data) = asset_img {
                        gpui::img(img_data)
                            .size_full()
                            .object_fit(gpui::ObjectFit::Cover)
                            .into_any_element()
                    } else {
                        icon("shop", 36., "--white").into_any_element()
                    })
                    .into_any_element()
            }
            GameShopGraphicSource::Video { .. } => div()
                .w(px(288.))
                .h(px(192.))
                .rounded(px(8.))
                .overflow_hidden()
                .bg(rgba(0x1f1538ff))
                .border_1()
                .border_color(rgba(0xffffff24))
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap(px(8.))
                .child(
                    div()
                        .size(px(44.))
                        .rounded_full()
                        .bg(rgba(0xffffff26))
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(icon("clips_play", 20., "--white")),
                )
                .child(
                    div()
                        .text_size(px(12.))
                        .text_color(rgba(0xffffffcc))
                        .child(dc_intl::t("ulQB+t")),
                )
                .into_any_element(),
        };

        let card = div()
            .id("game-shop-promo-popover")
            .occlude()
            .w(px(320.))
            .p(px(16.))
            .rounded(px(12.))
            .bg(gpui::linear_gradient(
                155.,
                gpui::linear_color_stop(rgba(0x4b2a99ff), 0.),
                gpui::linear_color_stop(rgba(0x1e1b2eff), 1.),
            ))
            .border_1()
            .border_color(rgba(0xffffff2e))
            .shadow(vec![gpui::BoxShadow {
                color: gpui::hsla(0., 0., 0., 0.45),
                offset: gpui::point(px(0.), px(10.)),
                blur_radius: px(28.),
                spread_radius: px(0.),
                inset: false,
            }])
            .flex()
            .flex_col()
            .gap(px(12.))
            .on_click(|_, _, cx| cx.stop_propagation())
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .px(px(8.))
                            .h(px(20.))
                            .rounded(px(10.))
                            .bg(rgba(0xffffff29))
                            .flex()
                            .items_center()
                            .text_size(px(11.))
                            .font_weight(gpui::FontWeight(700.))
                            .text_color(color("--white"))
                            .child("NEW"),
                    )
                    .child(
                        div()
                            .id("game-shop-popover-close")
                            .size(px(24.))
                            .rounded(px(6.))
                            .hover(|s| s.bg(rgba(0xffffff22)))
                            .cursor_pointer()
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(icon("modal_close", 14., "--white"))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.shop.game_shop_popover = None;
                                cx.notify();
                            })),
                    ),
            )
            .child(graphic_box)
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(4.))
                    .child(
                        div()
                            .text_size(px(16.))
                            .line_height(px(20.))
                            .font_weight(gpui::FontWeight(700.))
                            .text_color(color("--white"))
                            .child(SharedString::from(pop.title.clone())),
                    )
                    .child(
                        div()
                            .text_size(px(14.))
                            .line_height(px(18.))
                            .text_color(rgba(0xffffffd9))
                            .child(SharedString::from(pop.body.clone())),
                    ),
            )
            .child(
                div()
                    .id("game-shop-popover-cta")
                    .w_full()
                    .h(px(38.))
                    .rounded(px(8.))
                    .bg(color("--control-primary-background-default"))
                    .hover(|s| s.bg(color("--control-primary-background-hover")))
                    .cursor_pointer()
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_size(px(14.))
                    .font_weight(gpui::FontWeight(600.))
                    .text_color(color("--control-primary-text-default"))
                    .child(SharedString::from(pop.action_label.clone()))
                    .on_mouse_down(
                        gpui::MouseButton::Left,
                        cx.listener(move |this, _, _, _| {
                            this.shop_request(
                                &storefront_key(&app_prefetch),
                                storefront_route(&app_prefetch),
                            );
                        }),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.shop.game_shop_popover = None;
                        this.shop.open = true;
                        this.shop_open_game_shop(&app_id, 0, cx);
                    })),
            );

        Some(
            gpui::deferred(
                div()
                    .id("game-shop-popover-anchor")
                    .absolute()
                    .left(px(320.))
                    .top(px(132.))
                    .child(card),
            )
            .with_priority(3)
            .into_any_element(),
        )
    }

    /// Capture-harness opener for module 849134 (`game-shop-popover [sku|asset|video|close]`).
    #[cfg(feature = "capture")]
    pub(crate) fn script_game_shop_popover(&mut self, arg: &str, cx: &mut Context<Self>) -> String {
        let mode = arg.trim();
        if mode == "close" {
            self.shop.game_shop_popover = None;
            cx.notify();
            return "game-shop-popover: closed".to_owned();
        }
        let guild_id = self
            .guild
            .or_else(|| self.stores.guilds.first().map(|g| g.id))
            .unwrap_or(100);
        self.shop.game_shop_popover = match mode {
            "video" => discord_announcement_popover(
                guild_id,
                "1346069614634864772",
                "Season 2 Cosmetics Drop",
                "Watch the trailer and grab limited-edition bundles directly in the Game Shop.",
                None,
                Some("https://cdn.discordapp.com/assets/game-shop-drop.webm"),
                None,
            ),
            "asset" => discord_announcement_popover(
                guild_id,
                "1346069614634864772",
                "New Hero Skins Available",
                "Explore the latest drops for your squad in the Game Shop!",
                None,
                None,
                Some("https://cdn.discordapp.com/app-assets/1346069614634864772/store/hero.webp"),
            ),
            _ => application_announcement_popover(
                guild_id,
                "1346069614634864772",
                "Marvel Rivals",
                "1346069614634864900",
                Some("1346069614634864901"),
            ),
        };
        cx.notify();
        format!("game-shop-popover: {mode}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn the_storefront_is_read_from_the_application_route() {
        assert_eq!(
            storefront_route("1346069614634864772"),
            "/partner-sdk/applications/1346069614634864772/application-storefront"
        );
        assert_eq!(storefront_key("1"), "shop:gameshop:1");
    }

    #[test]
    fn prices_show_as_intl_does_for_en_us() {
        assert_eq!(format_price(49489, "try", 2), "TRY\u{a0}494.89");
        assert_eq!(format_price(101279, "try", 2), "TRY\u{a0}1,012.79");
        assert_eq!(format_price(299, "usd", 2), "$2.99");
        assert_eq!(
            format_price(5000, "discord_orb", 0),
            "DISCORD_ORB\u{a0}5,000"
        );
    }

    #[test]
    fn titles_in_capitals_are_capitalised() {
        assert_eq!(display_title("NEW"), "New");
        assert_eq!(display_title("HOT"), "Hot");
        assert_eq!(display_title("Spring Sale"), "Spring Sale");
        assert_eq!(display_title("DON'T MISS"), "Don't Miss");
    }

    #[test]
    fn the_grid_fits_246px_columns_with_a_24px_gap() {
        assert_eq!(grid_columns(849.), 3);
        assert_eq!(grid_columns(500.), 1);
        assert_eq!(grid_columns(540.), 2);
    }

    #[test]
    fn a_storefront_gives_pages_listings_and_prices() {
        let body = json!({
            "application_id": "9",
            "pages": [{
                "title": "NEW",
                "sku_ids": ["1"],
                "sections": [{"title": "", "sku_ids": ["2"]}],
                "leaderboard": {"background_image_asset_id": "77"}
            }],
            "store_listings": [
                {"sku": {"id": "1", "name": "LUXURY", "thumbnail_asset_id": "5",
                    "price": {"amount": 49489, "currency": "try", "currency_exponent": 2},
                    "tenant_metadata": {"social_layer": {"card_image_asset_id": "6", "card_background_image_asset_id": "7",
                        "carousel_items": [{"label_icon_asset_id": "8"}]}}}},
                {"sku": {"id": "2", "name": "UPGRADED", "thumbnail_asset_id": "5",
                    "price": {"amount": 100, "currency": "try", "currency_exponent": 2}}}
            ],
            "storefront_pricing": {
                "sku_price_map": {"2": {"pricing_result_id": "p"}},
                "pricing_result_id_map": {"p": {"0": {"user_price": [
                    {"amount": 101279, "currency": "try", "exponent": 2},
                    {"amount": 5000, "currency": "discord_orb", "exponent": 0}]}}}
            }
        });
        let store = parse_storefront(&body).unwrap();
        assert_eq!(store.app, "9");
        assert_eq!(store.pages[0].skus, vec!["1".to_owned()]);
        assert_eq!(store.pages[0].sections[0].skus, vec!["2".to_owned()]);
        assert_eq!(store.pages[0].background.as_deref(), Some("77"));
        let first = &store.listings["1"];
        assert_eq!(first.card_image.as_deref(), Some("6"));
        assert_eq!(first.card_background.as_deref(), Some("7"));
        assert_eq!(first.label_icon.as_deref(), Some("8"));
        assert_eq!(first.price.as_deref(), Some("TRY\u{a0}494.89"));
        // A sku in the pricing map shows the mapped price; one without a card image falls back to its thumbnail.
        let second = &store.listings["2"];
        assert_eq!(second.price.as_deref(), Some("TRY\u{a0}1,012.79"));
        assert_eq!(second.card_image.as_deref(), Some("5"));
        assert_eq!(second.orb_price, Some(5000));
        assert_eq!(
            asset_url("9", "6", 512),
            "https://cdn.discordapp.com/app-assets/9/store/6.webp?size=512"
        );
    }

    #[test]
    fn coupon_complete_783481_and_orb_cta_787844_format_correctly() {
        let with_pct = coupon_complete_title(Some(25));
        assert!(with_pct.contains("25%"), "got {with_pct}");
        assert_eq!(coupon_complete_title(None), dc_intl::t("Xtf9wP"));
        let sub = coupon_complete_subtitle("Marvel Rivals");
        assert!(sub.contains("Marvel Rivals"), "got {sub}");
        assert_eq!(
            orb_coupon_cta_label(500, 1200, CouponSurface::Storefront),
            (dc_intl::t("SymzJC").to_owned(), false)
        );
        assert_eq!(
            orb_coupon_cta_label(1500, 1200, CouponSurface::Storefront),
            (dc_intl::t("BxjHiu").to_owned(), true)
        );
        let (pdp_label, can_spend) =
            orb_coupon_cta_label(1500, 1200, CouponSurface::ProductDetails);
        assert!(can_spend);
        assert!(pdp_label.contains("1,200") || pdp_label.contains("1200"));
        let req = super::super::super::orb_wallet::redeem_virtual_currency_sku_request("999");
        assert!(matches!(
            req,
            crate::net::Request::Rest { route, .. } if route == "/virtual-currency/skus/999/redeem"
        ));
    }

    #[test]
    fn game_shop_popover_849134_builders_and_intl() {
        for key in ["ulQB+t", "RzWDqY", "vyaWs7", "7PvvS9", "9J4h1a"] {
            assert!(!dc_intl::t(key).is_empty(), "empty 849134 key {key}");
        }
        assert!(application_announcement_popover(1, "app9", "Marvel Rivals", "", None).is_none());
        let sku_pop =
            application_announcement_popover(1, "app9", "Marvel Rivals", "asset42", Some("bg7"))
                .expect("sku popover");
        assert_eq!(sku_pop.title, dc_intl::t("7PvvS9"));
        assert!(sku_pop.body.contains("Marvel Rivals"));
        assert_eq!(sku_pop.action_label, dc_intl::t("RzWDqY"));
        assert!(matches!(
            sku_pop.graphic,
            GameShopGraphicSource::Sku {
                ref image_url,
                background_image_url: Some(ref bg),
                alt_text,
            } if image_url.ends_with("/app-assets/app9/store/asset42.webp?size=256")
                && bg.ends_with("/app-assets/app9/store/bg7.webp?size=256")
                && alt_text == dc_intl::t("ulQB+t")
        ));
        assert!(discord_announcement_popover(1, "app9", "t", "b", None, None, Some("")).is_none());
        let vid_pop = discord_announcement_popover(
            1,
            "app9",
            "Season Drop",
            "Check out the new skins.",
            None,
            Some("https://cdn.example/drop.webm"),
            Some("https://cdn.example/drop.png"),
        )
        .expect("video popover");
        assert!(matches!(
            vid_pop.graphic,
            GameShopGraphicSource::Video { .. }
        ));
        assert_eq!(vid_pop.action_label, dc_intl::t("RzWDqY"));
    }

    #[test]
    fn storefront_promo_banner_962299_branches_and_countdown() {
        assert!(!dc_intl::t("pj0XBN").is_empty());
        assert!(dc_intl::has_key("BXpdIg"));
        let nitro_promo = StorefrontPromotion {
            text: "Get 20% off with Nitro".into(),
            ends_at_secs: Some(200_000),
            flavor: "nitro".into(),
            needs_nitro: true,
            ..Default::default()
        };
        assert_eq!(nitro_promo.branch(), PromoBranch::NitroUpsell);
        assert!(nitro_promo.is_nitro_pink());
        assert!(!nitro_promo.is_expired(100_000));
        assert!(nitro_promo.is_expired(200_000));

        let orbs_promo = StorefrontPromotion {
            text: "Redeem coupon with Orbs".into(),
            coupon_sku_id: Some("sku1".into()),
            coupon_orb_price: Some(500),
            coupon_earned_or_consumed: false,
            ..Default::default()
        };
        assert_eq!(orbs_promo.branch(), PromoBranch::OrbsCoupon);
        assert!(orbs_promo.is_nitro_pink());

        let info_promo = StorefrontPromotion {
            text: "Seasonal drop live".into(),
            ..Default::default()
        };
        assert_eq!(info_promo.branch(), PromoBranch::Info);
        assert!(!info_promo.is_nitro_pink());

        assert_eq!(promo_countdown_label(3661), "01:01:01");
        assert!(promo_countdown_label(3 * 86_400).contains('3'));
    }
}
