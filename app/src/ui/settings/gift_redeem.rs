//! Gift Inventory > Redeem Codes: the code field, the lookup and the accept dialog.
//!
//! Official flow (`cz`, module 158935; `Vd`, `v3`, `UJ`, `n$`, `e_`, `D`/`b` of module 45938; `redeemGiftCode` of module
//! 75255; the modal is module 935263):
//!
//! - Redeem (or Enter) trims the text, normalises it (`Vd`: the last `/` segment, an optional `WUMP-` prefix, one of
//!   the four code shapes, the dashes dropped) and sends `GET /entitlements/gift-codes/{code}` with
//!   `with_application=false&with_subscription_plan=false`. A text that is no code, a failed lookup and a code that
//!   holds a promotion all end as the error "Invalid code entered" under the box; the promotion also offers its own
//!   page (`gPt3PE`).
//! - A found gift opens the accept dialog and empties the box. Its button sends
//!   `POST /entitlements/gift-codes/{code}/redeem` with `{channel_id, gateway_checkout_context}`; the answer's error
//!   code picks the text (`b`).
//!
//! A harness run is inert: the lookup (a GET) goes out, the redeem is refused by `net.rs` and its answer shows the
//! dialog's generic failure text. The dialog's own layout has no capture of the official (the bundle's markup is read,
//! not measured).

use super::Root;
use crate::editor::{Editor, EditorEvent};
use crate::net::Request;
use crate::ui::{color, editor_style, icon, modal};
use gpui::{
    AnyElement, Context, Entity, FontWeight, IntoElement, Subscription, Window, div, prelude::*, px,
};
use serde_json::{Value, json};

pub(super) const RESOLVE_KEY: &str = "settings-gift-resolve";
pub(super) const REDEEM_KEY: &str = "settings-gift-redeem";

/// `GIFT_CODE_RESOLVE` / `GIFT_CODE_REDEEM` of module 652215, on the REST root.
const GIFT_CODES: &str = "/entitlements/gift-codes/";

/// The characters of the 4/5-group shapes (`N` in module 45938): no `I`, `L`, `O`, `i`, `l`, `o`, `0` or `1`.
const CLEAR: &str = "abcdefghjkmnpqrstuvwxyzABCDEFGHJKMNPQRSTUVWXYZ23456789";

/// One group of a code shape: its length and the characters it takes.
#[derive(Clone, Copy)]
enum Kind {
    Clear,
    Letters,
    Alnum,
}

impl Kind {
    fn takes(self, c: char) -> bool {
        match self {
            Kind::Clear => CLEAR.contains(c),
            Kind::Letters => c.is_ascii_alphabetic(),
            Kind::Alnum => c.is_ascii_alphanumeric(),
        }
    }
}

/// The four shapes of `C` (module 45938): `N(4,4)`, `N(4,6)`, `N(5,3)` and four letters, four letters or digits, four
/// letters. Each pair of groups may be joined by one dash (`-?`).
const SHAPES: [&[(usize, Kind)]; 4] = [
    &[(4, Kind::Clear); 4],
    &[(4, Kind::Clear); 6],
    &[(5, Kind::Clear); 3],
    &[(4, Kind::Letters), (4, Kind::Alnum), (4, Kind::Letters)],
];

fn matches_shape(text: &str, shape: &[(usize, Kind)]) -> bool {
    let chars: Vec<char> = text.chars().collect();
    let mut at = 0;
    for (index, (len, kind)) in shape.iter().enumerate() {
        if index > 0 && chars.get(at) == Some(&'-') {
            at += 1;
        }
        if at + len > chars.len() || !chars[at..at + len].iter().all(|c| kind.takes(*c)) {
            return false;
        }
        at += len;
    }
    at == chars.len()
}

/// `Vd` (module 45938): the code of what was typed or pasted, `None` when it is no code. The last `/` segment is
/// taken (a gift link), `^(WUMP-?)?(shape)$` must match and the dashes are dropped.
pub(super) fn normalize(input: &str) -> Option<String> {
    let last = input.trim().rsplit('/').next()?;
    let mut bare = vec![last];
    if let Some(rest) = last.strip_prefix("WUMP") {
        // The prefix is greedy, with the dash it may carry; the regex falls back to no prefix when the rest fails.
        bare.insert(0, rest.strip_prefix('-').unwrap_or(rest));
        if rest.starts_with('-') {
            bare.insert(1, rest);
        }
    }
    bare.into_iter()
        .find(|text| SHAPES.iter().any(|shape| matches_shape(text, shape)))
        .map(|text| text.replace('-', ""))
}

/// The lookup of a code (`GIFT_CODE_RESOLVE` with the two flags `resolveGiftCode(code)` leaves false).
pub(super) fn resolve_route(code: &str) -> String {
    format!("{GIFT_CODES}{code}?with_application=false&with_subscription_plan=false")
}

/// The lookup GiftCodeStore sends for a code in a message (`resolveGiftCode(code, false, true)`, module 30793).
pub(in crate::ui) fn message_resolve_route(code: &str) -> String {
    format!("{GIFT_CODES}{code}?with_application=false&with_subscription_plan=true")
}

pub(super) fn redeem_route(code: &str) -> String {
    format!("{GIFT_CODES}{code}/redeem")
}

