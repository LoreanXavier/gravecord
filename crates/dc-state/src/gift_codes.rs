//! GiftCodeStore (module 30793) and the gift links of a message (`e7` / `pF` of module 45938).
//!
//! A message's codes come from its content, or from its one `gift` embed's url when it is a `CUSTOM_GIFT` (41)
//! message (`pF`). `e7` takes at most three distinct codes: first the gift links (`discord.gift/`,
//! `discordapp.com/gifts/`, `discord.com/gifts/`), then the promotion links, each one's non-alphanumeric characters
//! dropped. A link counts only at the start of the text, after a space or right after `http://`/`https://`.
//!
//! The store keeps the resolved codes as `GiftCode` records (module 7133 `createFromServer`), the codes being
//! resolved, resolved (found or not) and being accepted, and each code's last error. A resolve answer also feeds what
//! UserStore, SKUStore, StoreListingStore and SubscriptionPlanStore take from it (the gifter, `store_listing.sku`,
//! `store_listing` and `subscription_plan`); those parts are kept here, by the same keys, for the gift card.
use crate::{Stores, text};
use chrono::{DateTime, Utc};
use serde_json::Value;
use std::collections::HashMap;

/// `GLOBAL_ENV.GIFT_CODE_HOST` of the pinned build, then the two hosts module 45938 adds.
const GIFT_HOSTS: [&str; 3] = ["discord.gift", "discordapp.com/gifts", "discord.com/gifts"];
/// The promotion hosts of `S` (module 45938).
const PROMOTION_HOSTS: [&str; 2] = ["discord.com/billing/promotions", "promos.discord.gg"];
/// `CUSTOM_GIFT` of module 448761.
const CUSTOM_GIFT: u64 = crate::message_kinds::CUSTOM_GIFT;
/// `FYj` of module 652215: the collectibles (Shop) application.
pub const COLLECTIBLES_APPLICATION_ID: &str = "1096190356233670716";
/// `NOT_SELF_REDEEMABLE` and `EXISTING_PREMIUM_SUBSCRIPTION_DISALLOWED` of module 7133.
const NOT_SELF_REDEEMABLE: u64 = 4;
const EXISTING_PREMIUM_SUBSCRIPTION_DISALLOWED: u64 = 2;
/// The API error codes the store's redeem failure reads (`t02` of module 652215).
pub const UNKNOWN_GIFT_CODE: i64 = 10038;
pub const INVALID_GIFT_REDEMPTION_EXHAUSTED: i64 = 50050;
pub const INVALID_GIFT_REDEMPTION_CLIENT_UPDATE_REQUIRED: i64 = 50283;

/// Where a match may begin: the start of the text, a space, or `http://`/`https://` (`(?: |^|https?://)`).
fn link_starts(text: &str, host_at: usize) -> bool {
    let before = &text[..host_at];
    before.is_empty()
        || before.ends_with(' ')
        || before.ends_with("http://")
        || before.ends_with("https://")
}

/// `[a-z0-9-]+` under the `i` flag.
fn code_run(text: &str) -> &str {
    let end = text
        .find(|c: char| !(c.is_ascii_alphanumeric() || c == '-'))
        .unwrap_or(text.len());
    &text[..end]
}

/// `y`: everything but `A-Za-z0-9` dropped.
fn strip(code: &str) -> String {
    code.chars().filter(char::is_ascii_alphanumeric).collect()
}

/// The matches of one regex in text order: (start, code).
fn matches(text: &str, hosts: &[&str], promotion: bool) -> Vec<(usize, String)> {
    let lower = text.to_ascii_lowercase();
    let mut found: Vec<(usize, String)> = Vec::new();
    for host in hosts {
        // The regexes carry `gi`: the hosts match in any case.
        for (at, _) in lower.match_indices(host) {
            if !link_starts(text, at) {
                continue;
            }
            let rest = &text[at + host.len()..];
            let code = if promotion {
                // `(/|(/)?\?code=)([a-z0-9-]+)`.
                let after = rest
                    .strip_prefix("/?code=")
                    .or_else(|| rest.strip_prefix("?code="))
                    .or_else(|| rest.strip_prefix('/'));
                after.map(code_run)
            } else {
                rest.strip_prefix('/').map(code_run)
            };
            if let Some(code) = code.filter(|code| !code.is_empty()) {
                found.push((at, code.to_owned()));
            }
        }
    }
    found.sort_by_key(|(at, _)| *at);
    found
}

