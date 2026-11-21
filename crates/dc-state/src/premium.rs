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
    // The exponent comes from a billing answer; past 30 the power would overflow `u128`.
    let scale = 10u128.pow(exponent.min(30));
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

/// Premium plans the subscription record can hold that `plan_id` does not list: the legacy and the group plans
/// (the plan table `hd` of module 682901).
pub mod extra_plan_id {
    pub const MONTH_LEGACY: u64 = 511651856145973248;
    pub const YEAR_LEGACY: u64 = 511651860671627264;
    pub const GROUP_MONTH: u64 = 1432523012455792640;
    /// The "None" plans a guild-boost-only subscription carries instead of a premium plan.
    pub const NONE_MONTH: u64 = 628379151761408000;
    pub const NONE_YEAR: u64 = 628381571568631808;
    pub const NONE_3_MONTH: u64 = 944265614527037440;
    pub const NONE_6_MONTH: u64 = 944265636643602432;
}

/// `SubscriptionStatus` (module 652215).
pub mod status {
    pub const UNPAID: u64 = 0;
    pub const ACTIVE: u64 = 1;
    pub const PAST_DUE: u64 = 2;
    pub const CANCELED: u64 = 3;
    pub const ENDED: u64 = 4;
    pub const ACCOUNT_HOLD: u64 = 6;
}

/// `SubscriptionTypes.PREMIUM` (module 652215): the Nitro subscription, as against a guild or application one.
pub const SUBSCRIPTION_TYPE_PREMIUM: u64 = 1;

/// The premium type a plan grants (`hd[plan].premiumType` of module 682901); `None` for boost and "none" plans.
pub fn plan_premium_type(plan: u64) -> Option<PremiumType> {
    use extra_plan_id as extra;
    match plan {
        plan_id::MONTH_TIER_0 | plan_id::YEAR_TIER_0 => Some(PremiumType::Basic),
        plan_id::MONTH_TIER_1 | plan_id::YEAR_TIER_1 => Some(PremiumType::Classic),
        plan_id::MONTH_TIER_2
        | plan_id::YEAR_TIER_2
        | plan_id::THREE_MONTH_TIER_2
        | plan_id::SIX_MONTH_TIER_2
        | extra::MONTH_LEGACY
        | extra::YEAR_LEGACY
        | extra::GROUP_MONTH => Some(PremiumType::Nitro),
        _ => None,
    }
}

/// The SKU a premium plan belongs to (`hd[plan].skuId` of module 682901).
pub fn plan_sku(plan: u64) -> Option<u64> {
    use extra_plan_id as extra;
    match plan {
        plan_id::MONTH_TIER_0 | plan_id::YEAR_TIER_0 => Some(sku::TIER_0),
        plan_id::MONTH_TIER_1 | plan_id::YEAR_TIER_1 => Some(sku::TIER_1),
        plan_id::MONTH_TIER_2
        | plan_id::YEAR_TIER_2
        | plan_id::THREE_MONTH_TIER_2
        | plan_id::SIX_MONTH_TIER_2
        | extra::GROUP_MONTH => Some(sku::TIER_2),
        extra::MONTH_LEGACY | extra::YEAR_LEGACY => Some(sku::LEGACY),
        _ => None,
    }
}

/// `m1` of module 832946: the plans a subscription without a premium plan carries.
fn is_none_plan(plan: u64) -> bool {
    use extra_plan_id as extra;
    matches!(
        plan,
        extra::NONE_MONTH | extra::NONE_YEAR | extra::NONE_3_MONTH | extra::NONE_6_MONTH
    )
}

/// The tier's name in the plan lists and the Subscriptions notice (`Mr` of module 682901: `t9uG/o`, `FSOz78`,
/// `lG6a5x`).
pub fn tier_display_name(tier: PremiumType) -> Option<&'static str> {
    match tier {
        PremiumType::Basic => Some("Nitro Basic"),
        PremiumType::Classic => Some("Nitro Classic"),
        PremiumType::Nitro => Some("Nitro"),
        PremiumType::None => None,
    }
}

