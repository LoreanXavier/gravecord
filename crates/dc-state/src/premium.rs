//! Nitro: the account's tier, the subscription plans and prices the API returns, and how the client formats them.
//!
//! Constants come from the pinned build's premium constants module (`design/snapshot/modules/682901.js`:
//! `PremiumTypes`, the `SKU` map, the plan ids). Price display is `465323.js` (`$g`: minor units over `10^exponent`,
//! then `Intl.NumberFormat(locale, {style: "currency"})`) and the plan price lookup is `722178.js`
//! (`paymentSourcePrices[source]` first, else `countryPrices.prices`; the first entry).

use serde_json::Value;
use std::collections::BTreeMap;

/// `PremiumTypes` plus none. The wire numbers are not in tier order: Nitro Basic is 3, Classic is 1, Nitro is 2.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PremiumType {
    None,
    /// `TIER_1`, "Nitro Classic" (no longer sold).
    Classic,
    /// `TIER_2`, "Nitro".
    Nitro,
    /// `TIER_0`, "Nitro Basic".
    Basic,
}

impl PremiumType {
    pub fn from_wire(value: Option<u64>) -> Self {
        match value {
            Some(1) => Self::Classic,
            Some(2) => Self::Nitro,
            Some(3) => Self::Basic,
            _ => Self::None,
        }
    }

    pub fn wire(self) -> Option<u64> {
        match self {
            Self::None => None,
            Self::Classic => Some(1),
            Self::Nitro => Some(2),
            Self::Basic => Some(3),
        }
    }

    pub fn is_subscriber(self) -> bool {
        self != Self::None
    }
}

/// `SKU` in the premium constants module.
pub mod sku {
    pub const TIER_0: u64 = 978380684370378762;
    pub const TIER_1: u64 = 521846918637420545;
    pub const TIER_2: u64 = 521847234246082599;
    pub const GUILD: u64 = 590663762298667008;
    pub const LEGACY: u64 = 521842865731534868;
}

/// The plan ids of the premium constants module (`F`).
pub mod plan_id {
    pub const MONTH_TIER_0: u64 = 978380692553465866;
    pub const YEAR_TIER_0: u64 = 1024422698568122368;
    pub const MONTH_TIER_1: u64 = 511651871736201216;
    pub const YEAR_TIER_1: u64 = 511651876987469824;
    pub const MONTH_TIER_2: u64 = 511651880837840896;
    pub const YEAR_TIER_2: u64 = 511651885459963904;
    pub const THREE_MONTH_TIER_2: u64 = 642251038925127690;
    pub const SIX_MONTH_TIER_2: u64 = 944037208325619722;
    pub const MONTH_GUILD: u64 = 590665532894740483;
    pub const YEAR_GUILD: u64 = 590665538238152709;
    pub const THREE_MONTH_GUILD: u64 = 944037355453415424;
    pub const SIX_MONTH_GUILD: u64 = 944037391444738048;
}

/// `PremiumTypes` of an SKU (`k` in the constants module).
pub fn type_of_sku(sku: u64) -> PremiumType {
    match sku {
        sku::TIER_0 => PremiumType::Basic,
        sku::TIER_1 => PremiumType::Classic,
        sku::TIER_2 | sku::LEGACY => PremiumType::Nitro,
        _ => PremiumType::None,
    }
}

/// `SubscriptionPlanInterval`: 1 month, 2 year, 3 day.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Interval {
    Month,
    Year,
    Day,
}

/// One price of a plan: `amount` in minor units of `currency`, `exponent` decimals.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Price {
    pub amount: i64,
    pub currency: String,
    pub exponent: u32,
}

impl Price {
    fn from_json(value: &Value) -> Option<Self> {
        Some(Self {
            amount: value["amount"].as_i64()?,
            currency: value["currency"].as_str()?.to_owned(),
            exponent: value["exponent"].as_u64().unwrap_or(2) as u32,
        })
    }

    /// `Intl.NumberFormat("en-US", {style: "currency"})` of the amount in major units.
    pub fn display(&self) -> String {
        format_currency(self.amount, self.exponent, &self.currency)
    }