/// The body `redeemGiftCode` (module 75255) sends without a payment source: `channel_id` is the channel the gift was
/// opened from (the embed's `channelContext`, `null` from Gift Inventory), `payment_source_id` is `undefined` (left
/// out of the JSON) and `gateway_checkout_context` is `null` (`ob`, module 739508).
pub(super) fn redeem_body(channel: Option<dc_state::Id>) -> Value {
    json!({"channel_id": channel.map(|id| id.to_string()), "gateway_checkout_context": null})
}

fn resolve_request(code: &str) -> Request {
    Request::Rest {
        key: Some(RESOLVE_KEY.to_owned()),
        method: reqwest::Method::GET,
        route: resolve_route(code),
        body: None,
    }
}

fn redeem_request(code: &str, channel: Option<dc_state::Id>) -> Request {
    Request::Rest {
        key: Some(REDEEM_KEY.to_owned()),
        method: reqwest::Method::POST,
        route: redeem_route(code),
        body: Some(redeem_body(channel)),
    }
}

/// The API error codes `b` (module 45938) tells apart (`t02` of module 652215).
const UNKNOWN_GIFT_CODE: i64 = 10038;
const INVALID_GIFT_REDEMPTION_EXHAUSTED: i64 = 50050;
const INVALID_GIFT_REDEMPTION_OWNED: i64 = 50051;
const INVALID_GIFT_SELF_REDEMPTION: i64 = 50054;
const INVALID_GIFT_REDEMPTION_FRAUD_REJECTED: i64 = 50097;
const INVALID_GIFT_REDEMPTION_SUBSCRIPTION_MANAGED: i64 = 100021;
const INVALID_GIFT_REDEMPTION_SUBSCRIPTION_INCOMPATIBLE: i64 = 100023;
const INVALID_GIFT_REDEMPTION_INVOICE_OPEN: i64 = 100024;
const BILLING_NON_REFUNDABLE_PAYMENT_SOURCE: i64 = 100060;

/// `b(error, user)`: the text of a failed redeem. `nitro` says the account has Nitro (`YE(user, TIER_2)`), which
/// names the plan the account is already on.
pub(super) fn error_message(code: Option<i64>, nitro: bool) -> String {
    match code {
        // `wa9h7F`
        Some(INVALID_GIFT_SELF_REDEMPTION) => {
            dc_intl::tr("You can't accept this gift. You need to give this gift to someone else.").to_owned()
        }
        // `Iw2TUW`
        Some(INVALID_GIFT_REDEMPTION_EXHAUSTED) => {
            dc_intl::tr("You couldn't claim this gift because someone else beat you to the punch. That's jazz baby!").to_owned()
        }
        // `mdLtb5`
        Some(INVALID_GIFT_REDEMPTION_OWNED) => {
            dc_intl::tr("You couldn't claim this gift because you already have it.").to_owned()
        }
        // `roztIr`
        Some(UNKNOWN_GIFT_CODE) => {
            dc_intl::tr("Bummer! You couldn't claim this gift because it has been revoked.").to_owned()
        }
        // `4YTHKw` with `lG6a5x` or `FSOz78`
        Some(INVALID_GIFT_REDEMPTION_SUBSCRIPTION_INCOMPATIBLE) => dc_intl::format(
            "4YTHKw",
            &[(
                "planName",
                if nitro { dc_intl::t("lG6a5x") } else { dc_intl::t("FSOz78") }.into(),
            )],
        ),
        // `9i1J30`
        Some(INVALID_GIFT_REDEMPTION_SUBSCRIPTION_MANAGED) => {
            dc_intl::tr("Unfortunately, since you have an active subscription through an external provider, you cannot accept Nitro gifts.").to_owned()
        }
        // `U26WX+`
        Some(INVALID_GIFT_REDEMPTION_INVOICE_OPEN) => {
            dc_intl::tr("Unfortunately, you're unable to accept this gift because your subscription has an open invoice.").to_owned()
        }
        // `ypuSd8`
        Some(INVALID_GIFT_REDEMPTION_FRAUD_REJECTED) => {
            dc_intl::tr("Sorry, seems like something's gone wrong and you can't use that code.").to_owned()
        }
        // `mXMmWE`
        Some(BILLING_NON_REFUNDABLE_PAYMENT_SOURCE) => {
            dc_intl::tr("Sorry, this payment source cannot be used for redemption").to_owned()
        }
        // `s9+XlB`
        _ => dc_intl::tr("Something funky happened and we failed to secure the bag...try again?").to_owned(),
    }
}

/// The tier of a gift's SKU (`WN` of module 682901): 1 Classic, 2 Nitro, 3 Basic.
fn tier_of(sku: &str) -> Option<u64> {
    match sku {
        "978380684370378762" => Some(3),
        "521846918637420545" => Some(1),
        "521847234246082599" | "628379670982688768" | "521842865731534868" => Some(2),
        _ => None,
    }
}

/// A subscription plan: its interval (1 month, 2 year) and the number of them. The official reads the plan from the
/// subscription plan store; the ids are the `F` constants of module 682901.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(in crate::ui) struct Plan {
    pub year: bool,
    pub count: u64,
}

fn plan_of(id: &str) -> Option<Plan> {
    let (year, count) = match id {
        // Basic, Classic and Nitro by the month; Classic and the legacy plan too.
        "978380692553465866" | "511651871736201216" | "511651880837840896"
        | "511651856145973248" | "590665532894740483" | "628379151761408000" => (false, 1),
        "1024422698568122368"
        | "511651876987469824"
        | "511651885459963904"
        | "511651860671627264"
        | "590665538238152709"
        | "628381571568631808" => (true, 1),
        "642251038925127690" | "944037355453415424" | "944265614527037440" => (false, 3),
        "944037208325619722" | "944037391444738048" | "944265636643602432" => (false, 6),
        _ => return None,
    };
    Some(Plan { year, count })
}

