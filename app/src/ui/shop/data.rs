//! The reads behind the Shop page and the rules that turn answers into what the page draws. Routes and query keys follow
//! the bundle: `COLLECTIBLES_SHOP`, `COLLECTIBLES_CATEGORIES_V2`, `COLLECTIBLES_SEARCH` and `COLLECTIBLES_PRODUCTS` in
//! module 652215, the fetchers of module 110676 and the query builder of module 181774, the storefront fetchers of module
//! 350172 and 540737 and the search of module 946716. Every read here is a GET.

/// The collectibles storefront application (`FYj` in module 652215).
pub const APPLICATION_ID: &str = "1096190356233670716";
/// The locale every shop read sends (`intl.locale`).
pub const LOCALE: &str = "en-US";

/// `VARIANTS_GROUP` (module 635358): variants of one product come back as a group.
const VARIANTS_GROUP: u8 = 2;

/// `GET /collectibles-shop` for a tab: the hook of module 208930 adds `variants_return_style`, `include_bundles` and
/// `include_dynamic_blocks`, and `ao` of module 181774 turns the options into the query.
pub fn shop_home_route(tab: &str) -> String {
    format!(
        "/collectibles-shop?tab={tab}&include_bundles=true&include_dynamic_blocks=true&variants_return_style={VARIANTS_GROUP}"
    )
}

/// `GET /storefront/collections/applications` for one page of the catalog's collections (`cS` of module 350172).
pub fn collections_route(offset: u32, limit: u32) -> String {
    format!(
        "/storefront/collections/applications?application_id={APPLICATION_ID}&use_shop_ordering=true&offset={offset}&limit={limit}&include_pricing=true&locale={LOCALE}&with_bundled_skus=true&include_google_sku_ids=false&include_unpublished_products=false&include_unpublished_collections=false&ignore_cache=false"
    )
}

/// `GET /collectibles-products/<sku>` (`er` of module 110676).
pub fn product_route(sku: &str) -> String {
    format!("/collectibles-products/{sku}?locale={LOCALE}&include_bundles=true")
}

/// `GET /storefront/products/skus`: the batch the feed, the shelves and the wishlist fill products from (`tu` of module
/// 540737). The bundle's request class repeats the key for each id.
pub fn products_route(skus: &[String]) -> String {
    let ids: Vec<String> = skus.iter().map(|sku| format!("sku_ids={sku}")).collect();
    format!(
        "/storefront/products/skus?{}&locale={LOCALE}&with_bundled_skus=true&include_google_sku_ids=false&ignore_cache=false",
        ids.join("&")
    )
}

/// `GET /users/@me/virtual-currency/balance`: `{ "balance": <orbs> }` (module 986630).
pub const BALANCE_ROUTE: &str = "/users/@me/virtual-currency/balance";

/// The purchase types a price table is keyed by (`lid` in module 652215): the default price and the Nitro price.
pub const PRICE_DEFAULT: &str = "0";
pub const PRICE_NITRO: &str = "4";

/// The 1-day Nitro credit, which the feed and the Orbs grid hide (`PREMIUM_TIER_2_1_DAY` in module 253782).
pub const NITRO_1_DAY: &str = "1298745361602449479";

/// The Orbs currency name inside a price list (`Yr.DISCORD_ORB`).
pub const ORB: &str = "discord_orb";

/// Item kinds of a product (`R` in module 575593).
pub const KIND_AVATAR_DECORATION: i64 = 0;
pub const KIND_PROFILE_EFFECT: i64 = 1;
pub const KIND_NAMEPLATE: i64 = 2;
pub const KIND_PROFILE_FRAME: i64 = 3;
pub const KIND_BUNDLE: i64 = 1000;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Price {
    pub amount: i64,
    pub currency: String,
    pub exponent: u32,
}

impl Price {
    /// The amount the way `$g` of module 993408 writes it: `TRY 329`, `TRY 94.99`; Orbs print as the bare number.
    pub fn text(&self) -> String {
        if self.currency == ORB {
            return self.amount.to_string();
        }
        let divisor = 10_i64.pow(self.exponent);
        let (whole, part) = (self.amount / divisor, self.amount % divisor);
        let number = if part == 0 {
            whole.to_string()
        } else {
            format!("{whole}.{part:0width$}", width = self.exponent as usize)
        };
        format!("{} {number}", self.currency.to_uppercase())
    }
}

