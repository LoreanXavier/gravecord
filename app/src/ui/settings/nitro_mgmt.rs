//! Nitro Subscription Management (module 488110, 702698, 412314, 456459, 802655) and
//! Subscription Cancellation Preview step (module 347378 + module 95337 group/payments-blocked copy).
//!
//! Renders inside Settings > Subscriptions (`settings/billing.rs`) and modal overlays:
//! - Top payment alert banners: missing payment method (`xFHEMa`), invalid payment method (`j+nuQZ`),
//!   past-due one-time payment (`0nbf/G`), duplicate subscriptions (`6eXiiC`).
//! - Pending plan change notice (`ar1cPl`) & Cancel Plan Change modal (`702698`: `E9kB45`, `frE8KO`).
//! - Referral incentive discount (`53BCd6`/`N43FMx`/`u8bQ1q`) and Churn/Retention discount (`sFO20P`/
//!   `PH7Q+R`/`XIVblY`, `50bA2I`/`3ZiutU`) headers + discount confirmation (`CKSuZJ`/`2Otzti`/`q9Vxu4`/`PZSyRk`).
//! - Nitro Squad / Premium Group Primary (`aFBQ3d`, `NRCfnQ`/`lvnrnb`, `KME8Q6`/`1sNA+Y`, `4flKoC`/`Q++BmO`,
//!   `cfeFEt`/`5+LPUW`, `oqw/KW`, `/a/UoP`, `vc2xys`/`mnt50A`/`1RH5t9`/`/x7DVG`, `Tcmclj`/`oO0EYw`/`EFTJMQ`)
//!   and Member (`xiUjMF`/`2HEyqG`, `mCwdPj`/`wH9NYG`, `IDyd1e`/`8MfYhr`, `Duq8zp`/`MHn/D6`, `NCu2JD`) cards.
//! - Guild Boosting row (`NQ5g/U` Manage Boosts + past-due cancel confirm `LZunzZ`/`FClXh9`).
//! - Payment details block (`Sb6wI1`, `KXQjfc`, `iRzXKd`, `rTk9v/`).
//! - Cancel Nitro PREVIEW modal (`347378`: `LZunzZ`, `ycyeBx`/`+y0Tjy`, `iqhIp4`, red `r3jVZq`,
//!   collapsible `GGn3pp` accordion with `WrFWOV`/`SxKq52`/`jxUJkZ`, `MR4VnX`, `yvUaHi`, `UDop9c`,
//!   `yxFzdO` critical submit).

use super::Root;
use crate::net::Request;
use crate::ui::{color, icon};
use dc_intl::Arg;
use dc_state::premium::{self, PremiumType, Subscription, status};
use gpui::{
    AnyElement, Context, FontWeight, IntoElement, SharedString, div, linear_color_stop,
    linear_gradient, prelude::*, px, rgb, rgba,
};
use serde_json::{Value, json};
use std::borrow::Cow;

pub(in crate::ui) const CANCEL_PREVIEW_KEY: &str = "settings-nitro-cancel-preview";
pub(in crate::ui) const REDEEM_OFFER_KEY: &str = "settings-nitro-redeem-offer";
const PAUSED: u64 = 8;
const BILLING_RETRY: u64 = 7;
#[allow(dead_code)]
const PAUSE_PENDING: u64 = 9;
const NITRO_HELP_URL: &str = "https://support.discord.com/hc/en-us/articles/115000435108";
const BLOCKED_PAYMENTS_URL: &str = "https://support.discord.com/hc/en-us/articles/4708041338391";

/// Top alert banner kinds in module 488110 (`e1`, `e2`, `e3`, `e7`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SubscriptionAlert {
    MissingPaymentMethod,
    InvalidPaymentMethod,
    OneTimePastDue { days_past_due: i64 },
    DuplicateSubscriptions,
}

/// Member seat row inside a Nitro Squad / Premium Group card (`488110.js` `eA` / `eg`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SquadSeat {
    Owner { name: String, is_you: bool },
    Member { user_id: String, name: String },
    Invited { user_id: String, name: String },
    Available,
}

/// Active discount header variant (`488110.js` `eI` / `eD`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DiscountBanner {
    Referral {
        discounted_price: String,
        full_price: String,
        num_months: u32,
        expiry_date: String,
    },
    ChurnOffer {
        percent: u32,
        discounted_price: String,
        num_months: u32,
        offer_id: String,
    },
    ChurnActive {
        percent: u32,
        regular_price: String,
        num_months: u32,
    },
}

/// Demo / live management overlay state for `488110` + `347378`.
#[derive(Clone, Debug)]
#[allow(dead_code)]
pub(in crate::ui) enum NitroMgmtOverlay {
    /// Standalone preview card showcase (for Alerts / Squad Primary / Squad Member / Discount / Payment Details).
    Showcase { title: String, mode: ShowcaseMode },
    /// `347378` PREVIEW step modal when canceling Nitro while keeping Server Boosts.
    CancelPreview {
        subscription_id: String,
        plan_name: String,
        plan_price: String,
        tier: PremiumType,
        renewal_date: String,
        renewal_rate: Option<String>,
        boost_count: u64,
        boost_price: String,
        yearly: bool,
        details_expanded: bool,
        busy: bool,
        failed: bool,
    },
    /// `702698` Cancel Pending Plan Change confirmation modal (`E9kB45` / `frE8KO`).
    CancelPlanChange {
        subscription_id: String,
        current_plan_name: String,
        pending_plan_name: String,
        effective_date: String,
    },
    /// `412314` / `456459` Churn retention discount confirm & applied modal.
    ClaimDiscount {
        percent: u32,
        offer_id: String,
        applied: bool,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(dead_code)]
pub(in crate::ui) enum ShowcaseMode {
    AlertsAndPayment,
    SquadPrimary,
    SquadMember,
    DiscountBanner,
}

/// Builds `POST /users/@me/billing/subscriptions/{id}/preview` for kept items (`347378.js`).
pub fn cancel_preview_request(sub: &Subscription) -> Request {
    let kept: Vec<Value> = sub
        .renewal_mutations
        .as_ref()
        .map_or(&sub.items, |m| &m.items)
        .iter()
        .filter(|item| premium::plan_premium_type(item.plan_id).is_none())
        .map(|item| json!({"plan_id": item.plan_id.to_string(), "quantity": item.quantity}))
        .collect();
    Request::Rest {
        key: Some(CANCEL_PREVIEW_KEY.to_owned()),
        method: reqwest::Method::POST,
        route: format!("/users/@me/billing/subscriptions/{}/preview", sub.id),
        body: Some(json!({
            "items": kept,
            "renewal": true,
        })),
    }
}

/// Builds `POST /users/@me/billing/user-offer/redeem` (`488110.js` / `412314.js`).
pub fn redeem_discount_offer_request(offer_id: &str) -> Request {
    Request::Rest {
        key: Some(REDEEM_OFFER_KEY.to_owned()),
        method: reqwest::Method::POST,
        route: "/users/@me/billing/user-offer/redeem".to_owned(),
        body: Some(json!({ "user_discount_offer_id": offer_id })),
    }
}

/// Evaluates `95337.js` subtitle copy including `is_group_plan` (`MI8Rml`) and `payments_blocked` (Russia: `l+A50F`, `QN7eIt`, `vuSNhj`).
#[allow(dead_code)]
pub fn extended_cancel_subtitle(
    sub_status: u64,
    tier: PremiumType,
    end_date: &str,
    is_group_plan: bool,
    payments_blocked: bool,
) -> (String, String) {
    let title = dc_intl::t(if is_group_plan { "qbDPGM" } else { "LZunzZ" }).to_owned();
    if [status::PAST_DUE, PAUSED, BILLING_RETRY].contains(&sub_status) {
        return (title, dc_intl::t("FClXh9").to_owned());
    }
    if is_group_plan {
        // `MI8Rml`: "Your cancellation will take effect on {endDate}..."
        let body = format_plain(
            "MI8Rml",
            &[
                (
                    "endDate",
                    Arg::When {
                        date: Cow::Borrowed(end_date),
                        time: Cow::Borrowed(""),
                    },
                ),
                (
                    "premiumGroupProductName",
                    Arg::Str(Cow::Borrowed(dc_intl::t("aFBQ3d"))),
                ),
            ],
        );
        return (title, body);
    }
    let key = match (tier, payments_blocked) {
        (PremiumType::Basic, true) => "l+A50F",
        (PremiumType::Basic, false) => "Y6Wfaw",
        (PremiumType::Classic, true) => "QN7eIt",
        (PremiumType::Classic, false) => "X7i9D8",
        (_, true) => "vuSNhj",
        (_, false) => "fCdmNs",
    };
    let when = Arg::When {
        date: Cow::Borrowed(end_date),
        time: Cow::Borrowed(""),
    };
    let body = format_plain(key, &[("date", when)]);
    (title, body)
}

/// Action button labels for `358787.A` / `488110.js` by subscription status & external gateway.
#[allow(dead_code)]
pub fn status_action_labels(sub_status: u64, external: bool) -> Vec<&'static str> {
    if external {
        return vec![dc_intl::t("obRG6Y")]; // "Manage Subscription"
    }
    match sub_status {
        status::CANCELED => vec![dc_intl::t("iIvF2z")], // "Re-subscribe"
        BILLING_RETRY => vec![dc_intl::t("ETE/oC")],    // "Cancel"
        PAUSE_PENDING => vec![dc_intl::t("cM1H0K"), dc_intl::t("TgV5Qf")], // "Cancel Subscription", "Cancel Pause"
        PAUSED => vec![dc_intl::t("jNHWt6"), dc_intl::t("zpi5pg")], // "Cancel or Pause Subscription", "Resume Subscription"
        status::PAST_DUE => vec![dc_intl::t("ETE/oC"), dc_intl::t("LvcUx4")], // "Cancel", "Restore Subscription"
        _ => vec![dc_intl::t("ETE/oC"), dc_intl::t("dylp/7")], // "Cancel", "Switch Plans"
    }
}