/// What the lookup answered, as `GiftCode.createFromServer` (module 7133) reads it.
#[derive(Clone, Debug, PartialEq)]
pub(in crate::ui) struct Gift {
    pub code: String,
    pub sku_id: String,
    pub sku_name: String,
    pub plan_id: Option<String>,
    pub uses: u64,
    pub max_uses: u64,
    pub gift_style: Option<i64>,
    pub sender: String,
    pub promotion: bool,
    /// The resolve's `subscription_plan` (SubscriptionPlanStore), when it came with the answer.
    pub plan_detail: Option<Plan>,
}

impl Gift {
    pub(super) fn from_json(body: &Value) -> Option<Self> {
        let code = body["code"].as_str()?.to_owned();
        let text = |value: &Value| value.as_str().map(str::to_owned);
        let plan_id =
            text(&body["subscription_plan"]["id"]).or_else(|| text(&body["subscription_plan_id"]));
        let user = &body["user"];
        let sender = text(&user["global_name"])
            .or_else(|| text(&user["username"]))
            .unwrap_or_default();
        Some(Self {
            code,
            sku_id: text(&body["sku_id"]).unwrap_or_default(),
            sku_name: text(&body["store_listing"]["sku"]["name"]).unwrap_or_default(),
            plan_id,
            uses: body["uses"].as_u64().unwrap_or(0),
            max_uses: body["max_uses"].as_u64().unwrap_or(1),
            gift_style: body["gift_style"].as_i64(),
            sender,
            promotion: !body["promotion"].is_null(),
            plan_detail: dc_state::gift_codes::SubscriptionPlan::from_value(
                &body["subscription_plan"],
            )
            .map(|plan| Plan {
                year: !plan.is_month(),
                count: plan.interval_count,
            }),
        })
    }

    /// The accept dialog's gift from GiftCodeStore's record, the SKU's name and the gifter's name (the embed's
    /// `handleAccept`).
    pub(in crate::ui) fn from_store(
        gift: &dc_state::gift_codes::GiftCode,
        sku_name: &str,
        sender: &str,
    ) -> Self {
        Self {
            code: gift.code.clone(),
            sku_id: gift.sku_id.clone(),
            sku_name: sku_name.to_owned(),
            plan_id: gift.subscription_plan_id.clone(),
            uses: gift.uses,
            max_uses: gift.max_uses,
            gift_style: gift.gift_style.map(|style| style as i64),
            sender: sender.to_owned(),
            promotion: gift.promotion,
            plan_detail: gift.subscription_plan.as_ref().map(|plan| Plan {
                year: !plan.is_month(),
                count: plan.interval_count,
            }),
        }
    }

    /// `isSubscription`.
    fn is_subscription(&self) -> bool {
        self.plan_id.is_some()
    }

    /// `isClaimed`.
    fn is_claimed(&self) -> bool {
        self.uses >= self.max_uses
    }

    fn plan(&self) -> Option<Plan> {
        self.plan_detail
            .or_else(|| self.plan_id.as_deref().and_then(plan_of))
    }
}

/// The steps of the dialog (`frR`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(in crate::ui) enum Step {
    Open,
    Confirm,
    Success,
    Error,
}

/// The accept dialog's state (`tR`'s `state` and the store's accepting flag).
pub(in crate::ui) struct Modal {
    pub gift: Gift,
    /// The API error code of a failed redeem; `Some(None)` is a failure without one.
    pub error: Option<Option<i64>>,
    pub accepted: bool,
    pub accepting: bool,
    pub opened: bool,
    /// A gift with a style is shown wrapped first (`isCustomGift`).
    custom: bool,
    /// The channel the gift was opened from (`channelContext`), sent with the redeem.
    pub channel: Option<dc_state::Id>,
}

impl Modal {
    pub(in crate::ui) fn new(gift: Gift, channel: Option<dc_state::Id>) -> Self {
        let custom = gift.gift_style.is_some();
        Self {
            gift,
            error: None,
            accepted: false,
            accepting: false,
            opened: false,
            custom,
            channel,
        }
    }

    /// `v3`: with no error, a wrapped gift that is not opened, accepted or being accepted is the Open step; an accepted
    /// subscription is Success; the rest is Confirm. The library application is never known here (the gift's game is
    /// not modelled), so it is `null`, which keeps an error-free dialog out of the Error step.
    pub(in crate::ui) fn step(&self) -> Step {
        if self.error.is_some() {
            return Step::Error;
        }
        if self.custom && !self.opened && !self.accepted && !self.accepting {
            return Step::Open;
        }
        if self.accepted && self.gift.is_subscription() {
            Step::Success
        } else {
            Step::Confirm
        }
    }
}

fn plain(nodes: Vec<(String, Option<Box<str>>)>) -> String {
    nodes.into_iter().map(|(text, _)| text).collect()
}