    /// The same amount spread over twelve months (the "billed annually" line), rounded to a minor unit.
    pub fn per_month_of_year(&self) -> Price {
        Price {
            amount: (self.amount as f64 / 12.0).round() as i64,
            ..self.clone()
        }
    }
}

/// The prices of one purchase type (0 is the default): the country's, and per saved payment source.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PurchasePrices {
    pub country_code: String,
    pub country: Vec<Price>,
    pub sources: BTreeMap<String, Vec<Price>>,
}

/// `SubscriptionPlan` (`832946.js`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Plan {
    pub id: u64,
    pub name: String,
    pub interval: Interval,
    pub interval_count: u32,
    pub tax_inclusive: bool,
    pub sku_id: u64,
    pub currency: String,
    pub price: i64,
    pub prices: BTreeMap<u32, PurchasePrices>,
}

impl Plan {
    pub fn from_json(value: &Value) -> Option<Self> {
        let interval = match value["interval"].as_u64()? {
            1 => Interval::Month,
            2 => Interval::Year,
            3 => Interval::Day,
            _ => return None,
        };
        let mut prices = BTreeMap::new();
        if let Some(map) = value["prices"].as_object() {
            for (kind, entry) in map {
                let country = entry["country_prices"]["prices"]
                    .as_array()
                    .map(|list| list.iter().filter_map(Price::from_json).collect())
                    .unwrap_or_default();
                let sources = entry["payment_source_prices"]
                    .as_object()
                    .map(|map| {
                        map.iter()
                            .map(|(source, list)| {
                                let list = list
                                    .as_array()
                                    .map(|list| list.iter().filter_map(Price::from_json).collect())
                                    .unwrap_or_default();
                                (source.clone(), list)
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                prices.insert(
                    kind.parse().ok()?,
                    PurchasePrices {
                        country_code: entry["country_prices"]["country_code"]
                            .as_str()
                            .unwrap_or_default()
                            .to_owned(),
                        country,
                        sources,
                    },
                );
            }
        }
        Some(Self {
            id: value["id"].as_str()?.parse().ok()?,
            name: value["name"].as_str()?.to_owned(),
            interval,
            interval_count: value["interval_count"].as_u64()? as u32,
            tax_inclusive: value["tax_inclusive"].as_bool().unwrap_or(false),
            sku_id: value["sku_id"].as_str()?.parse().ok()?,
            currency: value["currency"].as_str()?.to_owned(),
            price: value["price"].as_i64()?,
            prices,
        })
    }

    /// The price shown for a purchase type (0 default) and an optional saved payment source: the source's own prices
    /// when it has any, else the country's; the first entry (`722178.js`, `269249.js`).
    pub fn price(&self, purchase_type: u32, source: Option<&str>) -> Option<Price> {
        let entry = self.prices.get(&purchase_type)?;
        if let Some(list) = source.and_then(|source| entry.sources.get(source))
            && let Some(first) = list.first()
        {
            return Some(first.clone());
        }
        entry.country.first().cloned()
    }

    pub fn is_monthly(&self) -> bool {
        self.interval == Interval::Month && self.interval_count == 1
    }

    pub fn is_yearly(&self) -> bool {
        self.interval == Interval::Year && self.interval_count == 1
    }
}

/// A list of plans from `GET /store/published-listings/skus/{sku}/subscription-plans`.
pub fn parse_plans(body: &Value) -> Vec<Plan> {
    body.as_array()
        .map(|list| list.iter().filter_map(Plan::from_json).collect())
        .unwrap_or_default()
}

/// The monthly or yearly plan of an SKU among parsed plans.
pub fn find_plan(plans: &[Plan], sku: u64, yearly: bool) -> Option<&Plan> {
    plans.iter().find(|plan| {
        plan.sku_id == sku
            && if yearly {
                plan.is_yearly()
            } else {
                plan.is_monthly()
            }
    })
}

/// Decimal digits `Intl` uses by default for a currency (ISO 4217 minor units, as ICU has them).
fn default_digits(currency: &str) -> usize {
    match currency {
        "BIF" | "CLP" | "DJF" | "GNF" | "ISK" | "JPY" | "KMF" | "KRW" | "PYG" | "RWF" | "UGX"
        | "UYI" | "VND" | "VUV" | "XAF" | "XOF" | "XPF" => 0,
        "BHD" | "IQD" | "JOD" | "KWD" | "LYD" | "OMR" | "TND" => 3,
        _ => 2,
    }
}

/// The en-US narrow-or-standard symbol ICU prints for a currency; none means the ISO code is printed instead.
fn symbol(currency: &str) -> Option<&'static str> {
    Some(match currency {
        "USD" => "$",
        "EUR" => "€",
        "GBP" => "£",
        "JPY" => "¥",
        "INR" => "₹",
        "KRW" => "₩",
        "CNY" => "CN¥",
        "AUD" => "A$",
        "CAD" => "CA$",
        "BRL" => "R$",
        "MXN" => "MX$",
        "NZD" => "NZ$",
        "HKD" => "HK$",
        "TWD" => "NT$",
        "ILS" => "₪",
        "VND" => "₫",
        "PHP" => "₱",
        "XAF" => "FCFA",
        "XOF" => "F\u{202f}CFA",
        "XPF" => "CFPF",
        "XCD" => "EC$",
        _ => return None,
    })
}

fn group_thousands(digits: &str) -> String {
    let mut out = String::new();
    for (index, ch) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            out.push(',');
        }
        out.push(ch);
    }
    out
}

/// `$g(amount, currency)` for en-US: `$9.99`, `TRY 37.99` (a no-break space after a code), `¥1,000`.
pub fn format_currency(amount: i64, exponent: u32, currency: &str) -> String {
    let currency = &currency.to_ascii_uppercase();
    let negative = amount < 0;
    let magnitude = amount.unsigned_abs() as u128;
    let digits = default_digits(currency);
    let scale = 10u128.pow(exponent);
    // Round the major-unit value to the currency's digits, half away from zero, in integers.
    let target = 10u128.pow(digits as u32);
    let scaled = magnitude * target;
    let mut whole = scaled / scale;
    if scaled % scale * 2 >= scale {
        whole += 1;
    }
    let (major, minor) = (whole / target, whole % target);
    let mut number = group_thousands(&major.to_string());
    if digits > 0 {
        number.push('.');
        number.push_str(&format!("{minor:0digits$}"));
    }
    let sign = if negative { "-" } else { "" };
    match symbol(currency) {
        Some(symbol) => format!("{sign}{symbol}{number}"),
        None => format!("{sign}{currency}\u{a0}{number}"),
    }
}

/// "/ Month", "/ Year" and "every N months" (`CE` in `580630.js`; strings `AbOLNu`, `rS8FA+`, `Qc+9ww`).
pub fn price_with_interval(price: &str, interval: Interval, count: u32) -> Option<String> {
    match (interval, count) {
        (Interval::Year, _) => Some(format!("{price} / Year")),
        (Interval::Month, 1) => Some(format!("{price} / Month")),
        (Interval::Month, count) if count > 1 => Some(format!("{price} every {count} months")),
        _ => None,
    }
}

/// The perks the client gates on (`PremiumFeatures`, module 76573), as named in the tier table of module 722178.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Feature {
    AnimatedEmojis,
    EmojisEverywhere,
    StickersEverywhere,
    SoundboardEverywhere,
    CustomCallSounds,
    AnimatedAvatar,
    CustomDiscriminator,
    PremiumGuildMemberProfile,
    ProfileBadges,
    ProfilePremiumFeatures,
    ClientThemes,
    BoostDiscount,
    FreeBoosts,
    StreamMidQuality,
    StreamHighQuality,
    InstallPremiumApplications,
    VideoFilterAssets,
    IncreasedGuildLimit,
    IncreasedMessageLength,
    IncreasedVideoUploadQuality,
    IncreasedFileUploadSize,
    AppIcons,
    PremiumCollectibles,
}