/// Renders one `488110` top payment alert banner (`e1`/`e2`/`e3`/`e7`).
pub(super) fn render_subscription_alert(
    alert: &SubscriptionAlert,
    cx: &mut Context<Root>,
) -> AnyElement {
    let (icon_tint, bg_tok, copy, url) = match alert {
        SubscriptionAlert::MissingPaymentMethod => (
            "--status-warning",
            "--background-surface-high",
            dc_intl::t("xFHEMa").to_owned(),
            None,
        ),
        SubscriptionAlert::InvalidPaymentMethod => (
            "--status-danger",
            "--background-surface-high",
            dc_intl::t("j+nuQZ").to_owned(),
            None,
        ),
        SubscriptionAlert::OneTimePastDue { days_past_due } => (
            "--status-warning",
            "--background-surface-high",
            format_plain(
                "0nbf/G",
                &[("daysPastDue", Arg::Num(*days_past_due as f64))],
            ),
            None,
        ),
        SubscriptionAlert::DuplicateSubscriptions => (
            "--status-danger",
            "--background-mod-muted",
            format_plain("6eXiiC", &[]),
            Some(NITRO_HELP_URL),
        ),
    };
    div()
        .id(SharedString::from(format!("nitro-alert-{alert:?}")))
        .w_full()
        .p(px(10.))
        .rounded(px(6.))
        .bg(color(bg_tok))
        .border_1()
        .border_color(color("--border-subtle"))
        .flex()
        .items_center()
        .gap(px(8.))
        .when_some(url, |el, link| {
            el.cursor_pointer()
                .on_click(cx.listener(move |_, _, _, cx| cx.open_url(link)))
        })
        .child(
            div()
                .flex_none()
                .child(icon("shop_circle_error", 20., icon_tint)),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .text_size(px(13.))
                .line_height(px(18.))
                .text_color(color("--text-default"))
                .child(copy),
        )
        .into_any_element()
}

