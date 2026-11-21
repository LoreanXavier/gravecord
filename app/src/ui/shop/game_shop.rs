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

#[derive(Clone, Debug, Default, PartialEq)]
pub(in crate::ui) struct Storefront {
    pub app: String,
    pub pages: Vec<Page>,
    pub listings: HashMap<String, Listing>,
}

/// `GET /partner-sdk/applications/{id}/application-storefront` (`SOCIAL_LAYER_STOREFRONT_BY_APPLICATION_ID`).
pub(super) fn storefront_route(app: &str) -> String {
    format!("/partner-sdk/applications/{app}/application-storefront")
}

pub(super) fn storefront_key(app: &str) -> String {
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
                },
            ))
        })
        .collect();
    Some(Storefront {
        app,
        pages,
        listings,
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
                    div().h(px(20.)).flex().items_center().child(
                        text(14., 20., 600., "--white")
                            .truncate()
                            .child(listing.price.clone().unwrap_or_default()),
                    ),
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
        assert_eq!(
            asset_url("9", "6", 512),
            "https://cdn.discordapp.com/app-assets/9/store/6.webp?size=512"
        );
    }
}