impl PremiumType {
    /// `O(feature, user)` of module 722178: the perks of each tier, read from its table.
    pub fn has(self, feature: Feature) -> bool {
        use Feature::*;
        match self {
            PremiumType::None => false,
            PremiumType::Basic => matches!(
                feature,
                AnimatedEmojis
                    | EmojisEverywhere
                    | StickersEverywhere
                    | ProfileBadges
                    | VideoFilterAssets
                    | IncreasedVideoUploadQuality
                    | IncreasedFileUploadSize
                    | AppIcons
            ),
            PremiumType::Classic => matches!(
                feature,
                AnimatedEmojis
                    | EmojisEverywhere
                    | AnimatedAvatar
                    | CustomDiscriminator
                    | ProfileBadges
                    | BoostDiscount
                    | StreamMidQuality
                    | IncreasedVideoUploadQuality
                    | IncreasedFileUploadSize
                    | AppIcons
            ),
            PremiumType::Nitro => true,
        }
    }

    /// The longest message: 4000 characters with `INCREASED_MESSAGE_LENGTH`, else 2000.
    pub fn max_message_length(self) -> usize {
        if self.has(Feature::IncreasedMessageLength) {
            4000
        } else {
            2000
        }
    }

    /// The largest upload in bytes (`D` of 722178 and `g.f5`): 10 MiB without Nitro, 50 MiB for Basic and Classic,
    /// and for Nitro 1 GiB when the file upload rollout is on (the marketing copy "1GB uploads"), else 500 MiB.
    pub fn max_upload_bytes(self, rollout: bool) -> u64 {
        match self {
            PremiumType::None => 10 * 1024 * 1024,
            PremiumType::Basic | PremiumType::Classic => 50 * 1024 * 1024,
            PremiumType::Nitro if rollout => 1024 * 1024 * 1024,
            PremiumType::Nitro => 500 * 1024 * 1024,
        }
    }
}