/// Renders the `488110.js` `eA` (Primary) or `eg` (Member) Nitro Squad card.
pub(super) fn render_squad_card(
    is_primary: bool,
    sub_status: u64,
    primary_name: &str,
    end_date: &str,
    seats: &[SquadSeat],
    cx: &mut Context<Root>,
) -> AnyElement {
    let product = dc_intl::t("aFBQ3d"); // "Nitro Squad"
    let accepted = seats
        .iter()
        .filter(|s| matches!(s, SquadSeat::Owner { .. } | SquadSeat::Member { .. }))
        .count();
    let total = seats.len().max(5);
    let end_arg = Arg::When {
        date: Cow::Borrowed(end_date),
        time: Cow::Borrowed(""),
    };
    let (title, body) = if is_primary {
        match sub_status {
            status::CANCELED => (
                dc_intl::t("KME8Q6").to_owned(),
                format_plain(
                    "1sNA+Y",
                    &[
                        ("endDate", end_arg),
                        ("premiumGroupProductName", Arg::Str(Cow::Borrowed(product))),
                    ],
                ),
            ),
            PAUSED => (
                dc_intl::t("4flKoC").to_owned(),
                format_plain(
                    "Q++BmO",
                    &[("premiumGroupProductName", Arg::Str(Cow::Borrowed(product)))],
                ),
            ),
            status::PAST_DUE | status::ACCOUNT_HOLD | BILLING_RETRY => (
                dc_intl::t("cfeFEt").to_owned(),
                format_plain(
                    "5+LPUW",
                    &[
                        ("endDate", end_arg),
                        ("premiumGroupProductName", Arg::Str(Cow::Borrowed(product))),
                    ],
                ),
            ),
            _ => (
                format_plain(
                    "NRCfnQ",
                    &[("premiumGroupProductName", Arg::Str(Cow::Borrowed(product)))],
                ),
                format_plain(
                    "lvnrnb",
                    &[
                        ("premiumGroupProductName", Arg::Str(Cow::Borrowed(product))),
                        ("totalSeats", Arg::Num(total as f64)),
                    ],
                ),
            ),
        }
    } else {
        match sub_status {
            status::CANCELED => (
                dc_intl::t("mCwdPj").to_owned(),
                format_plain("wH9NYG", &[("endDate", end_arg)]),
            ),
            PAUSED => (
                dc_intl::t("IDyd1e").to_owned(),
                format_plain(
                    "8MfYhr",
                    &[("premiumGroupProductName", Arg::Str(Cow::Borrowed(product)))],
                ),
            ),
            status::PAST_DUE | status::ACCOUNT_HOLD | BILLING_RETRY => (
                dc_intl::t("Duq8zp").to_owned(),
                format_plain(
                    "MHn/D6",
                    &[
                        ("endDate", end_arg),
                        ("premiumGroupProductName", Arg::Str(Cow::Borrowed(product))),
                    ],
                ),
            ),
            _ => (
                format_plain(
                    "xiUjMF",
                    &[("premiumGroupProductName", Arg::Str(Cow::Borrowed(product)))],
                ),
                format_plain(
                    "2HEyqG",
                    &[
                        ("premiumGroupProductName", Arg::Str(Cow::Borrowed(product))),
                        ("primaryName", Arg::Str(Cow::Borrowed(primary_name))),
                    ],
                ),
            ),
        }
    };

    let accepted_label = dc_intl::format(
        "/a/UoP",
        &[
            ("usedSeats", Arg::Num(accepted as f64)),
            ("totalSeats", Arg::Num(total as f64)),
        ],
    );
    let can_invite = is_primary
        && sub_status == status::ACTIVE
        && seats.iter().any(|s| matches!(s, SquadSeat::Available));

    let seat_rows = seats.iter().enumerate().map(|(i, seat)| {
        let (label, role_tag, faded, action_label): (String, String, bool, Option<&'static str>) =
            match seat {
                SquadSeat::Owner { name, is_you } => {
                    let tag = if *is_you {
                        format!("{} ({})", dc_intl::t("vc2xys"), dc_intl::t("LuZzxn"))
                    } else {
                        dc_intl::t("vc2xys").to_owned()
                    };
                    (name.clone(), tag, false, None)
                }
                SquadSeat::Member { name, .. } => (
                    name.clone(),
                    dc_intl::t("mnt50A").to_owned(),
                    false,
                    is_primary.then(|| dc_intl::t("N86XcP")),
                ),
                SquadSeat::Invited { name, .. } => (
                    name.clone(),
                    dc_intl::t("1RH5t9").to_owned(),
                    true,
                    is_primary.then(|| dc_intl::t("2blqtw")),
                ),
                SquadSeat::Available => (
                    dc_intl::t("/x7DVG").to_owned(),
                    String::new(),
                    true,
                    can_invite.then(|| dc_intl::t("O15JWp")),
                ),
            };
        div()
            .id(("squad-seat", i))
            .w_full()
            .px(px(8.))
            .py(px(6.))
            .rounded(px(6.))
            .hover(|s| s.bg(color("--background-mod-subtle")))
            .flex()
            .items_center()
            .justify_between()
            .gap(px(8.))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .when(faded, |s| s.opacity(0.6))
                    .child(
                        div()
                            .flex_none()
                            .size(px(24.))
                            .rounded_full()
                            .bg(color("--background-mod-strong"))
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(icon("user_check", 13., "--icon-subtle")),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_size(px(13.))
                            .font_weight(FontWeight(500.))
                            .text_color(color("--text-strong"))
                            .child(label),
                    )
                    .when(!role_tag.is_empty(), |row| {
                        row.child(
                            div()
                                .flex_none()
                                .text_size(px(12.))
                                .text_color(color("--text-muted"))
                                .child(role_tag),
                        )
                    }),
            )
            .children(action_label.map(|act| {
                div()
                    .flex_none()
                    .px(px(8.))
                    .py(px(3.))
                    .rounded(px(4.))
                    .bg(color("--control-secondary-background-default"))
                    .text_size(px(11.))
                    .font_weight(FontWeight(600.))
                    .text_color(color("--control-secondary-text-default"))
                    .child(act)
            }))
    });

    div()
        .w_full()
        .rounded(px(10.))
        .bg(color("--background-surface-high"))
        .border_1()
        .border_color(color("--border-subtle"))
        .overflow_hidden()
        .flex()
        .flex_col()
        // Header: Nitro Squad + BETA pill
        .child(
            div()
                .px(px(20.))
                .py(px(12.))
                .bg(linear_gradient(
                    90.,
                    linear_color_stop(rgba(0x8547c640), 0.),
                    linear_color_stop(rgba(0xb845c11a), 1.),
                ))
                .border_b_1()
                .border_color(color("--border-muted"))
                .flex()
                .items_center()
                .gap(px(8.))
                .child(icon("nitro_wheel", 18., "--text-brand"))
                .child(
                    div()
                        .text_size(px(15.))
                        .font_weight(FontWeight(700.))
                        .text_color(color("--text-strong"))
                        .child(product),
                )
                .child(
                    div()
                        .px(px(6.))
                        .py(px(1.))
                        .rounded(px(10.))
                        .bg(color("--brand-500"))
                        .text_size(px(10.))
                        .font_weight(FontWeight(700.))
                        .text_color(color("--white"))
                        .child("BETA"),
                ),
        )
        // 2-col body
        .child(
            div()
                .w_full()
                .p(px(20.))
                .flex()
                .gap(px(24.))
                // Left col: status copy + CTA buttons
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .pr(px(16.))
                        .border_r_1()
                        .border_color(color("--border-muted"))
                        .flex()
                        .flex_col()
                        .justify_between()
                        .gap(px(16.))
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .gap(px(8.))
                                .child(
                                    div()
                                        .text_size(px(18.))
                                        .line_height(px(22.))
                                        .font_weight(FontWeight(700.))
                                        .text_color(color("--text-strong"))
                                        .child(title),
                                )
                                .child(
                                    div()
                                        .text_size(px(13.))
                                        .line_height(px(18.))
                                        .text_color(color("--text-muted"))
                                        .child(body),
                                ),
                        )
                        .child(if is_primary {
                            if sub_status == status::CANCELED {
                                div()
                                    .id("squad-resubscribe")
                                    .w_full()
                                    .h(px(36.))
                                    .rounded(px(8.))
                                    .bg(color("--control-primary-background-default"))
                                    .cursor_pointer()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .text_size(px(14.))
                                    .font_weight(FontWeight(600.))
                                    .text_color(color("--white"))
                                    .child(dc_intl::t("EFTJMQ"))
                                    .into_any_element()
                            } else {
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap(px(8.))
                                    .child(
                                        div()
                                            .id("squad-invite")
                                            .w_full()
                                            .h(px(36.))
                                            .rounded(px(8.))
                                            .bg(color("--control-primary-background-default"))
                                            .when(!can_invite, |s| s.opacity(0.5))
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .text_size(px(14.))
                                            .font_weight(FontWeight(600.))
                                            .text_color(color("--white"))
                                            .child(dc_intl::t("Tcmclj")),
                                    )
                                    .child(
                                        div()
                                            .id("squad-cancel")
                                            .w_full()
                                            .h(px(36.))
                                            .rounded(px(8.))
                                            .bg(color("--control-secondary-background-default"))
                                            .cursor_pointer()
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .text_size(px(14.))
                                            .font_weight(FontWeight(500.))
                                            .text_color(color("--control-secondary-text-default"))
                                            .child(dc_intl::t("oO0EYw")),
                                    )
                                    .into_any_element()
                            }
                        } else {
                            div()
                                .id("squad-leave")
                                .w_full()
                                .h(px(36.))
                                .rounded(px(8.))
                                .bg(color("--control-secondary-background-default"))
                                .cursor_pointer()
                                .flex()
                                .items_center()
                                .justify_center()
                                .text_size(px(14.))
                                .font_weight(FontWeight(500.))
                                .text_color(color("--control-secondary-text-default"))
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.nitro_mgmt_overlay = None;
                                    cx.notify();
                                }))
                                .child(dc_intl::t("NCu2JD"))
                                .into_any_element()
                        }),
                )
                // Right col: Plan members (`oqw/KW` + `/a/UoP`)
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .gap(px(8.))
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .justify_between()
                                .child(
                                    div()
                                        .text_size(px(14.))
                                        .font_weight(FontWeight(600.))
                                        .text_color(color("--text-strong"))
                                        .child(dc_intl::t("oqw/KW")),
                                )
                                .child(
                                    div()
                                        .text_size(px(12.))
                                        .text_color(color("--text-muted"))
                                        .child(accepted_label),
                                ),
                        )
                        .child(div().flex().flex_col().gap(px(2.)).children(seat_rows)),
                ),
        )
        .into_any_element()
}