/// `UJ`: the dialog's one button.
pub(super) fn button_text(step: Step, gift: &Gift, custom: bool) -> &'static str {
    match step {
        Step::Error => dc_intl::t("w19zb6"),
        Step::Success if gift.is_subscription() => dc_intl::t("ex5TKr"),
        Step::Success => dc_intl::t("OOkjql"),
        Step::Open => dc_intl::t("F8ktci"),
        Step::Confirm => {
            if custom {
                dc_intl::t("n6I6k4")
            } else if gift.gift_style.is_some() {
                if gift.is_claimed() {
                    dc_intl::t("OgpR0c")
                } else {
                    dc_intl::t("2BWscv")
                }
            } else if gift.is_subscription() {
                dc_intl::t("wQ1FHy")
            } else {
                dc_intl::t("OgpR0c")
            }
        }
    }
}

/// `n$`, and the custom branches of `firstHeaderText` (module 935263).
pub(super) fn header_text(modal: &Modal, step: Step, me: &str) -> String {
    let gift = &modal.gift;
    let sad_trombone = || plain(dc_intl::format_runs("JUvC0s", &[]));
    if modal.custom && step == Step::Error {
        return sad_trombone();
    }
    if modal.custom && !modal.accepted {
        if !modal.opened {
            return dc_intl::format("xHzRub", &[("recipientDisplayName", me.into())]);
        }
        // `0UR0u4`; `E7QYGI` (several months) lives in a bundle the string dump does not hold, so its en-US text is
        // written here. A Shop gift's item type (`collectiblesItemType`) is not fetched, so it takes `U4Ko2y`.
        return match gift.plan() {
            Some(plan) if !plan.year && plan.count > 1 => format!(
                "{} has gifted you Nitro for {} months!",
                gift.sender, plan.count
            ),
            Some(plan) => dc_intl::format(
                "0UR0u4",
                &[
                    ("sender", gift.sender.as_str().into()),
                    (
                        "timeInterval",
                        if plan.year {
                            dc_intl::t("tfqrhj")
                        } else {
                            dc_intl::t("FPybU7")
                        }
                        .into(),
                    ),
                ],
            ),
            None => dc_intl::format("U4Ko2y", &[("sender", gift.sender.as_str().into())]),
        };
    }
    let sku = || [("skuName", gift.sku_name.as_str().into())];
    match step {
        Step::Error => sad_trombone(),
        Step::Success if gift.is_subscription() => dc_intl::format("1C2BG/", &sku()),
        Step::Success => dc_intl::t("+BNMcF").to_owned(),
        _ if gift.is_subscription() => dc_intl::format("2VN4N9", &sku()),
        _ => dc_intl::t("RmamAI").to_owned(),
    }
}

/// `secondHeaderText`: a wrapped gift names its sender until it is opened (`DDO4Wz`).
pub(super) fn second_header_text(modal: &Modal) -> Option<String> {
    (modal.custom && !modal.opened && !modal.accepted)
        .then(|| dc_intl::format("DDO4Wz", &[("sender", modal.gift.sender.as_str().into())]))
}

/// `e_` and `D`: the text under the header.
pub(super) fn body_text(modal: &Modal, step: Step, nitro: bool) -> Option<String> {
    let gift = &modal.gift;
    if modal.custom && !modal.opened {
        return None;
    }
    let sku = || [("skuName", gift.sku_name.as_str().into())];
    Some(match step {
        Step::Error => error_message(modal.error.flatten(), nitro),
        Step::Success => match (gift.plan(), tier_of(&gift.sku_id)) {
            (Some(plan), Some(tier @ (1 | 2))) => {
                let key = match (tier, plan.year) {
                    (2, false) => "O2bEOt",
                    (2, true) => "ZEvHF+",
                    (_, false) => "gjKbF4",
                    (_, true) => "GIe7Bw",
                };
                dc_intl::format(
                    key,
                    &[("intervalCount", dc_intl::Arg::Num(plan.count as f64))],
                )
            }
            (Some(_), _) => dc_intl::t("5ayf7w").to_owned(),
            (None, _) => dc_intl::format("3CPsbo", &sku()),
        },
        Step::Confirm | Step::Open => match gift.plan() {
            // `P9eTKt` / `d8rUdy`, the count in bold.
            Some(plan) => plain(dc_intl::format_runs(
                if plan.year { "d8rUdy" } else { "P9eTKt" },
                &[
                    ("skuName", gift.sku_name.as_str().into()),
                    ("intervalCount", dc_intl::Arg::Num(plan.count as f64)),
                ],
            )),
            None => dc_intl::format("l6Ea4Z", &sku()),
        },
    })
}

/// What the page's field keeps between frames.
#[derive(Default)]
pub(in crate::ui) struct State {
    editor: Option<Entity<Editor>>,
    sub: Option<Subscription>,
    pub focused: bool,
    submitting: bool,
    error: bool,
    promo: bool,
    /// The code the lookup in flight asked about.
    asked: String,
    pub(in crate::ui) modal: Option<Modal>,
    /// The last request's outcome for the log and the harness.
    pub last: Option<String>,
}

impl State {
    pub(in crate::ui) fn open(&self) -> bool {
        self.modal.is_some()
    }

    /// Escape, the close button or the scrim: the dialog goes away and nothing is sent.
    pub(in crate::ui) fn close(&mut self) {
        self.modal = None;
    }
}