/// `ci` of module 682901: how far up the ladder a tier sits (Basic, Classic, Nitro).
fn tier_rank(tier: PremiumType) -> Option<u8> {
    match tier {
        PremiumType::Basic => Some(0),
        PremiumType::Classic => Some(1),
        PremiumType::Nitro => Some(2),
        PremiumType::None => None,
    }
}

/// One line of a subscription (`{id, plan_id, quantity}`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SubscriptionItem {
    pub plan_id: u64,
    pub quantity: u64,
}

/// A pending plan change (`renewal_mutations`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RenewalMutations {
    pub items: Vec<SubscriptionItem>,
}

/// A record of `GET /users/@me/billing/subscriptions` (`createFromServer` in module 191115), with the fields the
/// pages read. Dates stay as the API's ISO strings.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Subscription {
    pub id: String,
    pub kind: u64,
    pub user_id: Option<u64>,
    pub status: u64,
    pub items: Vec<SubscriptionItem>,
    /// The premium plan among the items, else the "none" plan of the same interval (`a9` of module 367888).
    pub plan_id: u64,
    pub renewal_mutations: Option<RenewalMutations>,
    pub payment_source_id: Option<String>,
    pub trial_id: Option<String>,
    pub trial_ends_at: Option<String>,
    pub created_at: Option<String>,
    pub canceled_at: Option<String>,
    pub current_period_start: Option<String>,
    pub current_period_end: Option<String>,
    pub streak_started_at: Option<String>,
    pub currency: Option<String>,
    pub payment_gateway: Option<u64>,
}

impl Subscription {
    pub fn from_json(value: &Value) -> Option<Self> {
        let items = |list: &Value| -> Option<Vec<SubscriptionItem>> {
            list.as_array()?
                .iter()
                .map(|item| {
                    Some(SubscriptionItem {
                        plan_id: item["plan_id"].as_str()?.parse().ok()?,
                        quantity: item["quantity"].as_u64().unwrap_or(1),
                    })
                })
                .collect()
        };
        let items_now = items(&value["items"])?;
        let first = items_now.first()?;
        let text = |key: &str| value[key].as_str().map(str::to_owned);
        let plan_id = if value["type"].as_u64() == Some(SUBSCRIPTION_TYPE_PREMIUM) {
            items_now
                .iter()
                .find(|item| plan_premium_type(item.plan_id).is_some())
                .map_or(first.plan_id, |item| item.plan_id)
        } else {
            first.plan_id
        };
        Some(Self {
            id: value["id"].as_str()?.to_owned(),
            kind: value["type"].as_u64()?,
            user_id: value["user_id"].as_str().and_then(|id| id.parse().ok()),
            status: value["status"].as_u64()?,
            plan_id,
            items: items_now,
            renewal_mutations: items(&value["renewal_mutations"]["items"])
                .map(|items| RenewalMutations { items }),
            payment_source_id: text("payment_source_id"),
            trial_id: text("trial_id"),
            trial_ends_at: text("trial_ends_at"),
            created_at: text("created_at"),
            canceled_at: text("canceled_at"),
            current_period_start: text("current_period_start"),
            current_period_end: text("current_period_end"),
            streak_started_at: text("streak_started_at"),
            currency: text("currency"),
            payment_gateway: value["payment_gateway"].as_u64(),
        })
    }

    /// `getPremiumPlanItem` (`Q` of module 722178): the item whose plan is a premium one; boost-only subscriptions
    /// have none.
    pub fn premium_item(&self) -> Option<&SubscriptionItem> {
        self.items
            .iter()
            .find(|item| plan_premium_type(item.plan_id).is_some())
    }

    /// The tier the subscription's premium plan grants.
    pub fn premium_type(&self) -> Option<PremiumType> {
        plan_premium_type(self.premium_item()?.plan_id)
    }