/// The "TRY 209 OFF" badge of the yearly option (`LQ`): twelve months at the monthly price minus the yearly price,
/// floored to whole major units and shown without decimals. `None` when the yearly plan saves nothing.
pub fn yearly_savings(monthly: &Price, yearly: &Price) -> Option<String> {
    if monthly.currency != yearly.currency || monthly.exponent != yearly.exponent {
        return None;
    }
    let scale = 10i64.pow(monthly.exponent);
    let saved = (monthly.amount * 12 - yearly.amount).div_euclid(scale);
    if saved <= 0 {
        return None;
    }
    let shown = format_currency(saved * scale, monthly.exponent, &monthly.currency);
    Some(match shown.rfind('.') {
        Some(dot) if shown[dot + 1..].chars().all(|c| c == '0') => shown[..dot].to_owned(),
        _ => shown,
    })
}

/// The body of `POST /users/@me/billing/subscriptions` (module 781300), field for field. Built and unit tested
/// only: no code path in the app sends it.
pub struct SubscribeRequest<'a> {
    pub plan_id: u64,
    pub payment_source_id: Option<&'a str>,
    pub currency: &'a str,
    pub expected_invoice_price: Option<&'a Price>,
    pub expected_renewal_price: Option<&'a Price>,
    pub load_id: Option<&'a str>,
}

impl SubscribeRequest<'_> {
    pub fn body(&self) -> Value {
        let price = |price: Option<&Price>| {
            price.map_or(Value::Null, |price| {
                serde_json::json!({
                    "amount": price.amount,
                    "currency": price.currency,
                })
            })
        };
        serde_json::json!({
            "items": [{"plan_id": self.plan_id.to_string(), "quantity": 1}],
            "payment_source_id": self.payment_source_id,
            "payment_source_token": null,
            "trial_id": null,
            "return_url": null,
            "code": null,
            "currency": self.currency,
            "metadata": null,
            "gateway_checkout_context": null,
            "purchase_token": null,
            "referral_code": null,
            "load_id": self.load_id,
            "gift_info_options": null,
            "expected_invoice_price": price(self.expected_invoice_price),
            "expected_renewal_price": price(self.expected_renewal_price),
        })
    }
}