impl Root {
    /// The box exists from the time settings open (`state.codeInput` of the component, always mounted).
    pub(super) fn gift_field_ready(&mut self, cx: &mut Context<Self>) {
        if self.settings_extra.gift.editor.is_some() {
            return;
        }
        let editor = cx.new(|cx| {
            let mut editor = Editor::new(editor_style(), cx);
            editor.set_single_line(true);
            // Enter submits the text; the box keeps what was typed until the lookup succeeds.
            editor.set_keeps_text(true);
            editor.set_placeholder("WUMP-AAAAA-BBBBB-CCCCC".to_owned());
            editor
        });
        let sub = cx.subscribe(&editor, |this, _, event: &EditorEvent, cx| {
            match event {
                // `handleChange`: typing clears the error.
                EditorEvent::Edited => {
                    this.settings_extra.gift.error = false;
                    this.settings_extra.gift.promo = false;
                }
                EditorEvent::Submit(_) => this.gift_submit(cx),
                EditorEvent::Cancel => {}
            }
            cx.notify();
        });
        self.settings_extra.gift.sub = Some(sub);
        self.settings_extra.gift.editor = Some(editor);
    }

    /// Render hook: the box with the focus gets the active border.
    pub(in crate::ui) fn gift_focus(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(editor) = self.settings_extra.gift.editor.clone() else {
            return;
        };
        let now = gpui::Focusable::focus_handle(editor.read(cx), cx).is_focused(window);
        if now != self.settings_extra.gift.focused {
            self.settings_extra.gift.focused = now;
            cx.notify();
        }
    }

    fn gift_text(&self, cx: &gpui::App) -> String {
        self.settings_extra
            .gift
            .editor
            .as_ref()
            .map(|editor| editor.read(cx).text().to_owned())
            .unwrap_or_default()
    }

    /// `handleSubmit`: the lookup.
    pub(in crate::ui) fn gift_submit(&mut self, cx: &mut Context<Self>) {
        let text = self.gift_text(cx);
        if text.is_empty() || self.settings_extra.gift.submitting {
            return;
        }
        let Some(code) = normalize(&text) else {
            self.settings_extra.gift.error = true;
            self.settings_extra.gift.promo = false;
            self.settings_extra.gift.last = Some("no code".to_owned());
            cx.notify();
            return;
        };
        let gift = &mut self.settings_extra.gift;
        gift.submitting = true;
        gift.asked = code.clone();
        // `resolveGiftCode` dispatches `GIFT_CODE_RESOLVE` before its request (module 869038).
        self.stores
            .dispatch("GIFT_CODE_RESOLVE", &json!({"code": code.clone()}));
        let _ = self.requests.send(resolve_request(&code));
        cx.notify();
    }

    /// An answer of the lookup or of the redeem. True when the key was ours.
    pub(in crate::ui) fn gift_answered(
        &mut self,
        key: &str,
        body: &Value,
        cx: &mut Context<Self>,
    ) -> bool {
        let ok = body["ok"].as_bool() == Some(true);
        let status = body["status"].as_u64().unwrap_or(0);
        match key {
            RESOLVE_KEY => {
                let asked = self.settings_extra.gift.asked.clone();
                if ok {
                    self.stores.dispatch(
                        "GIFT_CODE_RESOLVE_SUCCESS",
                        &json!({"giftCode": body["body"].clone()}),
                    );
                } else {
                    self.stores.dispatch(
                        "GIFT_CODE_RESOLVE_FAILURE",
                        &json!({"code": asked, "error": body["body"].clone()}),
                    );
                }
                let gift = &mut self.settings_extra.gift;
                gift.submitting = false;
                let found = ok.then(|| Gift::from_json(&body["body"])).flatten();
                gift.last = Some(format!("resolve {status}"));
                match found {
                    // A promotion code is no gift: its own link under the box.
                    Some(found) if found.promotion => {
                        gift.promo = true;
                        gift.error = true;
                    }
                    Some(found) => {
                        gift.error = false;
                        gift.modal = Some(Modal::new(found, None));
                        if let Some(editor) = gift.editor.as_ref() {
                            editor.update(cx, |editor, _| editor.set_text(""));
                        }
                    }
                    None => gift.error = true,
                }
            }
            REDEEM_KEY => {
                if let Some(code) = self
                    .settings_extra
                    .gift
                    .modal
                    .as_ref()
                    .map(|modal| modal.gift.code.clone())
                {
                    if ok {
                        self.stores.dispatch(
                            "GIFT_CODE_REDEEM_SUCCESS",
                            &json!({"code": code, "entitlement": body["body"].clone()}),
                        );
                    } else {
                        self.stores.dispatch(
                            "GIFT_CODE_REDEEM_FAILURE",
                            &json!({"code": code, "error": body["body"].clone()}),
                        );
                    }
                }
                let gift = &mut self.settings_extra.gift;
                gift.last = Some(format!("redeem {status}"));
                if let Some(modal) = gift.modal.as_mut() {
                    modal.accepting = false;
                    if ok {
                        modal.accepted = true;
                    } else {
                        modal.error = Some(body["body"]["code"].as_i64());
                    }
                }
            }
            _ => return false,
        }
        cx.notify();
        true
    }

    /// The accept dialog's button.
    fn redeem_gift_pressed(&mut self, cx: &mut Context<Self>) {
        let Some(modal) = self.settings_extra.gift.modal.as_mut() else {
            return;
        };
        match modal.step() {
            Step::Error | Step::Success => self.settings_extra.gift.modal = None,
            Step::Open => modal.opened = true,
            Step::Confirm => {
                if modal.accepting {
                    return;
                }
                modal.accepting = true;
                let code = modal.gift.code.clone();
                let channel = modal.channel;
                // `redeemGiftCode` (module 75255): `GIFT_CODE_REDEEM`, then the POST.
                self.stores
                    .dispatch("GIFT_CODE_REDEEM", &json!({"code": code.clone()}));
                let _ = self.requests.send(redeem_request(&code, channel));
            }
        }
        cx.notify();
    }