    /// The plan's name in the settings notice (`getTierDisplayNameByPlanId`, or "Boost" for a boost-only record).
    pub fn display_name(&self) -> Option<&'static str> {
        match self.premium_item() {
            Some(item) => plan_premium_type(item.plan_id).and_then(tier_display_name),
            None => self
                .items
                .iter()
                .any(|item| {
                    matches!(
                        item.plan_id,
                        plan_id::MONTH_GUILD
                            | plan_id::YEAR_GUILD
                            | plan_id::THREE_MONTH_GUILD
                            | plan_id::SIX_MONTH_GUILD
                    )
                })
                .then_some("Boost"),
        }
    }

    /// `Nc` of module 722178: a plan change, a trial or a missed payment is pending, so the plan cannot be switched.
    pub fn switching_disabled(&self) -> bool {
        self.renewal_mutations.is_some()
            || self.trial_ends_at.is_some()
            || self.status == status::PAST_DUE
    }

    /// `premiumSince`: when the unbroken run of subscription began.
    pub fn premium_since(&self) -> Option<&str> {
        self.streak_started_at
            .as_deref()
            .or(self.created_at.as_deref())
    }
}

/// `getPremiumTypeSubscription` of the subscription store (module 166403): the account's Nitro subscription, among
/// the records that are not unpaid and belong to it.
pub fn premium_type_subscription(body: &Value, user: Option<u64>) -> Option<Subscription> {
    body.as_array()?
        .iter()
        .filter_map(Subscription::from_json)
        .filter(|sub| sub.status != status::UNPAID && sub.user_id == user)
        .find(|sub| sub.kind == SUBSCRIPTION_TYPE_PREMIUM)
}

/// What the subscribe button of a Nitro page reads and whether it works: `useSubscribeButtonText` of module 572079.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SubscribeCta {
    pub text: &'static str,
    pub disabled: bool,
    /// The hover text of a disabled button (`jXaaRk` and the plan-change notes of `Q8`).
    pub tooltip: Option<&'static str>,
}

/// `jXaaRk`.
pub const MANAGE_SUBSCRIPTION_TIP: &str =
    "To manage your Nitro subscription, go to the Subscriptions page in User Settings.";

/// `Q8` of module 722178: why a plan cannot be switched now.
fn switching_note(subscription: &Subscription) -> Option<&'static str> {
    let mut note = None;
    if let Some(mutations) = &subscription.renewal_mutations {
        let changes_plan = mutations
            .items
            .iter()
            .find(|item| plan_premium_type(item.plan_id).is_some())
            .is_none_or(|item| item.plan_id != subscription.plan_id);
        note = Some(if changes_plan {
            "You have to cancel your pending plan change before choosing another plan."
        } else {
            "You have a pending boost cancellation. You have to uncancel it before switching plans."
        });
    }
    if subscription.trial_ends_at.is_some() {
        note = Some("You cannot switch plans during your trial.");
    }
    note
}