/// The page "Pay Here" opens in the browser (module 36167 and the route table of 901123): the billing login
/// handoff, which redirects to the standalone checkout of the chosen plan once the handoff token is attached.
pub fn checkout_handoff_url(
    plan_id: u64,
    gift: bool,
    load_id: &str,
    key: &str,
    token: &str,
) -> String {
    let redirect =
        format!("/billing/premium/subscribe?plan_id={plan_id}&gift={gift}&load_id={load_id}");
    let encode = |text: &str| {
        text.bytes()
            .map(|byte| match byte {
                b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'*' => {
                    (byte as char).to_string()
                }
                b' ' => "+".to_owned(),
                _ => format!("%{byte:02X}"),
            })
            .collect::<String>()
    };
    format!(
        "https://discord.com/billing/login/handoff?handoff_key={}&redirect_to={}&handoff_token={}",
        encode(key),
        encode(&redirect),
        encode(token)
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn gates_follow_the_tier_table() {
        use Feature::*;
        let nitro = PremiumType::Nitro;
        let all = [
            AnimatedEmojis,
            EmojisEverywhere,
            StickersEverywhere,
            SoundboardEverywhere,
            CustomCallSounds,
            AnimatedAvatar,
            CustomDiscriminator,
            PremiumGuildMemberProfile,
            ProfileBadges,
            ProfilePremiumFeatures,
            ClientThemes,
            BoostDiscount,
            FreeBoosts,
            StreamMidQuality,
            StreamHighQuality,
            InstallPremiumApplications,
            VideoFilterAssets,
            IncreasedGuildLimit,
            IncreasedMessageLength,
            IncreasedVideoUploadQuality,
            IncreasedFileUploadSize,
            AppIcons,
            PremiumCollectibles,
        ];
        assert!(all.iter().all(|f| nitro.has(*f)));
        assert!(all.iter().all(|f| !PremiumType::None.has(*f)));
        // Basic: emoji and stickers everywhere, no soundboard, no themes, no HD or mid streaming.
        assert!(PremiumType::Basic.has(StickersEverywhere));
        assert!(!PremiumType::Basic.has(SoundboardEverywhere));
        assert!(!PremiumType::Basic.has(StreamMidQuality));
        assert!(!PremiumType::Basic.has(IncreasedMessageLength));
        // Classic streams mid quality but has no stickers everywhere and no longer messages.
        assert!(PremiumType::Classic.has(StreamMidQuality));
        assert!(!PremiumType::Classic.has(StreamHighQuality));
        assert!(!PremiumType::Classic.has(StickersEverywhere));
        assert!(PremiumType::Classic.has(AnimatedAvatar));
        assert!(!PremiumType::Basic.has(AnimatedAvatar));
    }

    #[test]
    fn message_and_upload_limits_by_tier() {
        assert_eq!(PremiumType::None.max_message_length(), 2000);
        assert_eq!(PremiumType::Basic.max_message_length(), 2000);
        assert_eq!(PremiumType::Classic.max_message_length(), 2000);
        assert_eq!(PremiumType::Nitro.max_message_length(), 4000);
        assert_eq!(PremiumType::None.max_upload_bytes(true), 10_485_760);
        assert_eq!(PremiumType::Basic.max_upload_bytes(true), 52_428_800);
        assert_eq!(PremiumType::Nitro.max_upload_bytes(true), 1_073_741_824);
        assert_eq!(PremiumType::Nitro.max_upload_bytes(false), 524_288_000);
    }

    #[test]
    fn savings_badge_floors_to_whole_units() {
        let monthly = Price {
            amount: 10499,
            currency: "try".into(),
            exponent: 2,
        };
        let yearly = Price {
            amount: 104999,
            currency: "try".into(),
            exponent: 2,
        };
        assert_eq!(
            yearly_savings(&monthly, &yearly).as_deref(),
            Some("TRY\u{a0}209")
        );
        let usd_monthly = Price {
            amount: 999,
            currency: "usd".into(),
            exponent: 2,
        };
        let usd_yearly = Price {
            amount: 9999,
            currency: "usd".into(),
            exponent: 2,
        };
        assert_eq!(
            yearly_savings(&usd_monthly, &usd_yearly).as_deref(),
            Some("$19")
        );
        assert_eq!(
            yearly_savings(
                &usd_monthly,
                &Price {
                    amount: 20000,
                    ..usd_yearly
                }
            ),
            None
        );
    }

    #[test]
    fn subscribe_body_has_the_official_fields() {
        let invoice = Price {
            amount: 104999,
            currency: "try".into(),
            exponent: 2,
        };
        let body = SubscribeRequest {
            plan_id: plan_id::YEAR_TIER_2,
            payment_source_id: Some("42"),
            currency: "try",
            expected_invoice_price: Some(&invoice),
            expected_renewal_price: Some(&invoice),
            load_id: Some("load"),
        }
        .body();
        assert_eq!(body["items"][0]["plan_id"], "511651885459963904");
        assert_eq!(body["items"][0]["quantity"], 1);
        assert_eq!(body["payment_source_id"], "42");
        assert_eq!(body["expected_invoice_price"]["amount"], 104999);
        for field in [
            "payment_source_token",
            "trial_id",
            "return_url",
            "code",
            "metadata",
            "gateway_checkout_context",
            "purchase_token",
            "referral_code",
            "gift_info_options",
        ] {
            assert!(body.get(field).is_some_and(Value::is_null), "{field}");
        }
    }

    #[test]
    fn handoff_url_encodes_the_redirect() {
        let url = checkout_handoff_url(511651885459963904, false, "abc", "k 1", "t/2");
        assert_eq!(
            url,
            "https://discord.com/billing/login/handoff?handoff_key=k+1&redirect_to=%2Fbilling%2Fpremium%2Fsubscribe%3Fplan_id%3D511651885459963904%26gift%3Dfalse%26load_id%3Dabc&handoff_token=t%2F2"
        );
    }

    fn basic_monthly() -> Value {
        json!({"id":"978380692553465866","name":"Nitro Basic Monthly","interval":1,"interval_count":1,
            "tax_inclusive":true,"sku_id":"978380684370378762","price":3799,"currency":"try","price_tier":null,
            "prices":{"0":{"country_prices":{"country_code":"TR","prices":[
                {"currency":"try","amount":3799,"exponent":2},{"currency":"usd","amount":138,"exponent":2}]},
                "payment_source_prices":{"77":[{"currency":"usd","amount":299,"exponent":2}]}},
              "1":{"country_prices":{"country_code":"TR","prices":[{"currency":"usd","amount":299,"exponent":2}]},
                "payment_source_prices":{}}}})
    }

    #[test]
    fn tiers_use_the_wire_numbers_of_premium_types() {
        assert_eq!(PremiumType::from_wire(Some(1)), PremiumType::Classic);
        assert_eq!(PremiumType::from_wire(Some(2)), PremiumType::Nitro);
        assert_eq!(PremiumType::from_wire(Some(3)), PremiumType::Basic);
        assert_eq!(PremiumType::from_wire(None), PremiumType::None);
        assert_eq!(PremiumType::from_wire(Some(0)), PremiumType::None);
        assert_eq!(PremiumType::Basic.wire(), Some(3));
        assert_eq!(type_of_sku(sku::TIER_0), PremiumType::Basic);
        assert_eq!(type_of_sku(sku::LEGACY), PremiumType::Nitro);
        assert_eq!(type_of_sku(sku::GUILD), PremiumType::None);
    }

    #[test]
    fn parses_a_plan_from_the_api() {
        let plan = Plan::from_json(&basic_monthly()).unwrap();
        assert_eq!(plan.id, plan_id::MONTH_TIER_0);
        assert_eq!(plan.sku_id, sku::TIER_0);
        assert!(plan.is_monthly() && !plan.is_yearly());
        assert_eq!(plan.price, 3799);
        assert_eq!(plan.prices.len(), 2);
        assert_eq!(plan.prices[&0].country_code, "TR");
        assert_eq!(plan.prices[&0].country.len(), 2);
    }

    #[test]
    fn price_lookup_prefers_the_payment_source_then_the_country() {
        let plan = Plan::from_json(&basic_monthly()).unwrap();
        assert_eq!(plan.price(0, None).unwrap().display(), "TRY\u{a0}37.99");
        assert_eq!(plan.price(0, Some("77")).unwrap().display(), "$2.99");
        // An unknown source falls back to the country price.
        assert_eq!(
            plan.price(0, Some("9")).unwrap().display(),
            "TRY\u{a0}37.99"
        );
        assert_eq!(plan.price(1, None).unwrap().display(), "$2.99");
        assert!(plan.price(5, None).is_none());
    }

    #[test]
    fn formats_prices_like_intl_en_us() {
        assert_eq!(format_currency(999, 2, "USD"), "$9.99");
        assert_eq!(format_currency(104999, 2, "TRY"), "TRY\u{a0}1,049.99");
        assert_eq!(format_currency(37900, 2, "TRY"), "TRY\u{a0}379.00");
        assert_eq!(format_currency(1000, 0, "JPY"), "¥1,000");
        assert_eq!(format_currency(499, 2, "EUR"), "€4.99");
        assert_eq!(format_currency(1999, 2, "GBP"), "£19.99");
        assert_eq!(format_currency(5, 2, "USD"), "$0.05");
        assert_eq!(format_currency(1234, 3, "KWD"), "KWD\u{a0}1.234");
        assert_eq!(format_currency(100000000, 2, "USD"), "$1,000,000.00");
        assert_eq!(format_currency(-250, 2, "USD"), "-$2.50");
    }

    #[test]
    fn exponent_and_default_digits_can_differ() {
        // 5 minor units at exponent 2 is 0.05; JPY prints no decimals and rounds half up.
        assert_eq!(format_currency(50, 2, "JPY"), "¥1");
        assert_eq!(format_currency(49, 2, "JPY"), "¥0");
    }

    #[test]
    fn yearly_price_per_month_is_rounded_to_a_minor_unit() {
        let yearly = Price {
            amount: 104999,
            currency: "try".into(),
            exponent: 2,
        };
        // The capture's "TRY 87.50/month billed annually".
        assert_eq!(yearly.per_month_of_year().display(), "TRY\u{a0}87.50");
    }

    #[test]
    fn intervals_read_like_the_strings() {
        assert_eq!(
            price_with_interval("$9.99", Interval::Month, 1).unwrap(),
            "$9.99 / Month"
        );
        assert_eq!(
            price_with_interval("$99.99", Interval::Year, 1).unwrap(),
            "$99.99 / Year"
        );
        assert_eq!(
            price_with_interval("$28.49", Interval::Month, 3).unwrap(),
            "$28.49 every 3 months"
        );
        assert!(price_with_interval("x", Interval::Day, 1).is_none());
    }

    #[test]
    fn finds_the_monthly_and_yearly_plan_of_an_sku() {
        let yearly = json!({"id":"1024422698568122368","name":"Nitro Basic Yearly","interval":2,"interval_count":1,
            "tax_inclusive":true,"sku_id":"978380684370378762","price":37900,"currency":"try","prices":{}});
        let plans = parse_plans(&json!([basic_monthly(), yearly]));
        assert_eq!(plans.len(), 2);
        assert_eq!(
            find_plan(&plans, sku::TIER_0, false).unwrap().id,
            plan_id::MONTH_TIER_0
        );
        assert_eq!(
            find_plan(&plans, sku::TIER_0, true).unwrap().id,
            plan_id::YEAR_TIER_0
        );
        assert!(find_plan(&plans, sku::TIER_2, false).is_none());
    }

    #[test]
    fn malformed_plans_are_skipped() {
        let plans = parse_plans(&json!([{"id": "x"}, basic_monthly(), 5]));
        assert_eq!(plans.len(), 1);
        assert!(parse_plans(&json!({"message": "nope"})).is_empty());
    }
}
