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
pub const KIND_EXTERNAL: i64 = 3000;

/// External skus with their own art (`EXTERNAL_FRACTIONAL_NITRO` coins and `EXTERNAL_ORB_BADGE`).
pub const NITRO_3_DAY: &str = "1333912750274904064";
pub const ORB_BADGE: &str = "1342211853484429445";

/// The animation file behind the coin cards; the artboard name selects the stack or the single coin.
pub const COINS_RIV: &str = "https://cdn.discordapp.com/assets/content/fac02dd925a746a61291e1f604a8affff663e9d8a22cd9b21749991ff745f1f2.riv";
pub const ORB_BADGE_PNG: &str = "https://cdn.discordapp.com/assets/content/615334270467aa3d5adc86cc67efee89f8380a87b945a96e89ec2eb37c27993d.png";

#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
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

#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
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

#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
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
    /// `preview_assets.fg_animated`: the bundle art a hovered card shows instead of `fg_static` (module 842385).
    pub fg_animated: Option<String>,
    pub bg_static: Option<String>,
    pub items: Vec<Item>,
    pub bundled: Vec<Product>,
    pub is_first_party: bool,
    /// `styles.background_colors` and `styles.button_colors` as 0xRRGGBB integers.
    pub background_colors: Vec<u32>,
    pub button_colors: Vec<u32>,
}