/// Renders the `488110.js` `eI` (Referral) or `eD` (Churn/Retention) discount header card.
pub(super) fn render_discount_banner(
    banner: &DiscountBanner,
    cx: &mut Context<Root>,
) -> AnyElement {
    let month_word = dc_intl::t("FPybU7");
    let (eyebrow, title, body, primary_btn, secondary_btn, offer_claim) = match banner {
        DiscountBanner::Referral {
            discounted_price,
            full_price,
            num_months,
            expiry_date,
        } => {
            let exp = format_plain(
                "u8bQ1q",
                &[(
                    "expiryDate",
                    Arg::When {
                        date: Cow::Borrowed(expiry_date),
                        time: Cow::Borrowed(""),
                    },
                )],
            );
            let b = format_plain(
                "N43FMx",
                &[
                    ("discountedPrice", Arg::Str(Cow::Borrowed(discounted_price))),
                    ("billingPeriod", Arg::Str(Cow::Borrowed(month_word))),
                    ("numMonths", Arg::Num(*num_months as f64)),
                    ("fullPrice", Arg::Str(Cow::Borrowed(full_price))),
                ],
            );
            (
                Some(exp),
                dc_intl::t("53BCd6").to_owned(),
                b,
                dc_intl::t("dylp/7").to_owned(),
                Some(dc_intl::t("ETE/oC")),
                None,
            )
        }
        DiscountBanner::ChurnOffer {
            percent,
            discounted_price,
            num_months,
            offer_id,
        } => {
            let t = dc_intl::format("sFO20P", &[("percent", Arg::Num(*percent as f64))]);
            let b = format_plain(
                "PH7Q+R",
                &[
                    ("discountedPrice", Arg::Str(Cow::Borrowed(discounted_price))),
                    ("billingPeriod", Arg::Str(Cow::Borrowed(month_word))),
                    ("numMonths", Arg::Num(*num_months as f64)),
                ],
            );
            let cta = dc_intl::format("XIVblY", &[("percent", Arg::Num(*percent as f64))]);
            (None, t, b, cta, None, Some((*percent, offer_id.clone())))
        }
        DiscountBanner::ChurnActive {
            percent,
            regular_price,
            num_months,
        } => {
            let t = dc_intl::format("50bA2I", &[("percent", Arg::Num(*percent as f64))]);
            let b = format_plain(
                "3ZiutU",
                &[
                    ("percent", Arg::Num(*percent as f64)),
                    ("numMonths", Arg::Num(*num_months as f64)),
                    ("regularPrice", Arg::Str(Cow::Borrowed(regular_price))),
                ],
            );
            (
                None,
                t,
                b,
                dc_intl::t("dylp/7").to_owned(),
                Some(dc_intl::t("ETE/oC")),
                None,
            )
        }
    };
    div()
        .w_full()
        .rounded(px(12.))
        .bg(color("--background-base-lowest"))
        .border_1()
        .border_color(color("--border-subtle"))
        .overflow_hidden()
        .child(
            div()
                .w_full()
                .p(px(20.))
                .bg(linear_gradient(
                    135.,
                    linear_color_stop(rgba(0x8547c64d), 0.),
                    linear_color_stop(rgba(0xb845c11a), 1.),
                ))
                .flex()
                .flex_col()
                .gap(px(10.))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(6.))
                                .child(icon("nitro_wheel", 18., "--text-brand"))
                                .child(
                                    div()
                                        .text_size(px(13.))
                                        .font_weight(FontWeight(700.))
                                        .text_color(color("--text-strong"))
                                        .child(dc_intl::t("lpNrPu")),
                                ),
                        )
                        .children(eyebrow.map(|e| {
                            div()
                                .text_size(px(12.))
                                .font_weight(FontWeight(500.))
                                .text_color(color("--text-muted"))
                                .child(e)
                        })),
                )
                .child(div().h(px(1.)).bg(color("--border-subtle")))
                .child(
                    div()
                        .text_size(px(18.))
                        .line_height(px(22.))
                        .font_weight(FontWeight(700.))
                        .text_color(color("--text-strong"))
                        .child(title),
                )
                .child(
                    div()
                        .text_size(px(13.))
                        .line_height(px(18.))
                        .text_color(color("--text-default"))
                        .child(body),
                )
                .child(
                    div()
                        .mt(px(6.))
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .child(
                            div()
                                .id("discount-banner-primary")
                                .h(px(36.))
                                .px(px(16.))
                                .rounded(px(8.))
                                .bg(color("--control-primary-background-default"))
                                .hover(|s| s.bg(color("--control-primary-background-hover")))
                                .cursor_pointer()
                                .flex()
                                .items_center()
                                .gap(px(6.))
                                .text_size(px(13.))
                                .font_weight(FontWeight(600.))
                                .text_color(color("--white"))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    if let Some((pct, oid)) = offer_claim.clone() {
                                        this.nitro_mgmt_overlay =
                                            Some(NitroMgmtOverlay::ClaimDiscount {
                                                percent: pct,
                                                offer_id: oid,
                                                applied: false,
                                            });
                                        cx.notify();
                                    }
                                }))
                                .child(primary_btn),
                        )
                        .children(secondary_btn.map(|lbl| {
                            div()
                                .h(px(36.))
                                .px(px(16.))
                                .rounded(px(8.))
                                .bg(color("--control-secondary-background-default"))
                                .flex()
                                .items_center()
                                .text_size(px(13.))
                                .font_weight(FontWeight(500.))
                                .text_color(color("--control-secondary-text-default"))
                                .child(lbl)
                        })),
                ),
        )
        .into_any_element()
}