    /// The accept dialog (`tR`): one button under a header and a text.
    pub(in crate::ui) fn gift_dialog(&self, _cx: &mut Context<Self>) -> Option<modal::Spec> {
        let modal = self.settings_extra.gift.modal.as_ref()?;
        let step = modal.step();
        let me = self
            .stores
            .current_user
            .as_ref()
            .map(|user| {
                user.global_name
                    .clone()
                    .unwrap_or_else(|| user.username.clone())
            })
            .unwrap_or_default();
        let nitro = self.premium_wire() == Some(2);
        let tone = if modal.accepting {
            modal::Tone::PrimaryLoading
        } else {
            modal::Tone::Primary
        };
        let second = second_header_text(modal);
        let body = body_text(modal, step, nitro);
        Some(modal::Spec {
            id: "settings-gift-modal",
            title: header_text(modal, step, &me).into(),
            subtitle: body
                .map(|text| div().child(text).into_any_element())
                .or(second.map(|text| div().child(text).into_any_element())),
            body: None,
            buttons: vec![modal::button(
                "settings-gift-primary",
                button_text(step, &modal.gift, modal.custom),
                tone,
                |this, _, cx| this.redeem_gift_pressed(cx),
            )],
            close: Box::new(|this, _, _| this.settings_extra.gift.close()),
        })
    }

    /// The code box and its Redeem button, drawn from the page (`cz`'s render): the box at y 59, 588 by 40, the button
    /// 100 by 40 beside it (`design/dom/settings/gift-inventory-darker-cozy.json`).
    pub(super) fn gift_redeem_row(&self, cx: &mut Context<Self>) -> AnyElement {
        let gift = &self.settings_extra.gift;
        let typed = !self.gift_text(cx).is_empty();
        let shown_error = gift.error && !gift.promo;
        let border = if shown_error {
            color("--border-feedback-critical")
        } else if gift.focused {
            color("--input-border-active")
        } else {
            color("--border-subtle")
        };
        let disabled = !typed;
        let mut field = div()
            .absolute()
            .left(px(0.))
            .top(px(59.))
            .w(px(588.))
            .h(px(40.))
            .rounded(px(8.))
            .border_1()
            .border_color(border)
            .bg(gpui::hsla(0., 0., 0., 0.12))
            .px(px(12.))
            .flex()
            .items_center();
        if let Some(editor) = gift.editor.clone() {
            field = field.child(div().flex_1().min_w(px(0.)).child(editor));
        }
        let mut row = div()
            .relative()
            .w_full()
            .h(px(106.))
            .child(
                div()
                    .absolute()
                    .left(px(0.))
                    .top(px(8.))
                    .text_size(px(16.))
                    .line_height(px(20.))
                    .font_weight(FontWeight(500.))
                    .text_color(color("--text-strong"))
                    .child(dc_intl::tr("Received a code for Nitro or a game?")),
            )
            .child(
                div()
                    .absolute()
                    .left(px(0.))
                    .top(px(33.))
                    .text_size(px(14.))
                    .line_height(px(18.))
                    .text_color(color("--text-muted"))
                    .child(dc_intl::tr("That\u{2019}s exciting! Enter it below.")),
            )
            .child(field)
            .child(
                div()
                    .id("gift-redeem")
                    .absolute()
                    .left(px(596.))
                    .top(px(59.))
                    .w(px(100.))
                    .h(px(40.))
                    .rounded(px(8.))
                    .border_1()
                    .border_color(gpui::hsla(0., 0., 1., 0.08))
                    .bg(color("--control-primary-background-default"))
                    .when(disabled, |this| this.opacity(0.5).cursor_not_allowed())
                    .when(!disabled && !gift.submitting, |this| this.cursor_pointer())
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_size(px(16.))
                    .line_height(px(20.))
                    .font_weight(FontWeight(500.))
                    .text_color(color("--white"))
                    .when(gift.submitting, |this| {
                        this.child(crate::ui::components::dots("gift-redeem", "--white"))
                    })
                    .when(!gift.submitting, |this| this.child(dc_intl::tr("Redeem")))
                    .on_click(cx.listener(|this, _, _, cx| {
                        if this.settings_extra.gift.submitting {
                            return;
                        }
                        this.gift_submit(cx);
                    })),
            );
        if shown_error {
            row = row.child(
                div()
                    .absolute()
                    .left(px(0.))
                    .top(px(107.))
                    .flex()
                    .items_center()
                    .gap(px(4.))
                    .child(icon("login_error", 16., "--text-feedback-critical"))
                    .child(
                        div()
                            .text_size(px(12.))
                            .line_height(px(16.))
                            .text_color(color("--text-feedback-critical"))
                            // `Y11a2u`
                            .child(dc_intl::tr("Invalid code entered")),
                    ),
            );
        }
        if gift.promo {
            let code = self.gift_text(cx);
            row = row.child(
                div()
                    .absolute()
                    .left(px(0.))
                    .top(px(107.))
                    .flex()
                    .gap(px(4.))
                    .text_size(px(12.))
                    .line_height(px(16.))
                    .text_color(color("--text-muted"))
                    // `gPt3PE`
                    .child("This looks like a promotion code.")
                    .child(
                        div()
                            .id("gift-promo-link")
                            .cursor_pointer()
                            .text_color(color("--text-link"))
                            .child("Click here")
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.settings_open_url(
                                    &format!("https://discord.com/billing/promotions/{code}"),
                                    cx,
                                );
                            })),
                    )
                    .child("to redeem."),
            );
        }
        if gift.error || gift.promo {
            row = row.h(px(130.));
        }
        row.into_any_element()
    }
}