/// `e7`: up to three codes, gift links before promotion links, each once.
pub fn gift_codes_in(text: &str) -> Vec<String> {
    let mut codes: Vec<String> = Vec::new();
    for (_, code) in
        matches(text, &GIFT_HOSTS, false)
            .into_iter()
            .chain(matches(text, &PROMOTION_HOSTS, true))
    {
        if codes.len() >= 3 {
            break;
        }
        let code = strip(&code);
        if !codes.contains(&code) {
            codes.push(code);
        }
    }
    codes
}

/// `pF`: a custom gift message carries its link in its single `gift` embed.
pub fn is_custom_gift(kind: u64, embeds: &[crate::Embed]) -> bool {
    kind == CUSTOM_GIFT && embeds.len() == 1 && embeds[0].kind == "gift"
}

/// The `giftCodes` of a message record (module 432005): `e7` of the gift embed's url, or of the content.
pub fn message_gift_codes(kind: u64, content: &str, embeds: &[crate::Embed]) -> Vec<String> {
    if is_custom_gift(kind, embeds) {
        embeds[0]
            .url
            .as_deref()
            .map(gift_codes_in)
            .unwrap_or_default()
    } else {
        gift_codes_in(content)
    }
}

/// A subscription plan as SubscriptionPlanStore keeps it (module 832946): `interval` 1 is a month, 2 a year.
#[derive(Clone, Debug, PartialEq)]
pub struct SubscriptionPlan {
    pub id: String,
    pub sku_id: String,
    pub name: String,
    pub interval: u64,
    pub interval_count: u64,
}

impl SubscriptionPlan {
    pub fn from_value(value: &Value) -> Option<Self> {
        Some(Self {
            id: text(&value["id"])?,
            sku_id: text(&value["sku_id"]).unwrap_or_default(),
            name: text(&value["name"]).unwrap_or_default(),
            interval: value["interval"].as_u64().unwrap_or(1),
            interval_count: value["interval_count"].as_u64().unwrap_or(1),
        })
    }

    pub fn is_month(&self) -> bool {
        self.interval == 1
    }
}

/// The SKU of a gift's store listing (SKUStore's record, the fields the gift card reads).
#[derive(Clone, Debug, PartialEq)]
pub struct Sku {
    pub id: String,
    pub name: String,
    pub kind: u64,
    pub product_line: Option<u64>,
    pub application_id: String,
    pub release_date: Option<String>,
    /// `PREORDER_DATE`-free preorder: a release date in the future (`isPreorder`).
    pub preorder: bool,
}

/// The listing of a SKU (StoreListingStore's record): the art the card shows when the gift has no style.
#[derive(Clone, Debug, PartialEq)]
pub struct StoreListing {
    pub id: String,
    pub sku_id: String,
    pub tagline: Option<String>,
    /// `thumbnail`: its id and mime type.
    pub thumbnail: Option<(String, String)>,
}

/// `GiftCode` (module 7133).
#[derive(Clone, Debug, PartialEq)]
pub struct GiftCode {
    pub user_id: Option<String>,
    pub code: String,
    pub sku_id: String,
    pub application_id: Option<String>,
    pub uses: u64,
    pub max_uses: u64,
    pub expires_at: Option<DateTime<Utc>>,
    pub redeemed: bool,
    pub store_listing_id: Option<String>,
    pub subscription_plan_id: Option<String>,
    pub subscription_plan: Option<SubscriptionPlan>,
    pub revoked: bool,
    pub entitlement_branches: Option<Vec<String>>,
    pub flags: u64,
    pub gift_style: Option<u64>,
    pub promotion: bool,
}