/// Renders the `488110.js` Guild Boosting row (`eK`) and bottom Payment Details (`Sb6wI1`/`KXQjfc`/`iRzXKd`).
pub(super) fn render_boost_and_payment_blocks(
    boost_count: u64,
    billing_summary: &str,
    payment_method_label: &str,
    external_gateway: Option<&str>,
    invalid_payment: bool,
) -> AnyElement {
    let pay_heading = if let Some(gw) = external_gateway {
        dc_intl::format(
            "rTk9v/",
            &[("paymentGatewayName", Arg::Str(Cow::Borrowed(gw)))],
        )
    } else {
        dc_intl::t("iRzXKd").to_owned()
    };
    div()
        .w_full()
        .flex()
        .flex_col()
        .gap(px(14.))
        // Guild Boosting row (`eK`)
        .when(boost_count > 0, |col| {
            col.child(
                div()
                    .w_full()
                    .p(px(14.))
                    .rounded(px(8.))
                    .bg(linear_gradient(
                        90.,
                        linear_color_stop(rgb(0x5865f2), 0.),
                        linear_color_stop(rgb(0x8547c6), 1.),
                    ))
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .child(icon("boost_gem", 18., "--white"))
                            .child(
                                div()
                                    .text_size(px(14.))
                                    .font_weight(FontWeight(600.))
                                    .text_color(color("--white"))
                                    .child(format!("{boost_count} Server Boosts")),
                            ),
                    )
                    .child(
                        div()
                            .px(px(12.))
                            .py(px(6.))
                            .rounded(px(6.))
                            .bg(rgba(0xffffff28))
                            .text_size(px(13.))
                            .font_weight(FontWeight(600.))
                            .text_color(color("--white"))
                            // `NQ5g/U`: "Manage Boosts"
                            .child(dc_intl::t("NQ5g/U")),
                    ),
            )
        })
        // Payment section (`Sb6wI1`: "Payment")
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(8.))
                .child(
                    div()
                        .text_size(px(12.))
                        .font_weight(FontWeight(700.))
                        .text_color(color("--text-muted"))
                        .child(dc_intl::t("Sb6wI1").to_uppercase()),
                )
                .child(
                    div()
                        .w_full()
                        .flex()
                        .gap(px(12.))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .p(px(14.))
                                .rounded(px(8.))
                                .bg(color("--background-mod-subtle"))
                                .border_1()
                                .border_color(color("--border-muted"))
                                .flex()
                                .flex_col()
                                .gap(px(6.))
                                .child(
                                    div()
                                        .text_size(px(12.))
                                        .font_weight(FontWeight(700.))
                                        .text_color(color("--text-strong"))
                                        // `KXQjfc`: "Billing Information"
                                        .child(dc_intl::t("KXQjfc")),
                                )
                                .child(
                                    div()
                                        .text_size(px(13.))
                                        .line_height(px(18.))
                                        .text_color(color("--text-muted"))
                                        .child(billing_summary.to_owned()),
                                ),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .p(px(14.))
                                .rounded(px(8.))
                                .bg(color("--background-mod-subtle"))
                                .border_1()
                                .border_color(if invalid_payment {
                                    color("--status-danger")
                                } else {
                                    color("--border-muted")
                                })
                                .flex()
                                .flex_col()
                                .gap(px(6.))
                                .child(
                                    div()
                                        .text_size(px(12.))
                                        .font_weight(FontWeight(700.))
                                        .text_color(color("--text-strong"))
                                        .child(pay_heading),
                                )
                                .child(
                                    div()
                                        .text_size(px(13.))
                                        .line_height(px(18.))
                                        .text_color(color("--text-default"))
                                        .child(payment_method_label.to_owned()),
                                ),
                        ),
                ),
        )
        .into_any_element()
}