/// The button's text and state for a call to action naming `cta_sku` (`None` is the hero's plain Subscribe), given the
/// account's Nitro subscription and its `premium_type`. Without a subscription the button is a plain Subscribe; with
/// one it is disabled, naming the current plan, unless it offers an upgrade to Nitro.
pub fn subscribe_cta(
    cta_sku: Option<u64>,
    subscription: Option<&Subscription>,
    user: PremiumType,
) -> SubscribeCta {
    // `S`: the plan's tier when the subscription has a premium plan, else the account's own.
    let current = subscription
        .and_then(Subscription::premium_type)
        .unwrap_or(user);
    let upgrade = cta_sku == Some(sku::TIER_2)
        && matches!(current, PremiumType::Basic | PremiumType::Classic);
    let default_text = if upgrade { "Upgrade" } else { "Subscribe" };
    let switching = subscription.is_some_and(Subscription::switching_disabled);
    let locked = subscription.is_some_and(|sub| {
        sub.status != status::ACCOUNT_HOLD && !is_none_plan(sub.plan_id) && !upgrade
    });
    if !(locked || switching) {
        return SubscribeCta {
            text: default_text,
            disabled: false,
            tooltip: None,
        };
    }
    let mut text = default_text;
    let mut tooltip = None;
    let known = cta_sku
        .is_none_or(|sku| matches!(sku, sku::LEGACY | sku::TIER_0 | sku::TIER_1 | sku::TIER_2));
    if known {
        let offered = cta_sku.map_or(PremiumType::None, type_of_sku);
        if switching {
            tooltip = subscription.and_then(switching_note);
        } else if let (Some(offered), Some(current)) = (tier_rank(offered), tier_rank(current))
            && offered < current
        {
            text = "Subscribe";
            tooltip = Some(MANAGE_SUBSCRIPTION_TIP);
        } else if offered.is_subscriber() && offered == current {
            text = "Current Plan";
            tooltip = Some(MANAGE_SUBSCRIPTION_TIP);
        } else if !offered.is_subscriber() && current == PremiumType::Nitro {
            tooltip = Some(MANAGE_SUBSCRIPTION_TIP);
        }
    }
    SubscribeCta {
        text,
        disabled: true,
        tooltip,
    }
}

/// Remaining time breakdown for a Nitro trial/discount offer countdown (module 105390:
/// `qVGNkU`, `wyg9a9`, `IzmIlV`, `iiLbvu`, `/DTtr6`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OfferCountdown {
    pub days: u64,
    pub hours: u64,
    pub minutes: u64,
    pub is_discount: bool,
    pub discount_percent: Option<u64>,
    pub is_tier0: bool,
    pub is_yearly: bool,
}

impl OfferCountdown {
    /// Parses `GET /users/@me/billing/user-offer` JSON relative to `now_secs` (UNIX seconds).
    /// Returns `None` if no active non-expired offer is present.
    pub fn from_user_offer(offer: &Value, now_secs: i64) -> Option<Self> {
        let (obj, is_discount) = if offer["user_discount_offer"].is_object() {
            (&offer["user_discount_offer"], true)
        } else if offer["user_trial_offer"].is_object() {
            (&offer["user_trial_offer"], false)
        } else {
            return None;
        };
        let expires = obj["expires_at"].as_str()?;
        let expires_secs = parse_rfc3339_epoch_secs(expires)?;
        let rem = expires_secs - now_secs;
        if rem <= 0 {
            return None;
        }
        let rem = rem as u64;
        let days = rem / 86_400;
        let hours = (rem % 86_400) / 3_600;
        let minutes = ((rem % 3_600) / 60).max(if days == 0 && hours == 0 { 1 } else { 0 });
        let discount_percent = is_discount
            .then(|| obj["discount"]["amount"].as_u64())
            .flatten();
        let plan_ids = obj["discount"]["plan_ids"]
            .as_array()
            .or_else(|| obj["subscription_trial"]["sku_id"].as_array());
        let is_tier0 = obj["subscription_trial"]["sku_id"].as_str() == Some("978380684370378762");
        let is_yearly = plan_ids.is_some_and(|arr| {
            arr.iter()
                .filter_map(Value::as_str)
                .any(|s| s == "511651885459963904")
        });
        Some(Self {
            days,
            hours,
            minutes,
            is_discount,
            discount_percent,
            is_tier0,
            is_yearly,
        })
    }
}