impl GiftCode {
    /// `createFromServer`.
    pub fn from_value(value: &Value) -> Option<Self> {
        let plan = SubscriptionPlan::from_value(&value["subscription_plan"]);
        Some(Self {
            user_id: text(&value["user"]["id"]),
            code: text(&value["code"])?,
            sku_id: text(&value["sku_id"]).unwrap_or_default(),
            application_id: text(&value["application_id"]),
            uses: value["uses"].as_u64().unwrap_or(0),
            max_uses: value["max_uses"].as_u64().unwrap_or(1),
            expires_at: value["expires_at"]
                .as_str()
                .and_then(|at| DateTime::parse_from_rfc3339(at).ok())
                .map(|at| at.with_timezone(&Utc)),
            redeemed: value["redeemed"].as_bool().unwrap_or(false),
            store_listing_id: text(&value["store_listing"]["id"]),
            subscription_plan_id: plan
                .as_ref()
                .map(|plan| plan.id.clone())
                .or_else(|| text(&value["subscription_plan_id"])),
            subscription_plan: plan,
            revoked: false,
            entitlement_branches: value["entitlement_branches"].as_array().map(|list| {
                list.iter()
                    .filter_map(|item| item.as_str().map(str::to_owned))
                    .collect()
            }),
            flags: value["flags"].as_u64().unwrap_or(0),
            gift_style: value["gift_style"].as_u64(),
            promotion: !value["promotion"].is_null(),
        })
    }

    /// `merge`: the fields a later answer for the same code carries replace the stored ones.
    fn merge(&mut self, newer: GiftCode) {
        let revoked = self.revoked;
        *self = newer;
        self.revoked = revoked;
    }

    pub fn is_expired(&self, now: DateTime<Utc>) -> bool {
        self.expires_at.is_some_and(|at| now > at)
    }

    pub fn has_multiple_copies(&self) -> bool {
        self.max_uses > 1
    }

    pub fn is_claimed(&self) -> bool {
        self.uses >= self.max_uses
    }

    pub fn remaining_uses(&self) -> u64 {
        self.max_uses.saturating_sub(self.uses)
    }

    pub fn is_subscription(&self) -> bool {
        self.subscription_plan_id.is_some()
    }

    pub fn is_self_redeemable(&self) -> bool {
        self.flags & NOT_SELF_REDEEMABLE == 0
    }

    pub fn is_existing_premium_subscription_disallowed(&self) -> bool {
        self.flags & EXISTING_PREMIUM_SUBSCRIPTION_DISALLOWED != 0
    }

    /// `hU` of module 993408: a Shop (collectibles) gift.
    pub fn is_collectibles(&self) -> bool {
        self.application_id.as_deref() == Some(COLLECTIBLES_APPLICATION_ID)
    }
}

/// The gifter as UserStore keeps it from a resolve (`eO` of module 287809), for `getUserTag`.
#[derive(Clone, Debug, PartialEq)]
pub struct Gifter {
    pub id: String,
    pub username: String,
    pub discriminator: String,
}

impl Gifter {
    /// `getUserTag` (module 74227): the username, with `#discriminator` for a legacy tag.
    pub fn tag(&self) -> String {
        if self.discriminator.is_empty() || self.discriminator == "0" {
            self.username.clone()
        } else {
            format!("{}#{}", self.username, self.discriminator)
        }
    }
}

#[derive(Default)]
pub struct GiftCodeStore {
    codes: HashMap<String, GiftCode>,
    resolving: Vec<String>,
    resolved: Vec<String>,
    accepting: Vec<String>,
    errors: HashMap<String, Value>,
    fetching: std::collections::HashSet<String>,
    loaded_at: HashMap<String, i64>,
    skus: HashMap<String, Sku>,
    listings: HashMap<String, StoreListing>,
    plans: HashMap<String, SubscriptionPlan>,
    gifters: HashMap<String, Gifter>,
}

/// `Kx` of module 45938.
fn plan_key(sku: &str, plan: Option<&str>) -> String {
    format!("{sku}:{}:", plan.unwrap_or(""))
}

impl GiftCodeStore {
    /// `g`: a code from the server, merged into the one already kept.
    fn put(&mut self, value: &Value) {
        let Some(gift) = GiftCode::from_value(value) else {
            return;
        };
        match self.codes.get_mut(&gift.code) {
            Some(kept) => kept.merge(gift),
            None => {
                self.codes.insert(gift.code.clone(), gift);
            }
        }
    }