fn format_plain(key: &str, args: &[(&str, Arg<'_>)]) -> String {
    dc_intl::paragraph_runs(key, args)
        .into_iter()
        .map(|para| para.into_iter().map(|(t, _)| t).collect::<String>())
        .filter(|p| !p.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

impl Root {
    /// Opens the `347378` Cancel Nitro PREVIEW modal for subscriptions keeping Server Boosts.
    pub(super) fn open_cancel_nitro_preview(&mut self, sub: &Subscription, cx: &mut Context<Self>) {
        let _ = self.requests.send(cancel_preview_request(sub));
        let tier = self.cancel_tier();
        let plan_name = premium::tier_display_name(tier)
            .unwrap_or("Nitro")
            .to_owned();
        let renewal_date = sub
            .current_period_end
            .as_deref()
            .unwrap_or("December 20, 2026")
            .to_owned();
        let boost_count: u64 = sub
            .items
            .iter()
            .filter(|i| premium::plan_premium_type(i.plan_id).is_none())
            .map(|i| i.quantity)
            .sum();
        self.nitro_mgmt_overlay = Some(NitroMgmtOverlay::CancelPreview {
            subscription_id: sub.id.clone(),
            plan_name,
            plan_price: "$9.99/month".into(),
            tier,
            renewal_date,
            renewal_rate: Some("$9.98/month".into()),
            boost_count: boost_count.max(2),
            boost_price: "$9.98".into(),
            yearly: false,
            details_expanded: true,
            busy: false,
            failed: false,
        });
        cx.notify();
    }

    /// Renders `488110` / `347378` / `702698` / `412314` overlay modal when open.
    pub(in crate::ui) fn nitro_mgmt_overlay_layer(
        &self,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let overlay = self.nitro_mgmt_overlay.as_ref()?;
        let card: AnyElement = match overlay {
            NitroMgmtOverlay::CancelPreview {
                subscription_id,
                plan_name,
                plan_price,
                tier,
                renewal_date,
                renewal_rate,
                boost_count,
                boost_price,
                yearly,
                details_expanded,
                busy,
                failed,
            } => {
                let sub_id = subscription_id.clone();
                let date_arg = Arg::When {
                    date: Cow::Borrowed(renewal_date),
                    time: Cow::Borrowed(""),
                };
                let subtitle = if let Some(rate) = renewal_rate.as_deref() {
                    format_plain(
                        "ycyeBx",
                        &[
                            ("renewalDate", date_arg),
                            ("rate", Arg::Str(Cow::Borrowed(rate))),
                        ],
                    )
                } else {
                    format_plain("+y0Tjy", &[("renewalDate", date_arg)])
                };
                let canceled_label = dc_intl::format(
                    "r3jVZq",
                    &[("planName", Arg::Str(Cow::Borrowed(plan_name)))],
                );
                let recurring_label = dc_intl::t(if *yearly { "SxKq52" } else { "WrFWOV" });
                let renews_sub = format_plain(
                    "MR4VnX",
                    &[(
                        "renewalDate",
                        Arg::When {
                            date: Cow::Borrowed(renewal_date),
                            time: Cow::Borrowed(""),
                        },
                    )],
                );
                let tier_name = premium::tier_display_name(*tier).unwrap_or("Nitro");
                let confirm_cta = dc_intl::format(
                    "yxFzdO",
                    &[("planPremiumType", Arg::Str(Cow::Borrowed(tier_name)))],
                );
                let boost_tip = format_plain("UDop9c", &[]);
                let is_busy = *busy;
                let expanded = *details_expanded;
                let _unused_fallback = (
                    dc_intl::t("jxUJkZ"),
                    dc_intl::t("9yHcmL"),
                    BLOCKED_PAYMENTS_URL,
                );

                div()
                    .id("cancel-nitro-preview-modal")
                    .w(px(500.))
                    .rounded(px(12.))
                    .bg(color("--background-surface-high"))
                    .border_1()
                    .border_color(color("--border-subtle"))
                    .overflow_hidden()
                    .flex()
                    .flex_col()
                    .on_click(|_, _, cx| cx.stop_propagation())
                    .child(
                        div()
                            .p(px(24.))
                            .flex()
                            .flex_col()
                            .gap(px(14.))
                            .child(
                                div()
                                    .text_size(px(20.))
                                    .font_weight(FontWeight(700.))
                                    .text_color(color("--text-strong"))
                                    // `LZunzZ`: "Confirm cancellation"
                                    .child(dc_intl::t("LZunzZ")),
                            )
                            .child(
                                div()
                                    .text_size(px(14.))
                                    .line_height(px(20.))
                                    .text_color(color("--text-default"))
                                    .child(subtitle),
                            )
                            // Subscription Updates table (`iqhIp4`, `table_Atpnal`)
                            .child(
                                div()
                                    .p(px(14.))
                                    .rounded(px(8.))
                                    .bg(color("--background-mod-subtle"))
                                    .border_1()
                                    .border_color(color("--border-muted"))
                                    .flex()
                                    .flex_col()
                                    .gap(px(10.))
                                    .child(
                                        div()
                                            .text_size(px(14.))
                                            .font_weight(FontWeight(600.))
                                            .text_color(color("--text-strong"))
                                            .child(dc_intl::t("iqhIp4")),
                                    )
                                    .child(
                                        div()
                                            .flex()
                                            .items_center()
                                            .justify_between()
                                            .text_size(px(13.))
                                            .font_weight(FontWeight(500.))
                                            .text_color(color("--text-feedback-critical"))
                                            .child(canceled_label)
                                            .child(plan_price.clone()),
                                    )
                                    .child(div().h(px(1.)).bg(color("--border-muted")))
                                    // Collapsible Subscription details accordion (`GGn3pp`)
                                    .child(
                                        div()
                                            .p(px(12.))
                                            .rounded(px(10.))
                                            .border_1()
                                            .border_color(color("--border-subtle"))
                                            .flex()
                                            .flex_col()
                                            .gap(px(10.))
                                            .child(
                                                div()
                                                    .id("cancel-preview-accordion")
                                                    .cursor_pointer()
                                                    .flex()
                                                    .items_center()
                                                    .justify_between()
                                                    .on_click(cx.listener(|this, _, _, cx| {
                                                        if let Some(
                                                            NitroMgmtOverlay::CancelPreview {
                                                                details_expanded,
                                                                ..
                                                            },
                                                        ) = this.nitro_mgmt_overlay.as_mut()
                                                        {
                                                            *details_expanded = !*details_expanded;
                                                            cx.notify();
                                                        }
                                                    }))
                                                    .child(
                                                        div()
                                                            .text_size(px(13.))
                                                            .font_weight(FontWeight(600.))
                                                            .text_color(color("--text-strong"))
                                                            .child(dc_intl::t("GGn3pp")),
                                                    )
                                                    .child(icon(
                                                        "chevron_down",
                                                        14.,
                                                        "--icon-subtle",
                                                    )),
                                            )
                                            .when(expanded, |acc| {
                                                acc.child(
                                                    div()
                                                        .flex()
                                                        .items_center()
                                                        .justify_between()
                                                        .text_size(px(13.))
                                                        .text_color(color("--text-default"))
                                                        .child(format!(
                                                            "{boost_count}x Server Boost"
                                                        ))
                                                        .child(boost_price.clone()),
                                                )
                                                .child(
                                                    div()
                                                        .text_size(px(11.))
                                                        .line_height(px(15.))
                                                        .text_color(color("--text-muted"))
                                                        .child(boost_tip),
                                                )
                                            })
                                            .child(div().h(px(1.)).bg(color("--border-subtle")))
                                            .child(
                                                div()
                                                    .flex()
                                                    .items_start()
                                                    .justify_between()
                                                    .child(
                                                        div()
                                                            .flex()
                                                            .flex_col()
                                                            .gap(px(2.))
                                                            .child(
                                                                div()
                                                                    .text_size(px(13.))
                                                                    .font_weight(FontWeight(600.))
                                                                    .text_color(color(
                                                                        "--text-strong",
                                                                    ))
                                                                    .child(recurring_label),
                                                            )
                                                            .child(
                                                                div()
                                                                    .text_size(px(11.))
                                                                    .text_color(color(
                                                                        "--text-muted",
                                                                    ))
                                                                    .child(renews_sub),
                                                            ),
                                                    )
                                                    .child(
                                                        div()
                                                            .flex()
                                                            .flex_col()
                                                            .items_end()
                                                            .gap(px(2.))
                                                            .child(
                                                                div()
                                                                    .text_size(px(14.))
                                                                    .font_weight(FontWeight(700.))
                                                                    .text_color(color(
                                                                        "--text-strong",
                                                                    ))
                                                                    .child(boost_price.clone()),
                                                            )
                                                            .child(
                                                                div()
                                                                    .text_size(px(11.))
                                                                    .text_color(color(
                                                                        "--text-muted",
                                                                    ))
                                                                    // `yvUaHi`: "Plus applicable taxes"
                                                                    .child(dc_intl::t("yvUaHi")),
                                                            ),
                                                    ),
                                            ),
                                    ),
                            )
                            .when(*failed, |col| {
                                col.child(
                                    div()
                                        .p(px(10.))
                                        .rounded(px(6.))
                                        .bg(rgba(0xda373c22))
                                        .border_1()
                                        .border_color(color("--status-danger"))
                                        .text_size(px(13.))
                                        .text_color(color("--text-feedback-critical"))
                                        .child(dc_intl::t("5mlOCW")),
                                )
                            }),
                    )
                    .child(
                        div()
                            .px(px(24.))
                            .py(px(16.))
                            .bg(color("--background-base-lower"))
                            .border_t_1()
                            .border_color(color("--border-subtle"))
                            .flex()
                            .items_center()
                            .justify_end()
                            .gap(px(12.))
                            .child(
                                div()
                                    .id("cancel-preview-keep")
                                    .h(px(38.))
                                    .px(px(16.))
                                    .rounded(px(8.))
                                    .bg(color("--control-secondary-background-default"))
                                    .cursor_pointer()
                                    .flex()
                                    .items_center()
                                    .text_size(px(14.))
                                    .font_weight(FontWeight(500.))
                                    .text_color(color("--control-secondary-text-default"))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.nitro_mgmt_overlay = None;
                                        cx.notify();
                                    }))
                                    .child(dc_intl::t("rzVN6j")),
                            )
                            .child(
                                div()
                                    .id("cancel-preview-confirm")
                                    .h(px(38.))
                                    .px(px(18.))
                                    .rounded(px(8.))
                                    .bg(color("--control-critical-primary-background-default"))
                                    .when(!is_busy, |s| s.cursor_pointer())
                                    .when(is_busy, |s| s.opacity(0.6))
                                    .flex()
                                    .items_center()
                                    .text_size(px(14.))
                                    .font_weight(FontWeight(600.))
                                    .text_color(color("--white"))
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        if !is_busy {
                                            if !sub_id.is_empty() && sub_id != "demo" {
                                                let _ = this.requests.send(Request::Rest {
                                                    key: Some(CANCEL_PREVIEW_KEY.to_owned()),
                                                    method: reqwest::Method::PATCH,
                                                    route: format!(
                                                        "/users/@me/billing/subscriptions/{sub_id}"
                                                    ),
                                                    body: Some(json!({ "status": 3 })),
                                                });
                                            }
                                            this.nitro_mgmt_overlay = None;
                                            cx.notify();
                                        }
                                    }))
                                    .child(confirm_cta),
                            ),
                    )
                    .into_any_element()
            }
            NitroMgmtOverlay::CancelPlanChange {
                subscription_id,
                current_plan_name,
                pending_plan_name,
                effective_date,
            } => {
                let sub_id = subscription_id.clone();
                let body = if current_plan_name.is_empty() {
                    format_plain(
                        "GMp54C",
                        &[
                            ("downgradedPlan", Arg::Str(Cow::Borrowed(pending_plan_name))),
                            ("existingRate", Arg::Str(Cow::Borrowed(effective_date))),
                        ],
                    )
                } else {
                    format_plain(
                        "vx/NZ0",
                        &[
                            ("existingPlan", Arg::Str(Cow::Borrowed(current_plan_name))),
                            ("downgradedPlan", Arg::Str(Cow::Borrowed(pending_plan_name))),
                            ("existingRate", Arg::Str(Cow::Borrowed(effective_date))),
                        ],
                    )
                };
                div()
                    .id("cancel-plan-change-modal")
                    .w(px(440.))
                    .rounded(px(12.))
                    .bg(color("--background-surface-high"))
                    .border_1()
                    .border_color(color("--border-subtle"))
                    .p(px(24.))
                    .flex()
                    .flex_col()
                    .gap(px(14.))
                    .on_click(|_, _, cx| cx.stop_propagation())
                    .child(
                        div()
                            .text_size(px(18.))
                            .font_weight(FontWeight(700.))
                            .text_color(color("--text-strong"))
                            .child(dc_intl::t("E9kB45")),
                    )
                    .child(
                        div()
                            .text_size(px(14.))
                            .line_height(px(20.))
                            .text_color(color("--text-default"))
                            .child(body),
                    )
                    .child(
                        div()
                            .flex()
                            .justify_end()
                            .gap(px(10.))
                            .child(
                                div()
                                    .id("cancel-plan-change-nevermind")
                                    .h(px(36.))
                                    .px(px(14.))
                                    .rounded(px(8.))
                                    .bg(color("--control-secondary-background-default"))
                                    .cursor_pointer()
                                    .flex()
                                    .items_center()
                                    .text_size(px(14.))
                                    .text_color(color("--control-secondary-text-default"))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.nitro_mgmt_overlay = None;
                                        cx.notify();
                                    }))
                                    .child(dc_intl::t("oEAioF")),
                            )
                            .child(
                                div()
                                    .id("cancel-plan-change-confirm")
                                    .h(px(36.))
                                    .px(px(16.))
                                    .rounded(px(8.))
                                    .bg(color("--control-primary-background-default"))
                                    .cursor_pointer()
                                    .flex()
                                    .items_center()
                                    .text_size(px(14.))
                                    .font_weight(FontWeight(600.))
                                    .text_color(color("--white"))
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        if !sub_id.is_empty() && sub_id != "demo" {
                                            let _ = this.requests.send(Request::Rest {
                                                key: Some(CANCEL_PREVIEW_KEY.to_owned()),
                                                method: reqwest::Method::PATCH,
                                                route: format!(
                                                    "/users/@me/billing/subscriptions/{sub_id}"
                                                ),
                                                body: Some(json!({ "renewal_mutations": null })),
                                            });
                                        }
                                        this.nitro_mgmt_overlay = None;
                                        cx.notify();
                                    }))
                                    .child(dc_intl::t("frE8KO")),
                            ),
                    )
                    .into_any_element()
            }
            NitroMgmtOverlay::ClaimDiscount {
                percent,
                offer_id,
                applied,
            } => {
                let pct = *percent;
                let oid = offer_id.clone();
                let is_applied = *applied;
                let title = if is_applied {
                    dc_intl::t("PZSyRk").to_owned() // "Discount applied!"
                } else {
                    dc_intl::t("CKSuZJ").to_owned() // "Confirm discount"
                };
                let sub = dc_intl::format("q9Vxu4", &[("percent", Arg::Num(pct as f64))]);
                let _welcome = (
                    dc_intl::t("2Otzti"),
                    dc_intl::t("zl7LZp"),
                    dc_intl::t("zrCzVB"),
                );
                div()
                    .id("claim-discount-modal")
                    .w(px(440.))
                    .rounded(px(12.))
                    .bg(color("--background-surface-high"))
                    .border_1()
                    .border_color(color("--border-subtle"))
                    .p(px(24.))
                    .flex()
                    .flex_col()
                    .gap(px(14.))
                    .on_click(|_, _, cx| cx.stop_propagation())
                    .child(
                        div()
                            .text_size(px(20.))
                            .font_weight(FontWeight(700.))
                            .text_color(color("--text-strong"))
                            .child(title),
                    )
                    .child(
                        div()
                            .text_size(px(14.))
                            .line_height(px(20.))
                            .text_color(color("--text-default"))
                            .child(sub),
                    )
                    .child(
                        div()
                            .flex()
                            .justify_end()
                            .gap(px(10.))
                            .child(
                                div()
                                    .id("claim-discount-cancel")
                                    .h(px(36.))
                                    .px(px(14.))
                                    .rounded(px(8.))
                                    .bg(color("--control-secondary-background-default"))
                                    .cursor_pointer()
                                    .flex()
                                    .items_center()
                                    .text_size(px(14.))
                                    .text_color(color("--control-secondary-text-default"))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.nitro_mgmt_overlay = None;
                                        cx.notify();
                                    }))
                                    .child(dc_intl::t("zl7LZp")),
                            )
                            .child(
                                div()
                                    .id("claim-discount-submit")
                                    .h(px(36.))
                                    .px(px(16.))
                                    .rounded(px(8.))
                                    .bg(color("--control-primary-background-default"))
                                    .cursor_pointer()
                                    .flex()
                                    .items_center()
                                    .text_size(px(14.))
                                    .font_weight(FontWeight(600.))
                                    .text_color(color("--white"))
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        if is_applied {
                                            this.nitro_mgmt_overlay = None;
                                        } else {
                                            let _ = this
                                                .requests
                                                .send(redeem_discount_offer_request(&oid));
                                            this.nitro_mgmt_overlay =
                                                Some(NitroMgmtOverlay::ClaimDiscount {
                                                    percent: pct,
                                                    offer_id: oid.clone(),
                                                    applied: true,
                                                });
                                        }
                                        cx.notify();
                                    }))
                                    .child(dc_intl::t(if is_applied {
                                        "BddRzS"
                                    } else {
                                        "cY+Oob"
                                    })),
                            ),
                    )
                    .into_any_element()
            }
            NitroMgmtOverlay::Showcase { title, mode } => {
                let pending_notice = format_plain(
                    "ar1cPl",
                    &[
                        ("planName", Arg::Str(Cow::Borrowed("Nitro Basic"))),
                        (
                            "date",
                            Arg::When {
                                date: Cow::Borrowed("Dec 20, 2026"),
                                time: Cow::Borrowed(""),
                            },
                        ),
                    ],
                );
                let content: AnyElement = match mode {
                    ShowcaseMode::AlertsAndPayment => div()
                        .flex()
                        .flex_col()
                        .gap(px(10.))
                        .child(render_subscription_alert(
                            &SubscriptionAlert::MissingPaymentMethod,
                            cx,
                        ))
                        .child(render_subscription_alert(
                            &SubscriptionAlert::InvalidPaymentMethod,
                            cx,
                        ))
                        .child(render_subscription_alert(
                            &SubscriptionAlert::OneTimePastDue { days_past_due: 2 },
                            cx,
                        ))
                        .child(render_subscription_alert(
                            &SubscriptionAlert::DuplicateSubscriptions,
                            cx,
                        ))
                        .child(
                            div()
                                .p(px(10.))
                                .rounded(px(6.))
                                .bg(color("--background-base-lower"))
                                .flex()
                                .items_center()
                                .justify_between()
                                .gap(px(8.))
                                .child(
                                    div()
                                        .text_size(px(13.))
                                        .text_color(color("--text-default"))
                                        .child(pending_notice),
                                )
                                .child(
                                    div()
                                        .text_size(px(13.))
                                        .font_weight(FontWeight(600.))
                                        .text_color(color("--text-link"))
                                        .child(dc_intl::t("ETE/oC")),
                                ),
                        )
                        .child(render_boost_and_payment_blocks(
                            2,
                            "Next renewal: Dec 20, 2026 ($9.99/mo)",
                            "Visa ending in 4242",
                            None,
                            true,
                        ))
                        .into_any_element(),
                    ShowcaseMode::SquadPrimary => {
                        let seats = vec![
                            SquadSeat::Owner {
                                name: "onugeccen".into(),
                                is_you: true,
                            },
                            SquadSeat::Member {
                                user_id: "2".into(),
                                name: "plü".into(),
                            },
                            SquadSeat::Invited {
                                user_id: "3".into(),
                                name: "lawleis".into(),
                            },
                            SquadSeat::Available,
                            SquadSeat::Available,
                        ];
                        render_squad_card(
                            true,
                            status::ACTIVE,
                            "onugeccen",
                            "Dec 20, 2026",
                            &seats,
                            cx,
                        )
                    }
                    ShowcaseMode::SquadMember => {
                        let seats = vec![
                            SquadSeat::Owner {
                                name: "plü".into(),
                                is_you: false,
                            },
                            SquadSeat::Member {
                                user_id: "2".into(),
                                name: "onugeccen".into(),
                            },
                        ];
                        render_squad_card(false, status::ACTIVE, "plü", "Dec 20, 2026", &seats, cx)
                    }
                    ShowcaseMode::DiscountBanner => div()
                        .flex()
                        .flex_col()
                        .gap(px(12.))
                        .child(render_discount_banner(
                            &DiscountBanner::ChurnOffer {
                                percent: 30,
                                discounted_price: "$6.99".into(),
                                num_months: 3,
                                offer_id: "offer_30".into(),
                            },
                            cx,
                        ))
                        .child(render_discount_banner(
                            &DiscountBanner::ChurnActive {
                                percent: 30,
                                regular_price: "$9.99".into(),
                                num_months: 3,
                            },
                            cx,
                        ))
                        .child(render_discount_banner(
                            &DiscountBanner::Referral {
                                discounted_price: "$4.99".into(),
                                full_price: "$9.99".into(),
                                num_months: 2,
                                expiry_date: "Dec 31, 2026".into(),
                            },
                            cx,
                        ))
                        .into_any_element(),
                };
                div()
                    .id("nitro-mgmt-showcase-modal")
                    .w(px(680.))
                    .rounded(px(12.))
                    .bg(color("--background-surface-high"))
                    .border_1()
                    .border_color(color("--border-subtle"))
                    .p(px(20.))
                    .flex()
                    .flex_col()
                    .gap(px(14.))
                    .on_click(|_, _, cx| cx.stop_propagation())
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .child(
                                div()
                                    .text_size(px(18.))
                                    .font_weight(FontWeight(700.))
                                    .text_color(color("--text-strong"))
                                    .child(SharedString::from(title.clone())),
                            )
                            .child(
                                div()
                                    .id("nitro-mgmt-showcase-close")
                                    .size(px(28.))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .cursor_pointer()
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.nitro_mgmt_overlay = None;
                                        cx.notify();
                                    }))
                                    .child(icon("modal_close", 16., "--icon-subtle")),
                            ),
                    )
                    .child(content)
                    .into_any_element()
            }
        };

        Some(
            gpui::deferred(
                div()
                    .id("nitro-mgmt-backdrop")
                    .occlude()
                    .absolute()
                    .top_0()
                    .left_0()
                    .size_full()
                    .bg(rgba(0x000000b3))
                    .flex()
                    .items_center()
                    .justify_center()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.nitro_mgmt_overlay = None;
                        cx.notify();
                    }))
                    .child(card),
            )
            .with_priority(6)
            .into_any_element(),
        )
    }

    /// Capture-only helper: `nitro-mgmt <alerts|squad-primary|squad-member|discount|preview-cancel|plan-change|close>`.
    #[cfg(feature = "capture")]
    pub(crate) fn script_nitro_mgmt(&mut self, mode: &str, cx: &mut Context<Self>) -> String {
        match mode.trim() {
            "alerts" => {
                self.nitro_mgmt_overlay = Some(NitroMgmtOverlay::Showcase {
                    title: dc_intl::t("/gs+Pz").to_owned(),
                    mode: ShowcaseMode::AlertsAndPayment,
                });
                cx.notify();
                "nitro-mgmt: alerts".into()
            }
            "squad-primary" => {
                self.nitro_mgmt_overlay = Some(NitroMgmtOverlay::Showcase {
                    title: dc_intl::t("/gs+Pz").to_owned(),
                    mode: ShowcaseMode::SquadPrimary,
                });
                cx.notify();
                "nitro-mgmt: squad-primary".into()
            }
            "squad-member" => {
                self.nitro_mgmt_overlay = Some(NitroMgmtOverlay::Showcase {
                    title: dc_intl::t("/gs+Pz").to_owned(),
                    mode: ShowcaseMode::SquadMember,
                });
                cx.notify();
                "nitro-mgmt: squad-member".into()
            }
            "discount" => {
                self.nitro_mgmt_overlay = Some(NitroMgmtOverlay::Showcase {
                    title: dc_intl::t("/gs+Pz").to_owned(),
                    mode: ShowcaseMode::DiscountBanner,
                });
                cx.notify();
                "nitro-mgmt: discount".into()
            }
            "preview-cancel" => {
                self.nitro_mgmt_overlay = Some(NitroMgmtOverlay::CancelPreview {
                    subscription_id: "100".into(),
                    plan_name: "Nitro".into(),
                    plan_price: "$9.99/month".into(),
                    tier: PremiumType::Nitro,
                    renewal_date: "December 20, 2026".into(),
                    renewal_rate: Some("$9.98/month".into()),
                    boost_count: 2,
                    boost_price: "$9.98".into(),
                    yearly: false,
                    details_expanded: true,
                    busy: false,
                    failed: false,
                });
                cx.notify();
                "nitro-mgmt: preview-cancel".into()
            }
            "plan-change" => {
                self.nitro_mgmt_overlay = Some(NitroMgmtOverlay::CancelPlanChange {
                    subscription_id: "100".into(),
                    current_plan_name: "Nitro".into(),
                    pending_plan_name: "Nitro Basic".into(),
                    effective_date: "$9.99/month".into(),
                });
                cx.notify();
                "nitro-mgmt: plan-change".into()
            }
            "close" => {
                self.nitro_mgmt_overlay = None;
                cx.notify();
                "nitro-mgmt: closed".into()
            }
            other => format!("nitro-mgmt: unknown mode {other}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extended_cancel_subtitle_covers_group_plan_and_blocked_payments() {
        let (gt, gb) = extended_cancel_subtitle(
            status::ACTIVE,
            PremiumType::Nitro,
            "Dec 20, 2026",
            true,
            false,
        );
        assert_eq!(gt, "Cancel subscription");
        assert!(gb.contains("Nitro Squad"));

        let (_, ru) = extended_cancel_subtitle(
            status::ACTIVE,
            PremiumType::Nitro,
            "Dec 20, 2026",
            false,
            true,
        );
        assert!(ru.contains("Russia"));

        let (_, imm) = extended_cancel_subtitle(
            status::PAST_DUE,
            PremiumType::Nitro,
            "Dec 20, 2026",
            false,
            false,
        );
        assert!(imm.contains("immediately"));
    }

    #[test]
    fn status_action_labels_cover_all_488110_states() {
        assert_eq!(
            status_action_labels(status::ACTIVE, true),
            vec!["Manage Subscription"]
        );
        assert_eq!(
            status_action_labels(status::CANCELED, false),
            vec!["Re-subscribe"]
        );
        assert_eq!(
            status_action_labels(PAUSED, false),
            vec!["Cancel or Pause Subscription", "Resume Subscription"]
        );
        assert_eq!(
            status_action_labels(PAUSE_PENDING, false),
            vec!["Cancel Subscription", "Cancel Pause"]
        );
        assert_eq!(
            status_action_labels(status::PAST_DUE, false),
            vec!["Cancel", "Restore Subscription"]
        );
    }
}