/// Minimal ISO-8601 / RFC-3339 UTC timestamp parser (`YYYY-MM-DDTHH:MM:SS...`) -> epoch seconds.
fn parse_rfc3339_epoch_secs(s: &str) -> Option<i64> {
    let b = s.as_bytes();
    if b.len() < 19 || b[4] != b'-' || b[7] != b'-' || b[13] != b':' || b[16] != b':' {
        return None;
    }
    let y: i64 = s[0..4].parse().ok()?;
    let m: i64 = s[5..7].parse().ok()?;
    let d: i64 = s[8..10].parse().ok()?;
    let hh: i64 = s[11..13].parse().ok()?;
    let mm: i64 = s[14..16].parse().ok()?;
    let ss: i64 = s[17..19].parse().ok()?;
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }
    // Howard Hinnant's days-from-civil algorithm.
    let y_adj = y - i64::from(m <= 2);
    let era = y_adj.div_euclid(400);
    let yoe = y_adj.rem_euclid(400);
    let mp = m + if m > 2 { -3 } else { 9 };
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days_since_epoch = era * 146_097 + doe - 719_468;
    Some(days_since_epoch * 86_400 + hh * 3_600 + mm * 60 + ss)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn an_absurd_currency_exponent_does_not_overflow() {
        let _ = format_currency(999, 200, "USD");
        let _ = format_currency(i64::MIN, 40, "JPY");
    }

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

    fn subscription(plan: u64, status: u64) -> Subscription {
        Subscription::from_json(&json!({
            "id": "1", "type": 1, "user_id": "9", "status": status,
            "items": [{"id": "5", "plan_id": plan.to_string(), "quantity": 1}],
            "created_at": "2025-01-02T03:04:05.000000+00:00",
            "streak_started_at": "2024-06-01T00:00:00.000000+00:00",
            "current_period_end": "2026-12-02T03:04:05.000000+00:00",
            "renewal_mutations": null, "trial_ends_at": null,
        }))
        .unwrap()
    }

    #[test]
    fn a_subscription_record_reads_its_plan_and_tier() {
        let sub = subscription(plan_id::YEAR_TIER_2, 1);
        assert_eq!(sub.plan_id, plan_id::YEAR_TIER_2);
        assert_eq!(sub.premium_type(), Some(PremiumType::Nitro));
        assert_eq!(
            sub.premium_since(),
            Some("2024-06-01T00:00:00.000000+00:00")
        );
        assert!(!sub.switching_disabled());
        let boost = Subscription::from_json(&json!({
            "id": "2", "type": 1, "user_id": "9", "status": 1,
            "items": [{"plan_id": plan_id::MONTH_GUILD.to_string(), "quantity": 2}],
        }))
        .unwrap();
        assert_eq!(boost.premium_type(), None);
        assert_eq!(boost.display_name(), Some("Boost"));
        assert_eq!(sub.display_name(), Some("Nitro"));
        assert_eq!(
            subscription(plan_id::YEAR_TIER_0, 1).display_name(),
            Some("Nitro Basic")
        );
        assert!(
            subscription(plan_id::MONTH_TIER_0, 1)
                .premium_item()
                .is_some()
        );
    }

    #[test]
    fn the_store_picks_the_accounts_paid_nitro_subscription() {
        let body = json!([
            {"id": "1", "type": 2, "user_id": "9", "status": 1,
             "items": [{"plan_id": plan_id::MONTH_GUILD.to_string(), "quantity": 1}]},
            {"id": "2", "type": 1, "user_id": "8", "status": 1,
             "items": [{"plan_id": plan_id::MONTH_TIER_2.to_string(), "quantity": 1}]},
            {"id": "3", "type": 1, "user_id": "9", "status": 0,
             "items": [{"plan_id": plan_id::MONTH_TIER_2.to_string(), "quantity": 1}]},
            {"id": "4", "type": 1, "user_id": "9", "status": 1,
             "items": [{"plan_id": plan_id::YEAR_TIER_0.to_string(), "quantity": 1}]},
        ]);
        assert_eq!(premium_type_subscription(&body, Some(9)).unwrap().id, "4");
        assert!(premium_type_subscription(&body, Some(7)).is_none());
        assert!(premium_type_subscription(&json!([]), Some(9)).is_none());
    }

    #[test]
    fn the_subscribe_button_by_tier() {
        let nitro = subscription(plan_id::MONTH_TIER_2, 1);
        let basic = subscription(plan_id::YEAR_TIER_0, 1);
        let classic = subscription(plan_id::MONTH_TIER_1, 1);
        // No subscription: every button is a plain Subscribe.
        for sku in [None, Some(sku::TIER_0), Some(sku::TIER_2)] {
            let cta = subscribe_cta(sku, None, PremiumType::None);
            assert_eq!(
                (cta.text, cta.disabled, cta.tooltip),
                ("Subscribe", false, None)
            );
        }
        // A Nitro subscriber: the hero's button is disabled and says so on hover; the Nitro card reads Current Plan
        // and the Basic card a disabled Subscribe.
        let hero = subscribe_cta(None, Some(&nitro), PremiumType::Nitro);
        assert_eq!((hero.text, hero.disabled), ("Subscribe", true));
        assert_eq!(hero.tooltip, Some(MANAGE_SUBSCRIPTION_TIP));
        let card = subscribe_cta(Some(sku::TIER_2), Some(&nitro), PremiumType::Nitro);
        assert_eq!((card.text, card.disabled), ("Current Plan", true));
        let lower = subscribe_cta(Some(sku::TIER_0), Some(&nitro), PremiumType::Nitro);
        assert_eq!((lower.text, lower.disabled), ("Subscribe", true));
        // A Basic subscriber may upgrade: the hero asks for Nitro and both Nitro buttons work.
        let up = subscribe_cta(Some(sku::TIER_2), Some(&basic), PremiumType::Basic);
        assert_eq!((up.text, up.disabled, up.tooltip), ("Upgrade", false, None));
        let own = subscribe_cta(Some(sku::TIER_0), Some(&basic), PremiumType::Basic);
        assert_eq!((own.text, own.disabled), ("Current Plan", true));
        let classic_card = subscribe_cta(Some(sku::TIER_0), Some(&classic), PremiumType::Classic);
        assert_eq!(
            (classic_card.text, classic_card.disabled),
            ("Subscribe", true)
        );
        // A past-due Basic subscriber cannot switch even to Nitro: the note says why.
        let mut late = subscription(plan_id::MONTH_TIER_0, status::PAST_DUE);
        late.status = status::PAST_DUE;
        let held = subscribe_cta(Some(sku::TIER_2), Some(&late), PremiumType::Basic);
        assert_eq!((held.text, held.disabled), ("Upgrade", true));
        // An account on hold is treated as having no subscription.
        let hold = subscription(plan_id::MONTH_TIER_2, status::ACCOUNT_HOLD);
        assert!(!subscribe_cta(None, Some(&hold), PremiumType::Nitro).disabled);
    }

    #[test]
    fn a_pending_plan_change_is_the_buttons_note() {
        let mut sub = subscription(plan_id::MONTH_TIER_0, 1);
        sub.renewal_mutations = Some(RenewalMutations {
            items: vec![SubscriptionItem {
                plan_id: plan_id::MONTH_TIER_2,
                quantity: 1,
            }],
        });
        let cta = subscribe_cta(Some(sku::TIER_2), Some(&sub), PremiumType::Basic);
        assert!(cta.disabled);
        assert_eq!(
            cta.tooltip,
            Some("You have to cancel your pending plan change before choosing another plan.")
        );
        sub.trial_ends_at = Some("2026-12-01T00:00:00+00:00".into());
        assert_eq!(
            subscribe_cta(None, Some(&sub), PremiumType::Basic).tooltip,
            Some("You cannot switch plans during your trial.")
        );
    }

    #[test]
    fn malformed_plans_are_skipped() {
        let plans = parse_plans(&json!([{"id": "x"}, basic_monthly(), 5]));
        assert_eq!(plans.len(), 1);
        assert!(parse_plans(&json!({"message": "nope"})).is_empty());
    }
}