/// Discount source shown beside a Shop price line (`993408.D0`, module 219103).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiscountSource {
    Nitro,
    ThirdParty,
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
        // `premiumType: 0 === n ? null : n` (module 696444): 0 is no Nitro tier.
        premium_type: value["premium_type"].as_i64().filter(|tier| *tier != 0),
        category_sku_id: text("category_sku_id"),
        prices,
        fg_static: value["preview_assets"]["fg_static"]
            .as_str()
            .map(str::to_owned),
        fg_animated: value["preview_assets"]["fg_animated"]
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
    /// What the modal previews for the bundle's selected slide (`selectedBundleSlide` of module 355735): the item
    /// alone, as a product of its own kind. Any other product previews itself.
    pub fn slide(&self, index: usize) -> Product {
        if self.kind != KIND_BUNDLE {
            return self.clone();
        }
        match self.items.get(index) {
            Some(item) => Product {
                kind: item.kind,
                sku_id: item.sku_id.clone(),
                items: vec![item.clone()],
                ..self.clone()
            },
            None => self.clone(),
        }
    }

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

    /// Minimum discount percentage displayed on a Shop price line (`>= 5` in module 219103).
    pub const MIN_DISCOUNT_PERCENT: i64 = 5;

    /// Non-bundle fiat discount source and percentage (`993408.D0`, module 219103): compares default
    /// fiat price against Nitro (`PRICE_NITRO`) or third-party (e.g. Xbox Game Pass) discounted price.
    pub fn fiat_discount(&self, nitro: bool) -> Option<(DiscountSource, i64)> {
        let base = self.price(PRICE_DEFAULT, None)?.amount;
        if base <= 0 {
            return None;
        }
        if nitro
            && let Some(discounted) = self.price(PRICE_NITRO, None)
            && discounted.amount < base
        {
            let pct = ((base - discounted.amount) as f64 / base as f64 * 100.).round() as i64;
            if pct >= Self::MIN_DISCOUNT_PERCENT {
                return Some((DiscountSource::Nitro, pct));
            }
        }
        // Third-party discount bucket (e.g. Xbox Game Pass `THIRDPARTY_CROISSANT`, key `"12"`).
        if let Some(discounted) = self.price("12", None)
            && discounted.amount < base
        {
            let pct = ((base - discounted.amount) as f64 / base as f64 * 100.).round() as i64;
            if pct >= Self::MIN_DISCOUNT_PERCENT {
                return Some((DiscountSource::ThirdParty, pct));
            }
        }
        None
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
    /// `pdp_bg_url`: the already blurred art behind the product modal's preview (`pdpBgUrl`, module 212407).
    pub pdp_bg_url: Option<String>,
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
        pdp_bg_url: string(value, "pdp_bg_url"),
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
    Hero(Box<Hero>),
    Featured(Vec<SubBlock>),
    Shelf {
        name: String,
        category: String,
        skus: Vec<String>,
        button: Option<String>,
        /// `desktop_background_image` and `title_color` (the shelf record's `desktopBackgroundImage` and
        /// `titleColor`): a shelf drawn on a picture, in the rounded box of `withBackground_dg2ugo`.
        background: Option<String>,
        title_color: Option<String>,
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

/// The hero block (`type` 0): the record `tl`/`eB.A` of module 253782 and 353448 draw.
#[derive(Clone, Debug, Default)]
pub struct Hero {
    pub category: String,
    pub title: Option<String>,
    pub summary: String,
    pub skus: Vec<String>,
    pub banner: Option<String>,
    pub logo: Option<String>,
    pub background_style: Option<String>,
    /// `banner_display_config.responsive`: the banner is 24vw tall (at least 200px) and the header 18vw (at least
    /// 130px) instead of 350 and 255 (`responsive_XBXDcd`).
    pub responsive: bool,
    /// `logo_display_config.desktop_max_height` (`toDesktopStyles` of module 719986): replaces the logo's 90px cap.
    pub logo_max_height: Option<f32>,
    pub unpublished_at: Option<String>,
    pub banner_text_color: Option<String>,
}

/// A tile of the featured block (`subblocks[]`). Only `type` 0 (`CATEGORY`, module 424918) is drawn; module 253782
/// renders nothing for any other type.
#[derive(Clone, Debug)]
pub struct SubBlock {
    pub kind: i64,
    pub category: String,
    pub asset: Option<String>,
    pub body_text: Option<String>,
    pub text_color: Option<String>,
    pub unpublished_at: Option<String>,
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
        0 => Some(Block::Hero(Box::new(Hero {
            category: block["category_sku_id"].as_str()?.to_owned(),
            title: string(block, "title"),
            summary: text("summary").trim().to_owned(),
            skus: ids(&block["ranked_sku_ids"]),
            banner: string(block, "hero_banner_url"),
            logo: string(block, "hero_logo_url"),
            background_style: string(&block["banner_display_config"], "background_style"),
            responsive: block["banner_display_config"]["responsive"] == true,
            logo_max_height: block["logo_display_config"]["desktop_max_height"]
                .as_f64()
                .map(|height| height as f32),
            unpublished_at: string(block, "unpublished_at"),
            banner_text_color: string(block, "banner_text_color"),
        }))),
        1 => Some(Block::Featured(
            block["subblocks"]
                .as_array()
                .map(|list| {
                    list.iter()
                        .map(|sub| SubBlock {
                            kind: sub["type"].as_i64().unwrap_or(-1),
                            category: sub["category_sku_id"]
                                .as_str()
                                .unwrap_or_default()
                                .to_owned(),
                            asset: string(sub, "asset_url"),
                            body_text: string(sub, "body_text"),
                            text_color: string(sub, "banner_text_color"),
                            unpublished_at: string(sub, "unpublished_at"),
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
            background: string(block, "desktop_background_image"),
            title_color: string(block, "title_color"),
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

/// Complete product records embedded directly inside a `/wishlists/<id>` payload (`wishlist_items[*].sku`).
/// Minimal `{sku: {id}}` stubs without collectibles metadata are skipped so batch lookup still runs for them.
pub fn parse_wishlist_products(body: &serde_json::Value) -> Vec<Product> {
    let mut out = Vec::new();
    for entry in body["wishlist_items"].as_array().into_iter().flatten() {
        let sku = &entry["sku"];
        if !sku.is_object() {
            continue;
        }
        if sku["tenant_metadata"]["collectibles"]["type"].is_i64() {
            let shaped = storefront_sku(sku, &serde_json::Value::Null, &serde_json::Value::Null);
            out.extend(parse_product(&shaped));
        } else if sku["sku_id"].is_string()
            && sku["type"].is_i64()
            && sku["name"].as_str().is_some_and(|n| !n.is_empty())
        {
            out.extend(parse_product(sku));
        }
    }
    out
}

fn push_effect_item(
    parent_sku: Option<u64>,
    item: &serde_json::Value,
    out: &mut Vec<(u64, dc_state::profile_effects::ProfileEffect)>,
) {
    if !item["effects"]
        .as_array()
        .is_some_and(|effects| !effects.is_empty())
    {
        return;
    }
    if let Some(effect) = dc_state::profile_effects::ProfileEffect::from_item(item) {
        let key_sku = parent_sku.unwrap_or(effect.sku_id);
        out.push((key_sku, effect));
    }
}

fn collect_effects_from_sku(
    sku: &serde_json::Value,
    out: &mut Vec<(u64, dc_state::profile_effects::ProfileEffect)>,
) {
    let sku_id = sku["id"]
        .as_str()
        .or_else(|| sku["sku_id"].as_str())
        .and_then(|s| s.parse::<u64>().ok());
    let item = &sku["tenant_metadata"]["collectibles"]["item"];
    if item.is_object() {
        push_effect_item(sku_id, item, out);
    }
    for sub in sku["bundled_skus"].as_array().into_iter().flatten() {
        collect_effects_from_sku(sub, out);
    }
}

fn collect_effects_from_product(
    product: &serde_json::Value,
    out: &mut Vec<(u64, dc_state::profile_effects::ProfileEffect)>,
) {
    let sku_id = product["sku_id"]
        .as_str()
        .and_then(|s| s.parse::<u64>().ok());
    for item in product["items"].as_array().into_iter().flatten() {
        push_effect_item(sku_id, item, out);
    }
    for sub in product["bundled_products"].as_array().into_iter().flatten() {
        collect_effects_from_product(sub, out);
    }
    for sku in product["skus"].as_array().into_iter().flatten() {
        collect_effects_from_sku(sku, out);
    }
}

/// Complete profile effects (`effects` array present and non-empty) extracted from shop, storefront, or wishlist payloads.
pub fn parse_profile_effects(
    body: &serde_json::Value,
) -> Vec<(u64, dc_state::profile_effects::ProfileEffect)> {
    let mut out = Vec::new();
    collect_effects_from_product(body, &mut out);
    for product in body["products"].as_array().into_iter().flatten() {
        collect_effects_from_product(product, &mut out);
    }
    for category in body["categories"].as_array().into_iter().flatten() {
        for product in category["products"].as_array().into_iter().flatten() {
            collect_effects_from_product(product, &mut out);
        }
    }
    for entry in body["wishlist_items"].as_array().into_iter().flatten() {
        if entry["sku"].is_object() {
            collect_effects_from_sku(&entry["sku"], &mut out);
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
    /// `PROFILE_FRAME`, `BUNDLE`); the numbers of `CollectiblesItemType` are answered with a 400.
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
    /// The sort the Browse page picks until the shopper picks one (`h` of module 365491): relevance for colors, themes
    /// or a search text, popularity for item types alone, recency otherwise.
    pub fn default_sort(&self) -> Sort {
        if !self.colors.is_empty() || !self.themes.is_empty() || !self.search.trim().is_empty() {
            Sort::Relevance
        } else if !self.item_types.is_empty() {
            Sort::Popular
        } else {
            Sort::Recent
        }
    }

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
/// Arrays repeat the key without brackets (`item_types=A&item_types=B`): superagent's query writer (module 333405)
/// recurses into an array with the same key. Checked against the live server in a harness run: repeated keys with names
/// filter, bracketed keys are ignored and numbers answer 400. Answer:
/// `{skus: [sku ids], pagination: {has_more, limit, offset}}`.
pub fn search_route(filters: &Filters, sort: Sort, offset: u32, limit: u32) -> String {
    let mut query = Vec::new();
    for kind in &filters.item_types {
        query.push(format!("item_types={kind}"));
    }
    for color in &filters.colors {
        query.push(format!("colors={color}"));
    }
    for theme in &filters.themes {
        query.push(format!("themes={theme}"));
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

/// Order status `SIGNED` (`Re` in module 26279).
pub const ORDER_SIGNED: i64 = 2;

/// The waits between the entitlement reads after an order is signed (`_` in module 158317): the first read is at
/// once, a read that finds nothing is repeated after each of these.
pub const ENTITLEMENT_DELAYS_MS: [u64; 6] = [250, 500, 1000, 1500, 2500, 4250];

/// What the claim chain does after one answer (`redeemVirtualCurrency` of module 715054).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClaimNext {
    /// A write was refused by the harness: back to the review without a notice.
    Refused,
    /// The order exists: sign it.
    Sign {
        order: String,
        revision: Option<i64>,
    },
    /// The order is signed, or an entitlement read came back empty: read again after the wait of this attempt.
    Entitlements { attempt: usize },
    /// The entitlement exists.
    Done { consumed: bool },
    /// The review with this notice.
    Fail(&'static str),
    /// An answer that does not belong to the chain.
    Ignore,
}

/// The step that follows an answer of the claim chain. `key` is the request key, `body` the fetch answer
/// (`{body, fetch_failed, refused}`), `reads` the entitlement reads made so far.
pub fn claim_next(key: &str, body: &serde_json::Value, reads: usize) -> ClaimNext {
    let answer = &body["body"];
    let failed = body["fetch_failed"] == true;
    if body["refused"] == true {
        return ClaimNext::Refused;
    }
    match key {
        "shop:order:create" => match answer["id"].as_str() {
            Some(id) if !failed => ClaimNext::Sign {
                order: id.to_owned(),
                revision: answer["revision"].as_i64(),
            },
            _ => ClaimNext::Fail(sign_failure_text(answer).unwrap_or(GENERIC_CLAIM_ERROR)),
        },
        "shop:order:sign" => match sign_failure_text(answer) {
            Some(text) => ClaimNext::Fail(text),
            None if failed => ClaimNext::Fail(GENERIC_CLAIM_ERROR),
            None => ClaimNext::Entitlements { attempt: 0 },
        },
        "shop:order:entitlements" => match answer.as_array().filter(|list| !list.is_empty()) {
            Some(list) => ClaimNext::Done {
                consumed: list[0]["consumed"].as_bool().unwrap_or(false),
            },
            None if reads < ENTITLEMENT_DELAYS_MS.len() => {
                ClaimNext::Entitlements { attempt: reads + 1 }
            }
            None => ClaimNext::Fail(ENTITLEMENTS_LATE),
        },
        _ => ClaimNext::Ignore,
    }
}

/// What the checkout's critical notice says about an answer to the sign call (`$9` of module 80066). `None` is
/// a signed order. An order body with `unsatisfied_constraints` is the 400 of a refused signing; any other body
/// carries the API error `code`, or the order's own `error.billing_error_code`.
pub fn sign_failure_text(answer: &serde_json::Value) -> Option<&'static str> {
    const NOT_ENOUGH: &str = "Not enough Orbs.";
    const OWNED: &str = "Item already owned.";
    const GENERIC: &str = "There was an error claiming this item. Please try again later.";
    let by_code = |code: i64| match code {
        590001 => NOT_ENOUGH,
        100011 | 100096 => OWNED,
        100152 => "Your checkout session expired. Reopen checkout to continue.",
        _ => GENERIC,
    };
    if answer["id"].is_string() && answer["status"].is_number() {
        let reason = answer["unsatisfied_constraints"][0]["reason_code"].as_i64();
        if answer["status"].as_i64() == Some(ORDER_SIGNED) {
            return None;
        }
        return Some(match reason {
            Some(1000) => NOT_ENOUGH,
            Some(1003) => OWNED,
            Some(1010) => "You already own one or more items in this bundle.",
            Some(_) => GENERIC,
            None => match answer["error"]["billing_error_code"].as_i64() {
                Some(code) if answer["error"]["code"].as_i64() != Some(9999) => by_code(code),
                _ => GENERIC,
            },
        });
    }
    Some(match answer["code"].as_i64() {
        Some(code) => by_code(code),
        None => GENERIC,
    })
}

/// The static art of an external sku: the coin artboard of a Nitro credit, or the Orbs badge image.
pub fn external_art_url(sku: &str) -> Option<String> {
    match sku {
        NITRO_3_DAY => Some(format!("{COINS_RIV}#STACKED_COINS")),
        NITRO_1_DAY => Some(format!("{COINS_RIV}#SINGLE_COIN")),
        ORB_BADGE => Some(ORB_BADGE_PNG.to_owned()),
        _ => None,
    }
}

/// The type label after an item's name in a bundle (`Dm` of module 993408).
pub fn item_type_label(kind: i64) -> Option<&'static str> {
    match kind {
        KIND_AVATAR_DECORATION => Some("Avatar Decoration"),
        KIND_PROFILE_EFFECT => Some("Profile Effect"),
        KIND_NAMEPLATE => Some("Nameplate"),
        KIND_PROFILE_FRAME => Some("Profile Frame"),
        _ => None,
    }
}

/// Whether an external sku is a fractional Nitro credit (`EZ` of module 758836): its modal lists the perks.
pub fn is_fractional_nitro(sku: &str) -> bool {
    sku == NITRO_1_DAY || sku == NITRO_3_DAY
}

/// The perks the Nitro credit modal lists (`eW` of module 355735, strings `E1NP2x`, `kpMomJ`, `IDAfOy`, `myyAEr`,
/// `zTk8Ul`; the upload size of the second tier is 500 MB).
pub const NITRO_PERKS: [&str; 5] = [
    "Custom emoji anywhere",
    "Custom profiles",
    "500MB Uploads",
    "HD video streaming",
    "Color app themes and more!",
];

/// The notice for every failure that has no better text.
pub const GENERIC_CLAIM_ERROR: &str =
    "There was an error claiming this item. Please try again later.";

/// The legal line of the Orbs checkout (`IP93kX` of module 854461, `OrbsRedemption`).
pub const ORB_LEGAL: &str = "By clicking 'Claim with Orbs', you agree to the Paid Services Terms. Claiming an item from the Shop means you're claiming a limited license to use this item on Discord, learn more here. Orbs redemptions are non-refundable.";

/// "Bundle includes: ..." (`q` of module 355735): a decoration, a profile effect and a nameplate by name with their type
/// as the label, profile frames left out; joined with " & ", or with commas and a last " & " once a nameplate is in.
pub fn bundle_includes(bundled: &[Product]) -> String {
    let mut parts = Vec::new();
    let mut nameplate = false;
    for part in bundled {
        match part.kind {
            KIND_AVATAR_DECORATION => parts.push(format!("{} Decoration", part.name)),
            KIND_PROFILE_EFFECT => parts.push(format!("{} Profile Effect", part.name)),
            KIND_NAMEPLATE => {
                parts.push(format!("{} Nameplate", part.name));
                nameplate = true;
            }
            _ => {}
        }
    }
    let joined = if nameplate {
        let all = parts.join(", ");
        match all.rfind(", ") {
            Some(at) => format!("{} & {}", &all[..at], &all[at + 2..]),
            None => all,
        }
    } else {
        parts.join(" & ")
    };
    format!("Bundle includes: {joined}")
}

/// The line under a product's name (`Z` of module 355735): the server's summary, a bundle's `{joinedItems}` replaced by
/// the list above, or the type's own line when the server sent none.
pub fn product_summary(product: &Product) -> String {
    if !product.summary.is_empty() {
        if product.kind == KIND_BUNDLE && product.summary.contains("{joinedItems}") {
            return product.summary.replacen(
                "{joinedItems}",
                &bundle_includes(&product.bundled),
                1,
            );
        }
        return product.summary.clone();
    }
    match product.kind {
        KIND_AVATAR_DECORATION => "Give your avatar a new look.".to_owned(),
        KIND_PROFILE_EFFECT => "Show this effect when others view your profile.".to_owned(),
        KIND_NAMEPLATE => "Make your name stand out in servers and chats.".to_owned(),
        KIND_PROFILE_FRAME => "Elevate and wrap your profile with a frame.".to_owned(),
        KIND_BUNDLE => bundle_includes(&product.bundled),
        _ => String::new(),
    }
}

/// The link the share button copies (`f` of module 760429): the Shop route, `?tab=orbs` on the Orbs tab, and the sku.
pub fn share_link(sku: &str, orbs_tab: bool) -> String {
    let tab = if orbs_tab { "?tab=orbs" } else { "" };
    format!("https://discord.com/shop{tab}#itemSkuId={sku}")
}

/// What a button of the celebration modal does (`aG` of module 184264).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CollectedAction {
    /// Closes the modal and nothing else ("Close").
    Close,
    /// Closes it and opens Quest Home ("Earn More Orbs").
    EarnMoreOrbs,
    /// Closes it and opens the account popout of the user panel ("View My Badge").
    ViewBadge,
    /// Closes it and opens the Nitro page ("Explore Perks").
    ExplorePerks,
    /// Closes it and opens Settings on Subscriptions ("View Credits").
    ViewCredits,
    /// Closes it and opens the profile editor ("Edit User Profile").
    EditProfile,
    /// Wears the collected item (`handleUseNow` of module 450481).
    UseNow,
}

/// What the celebration modal says after a claim (`$k`, `v8` and `aG` of module 184264).
#[derive(Debug, PartialEq, Eq)]
pub struct Collected {
    pub title: String,
    pub subtitle: String,
    /// The buttons left to right: the label, whether it is the primary one and what it does.
    pub buttons: Vec<(&'static str, bool, CollectedAction)>,
}

/// Whether "Use Now" is offered (`canUseNow` of module 450481): never for the special skus, and a bundle only when no
/// two of its items are of one type.
pub fn can_use_now(product: &Product) -> bool {
    if product.sku_id == ORB_BADGE || product.sku_id == NITRO_3_DAY || product.sku_id == NITRO_1_DAY
    {
        return false;
    }
    if product.kind != KIND_BUNDLE {
        return true;
    }
    let mut seen = Vec::new();
    for item in &product.items {
        if seen.contains(&item.kind) {
            return false;
        }
        seen.push(item.kind);
    }
    true
}

pub fn collected_copy(product: &Product, consumed: bool) -> Collected {
    let (sku, name) = (product.sku_id.as_str(), product.name.as_str());
    let credit = sku == NITRO_3_DAY || sku == NITRO_1_DAY;
    if sku == ORB_BADGE {
        return Collected {
            title: "You Collected the Orbs Profile Badge!".to_owned(),
            subtitle: "The badge has been added to your profile.".to_owned(),
            buttons: vec![
                ("Earn More Orbs", false, CollectedAction::EarnMoreOrbs),
                ("View My Badge", true, CollectedAction::ViewBadge),
            ],
        };
    }
    if credit {
        let days = if sku == NITRO_1_DAY { 1 } else { 3 };
        return if consumed {
            Collected {
                title: "Nitro Activated".to_owned(),
                subtitle: format!(
                    "Your {days}-day Nitro credit begins now! Enjoy your premium perks. Learn more."
                ),
                buttons: vec![
                    ("Close", false, CollectedAction::Close),
                    ("Explore Perks", true, CollectedAction::ExplorePerks),
                ],
            }
        } else {
            Collected {
                title: "Nitro Credit Acquired".to_owned(),
                subtitle: format!(
                    "Your {days}-day Nitro credit will be used automatically at the end of your current billing cycle. Learn more."
                ),
                buttons: vec![
                    ("Close", false, CollectedAction::Close),
                    ("View Credits", true, CollectedAction::ViewCredits),
                ],
            }
        };
    }
    let can_use = can_use_now(product);
    let mut buttons = vec![("Edit User Profile", !can_use, CollectedAction::EditProfile)];
    if can_use {
        buttons.push(("Use Now", true, CollectedAction::UseNow));
    }
    Collected {
        title: "You did it! Put it on!".to_owned(),
        subtitle: format!("{name} has been added to your collection."),
        buttons,
    }
}

/// The toast "Use Now" shows once the writes went through (`getToastText` of module 450481).
pub fn use_now_toast(product: &Product) -> &'static str {
    match product.kind {
        KIND_AVATAR_DECORATION => "Your Avatar Decoration is updated!",
        KIND_NAMEPLATE => "Your Nameplate is updated!",
        KIND_PROFILE_FRAME => "Your Profile Frame is updated!",
        KIND_PROFILE_EFFECT => "Your Profile Effect is updated!",
        _ => "Your profile is updated!",
    }
}

/// The requests "Use Now" sends: the first item of each type goes on (`firstAvatarDecoration`, `firstNameplate`,
/// `firstProfileEffect` and `firstProfileFrame` of module 442759), the profile write before the account write the way
/// `handleUseNow` awaits them.
pub fn use_now_writes(product: &Product) -> Vec<super::super::profile_editor::save::Write> {
    use super::super::profile_editor::save::{Equipped, Pending};
    let first = |kind: i64| -> Option<dc_state::Id> {
        product
            .items
            .iter()
            .find(|item| item.kind == kind)
            .and_then(|item| item.sku_id.parse().ok())
    };
    let mut pending = Pending::new(None);
    pending.avatar_decoration = first(KIND_AVATAR_DECORATION).map(Some);
    pending.nameplate = first(KIND_NAMEPLATE).map(Some);
    pending.profile_effect = first(KIND_PROFILE_EFFECT).map(Some);
    pending.profile_frame = first(KIND_PROFILE_FRAME).map(Some);
    let writes = pending.writes(&Equipped::default());
    let (account, profile): (Vec<_>, Vec<_>) = writes
        .into_iter()
        .partition(|write| write.route == "/users/@me");
    profile.into_iter().chain(account).collect()
}

/// The notice when no entitlement shows up after every wait (`j2` in module 158317).
pub const ENTITLEMENTS_LATE: &str =
    "Your purchase is taking longer than expected to process, and will be available shortly.";

/// `GET /partner-sdk/storefront-config` (`SOCIAL_LAYER_STOREFRONT_CONFIG`): the storefronts the header lists.
pub const STOREFRONT_CONFIG_ROUTE: &str = "/partner-sdk/storefront-config";

/// The applications whose storefront the navigation lists (`collectibles_shop_navigation_enabled`), in the order of
/// the answer (module 685491).
pub fn storefront_nav_apps(body: &serde_json::Value) -> Vec<String> {
    body["storefronts"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|store| store["collectibles_shop_navigation_enabled"] == true)
        .filter_map(|store| store["application_id"].as_str().map(str::to_owned))
        .collect()
}

/// `GET /applications/public?application_ids=a&application_ids=b` (`fetchApplications` of module 627363).
pub fn applications_route(ids: &[String]) -> String {
    let query: Vec<String> = ids
        .iter()
        .map(|id| format!("application_ids={id}"))
        .collect();
    format!("/applications/public?{}", query.join("&"))
}

/// One entry of the Game Shops menu: the application's name and its 32px icon.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GameShop {
    pub id: String,
    pub name: String,
    pub icon: Option<String>,
}

/// The menu entries in the order of the storefront list, leaving out an application the answer does not name
/// (`flatMap` in module 685491).
pub fn parse_game_shops(ids: &[String], body: &serde_json::Value) -> Vec<GameShop> {
    let apps = body.as_array().cloned().unwrap_or_default();
    ids.iter()
        .filter_map(|id| {
            let app = apps.iter().find(|app| app["id"].as_str() == Some(id))?;
            let name = app["name"].as_str()?.to_owned();
            let icon = app["icon"].as_str().map(|icon| {
                format!("https://cdn.discordapp.com/app-icons/{id}/{icon}.png?size=32")
            });
            Some(GameShop {
                id: id.clone(),
                name,
                icon,
            })
        })
        .collect()
}

/// The account's wishlist ids: the keys of `wishlist_settings` in the profile answer (`getWishlistIds`).
pub fn wishlist_ids(profile: &serde_json::Value) -> Vec<String> {
    profile["wishlist_settings"]
        .as_object()
        .map(|settings| settings.keys().cloned().collect())
        .unwrap_or_default()
}

/// When a profile's first wishlist is read (`m` of module 634110): once, and again whenever the profile's
/// `wishlist_settings.<id>.updated_at` differs from the one the list was read for (`fetchWishlist(id, updatedAt)` of
/// module 808247 keeps it with the list). Gives the wishlist id and the request guard that carries that stamp.
pub fn wishlist_read_guard(user: u64, profile: &serde_json::Value) -> Option<(String, String)> {
    let id = wishlist_ids(profile).into_iter().next()?;
    let stamp = profile["wishlist_settings"][id.as_str()]["updated_at"]
        .as_str()
        .unwrap_or_default()
        .to_owned();
    Some((id, format!("profile-wishlist:{user}@{stamp}")))
}

/// `GET /wishlists/<id>?source=user_profile` (`fetchWishlist` of module 808247).
pub fn wishlist_route(id: &str) -> String {
    format!("/wishlists/{id}?source=user_profile")
}

/// The icons of the profile's `badges` as the image urls the preview card draws in front of the product's own badge.
pub fn profile_badge_urls(profile: &serde_json::Value) -> Vec<String> {
    profile["badges"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|badge| badge["icon"].as_str())
        .map(|icon| format!("https://cdn.discordapp.com/badge-icons/{icon}.png"))
        .collect()
}

/// `GET /users/<id>/profile`: where the wishlist ids come from (`fetchProfile` of module 803306).
pub fn own_profile_route(user: u64) -> String {
    format!(
        "/users/{user}/profile?with_mutual_guilds=false&with_mutual_friends=false&with_mutual_friends_count=false"
    )
}

/// The last navigation option that stays visible when the row is `container` wide (`Wv` of module 3666 with one
/// line): an option stays while it and the overflow button after it fit, the last option needs no overflow button.
pub fn last_visible(widths: &[f32], overflow: f32, gap: f32, container: f32) -> usize {
    let mut used = 0.0_f32;
    let mut last = 0;
    for (at, width) in widths.iter().enumerate() {
        if width.is_nan() || *width > container {
            continue;
        }
        let is_last = at == widths.len() - 1;
        let reserve = if is_last { 0. } else { overflow + gap };
        if (used + width + reserve).round() >= container.round() {
            break;
        }
        last = at;
        used += width + gap;
    }
    last
}

/// Claiming a premium collectible (module 987421): `PUT /users/@me/claim-premium-collectibles-product {sku_id}`.
pub fn claim_premium(sku: &str) -> Write {
    Write {
        method: "PUT",
        route: "/users/@me/claim-premium-collectibles-product".to_owned(),
        body: Some(serde_json::json!({ "sku_id": sku })),
    }
}

/// How far the skeleton pulse is from base-lower (0) to base-low (1) at `now` ms for a card whose animation starts
/// `delay` ms late: `pulse_kIZjPZ` keyframes 0% / 30% / 70% / 100% over 2 s, `ease-in-out` in each step,
/// `alternate`, and the first frame held during the delay (`animation-fill-mode: backwards`).
pub fn pulse_phase(now: f64, delay: f64) -> f32 {
    const PERIOD: f64 = 2000.;
    let run = now - delay;
    if run <= 0. {
        return 0.;
    }
    let cycle = (run / PERIOD).floor() as u64;
    let mut local = ((run % PERIOD) / PERIOD) as f32;
    if cycle % 2 == 1 {
        local = 1. - local;
    }
    // `ease-in-out` is cubic-bezier(.42, 0, .58, 1); the smoothstep curve stands in for it.
    let ease = |x: f32| x * x * (3. - 2. * x);
    if local < 0.3 {
        ease(local / 0.3)
    } else if local < 0.7 {
        1.
    } else {
        1. - ease((local - 0.7) / 0.3)
    }
}

/// The skus a product batch asked for that its answer did not return.
pub fn absent_skus(asked: &[String], answered: &[Product]) -> Vec<String> {
    asked
        .iter()
        .filter(|sku| !answered.iter().any(|product| &product.sku_id == *sku))
        .cloned()
        .collect()
}

/// The card width and the gap of the shop's rows and grids (`min-width: 246px` of `productCardContainer_UGbKo5`,
/// `--space-xl` measured 24px in the capture).
pub const CARD: f32 = 246.;
pub const GAP: f32 = 24.;
/// `centeredSection_XBXDcd`: `max-width: 1080px; margin: 0 auto` inside the block's 32px side padding.
pub const SECTION_MAX: f32 = 1080.;
pub const BLOCK_SIDE: f32 = 32.;

/// The width of a block's centred section when the shop's scroller is `content` wide (its 8px gutter left out).
pub fn section_width(content: f32) -> f32 {
    (content - 2. * BLOCK_SIDE).clamp(0., SECTION_MAX)
}

/// The columns of `feed_XBXDcd` (`repeat(auto-fill, minmax(246px, 1fr))` with a 24px gap) in a section `width`
/// wide, and the width of each.
pub fn feed_columns(width: f32) -> (usize, f32) {
    let count = (((width + GAP) / (CARD + GAP)).floor() as usize).max(1);
    let column = ((width - GAP * (count - 1) as f32) / count as f32).max(0.);
    (count, column)
}

/// Where the picture goes for CSS `cover` in a `width` x `height` box: scaled to cover the box, placed by `anchor`
/// (the `background-position` / `object-position` as fractions). Left, top, width, height.
pub fn cover_rect(
    width: f32,
    height: f32,
    aspect: f32,
    anchor: (f32, f32),
) -> (f32, f32, f32, f32) {
    if width <= 0. || height <= 0. || aspect <= 0. {
        return (0., 0., width.max(0.), height.max(0.));
    }
    let (drawn_width, drawn_height) = if width / height > aspect {
        (width, width / aspect)
    } else {
        (height * aspect, height)
    };
    (
        (width - drawn_width) * anchor.0,
        (height - drawn_height) * anchor.1,
        drawn_width,
        drawn_height,
    )
}

/// The scroll a shelf arrow goes to (`scrollStep: "page"` of module 151869): the first card that is cut, or else the
/// next one, is brought to the start (right arrow) or the end (left arrow) of the row with 16px to spare. `count`
/// cards of `CARD` with `GAP` between them; their `offsetLeft` counts the scroller box's 4px padding.
pub fn shelf_page_target(
    right: bool,
    scroll: f32,
    client: f32,
    count: usize,
    max: f32,
) -> Option<f32> {
    if count == 0 {
        return None;
    }
    const PAD: f32 = 4.;
    const SPARE: f32 = 16.;
    let left_of = |index: usize| PAD + index as f32 * (CARD + GAP);
    let right_of = |index: usize| left_of(index) + CARD;
    let (low, high) = (scroll + SPARE, scroll + client - SPARE);
    let visible = |index: &usize| right_of(*index) > low && left_of(*index) < high;
    let (index, cut) = if right {
        match (0..count).rev().find(visible) {
            Some(index) => (index, right_of(index) > high + 1.),
            None => (count - 1, false),
        }
    } else {
        match (0..count).find(visible) {
            Some(index) => (index, left_of(index) < low - 1.),
            None => (0, false),
        }
    };
    let step: isize = if right { 1 } else { -1 };
    let clamp_index = |index: isize| index.clamp(0, count as isize - 1) as usize;
    let index = if cut {
        index
    } else {
        clamp_index(index as isize + step)
    };
    let target = |index: usize| {
        let at = if right {
            left_of(index) - SPARE
        } else {
            right_of(index) - client + SPARE
        };
        at.clamp(0., max.max(0.))
    };
    let at = target(index);
    if (at - scroll).abs() > 0.5 {
        return Some(at);
    }
    let next = clamp_index(index as isize + step);
    (next != index).then(|| target(next))
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_bundle_previews_the_selected_slide_as_its_own_product() {
        let bundle = Product {
            sku_id: "b".to_owned(),
            kind: KIND_BUNDLE,
            items: vec![
                Item {
                    kind: KIND_AVATAR_DECORATION,
                    sku_id: "d".to_owned(),
                    ..Item::default()
                },
                Item {
                    kind: KIND_PROFILE_EFFECT,
                    sku_id: "e".to_owned(),
                    ..Item::default()
                },
            ],
            ..Product::default()
        };
        let effect = bundle.slide(1);
        assert_eq!(
            (effect.kind, effect.sku_id.as_str()),
            (KIND_PROFILE_EFFECT, "e")
        );
        assert_eq!(effect.items.len(), 1);
        assert_eq!(bundle.slide(9).kind, KIND_BUNDLE);
        let single = Product {
            kind: KIND_NAMEPLATE,
            ..Product::default()
        };
        assert_eq!(single.slide(0).kind, KIND_NAMEPLATE);
        assert_eq!(item_type_label(KIND_PROFILE_FRAME), Some("Profile Frame"));
        assert_eq!(item_type_label(KIND_BUNDLE), None);
        assert!(is_fractional_nitro(NITRO_3_DAY) && !is_fractional_nitro(ORB_BADGE));
    }

    #[test]
    fn the_default_sort_follows_the_filters() {
        let mut filters = Filters::default();
        assert_eq!(filters.default_sort(), Sort::Recent);
        filters.item_types.push("NAMEPLATE".to_owned());
        assert_eq!(filters.default_sort(), Sort::Popular);
        filters.themes.push("COLLECTIBLES_THEME_ANIME".to_owned());
        assert_eq!(filters.default_sort(), Sort::Relevance);
        let search = Filters {
            search: " x ".to_owned(),
            ..Filters::default()
        };
        assert_eq!(search.default_sort(), Sort::Relevance);
    }

    use super::*;
    use serde_json::json;

    #[test]
    fn a_wishlist_is_read_again_when_its_stamp_changes() {
        let at = |stamp: &str| json!({"wishlist_settings": {"77": {"updated_at": stamp}}});
        let (id, first) = wishlist_read_guard(5, &at("2026-01-01T00:00:00Z")).unwrap();
        assert_eq!(id, "77");
        let (_, same) = wishlist_read_guard(5, &at("2026-01-01T00:00:00Z")).unwrap();
        let (_, changed) = wishlist_read_guard(5, &at("2026-02-01T00:00:00Z")).unwrap();
        assert_eq!(first, same);
        assert_ne!(first, changed);
        assert!(wishlist_read_guard(5, &json!({"wishlist_settings": {}})).is_none());
    }

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
    fn the_hero_keeps_its_display_configs_and_tiles_keep_their_type() {
        // The shapes of the live tab (design/parity/shop-dump-home.json).
        let home = parse_home(&json!({
            "shop_blocks":[
                {"type":0,"category_sku_id":"9","name":"N","summary":" ","ranked_sku_ids":["1"],
                 "hero_banner_url":"b","hero_banner_animated_url":"a","hero_logo_url":"l",
                 "banner_display_config":{"desktop_max_height":null,"responsive":true,
                    "background_style":"linear-gradient(180deg, #110101 0%, #BA2025 100%)"},
                 "logo_display_config":{"desktop_max_height":120,"responsive":true,"background_style":null}},
                {"type":1,"subblocks":[
                    {"type":0,"category_sku_id":"7","name":"Dead by Daylight","asset_url":"t","body_text":null},
                    {"type":5,"category_sku_id":"6","name":"Other"}
                ]}
            ]
        }));
        let Block::Hero(hero) = &home.blocks[0] else {
            panic!("no hero");
        };
        assert!(hero.responsive);
        assert_eq!(hero.logo_max_height, Some(120.));
        assert_eq!(hero.logo.as_deref(), Some("l"));
        assert_eq!(hero.summary, "");
        let Block::Featured(subs) = &home.blocks[1] else {
            panic!("no featured block");
        };
        assert_eq!((subs[0].kind, subs[0].asset.as_deref()), (0, Some("t")));
        assert_eq!(subs[1].kind, 5);
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
            "/shop/search?item_types=AVATAR_DECORATION&item_types=NAMEPLATE&colors=COLLECTIBLES_COLOR_PURPLE&themes=COLLECTIBLES_THEME_ANIME&orbs_eligible=true&is_first_party=false&offset=0&limit=24&sort_type=price&sort_direction=asc&search=dead%20pool&published_collections_only=true"
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
    fn claim_chain_follows_recorded_replies() {
        // Recorded shapes: the created order, the signed order, the entitlement list.
        let created = json!({"body":{"id":"900","status":1,"revision":4}});
        assert_eq!(
            claim_next("shop:order:create", &created, 0),
            ClaimNext::Sign {
                order: "900".to_owned(),
                revision: Some(4)
            }
        );
        let sign = order_sign("900", Some(4));
        assert_eq!(
            (sign.method, sign.route.as_str(), sign.body),
            (
                "POST",
                "/billing/orders/900/sign",
                Some(json!({"expected_revision":4}))
            )
        );
        let signed = json!({"body":{"id":"900","status":2}});
        assert_eq!(
            claim_next("shop:order:sign", &signed, 0),
            ClaimNext::Entitlements { attempt: 0 }
        );
        assert_eq!(
            order_entitlements_route("900"),
            "/billing/orders/900/entitlements"
        );
        let empty = json!({"body":[]});
        for reads in 0..ENTITLEMENT_DELAYS_MS.len() {
            assert_eq!(
                claim_next("shop:order:entitlements", &empty, reads),
                ClaimNext::Entitlements { attempt: reads + 1 }
            );
        }
        assert_eq!(
            claim_next(
                "shop:order:entitlements",
                &empty,
                ENTITLEMENT_DELAYS_MS.len()
            ),
            ClaimNext::Fail(ENTITLEMENTS_LATE)
        );
        let granted = json!({"body":[{"id":"1","consumed":true}]});
        assert_eq!(
            claim_next("shop:order:entitlements", &granted, 2),
            ClaimNext::Done { consumed: true }
        );
        let kept = json!({"body":[{"id":"1"}]});
        assert_eq!(
            claim_next("shop:order:entitlements", &kept, 0),
            ClaimNext::Done { consumed: false }
        );
    }

    #[test]
    fn claim_chain_failures_show_the_official_notices() {
        let poor =
            json!({"body":{"id":"9","status":1,"unsatisfied_constraints":[{"reason_code":1000}]}});
        assert_eq!(
            claim_next("shop:order:sign", &poor, 0),
            ClaimNext::Fail("Not enough Orbs.")
        );
        let owned =
            json!({"body":{"id":"9","status":1,"unsatisfied_constraints":[{"reason_code":1003}]}});
        assert_eq!(
            claim_next("shop:order:sign", &owned, 0),
            ClaimNext::Fail("Item already owned.")
        );
        let down = json!({"fetch_failed":true,"body":{}});
        assert_eq!(
            claim_next("shop:order:create", &down, 0),
            ClaimNext::Fail(GENERIC_CLAIM_ERROR)
        );
        assert_eq!(
            claim_next("shop:order:sign", &down, 0),
            ClaimNext::Fail(GENERIC_CLAIM_ERROR)
        );
        let refused = json!({"refused":true});
        assert_eq!(
            claim_next("shop:order:create", &refused, 0),
            ClaimNext::Refused
        );
        assert_eq!(claim_next("shop:balance", &json!({}), 0), ClaimNext::Ignore);
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

    #[test]
    fn sign_answers_become_the_official_notices() {
        use serde_json::json;
        assert_eq!(sign_failure_text(&json!({"id":"1","status":2})), None);
        let refused = |reason: i64| json!({"id":"1","status":1,"unsatisfied_constraints":[{"reason_code":reason}]});
        assert_eq!(sign_failure_text(&refused(1000)), Some("Not enough Orbs."));
        assert_eq!(
            sign_failure_text(&refused(1003)),
            Some("Item already owned.")
        );
        assert_eq!(
            sign_failure_text(&refused(1010)),
            Some("You already own one or more items in this bundle.")
        );
        assert_eq!(
            sign_failure_text(&refused(1005)),
            Some("There was an error claiming this item. Please try again later.")
        );
        assert_eq!(
            sign_failure_text(&json!({"code":590001,"message":"x"})),
            Some("Not enough Orbs.")
        );
        assert_eq!(
            sign_failure_text(&json!({"code":100096})),
            Some("Item already owned.")
        );
        assert_eq!(
            sign_failure_text(&json!({"code":100152})),
            Some("Your checkout session expired. Reopen checkout to continue.")
        );
        assert_eq!(
            sign_failure_text(
                &json!({"id":"1","status":5,"error":{"code":1,"billing_error_code":100011}})
            ),
            Some("Item already owned.")
        );
        assert_eq!(
            sign_failure_text(&json!(null)),
            Some("There was an error claiming this item. Please try again later.")
        );
    }

    fn sample(sku: &str, name: &str, kind: i64) -> Product {
        Product {
            sku_id: sku.to_owned(),
            name: name.to_owned(),
            kind,
            ..Product::default()
        }
    }

    #[test]
    fn the_collected_modal_says_what_the_official_one_says() {
        use CollectedAction::*;
        let badge = collected_copy(
            &sample(ORB_BADGE, "Orbs Apprentice Badge", KIND_AVATAR_DECORATION),
            true,
        );
        assert_eq!(badge.title, "You Collected the Orbs Profile Badge!");
        assert_eq!(
            badge.buttons,
            vec![
                ("Earn More Orbs", false, EarnMoreOrbs),
                ("View My Badge", true, ViewBadge)
            ]
        );
        let credit = collected_copy(
            &sample(NITRO_3_DAY, "3-Day Nitro Credit", KIND_BUNDLE),
            false,
        );
        assert_eq!(credit.title, "Nitro Credit Acquired");
        assert!(
            credit
                .subtitle
                .starts_with("Your 3-day Nitro credit will be used")
        );
        assert_eq!(
            credit.buttons,
            vec![("Close", false, Close), ("View Credits", true, ViewCredits)]
        );
        let used = collected_copy(&sample(NITRO_1_DAY, "x", KIND_BUNDLE), true);
        assert_eq!(used.title, "Nitro Activated");
        assert_eq!(
            used.buttons,
            vec![
                ("Close", false, Close),
                ("Explore Perks", true, ExplorePerks)
            ]
        );
        let deco = collected_copy(&sample("9", "Deadpool", KIND_AVATAR_DECORATION), false);
        assert_eq!(deco.title, "You did it! Put it on!");
        assert_eq!(deco.subtitle, "Deadpool has been added to your collection.");
        assert_eq!(
            deco.buttons,
            vec![
                ("Edit User Profile", false, EditProfile),
                ("Use Now", true, UseNow)
            ]
        );
    }

    #[test]
    fn use_now_needs_one_item_of_each_type() {
        let item = |kind: i64, sku: &str| Item {
            kind,
            sku_id: sku.to_owned(),
            ..Item::default()
        };
        let mut bundle = sample("5", "Pair", KIND_BUNDLE);
        bundle.items = vec![
            item(KIND_AVATAR_DECORATION, "11"),
            item(KIND_NAMEPLATE, "12"),
        ];
        assert!(can_use_now(&bundle));
        bundle.items.push(item(KIND_NAMEPLATE, "13"));
        assert!(!can_use_now(&bundle));
        // A refused bundle offers only the editor, as the primary button.
        assert_eq!(
            collected_copy(&bundle, false).buttons,
            vec![("Edit User Profile", true, CollectedAction::EditProfile)]
        );
        assert!(!can_use_now(&sample(
            ORB_BADGE,
            "badge",
            KIND_AVATAR_DECORATION
        )));
    }

    #[test]
    fn use_now_writes_the_first_item_of_each_type() {
        let item = |kind: i64, sku: &str| Item {
            kind,
            sku_id: sku.to_owned(),
            ..Item::default()
        };
        let mut bundle = sample("5", "Pair", KIND_BUNDLE);
        bundle.items = vec![
            item(KIND_AVATAR_DECORATION, "11"),
            item(KIND_PROFILE_EFFECT, "14"),
            item(KIND_PROFILE_EFFECT, "15"),
        ];
        let writes = use_now_writes(&bundle);
        assert_eq!(writes.len(), 2);
        assert_eq!(writes[0].route, "/users/@me/profile");
        assert_eq!(
            writes[0].body["collectibles_sku_ids"],
            serde_json::json!(["14"])
        );
        assert_eq!(writes[1].route, "/users/@me");
        assert_eq!(writes[1].body["avatar_decoration_sku_id"], "11");
        assert_eq!(
            use_now_toast(&sample("1", "x", KIND_NAMEPLATE)),
            "Your Nameplate is updated!"
        );
        assert_eq!(use_now_toast(&bundle), "Your profile is updated!");
    }

    #[test]
    fn bundle_summary_names_the_parts_like_the_official_one() {
        let part = |kind: i64, name: &str| sample("1", name, kind);
        let mut bundle = sample("9", "Pair", KIND_BUNDLE);
        bundle.summary = "{joinedItems}".to_owned();
        bundle.bundled = vec![
            part(KIND_AVATAR_DECORATION, "Deadpool"),
            part(KIND_PROFILE_EFFECT, "Hearts"),
            part(KIND_PROFILE_FRAME, "Frame"),
        ];
        assert_eq!(
            product_summary(&bundle),
            "Bundle includes: Deadpool Decoration & Hearts Profile Effect"
        );
        bundle.bundled.push(part(KIND_NAMEPLATE, "Plate"));
        assert_eq!(
            product_summary(&bundle),
            "Bundle includes: Deadpool Decoration, Hearts Profile Effect & Plate Nameplate"
        );
        // The server's own line wins when it has no placeholder; a missing one falls back to the type's line.
        bundle.summary = "Two for one".to_owned();
        assert_eq!(product_summary(&bundle), "Two for one");
        let mut deco = sample("2", "x", KIND_AVATAR_DECORATION);
        assert_eq!(product_summary(&deco), "Give your avatar a new look.");
        deco.kind = KIND_NAMEPLATE;
        assert_eq!(
            product_summary(&deco),
            "Make your name stand out in servers and chats."
        );
    }

    #[test]
    fn the_share_link_and_the_preview_badges_follow_the_official_rules() {
        assert_eq!(
            share_link("12", false),
            "https://discord.com/shop#itemSkuId=12"
        );
        assert_eq!(
            share_link("12", true),
            "https://discord.com/shop?tab=orbs#itemSkuId=12"
        );
        let profile = serde_json::json!({"badges":[{"id":"a","icon":"73d2"},{"id":"b"}]});
        assert_eq!(
            profile_badge_urls(&profile),
            vec!["https://cdn.discordapp.com/badge-icons/73d2.png".to_owned()]
        );
        assert!(profile_badge_urls(&serde_json::Value::Null).is_empty());
    }
    #[test]
    fn the_header_reads_storefronts_applications_and_wishlists() {
        use serde_json::json;
        let config = json!({"storefronts":[
            {"application_id":"1","collectibles_shop_navigation_enabled":true},
            {"application_id":"2","collectibles_shop_navigation_enabled":false},
            {"application_id":"3","collectibles_shop_navigation_enabled":true}]});
        let ids = storefront_nav_apps(&config);
        assert_eq!(ids, vec!["1".to_owned(), "3".to_owned()]);
        assert_eq!(
            applications_route(&ids),
            "/applications/public?application_ids=1&application_ids=3"
        );
        let apps = json!([{"id":"3","name":"Rust","icon":"abc"},{"id":"1","name":"Marvel Rivals","icon":null}]);
        let shops = parse_game_shops(&ids, &apps);
        assert_eq!(shops[0].name, "Marvel Rivals");
        assert_eq!(shops[0].icon, None);
        assert_eq!(
            shops[1].icon.as_deref(),
            Some("https://cdn.discordapp.com/app-icons/3/abc.png?size=32")
        );
        let profile = json!({"wishlist_settings":{"77":{"visibility":1}}});
        assert_eq!(wishlist_ids(&profile), vec!["77".to_owned()]);
        assert_eq!(wishlist_route("77"), "/wishlists/77?source=user_profile");
    }

    #[test]
    fn the_nav_overflows_like_the_official_row() {
        // The 1280 window: Featured, Browse, Orbs Shop, Game Shops and Game Servers in a 513.5 row, the overflow button
        // 58.09 wide. Game Servers moves into More.
        let widths = [62.17, 72.95, 71.97, 108.23, 148.70];
        assert_eq!(last_visible(&widths, 58.09, 32., 513.5), 3);
        // A wider row holds all five.
        assert_eq!(last_visible(&widths, 58.09, 32., 700.), 4);
        // Without Game Shops the rest fits.
        let without = [62.17, 72.95, 71.97, 148.70];
        assert_eq!(last_visible(&without, 58.09, 32., 513.5), 3);
    }

    #[test]
    fn sections_are_centred_at_1080_and_feeds_fill_their_columns() {
        // 1280 window: a 897px scroller content (905 less the gutter), an 833px section, three 261.66px columns.
        assert_eq!(section_width(897.), 833.);
        let (count, column) = feed_columns(833.);
        assert_eq!(count, 3);
        assert!((column - 261.66).abs() < 0.01);
        // Wider windows stop at 1080 with four columns of 252.
        assert_eq!(section_width(1617.), 1080.);
        assert_eq!(feed_columns(1080.), (4, 252.));
        assert_eq!(feed_columns(100.).0, 1);
    }

    #[test]
    fn cover_keeps_the_anchored_edge() {
        // A 2:1 picture in a 404.5 x 236 tile anchored left: the full height, overflowing to the right only.
        let (left, top, width, height) = cover_rect(404.5, 236., 2., (0., 0.5));
        assert_eq!((left, top, height), (0., 0., 236.));
        assert_eq!(width, 472.);
        // Centred, the same picture loses as much on the left as on the right.
        let (left, ..) = cover_rect(404.5, 236., 2., (0.5, 0.5));
        assert!((left + 33.75).abs() < 0.01);
        // A tall picture anchored to the top keeps its top edge.
        let (_, top, _, height) = cover_rect(100., 100., 0.5, (0.5, 0.));
        assert_eq!((top, height), (0., 200.));
    }

    #[test]
    fn shelf_arrows_page_like_the_official_row() {
        // 14 cards in an 833px row (1280 window): the fourth card is cut, so the right arrow brings it to the start.
        let max = 14. * 246. + 13. * 24. + 8. - 833.;
        assert_eq!(shelf_page_target(true, 0., 833., 14, max), Some(798.));
        // From there the left arrow brings the card cut at the left to the end: back to the start.
        assert_eq!(shelf_page_target(false, 798., 833., 14, max), Some(0.));
        // At the end the right arrow has nothing further to show.
        assert_eq!(shelf_page_target(true, max, 833., 14, max), None);
        assert_eq!(shelf_page_target(true, 0., 833., 0, 0.), None);
    }

    #[test]
    fn premium_type_zero_is_no_nitro_tier() {
        let product = parse_product(&json!({"sku_id":"1","type":0,"premium_type":0})).unwrap();
        assert_eq!(product.premium_type, None);
        let product = parse_product(&json!({"sku_id":"1","type":0,"premium_type":2})).unwrap();
        assert_eq!(product.premium_type, Some(2));
    }

    #[test]
    fn absent_skus_are_the_ones_left_out_of_the_answer() {
        let asked = vec!["1".to_owned(), "2".to_owned()];
        let answered = vec![sample("2", "B", KIND_AVATAR_DECORATION)];
        assert_eq!(absent_skus(&asked, &answered), vec!["1".to_owned()]);
    }

    #[test]
    fn the_skeleton_pulse_holds_low_in_the_middle_and_alternates() {
        assert_eq!(pulse_phase(100., 400.), 0.);
        assert_eq!(pulse_phase(1000., 0.), 1.);
        assert_eq!(pulse_phase(0., 0.), 0.);
        assert_eq!(pulse_phase(2000., 0.), 0.);
        assert!(pulse_phase(0.0001, 0.) < 1e-10);
        // The second run plays backwards: 0.2 s into it is 0.2 s before the end of the first.
        assert!((pulse_phase(2200., 0.) - pulse_phase(1800., 0.)).abs() < 1e-4);
    }

    #[test]
    fn wishlist_embedded_skus_and_profile_effects_are_extracted_without_stub_shadowing() {
        let body = json!({
            "wishlist_items": [
                {
                    "sku_id": "100",
                    "sku": {"id": "100"}
                },
                {
                    "sku_id": "200",
                    "sku": {
                        "id": "200",
                        "name": "Galaxy Effect",
                        "description": "Stars",
                        "prices": {},
                        "tenant_metadata": {
                            "collectibles": {
                                "type": 1,
                                "premium_type": 0,
                                "item": {
                                    "type": 1,
                                    "sku_id": "201",
                                    "title": "Galaxy Effect",
                                    "thumbnailPreviewSrc": "https://cdn.discordapp.com/thumb.png",
                                    "effects": [{"src": "https://cdn.discordapp.com/layer.png", "zIndex": 1}]
                                }
                            }
                        }
                    }
                }
            ]
        });
        let products = parse_wishlist_products(&body);
        assert_eq!(products.len(), 1);
        assert_eq!(products[0].sku_id, "200");
        assert_eq!(products[0].kind, KIND_PROFILE_EFFECT);
        let effects = parse_profile_effects(&body);
        assert_eq!(effects.len(), 1);
        assert_eq!(effects[0].0, 200);
        assert_eq!(effects[0].1.sku_id, 201);
        assert_eq!(effects[0].1.layers.len(), 1);
    }
}