    fn take_resolve_parts(&mut self, value: &Value) {
        let listing = &value["store_listing"];
        if !listing.is_null() {
            let sku = &listing["sku"];
            if let Some(id) = text(&sku["id"]) {
                let release_date = text(&sku["release_date"]);
                let preorder = release_date
                    .as_deref()
                    .and_then(|at| DateTime::parse_from_rfc3339(at).ok())
                    .is_some_and(|at| at.with_timezone(&Utc) > Utc::now());
                self.skus.insert(
                    id.clone(),
                    Sku {
                        id: id.clone(),
                        name: text(&sku["name"]).unwrap_or_default(),
                        kind: sku["type"].as_u64().unwrap_or(0),
                        product_line: sku["product_line"].as_u64(),
                        application_id: text(&sku["application_id"]).unwrap_or_default(),
                        release_date,
                        preorder,
                    },
                );
                self.listings.insert(
                    id.clone(),
                    StoreListing {
                        id: text(&listing["id"]).unwrap_or_default(),
                        sku_id: id,
                        tagline: text(&listing["tagline"]),
                        thumbnail: text(&listing["thumbnail"]["id"]).map(|thumb| {
                            (
                                thumb,
                                text(&listing["thumbnail"]["mime_type"]).unwrap_or_default(),
                            )
                        }),
                    },
                );
            }
        }
        if let Some(plan) = SubscriptionPlan::from_value(&value["subscription_plan"]) {
            self.plans.insert(plan.id.clone(), plan);
        }
        let user = &value["user"];
        if let Some(id) = text(&user["id"]) {
            self.gifters.insert(
                id.clone(),
                Gifter {
                    id,
                    username: text(&user["username"]).unwrap_or_default(),
                    discriminator: text(&user["discriminator"]).unwrap_or_default(),
                },
            );
        }
    }

    fn push(list: &mut Vec<String>, code: &str) {
        if !list.iter().any(|kept| kept == code) {
            list.push(code.to_owned());
        }
    }

    fn drop_from(list: &mut Vec<String>, code: &str) {
        list.retain(|kept| kept != code);
    }
}

impl Stores {
    pub(crate) fn gift_codes_dispatch(&mut self, name: &str, data: &Value) {
        let store = &mut self.gift_codes;
        let code = data["code"].as_str().unwrap_or_default();
        match name {
            "GIFT_CODE_RESOLVE" => GiftCodeStore::push(&mut store.resolving, code),
            "GIFT_CODE_RESOLVE_SUCCESS" => {
                let gift = &data["giftCode"];
                let code = gift["code"].as_str().unwrap_or_default();
                GiftCodeStore::drop_from(&mut store.resolving, code);
                GiftCodeStore::push(&mut store.resolved, code);
                store.take_resolve_parts(gift);
                store.put(gift);
            }
            "GIFT_CODE_RESOLVE_FAILURE" => {
                GiftCodeStore::drop_from(&mut store.resolving, code);
                GiftCodeStore::push(&mut store.resolved, code);
                if !data["error"].is_null() {
                    store.errors.insert(code.to_owned(), data["error"].clone());
                }
            }
            "GIFT_CODE_REDEEM" => GiftCodeStore::push(&mut store.accepting, code),
            "GIFT_CODE_REDEEM_SUCCESS" => {
                GiftCodeStore::drop_from(&mut store.accepting, code);
                if let Some(gift) = store.codes.get_mut(code) {
                    gift.redeemed = true;
                    gift.uses += 1;
                }
            }
            "GIFT_CODE_REDEEM_FAILURE" => {
                GiftCodeStore::drop_from(&mut store.accepting, code);
                store.errors.insert(code.to_owned(), data["error"].clone());
                if let Some(gift) = store.codes.get_mut(code) {
                    match data["error"]["code"].as_i64() {
                        Some(UNKNOWN_GIFT_CODE) => gift.revoked = true,
                        Some(INVALID_GIFT_REDEMPTION_EXHAUSTED) => gift.uses = gift.max_uses,
                        _ => {}
                    }
                }
            }
            "GIFT_CODE_REVOKE_SUCCESS" => {
                store.codes.remove(code);
                GiftCodeStore::push(&mut store.resolved, code);
            }
            "GIFT_CODE_CREATE_SUCCESS" => store.put(&data["giftCode"]),
            "GIFT_CODES_FETCH" => {
                let key = plan_key(
                    data["skuId"].as_str().unwrap_or_default(),
                    data["subscriptionPlanId"].as_str(),
                );
                store.fetching.insert(key);
            }
            "GIFT_CODES_FETCH_SUCCESS" => {
                for gift in data["giftCodes"].as_array().into_iter().flatten() {
                    store.put(gift);
                }
                let key = plan_key(
                    data["skuId"].as_str().unwrap_or_default(),
                    data["subscriptionPlanId"].as_str(),
                );
                store
                    .loaded_at
                    .insert(key.clone(), Utc::now().timestamp_millis());
                store.fetching.remove(&key);
            }
            "GIFT_CODES_FETCH_FAILURE" => {
                let key = plan_key(
                    data["skuId"].as_str().unwrap_or_default(),
                    data["subscriptionPlanId"].as_str(),
                );
                store.fetching.remove(&key);
            }
            "GIFT_CODE_UPDATE" => {
                if let Some(gift) = store.codes.get_mut(code) {
                    gift.uses = gift.uses.max(data["uses"].as_u64().unwrap_or(0));
                }
            }
            _ => {}
        }
    }