impl Price {
    /// The amount with every decimal the currency has, as the Buy button writes it: `TRY 329.00`.
    pub fn text_exact(&self) -> String {
        if self.currency == ORB {
            return self.amount.to_string();
        }
        let divisor = 10_i64.pow(self.exponent);
        format!(
            "{} {}.{:0width$}",
            self.currency.to_uppercase(),
            self.amount / divisor,
            self.amount % divisor,
            width = self.exponent as usize
        )
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Item {
    pub kind: i64,
    pub sku_id: String,
    pub asset: String,
    pub label: String,
    /// Profile effects: `thumbnailPreviewSrc`, the still the card and the modal show.
    pub thumbnail: Option<String>,
    /// Profile frames: the ids of the layers, drawn from `<item sku>/<layer>/static`.
    pub layers: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Product {
    pub sku_id: String,
    pub name: String,
    pub summary: String,
    pub kind: i64,
    pub premium_type: Option<i64>,
    pub category_sku_id: String,
    /// Price lists by purchase type (`prices[type].country_prices.prices`).
    pub prices: Vec<(String, Vec<Price>)>,
    pub fg_static: Option<String>,
    pub bg_static: Option<String>,
    pub items: Vec<Item>,
    pub bundled: Vec<Product>,
    pub is_first_party: bool,
    /// `styles.background_colors` and `styles.button_colors` as 0xRRGGBB integers.
    pub background_colors: Vec<u32>,
    pub button_colors: Vec<u32>,
}

/// One product as the storefront and `/collectibles-products` return it (`fromServer` of module 623373):
/// `{sku_id, name, summary, type, premium_type, category_sku_id, prices{<type>:{country_prices:{prices:[{amount,
/// currency, exponent}]}}}, preview_assets{fg_static, bg_static, fg_video}, items[{type, sku_id, asset, label}],
/// bundled_products[...], is_first_party, styles{background_colors, button_colors}}`.
pub fn parse_product(value: &serde_json::Value) -> Option<Product> {
    let sku_id = value["sku_id"].as_str()?.to_owned();
    let text = |key: &str| value[key].as_str().unwrap_or_default().to_owned();
    let mut prices: Vec<(String, Vec<Price>)> = value["prices"]
        .as_object()
        .map(|map| {
            map.iter()
                .map(|(kind, entry)| {
                    let list = entry["country_prices"]["prices"]
                        .as_array()
                        .map(|list| {
                            list.iter()
                                .map(|price| Price {
                                    amount: price["amount"].as_i64().unwrap_or(0),
                                    currency: price["currency"]
                                        .as_str()
                                        .unwrap_or_default()
                                        .to_owned(),
                                    exponent: price["exponent"].as_u64().unwrap_or(0) as u32,
                                })
                                .collect()
                        })
                        .unwrap_or_default();
                    (kind.clone(), list)
                })
                .collect()
        })
        .unwrap_or_default();
    prices.sort_by(|a, b| a.0.cmp(&b.0));
    let colors = |key: &str| -> Vec<u32> {
        value["styles"][key]
            .as_array()
            .map(|list| {
                list.iter()
                    .filter_map(|color| color.as_u64().map(|color| color as u32))
                    .collect()
            })
            .unwrap_or_default()
    };
    Some(Product {
        sku_id,
        name: text("name"),
        summary: text("summary"),
        kind: value["type"].as_i64().unwrap_or(-1),
        premium_type: value["premium_type"].as_i64(),
        category_sku_id: text("category_sku_id"),
        prices,
        fg_static: value["preview_assets"]["fg_static"]
            .as_str()
            .map(str::to_owned),
        bg_static: value["preview_assets"]["bg_static"]
            .as_str()
            .map(str::to_owned),
        items: value["items"]
            .as_array()
            .map(|list| {
                list.iter()
                    .map(|item| Item {
                        kind: item["type"].as_i64().unwrap_or(-1),
                        sku_id: item["sku_id"].as_str().unwrap_or_default().to_owned(),
                        asset: item["asset"].as_str().unwrap_or_default().to_owned(),
                        label: item["label"].as_str().unwrap_or_default().to_owned(),
                        thumbnail: item["thumbnailPreviewSrc"].as_str().map(str::to_owned),
                        layers: item["layers"]
                            .as_array()
                            .map(|list| {
                                list.iter()
                                    .filter_map(|layer| layer["id"].as_str().map(str::to_owned))
                                    .collect()
                            })
                            .unwrap_or_default(),
                    })
                    .collect()
            })
            .unwrap_or_default(),
        bundled: value["bundled_products"]
            .as_array()
            .map(|list| list.iter().filter_map(parse_product).collect())
            .unwrap_or_default(),
        is_first_party: value["is_first_party"].as_bool().unwrap_or(false),
        background_colors: colors("background_colors"),
        button_colors: colors("button_colors"),
    })
}

impl Product {
    /// `R` of module 993408: the first price of a purchase type, or the one in a given currency.
    pub fn price(&self, purchase_type: &str, currency: Option<&str>) -> Option<&Price> {
        let list = &self
            .prices
            .iter()
            .find(|(kind, _)| kind == purchase_type)?
            .1;
        match currency {
            None => list.first(),
            Some(currency) => list.iter().find(|price| price.currency == currency),
        }
    }

    pub fn is_bundle(&self) -> bool {
        self.kind == KIND_BUNDLE
    }

    /// `Ab` of module 623373: an Orbs price and no real-money price.
    pub fn orbs_exclusive(&self) -> bool {
        self.price(PRICE_DEFAULT, Some(ORB)).is_some() && self.fiat_price(false).is_none()
    }

    /// The real-money price a user pays: the Nitro list for a Nitro subscriber (`C` of module 993408).
    pub fn fiat_price(&self, nitro: bool) -> Option<&Price> {
        let kind = if nitro { PRICE_NITRO } else { PRICE_DEFAULT };
        let list = &self.prices.iter().find(|(entry, _)| entry == kind)?.1;
        list.iter().find(|price| price.currency != ORB)
    }

    /// `fT` of module 993408: what a bundle's parts cost at their default prices and the share the bundle saves.
    pub fn bundle_discount(&self, nitro: bool) -> Option<(i64, i64)> {
        if !self.is_bundle() {
            return None;
        }
        // An Orbs bundle compares Orbs prices (`C` of module 993408 picks the default list for the Orbs currency).
        let in_orbs = self.fiat_price(false).is_none();
        let part_price = |part: &'_ Product| -> Option<i64> {
            if in_orbs {
                part.price(PRICE_DEFAULT, Some(ORB))
                    .map(|price| price.amount)
            } else {
                part.fiat_price(false).map(|price| price.amount)
            }
        };
        let original: i64 = self
            .bundled
            .iter()
            .map(|part| part_price(part).unwrap_or(0))
            .sum();
        if original <= 0 {
            return None;
        }
        let paid = if in_orbs {
            self.price(PRICE_DEFAULT, Some(ORB))?.amount
        } else {
            self.fiat_price(nitro)?.amount
        };
        let percent = ((original - paid) as f64 / original as f64 * 100.).round() as i64;
        (percent > 0).then_some((original, percent))
    }

    /// The `{n}pc` prefix a bundle's name carries on a card.
    pub fn piece_count(&self) -> Option<usize> {
        self.is_bundle()
            .then_some(self.items.len())
            .filter(|count| *count > 0)
    }
}

#[derive(Clone, Debug, Default)]
pub struct Category {
    pub sku_id: String,
    pub name: String,
    pub products: Vec<Product>,
    pub hero_banner_url: Option<String>,
    pub hero_logo_url: Option<String>,
    pub catalog_banner_url: Option<String>,
    /// `unpublished_at`: when the collection leaves the shop (`YYYY-MM-DDTHH:MM:SS+00:00`).
    pub unpublished_at: Option<String>,
}

fn string(value: &serde_json::Value, key: &str) -> Option<String> {
    value[key]
        .as_str()
        .filter(|text| !text.is_empty())
        .map(str::to_owned)
}

/// A category of `/collectibles-shop` (`categories[]`: `{sku_id, name, summary, products[], hero_banner_url,
/// hero_logo_url, catalog_banner_url, logo_url, featured_block_url, hero_banner_display_config{background_style}}`)
/// or a collection of `/storefront/collections/applications` (`collections[]`: `{id, name, description, products[],
/// tenant_metadata}`).
pub fn parse_category(value: &serde_json::Value) -> Category {
    Category {
        sku_id: value["sku_id"]
            .as_str()
            .or_else(|| value["id"].as_str())
            .unwrap_or_default()
            .to_owned(),
        name: value["name"].as_str().unwrap_or_default().to_owned(),
        products: value["products"]
            .as_array()
            .map(|list| {
                list.iter()
                    .filter_map(|product| match product["skus"].is_array() {
                        true => parse_product(&storefront_product(product)),
                        false => parse_product(product),
                    })
                    .collect()
            })
            .unwrap_or_default(),
        hero_banner_url: string(value, "hero_banner_url"),
        unpublished_at: string(value, "unpublished_at"),
        hero_logo_url: string(value, "hero_logo_url"),
        catalog_banner_url: string(value, "catalog_banner_url").or_else(|| {
            string(
                &value["tenant_metadata"]["collectibles"],
                "catalog_banner_url",
            )
        }),
    }
}

/// One SKU record of the storefront (`/storefront/collections/applications`: `{id, type, name, description, prices,
/// preview_asset_paths, bundled_skus, tenant_metadata.collectibles{type, premium_type, item, is_first_party}}`) in the
/// shape `parse_product` reads. `styles` are the collection styles of the product that holds the SKU.
fn storefront_sku(
    sku: &serde_json::Value,
    category: &serde_json::Value,
    styles: &serde_json::Value,
) -> serde_json::Value {
    let meta = &sku["tenant_metadata"]["collectibles"];
    let bundled: Vec<serde_json::Value> = sku["bundled_skus"]
        .as_array()
        .map(|list| {
            list.iter()
                .map(|part| storefront_sku(part, category, styles))
                .collect()
        })
        .unwrap_or_default();
    let items: Vec<serde_json::Value> = if bundled.is_empty() {
        meta["item"]
            .as_object()
            .map(|_| meta["item"].clone())
            .into_iter()
            .collect()
    } else {
        bundled
            .iter()
            .flat_map(|part| part["items"].as_array().cloned().unwrap_or_default())
            .collect()
    };
    serde_json::json!({
        "sku_id": sku["id"],
        "name": sku["name"],
        "summary": sku["description"],
        "type": meta["type"],
        "premium_type": meta["premium_type"],
        "category_sku_id": category,
        "prices": sku["prices"],
        "preview_assets": sku["preview_asset_paths"],
        "items": items,
        "bundled_products": bundled,
        "is_first_party": meta["is_first_party"],
        "styles": styles,
    })
}

/// A product of the storefront (`{id, skus[], tenant_metadata.collectibles.primary_collection_id, ...}`): its first
/// SKU stands for it, as the default option does on a card.
fn storefront_product(product: &serde_json::Value) -> serde_json::Value {
    let meta = &product["tenant_metadata"]["collectibles"];
    match product["skus"].as_array().and_then(|skus| skus.first()) {
        Some(sku) => storefront_sku(
            sku,
            &meta["primary_collection_id"],
            &meta["primary_collection_styles"],
        ),
        None => serde_json::Value::Null,
    }
}

/// Days from the UTC date `now_secs` falls on to the date of `unpublished_at` (`G` of module 993408), for the
/// "N days left" pill; the pill shows when that is 60 or less (`HF`).
pub fn days_left(unpublished_at: &str, now_secs: i64) -> Option<i64> {
    let mut parts = unpublished_at.get(..10)?.split('-');
    let year: i64 = parts.next()?.parse().ok()?;
    let month: i64 = parts.next()?.parse().ok()?;
    let day: i64 = parts.next()?.parse().ok()?;
    let days = civil_days(year, month, day) - now_secs.div_euclid(86_400);
    (days <= 60).then_some(days)
}

/// Days since 1970-01-01 of a civil date.
fn civil_days(year: i64, month: i64, day: i64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let year_of_era = year - era * 400;
    let shifted = (month + 9) % 12;
    let day_of_year = (153 * shifted + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

/// The blocks of a shop tab (`type` values of module 110676: 0 hero, 1 featured, 2 feed, 3 wide banner, 4 shelf).
#[derive(Clone, Debug)]
pub enum Block {
    Hero {
        category: String,
        name: String,
        title: Option<String>,
        summary: String,
        skus: Vec<String>,
        banner: Option<String>,
        background_style: Option<String>,
    },
    Featured(Vec<SubBlock>),
    Shelf {
        name: String,
        category: String,
        skus: Vec<String>,
        button: Option<String>,
    },
    Feed {
        popular: Vec<String>,
        recommended: Vec<String>,
    },
    WideBanner {
        title: String,
        body: String,
        banner: Option<String>,
        logo: Option<String>,
    },
}

#[derive(Clone, Debug)]
pub struct SubBlock {
    pub category: String,
    pub asset: Option<String>,
}

#[derive(Clone, Debug, Default)]
pub struct Home {
    pub blocks: Vec<Block>,
    pub categories: Vec<Category>,
}

fn ids(value: &serde_json::Value) -> Vec<String> {
    value
        .as_array()
        .map(|list| {
            list.iter()
                .filter_map(|id| id.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default()
}

fn parse_block(block: &serde_json::Value) -> Option<Block> {
    let text = |key: &str| block[key].as_str().unwrap_or_default().to_owned();
    match block["type"].as_i64()? {
        0 => Some(Block::Hero {
            category: block["category_sku_id"].as_str()?.to_owned(),
            name: text("name"),
            title: string(block, "title"),
            summary: text("summary").trim().to_owned(),
            skus: ids(&block["ranked_sku_ids"]),
            banner: string(block, "hero_banner_url"),
            background_style: string(&block["banner_display_config"], "background_style"),
        }),
        1 => Some(Block::Featured(
            block["subblocks"]
                .as_array()
                .map(|list| {
                    list.iter()
                        .map(|sub| SubBlock {
                            category: sub["category_sku_id"]
                                .as_str()
                                .unwrap_or_default()
                                .to_owned(),
                            asset: string(sub, "asset_url"),
                        })
                        .collect()
                })
                .unwrap_or_default(),
        )),
        2 => Some(Block::Feed {
            popular: ids(&block["sorted_sku_ids"]["popular"]),
            recommended: ids(&block["sorted_sku_ids"]["recommended"]),
        }),
        3 => Some(Block::WideBanner {
            title: text("title"),
            body: text("body"),
            banner: string(block, "wide_banner_url"),
            logo: string(block, "logo_url"),
        }),
        4 => Some(Block::Shelf {
            name: text("name"),
            category: text("category_sku_id"),
            skus: ids(&block["ranked_sku_ids"]),
            button: string(block, "button_text").filter(|_| block["show_button"] == true),
        }),
        _ => None,
    }
}

/// `{shop_blocks:[...], categories:[...]}` (module 110676; shapes seen in a harness dump of the live tab).
pub fn parse_home(body: &serde_json::Value) -> Home {
    Home {
        blocks: body["shop_blocks"]
            .as_array()
            .map(|list| list.iter().filter_map(parse_block).collect())
            .unwrap_or_default(),
        categories: body["categories"]
            .as_array()
            .map(|list| list.iter().map(parse_category).collect())
            .unwrap_or_default(),
    }
}

/// `{collections:[...], total}` of the catalog read.
pub fn parse_collections(body: &serde_json::Value) -> (Vec<Category>, u64) {
    (
        body["collections"]
            .as_array()
            .map(|list| list.iter().map(parse_category).collect())
            .unwrap_or_default(),
        body["total"].as_u64().unwrap_or(0),
    )
}

/// `{products:[...]}` of the batch read.
pub fn parse_products(body: &serde_json::Value) -> Vec<Product> {
    let mut out = Vec::new();
    for product in body["products"].as_array().into_iter().flatten() {
        match product["skus"].as_array() {
            // The storefront read: every SKU of a product is a card of its own.
            Some(skus) => {
                let meta = &product["tenant_metadata"]["collectibles"];
                for sku in skus {
                    let shaped = storefront_sku(
                        sku,
                        &meta["primary_collection_id"],
                        &meta["primary_collection_styles"],
                    );
                    out.extend(parse_product(&shaped));
                }
            }
            None => out.extend(parse_product(product)),
        }
    }
    out
}

/// `{skus: [sku ids], pagination: {has_more}}` of the search read.
pub fn parse_search(body: &serde_json::Value) -> (Vec<String>, bool) {
    (
        ids(&body["skus"]),
        body["pagination"]["has_more"].as_bool().unwrap_or(false),
    )
}

/// The sorts of the Browse select (the browse page's `sort_type` and `sort_direction`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Sort {
    #[default]
    Recent,
    PriceLow,
    PriceHigh,
    Popular,
    Relevance,
    /// "For You": the feed block's own recommended order, never a search.
    Recommended,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Filters {
    /// Item types by the enum name the query carries (`AVATAR_DECORATION`, `NAMEPLATE`, `PROFILE_EFFECT`,
    /// `PROFILE_FRAME`, `BUNDLE`).
    pub item_types: Vec<String>,
    pub orbs_eligible: bool,
    /// The "Collabs" checkbox (`thirdPartyOnly`): sends `is_first_party=false`.
    pub collabs: bool,
    /// Colour enum names such as `COLLECTIBLES_COLOR_PURPLE`.
    pub colors: Vec<String>,
    /// Theme enum names such as `COLLECTIBLES_THEME_ANIME`.
    pub themes: Vec<String>,
    pub search: String,
}

impl Filters {
    /// `hasFilters`: anything chosen or typed.
    pub fn is_empty(&self) -> bool {
        *self == Filters::default()
    }

    /// `hasRelevanceFilters`: Relevance sorting only exists with a search text.
    pub fn toggled(list: &mut Vec<String>, value: &str) {
        match list.iter().position(|entry| entry == value) {
            Some(at) => {
                list.remove(at);
            }
            None => list.push(value.to_owned()),
        }
    }
}

/// `GET /shop/search` (module 365491 builds the query, module 946716 sends it): the filters, the sort and one page.
/// Arrays are written with brackets (`item_types[]=...`), which the bundle's query writer emits for lists. Answer:
/// `{skus: [sku ids], pagination: {has_more, limit, offset}}`.
pub fn search_route(filters: &Filters, sort: Sort, offset: u32, limit: u32) -> String {
    let mut query = Vec::new();
    for kind in &filters.item_types {
        query.push(format!("item_types[]={kind}"));
    }
    for color in &filters.colors {
        query.push(format!("colors[]={color}"));
    }
    for theme in &filters.themes {
        query.push(format!("themes[]={theme}"));
    }
    if filters.orbs_eligible {
        query.push("orbs_eligible=true".to_owned());
    }
    if filters.collabs {
        query.push("is_first_party=false".to_owned());
    }
    query.push(format!("offset={offset}"));
    query.push(format!("limit={limit}"));
    let (kind, direction) = match sort {
        Sort::Recent => ("recency", "desc"),
        Sort::PriceLow => ("price", "asc"),
        Sort::PriceHigh => ("price", "desc"),
        Sort::Popular => ("popularity", "desc"),
        Sort::Relevance | Sort::Recommended => ("relevance", "desc"),
    };
    query.push(format!("sort_type={kind}"));
    query.push(format!("sort_direction={direction}"));
    if !filters.search.is_empty() {
        let text: String = filters.search.chars().take(256).collect();
        query.push(format!("search={}", url_encode(&text)));
    }
    query.push("published_collections_only=true".to_owned());
    format!("/shop/search?{}", query.join("&"))
}

fn url_encode(text: &str) -> String {
    let mut out = String::new();
    for byte in text.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char)
            }
            other => out.push_str(&format!("%{other:02X}")),
        }
    }
    out
}

/// A write the page can ask for. Nothing here is sent by a harness run; the request shapes are unit-tested.
#[derive(Clone, Debug, PartialEq)]
pub struct Write {
    pub method: &'static str,
    pub route: String,
    pub body: Option<serde_json::Value>,
}

/// `addSkuToWishlist` of module 808247: `POST /users/@me/wishlist/items {sku_id}`.
pub fn wishlist_add(sku: &str) -> Write {
    Write {
        method: "POST",
        route: "/users/@me/wishlist/items".to_owned(),
        body: Some(serde_json::json!({ "sku_id": sku })),
    }
}

/// `removeSkuFromWishlist`: `DELETE /users/@me/wishlists/<wishlist>/items/<sku>`.
pub fn wishlist_remove(wishlist: &str, sku: &str) -> Write {
    Write {
        method: "DELETE",
        route: format!("/users/@me/wishlists/{wishlist}/items/{sku}"),
        body: None,
    }
}

/// `payment_gateway` of the virtual-currency (Orbs) gateway in the orders API.
pub const GATEWAY_VIRTUAL_CURRENCY: i64 = 8;

/// What the Orbs buttons show for a product and a balance (the checks of modules 355735 and the Orbs checkout):
/// no Orbs price is "not redeemable", a missing or smaller balance is "not enough".
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OrbButton {
    Ready,
    NotEnough,
    NotRedeemable,
}

impl OrbButton {
    pub fn check(price: Option<&Price>, balance: Option<i64>) -> Self {
        match (price, balance) {
            (None, _) => Self::NotRedeemable,
            (Some(price), Some(balance)) if price.amount <= balance => Self::Ready,
            _ => Self::NotEnough,
        }
    }

    /// The tooltip of the disabled product-modal button.
    pub fn modal_tip(self) -> Option<&'static str> {
        match self {
            Self::Ready => None,
            Self::NotEnough => Some("Not enough Orbs"),
            Self::NotRedeemable => Some("Not available"),
        }
    }

    /// The tooltip of the disabled checkout button.
    pub fn checkout_tip(self) -> Option<&'static str> {
        match self {
            Self::Ready => None,
            Self::NotEnough => Some("Not enough Orbs."),
            Self::NotRedeemable => Some("This item is not redeemable with Orbs."),
        }
    }
}

/// Step 1 of an Orbs claim: `POST /billing/orders` with the sku as a purchase line and the Orbs gateway.
pub fn orb_order_create(sku: &str) -> Write {
    Write {
        method: "POST",
        route: "/billing/orders".to_owned(),
        body: Some(serde_json::json!({
            "order_line_items": [{ "sku_id": sku, "quantity": 1, "purchase_type": 1 }],
            "billing_facet": { "payment_gateway": GATEWAY_VIRTUAL_CURRENCY },
        })),
    }
}

/// Step 2: `POST /billing/orders/<id>/sign`, with `expected_revision` when the order carries one.
pub fn order_sign(order: &str, expected_revision: Option<i64>) -> Write {
    let mut body = serde_json::json!({});
    if let Some(revision) = expected_revision {
        body["expected_revision"] = revision.into();
    }
    Write {
        method: "POST",
        route: format!("/billing/orders/{order}/sign"),
        body: Some(body),
    }
}

/// Step 3 (a read): `GET /billing/orders/<id>/entitlements`.
pub fn order_entitlements_route(order: &str) -> String {
    format!("/billing/orders/{order}/entitlements")
}

/// Claiming a premium collectible (module 987421): `PUT /users/@me/claim-premium-collectibles-product {sku_id}`.
pub fn claim_premium(sku: &str) -> Write {
    Write {
        method: "PUT",
        route: "/users/@me/claim-premium-collectibles-product".to_owned(),
        body: Some(serde_json::json!({ "sku_id": sku })),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn prices_print_like_the_bundle() {
        let price = |amount, currency: &str, exponent| Price {
            amount,
            currency: currency.to_owned(),
            exponent,
        };
        assert_eq!(price(32900, "try", 2).text(), "TRY 329");
        assert_eq!(price(9499, "try", 2).text(), "TRY 94.99");
        assert_eq!(price(905, "usd", 2).text(), "USD 9.05");
        assert_eq!(price(1400, ORB, 0).text(), "1400");
        assert_eq!(price(32900, "try", 2).text_exact(), "TRY 329.00");
    }

    fn bundle() -> serde_json::Value {
        let part = |amount| json!({"sku_id":"p","name":"Part","type":0,"prices":{"0":{"country_prices":{"prices":[{"amount":amount,"currency":"try","exponent":2}]}}}});
        json!({
            "sku_id":"1","name":"Pack","type":1000,"premium_type":0,
            "prices":{
                "0":{"country_prices":{"prices":[{"amount":32900,"currency":"try","exponent":2}]}},
                "4":{"country_prices":{"prices":[{"amount":24199,"currency":"try","exponent":2}]}}
            },
            "items":[{"type":0,"sku_id":"a","asset":"x","label":"l"}],
            "bundled_products":[part(9499), part(9499), part(9499), part(9499)]
        })
    }

    #[test]
    fn bundle_discount_is_the_share_against_the_parts() {
        let product = parse_product(&bundle()).unwrap();
        assert_eq!(product.bundle_discount(false), Some((37996, 13)));
        assert_eq!(product.fiat_price(true).unwrap().amount, 24199);
        assert!(!product.orbs_exclusive());
        assert_eq!(product.piece_count(), Some(1));
    }

    #[test]
    fn days_left_counts_utc_dates() {
        // 2026-11-10 12:00 UTC.
        let now = 1_794_312_000;
        assert_eq!(days_left("2026-11-14T07:59:59+00:00", now), Some(4));
        assert_eq!(days_left("2026-11-10T23:00:00+00:00", now), Some(0));
        assert_eq!(days_left("2027-06-01T00:00:00+00:00", now), None);
    }

    #[test]
    fn orbs_bundles_compare_orbs_prices() {
        let orb = |amount: i64| json!({"0":{"country_prices":{"prices":[{"amount":amount,"currency":"discord_orb","exponent":0}]}}});
        let product = parse_product(&json!({
            "sku_id":"1","name":"Swirl","type":1000,"prices":orb(8900),
            "bundled_products":[
                {"sku_id":"2","name":"A","type":0,"prices":orb(5000)},
                {"sku_id":"3","name":"B","type":2,"prices":orb(5500)}
            ]
        }))
        .unwrap();
        assert_eq!(product.bundle_discount(false), Some((10500, 15)));
    }

    #[test]
    fn orbs_only_products_have_no_real_price() {
        let product = parse_product(&json!({
            "sku_id":"2","name":"Credit","type":3000,
            "prices":{"0":{"country_prices":{"prices":[{"amount":1400,"currency":"discord_orb","exponent":0}]}}}
        }))
        .unwrap();
        assert!(product.orbs_exclusive());
    }

    #[test]
    fn home_blocks_parse_by_type() {
        let home = parse_home(&json!({
            "shop_blocks":[
                {"type":0,"category_sku_id":"9","name":"N","ranked_sku_ids":["1","2"],"hero_banner_url":"u"},
                {"type":4,"name":"S","category_sku_id":"8","ranked_sku_ids":["3"],"show_button":true,"button_text":"Shop All S"},
                {"type":2,"sorted_sku_ids":{"popular":["4"],"recommended":["5"]}},
                {"type":3,"title":"T","body":"B","disable_cta":true,"cta_text":null}
            ],
            "categories":[{"sku_id":"9","name":"N","products":[{"sku_id":"1","name":"X","type":0}]}]
        }));
        assert_eq!(home.blocks.len(), 4);
        assert!(
            matches!(&home.blocks[1], Block::Shelf { button: Some(text), .. } if text == "Shop All S")
        );
        assert!(
            matches!(&home.blocks[2], Block::Feed { recommended, .. } if recommended == &["5".to_owned()])
        );
        assert!(matches!(&home.blocks[3], Block::WideBanner { .. }));
        assert_eq!(home.categories[0].products[0].sku_id, "1");
    }

    #[test]
    fn search_asks_for_the_filters_and_the_sort() {
        let filters = Filters {
            item_types: vec!["AVATAR_DECORATION".to_owned(), "NAMEPLATE".to_owned()],
            orbs_eligible: true,
            colors: vec!["COLLECTIBLES_COLOR_PURPLE".to_owned()],
            themes: vec!["COLLECTIBLES_THEME_ANIME".to_owned()],
            collabs: true,
            search: "dead pool".to_owned(),
        };
        assert_eq!(
            search_route(&filters, Sort::PriceLow, 0, 24),
            "/shop/search?item_types[]=AVATAR_DECORATION&item_types[]=NAMEPLATE&colors[]=COLLECTIBLES_COLOR_PURPLE&themes[]=COLLECTIBLES_THEME_ANIME&orbs_eligible=true&is_first_party=false&offset=0&limit=24&sort_type=price&sort_direction=asc&search=dead%20pool&published_collections_only=true"
        );
        assert!(!filters.is_empty());
        assert!(Filters::default().is_empty());
    }

    #[test]
    fn toggling_a_filter_adds_then_removes_it() {
        let mut list = Vec::new();
        Filters::toggled(&mut list, "BUNDLE");
        assert_eq!(list, ["BUNDLE"]);
        Filters::toggled(&mut list, "BUNDLE");
        assert!(list.is_empty());
    }

    #[test]
    fn products_are_read_in_one_batch() {
        let route = products_route(&["1".to_owned(), "2".to_owned()]);
        assert!(route.starts_with("/storefront/products/skus?sku_ids=1&sku_ids=2&locale=en-US"));
    }

    #[test]
    fn the_batch_read_makes_a_card_per_sku() {
        let sku = |id: &str| {
            json!({"id":id,"name":"Cat Ears","description":"d",
            "tenant_metadata":{"collectibles":{"type":0,"premium_type":0}},"prices":{}})
        };
        let body = json!({"products":[{"id":"p","sku_ids":["a","b"],
            "tenant_metadata":{"collectibles":{"primary_collection_id":"9"}},
            "skus":[sku("a"), sku("b")]}]});
        let found = parse_products(&body);
        assert_eq!(found.len(), 2);
        assert_eq!(found[1].sku_id, "b");
        assert_eq!(found[1].category_sku_id, "9");
    }

    #[test]
    fn account_writes_are_built_not_sent() {
        assert_eq!(wishlist_add("7").body, Some(json!({"sku_id":"7"})));
        assert_eq!(
            wishlist_remove("w", "7").route,
            "/users/@me/wishlists/w/items/7"
        );
        assert_eq!(wishlist_remove("w", "7").method, "DELETE");
        assert_eq!(claim_premium("7").method, "PUT");
    }

    #[test]
    fn storefront_products_read_like_shop_products() {
        let body = json!({"id":"9","name":"Set","products":[{
            "id":"p1","tenant_metadata":{"collectibles":{"primary_collection_id":"9","primary_collection_styles":{"background_colors":[1]}}},
            "skus":[{"id":"s1","name":"Deadpool","description":"d","tenant_metadata":{"collectibles":{"type":0,"premium_type":0,
                "item":{"type":0,"sku_id":"i1","asset":"a","label":"l"}}},
                "prices":{"0":{"country_prices":{"prices":[{"amount":9499,"currency":"try","exponent":2}]}}}}]}]});
        let category = parse_category(&body);
        let product = &category.products[0];
        assert_eq!(product.sku_id, "s1");
        assert_eq!(product.category_sku_id, "9");
        assert_eq!(product.items[0].sku_id, "i1");
        assert_eq!(
            product.fiat_price(false).map(Price::text).as_deref(),
            Some("TRY 94.99")
        );
    }

    #[test]
    fn orb_claim_requests_are_built_not_sent() {
        let create = orb_order_create("7");
        assert_eq!(
            (create.method, create.route.as_str()),
            ("POST", "/billing/orders")
        );
        assert_eq!(
            create.body,
            Some(json!({
                "order_line_items": [{"sku_id":"7","quantity":1,"purchase_type":1}],
                "billing_facet": {"payment_gateway":8}
            }))
        );
        assert_eq!(order_sign("9", None).route, "/billing/orders/9/sign");
        assert_eq!(order_sign("9", None).body, Some(json!({})));
        assert_eq!(
            order_sign("9", Some(3)).body,
            Some(json!({"expected_revision":3}))
        );
        assert_eq!(
            order_entitlements_route("9"),
            "/billing/orders/9/entitlements"
        );
    }

    #[test]
    fn orb_button_follows_the_balance() {
        let price = Price {
            amount: 700,
            currency: ORB.to_owned(),
            exponent: 0,
        };
        assert_eq!(OrbButton::check(Some(&price), Some(700)), OrbButton::Ready);
        assert_eq!(
            OrbButton::check(Some(&price), Some(699)),
            OrbButton::NotEnough
        );
        assert_eq!(OrbButton::check(Some(&price), None), OrbButton::NotEnough);
        assert_eq!(OrbButton::check(None, Some(9)), OrbButton::NotRedeemable);
        assert_eq!(OrbButton::NotEnough.modal_tip(), Some("Not enough Orbs"));
        assert_eq!(
            OrbButton::NotEnough.checkout_tip(),
            Some("Not enough Orbs.")
        );
        assert_eq!(OrbButton::Ready.modal_tip(), None);
    }
}