/// The text a harness step types into the box and the Enter that submits it (`gift type <text>`, `gift submit`).
#[cfg(feature = "capture")]
impl Root {
    pub(crate) fn script_gift(&mut self, args: &[&str], cx: &mut Context<Self>) -> String {
        match args {
            ["type", text @ ..] => {
                self.gift_field_ready(cx);
                let text = text.join(" ");
                if let Some(editor) = self.settings_extra.gift.editor.clone() {
                    editor.update(cx, |editor, _| editor.set_text(&text));
                }
                self.settings_extra.gift.error = false;
                self.settings_extra.gift.promo = false;
                cx.notify();
                format!("typed {} characters", text.chars().count())
            }
            ["submit"] => {
                self.gift_submit(cx);
                format!(
                    "submitted, asked {:?}, last {:?}",
                    self.settings_extra.gift.asked, self.settings_extra.gift.last
                )
            }
            ["accept"] => {
                self.redeem_gift_pressed(cx);
                format!("accept, last {:?}", self.settings_extra.gift.last)
            }
            ["state"] => format!(
                "modal {:?} error {} promo {} submitting {} last {:?}",
                self.settings_extra
                    .gift
                    .modal
                    .as_ref()
                    .map(|modal| (modal.step(), modal.gift.code.clone())),
                self.settings_extra.gift.error,
                self.settings_extra.gift.promo,
                self.settings_extra.gift.submitting,
                self.settings_extra.gift.last
            ),
            _ => "gift: type <text> | submit | accept | state".to_owned(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_code_is_normalised_like_vd() {
        // The four shapes, with and without dashes and the `WUMP-` prefix.
        assert_eq!(
            normalize("WUMP-AAAAA-BBBBB-CCCCC").as_deref(),
            Some("AAAAABBBBBCCCCC")
        );
        assert_eq!(
            normalize("aaaaabbbbbccccc").as_deref(),
            Some("aaaaabbbbbccccc")
        );
        assert_eq!(
            normalize("abcd-efgh-jkmn-pqrs").as_deref(),
            Some("abcdefghjkmnpqrs")
        );
        assert_eq!(
            normalize("abcd-efgh-jkmn-pqrs-tuvw-xyza").as_deref(),
            Some("abcdefghjkmnpqrstuvwxyza")
        );
        // The letters/digits/letters shape of four-four-four.
        assert_eq!(normalize("AbCd1234EfGh").as_deref(), Some("AbCd1234EfGh"));
        assert_eq!(normalize("Abcd-1234-Efgh").as_deref(), Some("Abcd1234Efgh"));
        // A gift link: the last segment, trimmed.
        assert_eq!(
            normalize("  https://discord.gift/aaaaabbbbbccccc ").as_deref(),
            Some("aaaaabbbbbccccc")
        );
        assert_eq!(
            normalize("discord.com/gifts/WUMP-aaaaa-bbbbb-ccccc").as_deref(),
            Some("aaaaabbbbbccccc")
        );
    }

    #[test]
    fn a_text_that_is_no_code_is_refused() {
        // Too short, too long, a confusable character (`0`, `1`, `I`, `l`, `O`), another separator.
        assert_eq!(normalize("abc"), None);
        assert_eq!(normalize("aaaaa-bbbbb-cccc"), None);
        assert_eq!(normalize("aaaaabbbbbcccccdd"), None);
        assert_eq!(normalize("aaaaa0bbbbbccccc"), None);
        assert_eq!(normalize("aaaaa-bbbbb-cccc1"), None);
        assert_eq!(normalize("aaaaa bbbbb ccccc"), None);
        assert_eq!(normalize("aaaaa--bbbbbccccc"), None);
        assert_eq!(normalize(""), None);
        // The 12-character shape wants letters in its outer groups.
        assert_eq!(normalize("1234abcd5678"), None);
    }

    #[test]
    fn the_requests_have_the_official_routes_and_body() {
        assert_eq!(
            resolve_route("abcdefghjkmnpqrs"),
            "/entitlements/gift-codes/abcdefghjkmnpqrs?with_application=false&with_subscription_plan=false"
        );
        assert_eq!(
            redeem_route("abcdefghjkmnpqrs"),
            "/entitlements/gift-codes/abcdefghjkmnpqrs/redeem"
        );
        assert_eq!(
            redeem_body(None),
            json!({"channel_id": null, "gateway_checkout_context": null})
        );
        // From a gift card, the channel it sits in (`channelContext`).
        assert_eq!(
            redeem_body(Some(1146185577396469791)),
            json!({"channel_id": "1146185577396469791", "gateway_checkout_context": null})
        );
        assert_eq!(
            message_resolve_route("abc"),
            "/entitlements/gift-codes/abc?with_application=false&with_subscription_plan=true"
        );
        match resolve_request("abc") {
            Request::Rest { method, .. } => assert_eq!(method, reqwest::Method::GET),
            _ => panic!("the lookup is a REST read"),
        }
        match redeem_request("abc", None) {
            Request::Rest { method, body, .. } => {
                assert_eq!(method, reqwest::Method::POST);
                assert!(body.is_some());
            }
            _ => panic!("the redeem is a REST write"),
        }
    }

    #[test]
    fn the_redeem_is_a_write_and_the_lookup_is_a_read_for_the_inert_run() {
        assert_eq!(resolve_request("abc").inert(), crate::net::Inert::Allow);
        assert_eq!(redeem_request("abc", None).inert(), crate::net::Inert::Deny);
    }

    #[test]
    fn the_error_codes_pick_the_official_texts() {
        assert_eq!(
            error_message(Some(10038), true),
            "Bummer! You couldn't claim this gift because it has been revoked."
        );
        assert_eq!(
            error_message(Some(50051), true),
            "You couldn't claim this gift because you already have it."
        );
        assert!(
            error_message(Some(50050), true)
                .starts_with("You couldn't claim this gift because someone")
        );
        assert!(error_message(Some(50054), true).starts_with("You can't accept this gift."));
        assert!(error_message(Some(100023), true).ends_with("subscribed to Nitro."));
        assert!(error_message(Some(100023), false).ends_with("subscribed to Nitro Classic."));
        assert!(error_message(Some(100021), true).contains("external provider"));
        assert!(error_message(Some(100024), true).contains("open invoice"));
        assert!(error_message(Some(50097), true).starts_with("Sorry, seems like something"));
        assert!(error_message(Some(100060), true).contains("payment source"));
        // Any other code, and a failure without one.
        let generic = "Something funky happened and we failed to secure the bag...try again?";
        assert_eq!(error_message(Some(1), true), generic);
        assert_eq!(error_message(None, true), generic);
    }

    fn nitro_gift() -> Gift {
        Gift::from_json(&json!({
            "code": "abcdefghjkmnpqrs",
            "sku_id": "521847234246082599",
            "subscription_plan_id": "511651880837840896",
            "uses": 0,
            "max_uses": 1,
            "user": {"id": "1", "username": "wumpus", "global_name": "Wumpus"},
            "store_listing": {"sku": {"name": "Nitro"}},
        }))
        .unwrap()
    }

    #[test]
    fn a_subscription_gift_walks_confirm_success() {
        let mut modal = Modal::new(nitro_gift(), None);
        assert_eq!(modal.step(), Step::Confirm);
        assert_eq!(
            header_text(&modal, Step::Confirm, "me"),
            "You've been gifted Nitro"
        );
        assert_eq!(
            body_text(&modal, Step::Confirm, true).unwrap(),
            "Once you accept you will have Nitro for 1 month."
        );
        assert_eq!(button_text(Step::Confirm, &modal.gift, false), "I accept");
        modal.accepting = true;
        modal.accepting = false;
        modal.accepted = true;
        assert_eq!(modal.step(), Step::Success);
        assert_eq!(header_text(&modal, Step::Success, "me"), "You got Nitro!");
        assert_eq!(
            body_text(&modal, Step::Success, true).unwrap(),
            "Thanks to another generous human, you now have access to enhanced chat perks and Server Boosts for 1 month."
        );
        assert_eq!(button_text(Step::Success, &modal.gift, false), "Sweet!");
    }

    #[test]
    fn a_failed_redeem_is_the_error_step() {
        let mut modal = Modal::new(nitro_gift(), None);
        modal.error = Some(Some(50051));
        assert_eq!(modal.step(), Step::Error);
        assert_eq!(header_text(&modal, Step::Error, "me"), "Sad Trombone");
        assert_eq!(button_text(Step::Error, &modal.gift, false), "Oh ok.");
        assert_eq!(
            body_text(&modal, Step::Error, true).unwrap(),
            "You couldn't claim this gift because you already have it."
        );
    }

    #[test]
    fn a_styled_gift_opens_first_and_names_its_sender() {
        let mut gift = nitro_gift();
        gift.gift_style = Some(1);
        gift.plan_id = Some("642251038925127690".to_owned());
        let mut modal = Modal::new(gift, None);
        assert!(modal.custom);
        // `v3`: a wrapped gift starts at Open.
        assert_eq!(modal.step(), Step::Open);
        assert_eq!(button_text(Step::Open, &modal.gift, true), "Open gift");
        assert_eq!(header_text(&modal, Step::Open, "Me"), "To: Me");
        assert_eq!(second_header_text(&modal).as_deref(), Some("From: Wumpus"));
        assert_eq!(body_text(&modal, Step::Open, true), None);
        modal.opened = true;
        assert_eq!(modal.step(), Step::Confirm);
        assert_eq!(second_header_text(&modal), None);
        assert_eq!(
            header_text(&modal, Step::Confirm, "Me"),
            "Wumpus has gifted you Nitro for 3 months!"
        );
        assert_eq!(
            button_text(Step::Confirm, &modal.gift, true),
            "Woo! Accept Gift"
        );
        // Accepting a styled one-month gift names its interval.
        modal.gift.plan_id = Some("511651880837840896".to_owned());
        assert_eq!(
            header_text(&modal, Step::Confirm, "Me"),
            "Wumpus has gifted you Nitro for 1 month!"
        );
    }

    #[test]
    fn a_promotion_in_the_answer_is_not_a_gift() {
        let body = json!({"code": "x", "promotion": {"id": "1"}});
        assert!(Gift::from_json(&body).unwrap().promotion);
        assert!(!nitro_gift().promotion);
    }
}