    /// `get(code)`: an expired code reads as absent.
    pub fn gift_code(&self, code: &str) -> Option<&GiftCode> {
        self.gift_code_at(code, Utc::now())
    }

    pub fn gift_code_at(&self, code: &str, now: DateTime<Utc>) -> Option<&GiftCode> {
        self.gift_codes
            .codes
            .get(code)
            .filter(|gift| !gift.is_expired(now))
    }

    /// `getError(code)`: the error object of the last failed resolve or redeem.
    pub fn gift_code_error(&self, code: &str) -> Option<&Value> {
        self.gift_codes.errors.get(code)
    }

    /// `getIsResolving`.
    pub fn gift_code_is_resolving(&self, code: &str) -> bool {
        self.gift_codes.resolving.iter().any(|kept| kept == code)
    }

    /// `getIsResolved`.
    pub fn gift_code_is_resolved(&self, code: &str) -> bool {
        self.gift_codes.resolved.iter().any(|kept| kept == code)
    }

    /// `getIsAccepting`.
    pub fn gift_code_is_accepting(&self, code: &str) -> bool {
        self.gift_codes.accepting.iter().any(|kept| kept == code)
    }

    /// `getResolvingCodes`, `getResolvedCodes`, `getAcceptingCodes`.
    pub fn gift_codes_resolving(&self) -> &[String] {
        &self.gift_codes.resolving
    }

    pub fn gift_codes_resolved(&self) -> &[String] {
        &self.gift_codes.resolved
    }

    pub fn gift_codes_accepting(&self) -> &[String] {
        &self.gift_codes.accepting
    }

    /// `getForGifterSKUAndPlan`.
    pub fn gift_codes_for_gifter_sku_and_plan(
        &self,
        gifter: &str,
        sku: &str,
        plan: Option<&str>,
    ) -> Vec<&GiftCode> {
        let now = Utc::now();
        self.gift_codes
            .codes
            .values()
            .filter(|gift| {
                gift.user_id.as_deref() == Some(gifter)
                    && gift.sku_id == sku
                    && (plan.is_none() || gift.subscription_plan_id.as_deref() == plan)
                    && !gift.is_expired(now)
            })
            .collect()
    }

    /// `getUserGiftCodesFetchingForSKUAndPlan`.
    pub fn gift_codes_fetching_for(&self, sku: &str, plan: Option<&str>) -> bool {
        self.gift_codes.fetching.contains(&plan_key(sku, plan))
    }

    /// `getUserGiftCodesLoadedAtForSKUAndPlan`.
    pub fn gift_codes_loaded_at(&self, sku: &str, plan: Option<&str>) -> Option<i64> {
        self.gift_codes.loaded_at.get(&plan_key(sku, plan)).copied()
    }

    /// SKUStore `get` for a SKU a resolve brought.
    pub fn gift_sku(&self, sku: &str) -> Option<&Sku> {
        self.gift_codes.skus.get(sku)
    }

    /// StoreListingStore `getForSKU` for a SKU a resolve brought.
    pub fn gift_store_listing(&self, sku: &str) -> Option<&StoreListing> {
        self.gift_codes.listings.get(sku)
    }

    /// SubscriptionPlanStore `get`.
    pub fn gift_subscription_plan(&self, plan: &str) -> Option<&SubscriptionPlan> {
        self.gift_codes.plans.get(plan)
    }

    /// UserStore `getUser` for a gifter a resolve brought.
    pub fn gift_gifter(&self, user: &str) -> Option<&Gifter> {
        self.gift_codes.gifters.get(user)
    }

    /// A listing fetched for a gift whose resolve brought none (`STORE_LISTING_FETCH_SUCCESS`).
    pub fn gift_store_listing_fetched(&mut self, listing: &Value) {
        self.gift_codes
            .take_resolve_parts(&serde_json::json!({"store_listing": listing}));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn e7_takes_gift_then_promotion_links() {
        assert_eq!(
            gift_codes_in("https://discord.gift/AbCd-1234-EfGh and discord.com/gifts/xyz"),
            vec!["AbCd1234EfGh", "xyz"]
        );
        // Case-insensitive hosts, at the start or after a space.
        assert_eq!(gift_codes_in("DISCORD.GIFT/abc"), vec!["abc"]);
        // Promotion links come after every gift link, whatever their order in the text.
        assert_eq!(
            gift_codes_in(
                "https://discord.com/billing/promotions/promo-1 https://discord.gift/gift1"
            ),
            vec!["gift1", "promo1"]
        );
        assert_eq!(
            gift_codes_in("promos.discord.gg?code=AAA promos.discord.gg/?code=BBB"),
            vec!["AAA", "BBB"]
        );
    }

    #[test]
    fn e7_wants_a_space_start_or_scheme_before_the_host() {
        // `canary.discord.com/gifts/` is no match: `canary.` precedes the host.
        assert!(gift_codes_in("https://canary.discord.com/gifts/abc").is_empty());
        // A newline is not a space.
        assert!(gift_codes_in("look\ndiscord.gift/abc").is_empty());
        assert!(gift_codes_in("(discord.gift/abc)").is_empty());
        assert!(gift_codes_in("https://discord.gift/").is_empty());
        assert!(gift_codes_in("discord.giftx/abc").is_empty());
    }

    #[test]
    fn e7_stops_at_three_distinct_codes() {
        assert_eq!(
            gift_codes_in(
                "discord.gift/a discord.gift/a discord.gift/b discord.gift/c discord.gift/d"
            ),
            vec!["a", "b", "c"]
        );
    }

    fn embed(kind: &str, url: &str) -> crate::Embed {
        crate::Embed {
            kind: kind.to_owned(),
            url: Some(url.to_owned()),
            ..Default::default()
        }
    }

    #[test]
    fn a_custom_gift_message_reads_its_embed() {
        let embeds = vec![embed("gift", "https://discord.gift/fromembed")];
        assert_eq!(
            message_gift_codes(41, "discord.gift/fromcontent", &embeds),
            vec!["fromembed"]
        );
        // Another type, or two embeds, reads the content.
        assert_eq!(
            message_gift_codes(0, "discord.gift/fromcontent", &embeds),
            vec!["fromcontent"]
        );
        let two = vec![embeds[0].clone(), embed("rich", "x")];
        assert_eq!(
            message_gift_codes(41, "discord.gift/fromcontent", &two),
            vec!["fromcontent"]
        );
    }

    fn resolved() -> Value {
        json!({
            "code": "gift1",
            "sku_id": "1",
            "application_id": COLLECTIBLES_APPLICATION_ID,
            "uses": 0,
            "max_uses": 1,
            "redeemed": false,
            "gift_style": 4,
            "user": {"id": "9", "username": "wumpus", "discriminator": "0"},
            "store_listing": {"id": "5", "sku": {"id": "1", "name": "Decoration", "type": 3, "application_id": COLLECTIBLES_APPLICATION_ID}},
        })
    }

    #[test]
    fn resolve_moves_the_code_through_the_lists() {
        let mut stores = Stores::default();
        stores.dispatch("GIFT_CODE_RESOLVE", &json!({"code": "gift1"}));
        assert!(stores.gift_code_is_resolving("gift1"));
        assert!(!stores.gift_code_is_resolved("gift1"));
        stores.dispatch(
            "GIFT_CODE_RESOLVE_SUCCESS",
            &json!({"giftCode": resolved()}),
        );
        assert!(!stores.gift_code_is_resolving("gift1"));
        assert!(stores.gift_code_is_resolved("gift1"));
        let gift = stores.gift_code("gift1").unwrap();
        assert_eq!(gift.gift_style, Some(4));
        assert!(gift.is_collectibles());
        assert!(!gift.is_subscription());
        assert_eq!(stores.gift_sku("1").unwrap().name, "Decoration");
        assert_eq!(stores.gift_gifter("9").unwrap().tag(), "wumpus");
        // A failed resolve keeps its error and counts as resolved.
        stores.dispatch("GIFT_CODE_RESOLVE", &json!({"code": "bad"}));
        stores.dispatch(
            "GIFT_CODE_RESOLVE_FAILURE",
            &json!({"code": "bad", "error": {"code": 10038, "message": "Unknown Gift Code"}}),
        );
        assert!(stores.gift_code_is_resolved("bad"));
        assert!(stores.gift_code("bad").is_none());
        assert_eq!(stores.gift_code_error("bad").unwrap()["code"], 10038);
    }

    #[test]
    fn redeem_answers_change_the_record() {
        let mut stores = Stores::default();
        stores.dispatch(
            "GIFT_CODE_RESOLVE_SUCCESS",
            &json!({"giftCode": resolved()}),
        );
        stores.dispatch("GIFT_CODE_REDEEM", &json!({"code": "gift1"}));
        assert!(stores.gift_code_is_accepting("gift1"));
        stores.dispatch("GIFT_CODE_REDEEM_SUCCESS", &json!({"code": "gift1"}));
        assert!(!stores.gift_code_is_accepting("gift1"));
        let gift = stores.gift_code("gift1").unwrap();
        assert!(gift.redeemed);
        assert_eq!(gift.uses, 1);
        assert!(gift.is_claimed());

        let mut stores = Stores::default();
        stores.dispatch(
            "GIFT_CODE_RESOLVE_SUCCESS",
            &json!({"giftCode": resolved()}),
        );
        stores.dispatch(
            "GIFT_CODE_REDEEM_FAILURE",
            &json!({"code": "gift1", "error": {"code": 50050}}),
        );
        assert!(stores.gift_code("gift1").unwrap().is_claimed());
        stores.dispatch(
            "GIFT_CODE_REDEEM_FAILURE",
            &json!({"code": "gift1", "error": {"code": 10038}}),
        );
        assert!(stores.gift_code("gift1").unwrap().revoked);
        assert_eq!(stores.gift_code_error("gift1").unwrap()["code"], 10038);
        // A later resolve keeps the revoked mark (`merge` leaves fields the server does not send).
        stores.dispatch(
            "GIFT_CODE_RESOLVE_SUCCESS",
            &json!({"giftCode": resolved()}),
        );
        assert!(stores.gift_code("gift1").unwrap().revoked);
    }

    #[test]
    fn an_expired_code_reads_as_absent_and_updates_only_raise_uses() {
        let mut stores = Stores::default();
        let mut value = resolved();
        value["expires_at"] = json!("2026-01-01T00:00:00+00:00");
        value["max_uses"] = json!(3);
        stores.dispatch("GIFT_CODE_RESOLVE_SUCCESS", &json!({"giftCode": value}));
        let before = DateTime::parse_from_rfc3339("2025-12-31T00:00:00+00:00")
            .unwrap()
            .with_timezone(&Utc);
        assert!(stores.gift_code_at("gift1", before).is_some());
        assert!(stores.gift_code("gift1").is_none());
        stores.dispatch("GIFT_CODE_UPDATE", &json!({"code": "gift1", "uses": 2}));
        stores.dispatch("GIFT_CODE_UPDATE", &json!({"code": "gift1", "uses": 1}));
        let gift = stores.gift_code_at("gift1", before).unwrap();
        assert_eq!(gift.uses, 2);
        assert_eq!(gift.remaining_uses(), 1);
        assert!(gift.has_multiple_copies());
        stores.dispatch("GIFT_CODE_REVOKE_SUCCESS", &json!({"code": "gift1"}));
        assert!(stores.gift_code_at("gift1", before).is_none());
    }

    #[test]
    fn the_subscription_plan_comes_with_the_resolve() {
        let mut stores = Stores::default();
        let mut value = resolved();
        value["subscription_plan"] = json!({"id": "511651880837840896", "sku_id": "521847234246082599", "name": "Nitro Monthly", "interval": 1, "interval_count": 1});
        stores.dispatch("GIFT_CODE_RESOLVE_SUCCESS", &json!({"giftCode": value}));
        let gift = stores.gift_code("gift1").unwrap();
        assert!(gift.is_subscription());
        assert_eq!(
            gift.subscription_plan_id.as_deref(),
            Some("511651880837840896")
        );
        assert!(
            stores
                .gift_subscription_plan("511651880837840896")
                .unwrap()
                .is_month()
        );
    }

    #[test]
    fn flags_read_like_the_record() {
        let mut value = resolved();
        value["flags"] = json!(6);
        let gift = GiftCode::from_value(&value).unwrap();
        assert!(!gift.is_self_redeemable());
        assert!(gift.is_existing_premium_subscription_disallowed());
    }
}
