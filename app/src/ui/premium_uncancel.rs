//! `PREMIUM_UNCANCEL` notice bar predicate (module 596339) and Keep Nitro Winback Modal
//! (module 174705, What You Lose perk cards module 736560, Nitro Activated confirmation module 231308).
//!
//! Notice bar (`596339.js`, `notice_Q02kKo`):
//! - Storage dismissal key: `hideUncancelReminder` / `hideUncancelReminder-untilAtLeast`
//!   (dismissed until `premiumSubscription.currentPeriodEnd`).
//! - Holds iff: `status == CANCELED`, `!periodEnded`, `0 <= daysLeft <= 7`, `!canceledWithin1Day`,
//!   `!isPurchasedExternally`, `!isDismissed`.
//! - Background gradients: `TIER_0` purple->blue, `TIER_1` blue->dark-blue, `TIER_2` purple->pink.
//! - Action button dismisses the notice until `currentPeriodEnd` and opens `174705` Winback Modal.
//!
//! Winback modal (`174705.js`):
//! - Initial state: `Sngnzg` / `tdvIlU` title with `{daysLeft}`, `6Su2Xb` / `lE+P8X` subtitle,
//!   `736560` perk cards (`min-h: 140px`, 180px right preview, `"Learn More"` `hvVgAZ`),
//!   renewal billing disclosure (`O7JRza`, `0y5kAG`, `1/ucvu`, `rbwRlf`, `C9oRCx`, `TmmTgl`),
//!   secondary `XDpS4K` ("Dismiss") and primary `2+luBl` ("Keep Nitro").
//! - Reinstating sends `PATCH /users/@me/billing/subscriptions/{id}?location=Uncancel%20Winback%20Modal`
//!   with `{ status: 1, payment_source_id, items, currency, ... }` (`158032.Ir` -> `781300.nV`).
//! - Success state (`uncanceled = true`): `231308` gradient-bordered `"NITRO ACTIVATED"` banner,
//!   `AGf/ye` / `knvOVz` / `aTUr3Z` copy, and `TkTvBz` ("Sweet!") primary CTA.

use super::{Root, color, icon};
use crate::net::Request;
use chrono::{DateTime, Utc};
use dc_intl::Arg;
use dc_state::premium::{self, Interval, PremiumType, Subscription};
use gpui::{
    AnyElement, Context, FontWeight, IntoElement, div, linear_color_stop, linear_gradient,
    prelude::*, px, rgb,
};
use serde_json::json;
use std::borrow::Cow;

/// Storage dismissal key (`596339.js` `cF[PREMIUM_UNCANCEL]`).
pub const DISMISS_KEY: &str = "hideUncancelReminder";
/// REST response key prefix for reinstatement PATCH (`158032.Ir`).
pub(super) const UNCANCEL_KEY: &str = "nitro-uncancel:";

/// Renewal billing cadence for the legal disclosure in `174705.js`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(dead_code)]
pub enum RenewalBilling {
    Yearly,
    Monthly { interval_count: u32 },
    MonthlyDiscounted,
}

/// Active state of the `174705` Keep Nitro winback modal.
#[derive(Clone, Debug)]
pub(super) struct PremiumUncancelModal {
    pub subscription_id: String,
    pub premium_type: PremiumType,
    pub days_left: i64,
    pub plan_id: u64,
    pub payment_source_id: Option<String>,
    pub currency: String,
    pub price_display: String,
    pub discount_price_display: Option<String>,
    pub billing: RenewalBilling,
    pub pending_payment: bool,
    pub submitting: bool,
    pub uncanceled: bool,
    pub error: Option<String>,
}

/// Evaluates the `596339.js` `PREMIUM_UNCANCEL` predicate against `subscription` at `now`.
/// Returns `Some(days_left)` when the notice should be shown.
pub fn uncancel_days_left(sub: &Subscription, now: DateTime<Utc>) -> Option<i64> {
    if sub.status != premium::status::CANCELED {
        return None;
    }
    // External (Apple / Google / etc.) subscriptions cannot be reinstated in-app (`isPurchasedExternally`).
    if sub.payment_gateway.is_some() {
        return None;
    }
    let period_end = DateTime::parse_from_rfc3339(sub.current_period_end.as_deref()?)
        .ok()?
        .with_timezone(&Utc);
    if period_end < now {
        return None;
    }
    // `canceledWithin1Day`: skip if canceled within the last 24 hours.
    if let Some(canceled_at) = sub
        .canceled_at
        .as_deref()
        .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
        .map(|dt| dt.with_timezone(&Utc))
        && (now - canceled_at).num_days() <= 1
    {
        return None;
    }
    let start_of_day = now
        .date_naive()
        .and_hms_opt(0, 0, 0)
        .map(|naive| DateTime::<Utc>::from_naive_utc_and_offset(naive, Utc))
        .unwrap_or(now);
    let days_left = (period_end - start_of_day).num_days();
    (0..=7).contains(&days_left).then_some(days_left)
}

/// Checks `hideUncancelReminder-untilAtLeast` against `now` (`596339.js`).
pub fn is_notice_dismissed(now: DateTime<Utc>) -> bool {
    crate::storage::get(&format!("{DISMISS_KEY}-untilAtLeast"))
        .and_then(|v| {
            v.as_str()
                .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
        })
        .is_some_and(|until| until > now)
}

/// Persists notice dismissal until `current_period_end` (`596339.js`).
pub fn dismiss_notice_until(period_end: Option<&str>) {
    crate::storage::set(DISMISS_KEY, json!(true));
    if let Some(until) = period_end {
        crate::storage::set(&format!("{DISMISS_KEY}-untilAtLeast"), json!(until));
    }
}

/// Notice bar message (`596339.js`: `fXv4wm`, `ZOHZMr`, `outyHh`) and CTA label (`BkbUPM`, `Px978X`, `LW5tCE`).
pub fn notice_copy(premium_type: PremiumType, days_left: i64) -> (String, &'static str) {
    let args = [("daysLeft", Arg::Num(days_left as f64))];
    match premium_type {
        PremiumType::Classic => (dc_intl::format("fXv4wm", &args), dc_intl::t("BkbUPM")),
        PremiumType::Basic => (dc_intl::format("ZOHZMr", &args), dc_intl::t("Px978X")),
        _ => (dc_intl::format("outyHh", &args), dc_intl::t("LW5tCE")),
    }
}

/// Modal title & subtitle (`174705.js`: `Sngnzg`/`tdvIlU`, `6Su2Xb`/`lE+P8X`).
pub fn modal_header_copy(premium_type: PremiumType, days_left: i64) -> (String, String) {
    let args = [("daysLeft", Arg::Num(days_left as f64))];
    let title = if premium_type == PremiumType::Classic {
        dc_intl::format("Sngnzg", &args)
    } else {
        dc_intl::format("tdvIlU", &args)
    };
    let subtitle_key = if premium_type == PremiumType::Classic {
        "6Su2Xb"
    } else {
        "lE+P8X"
    };
    let subtitle = format_plain(subtitle_key, &[]);
    (title, subtitle)
}

/// Renewal legal disclosure copy (`174705.js`: `O7JRza`, `0y5kAG`, `1/ucvu`, `rbwRlf`, `C9oRCx`, `TmmTgl`).
pub fn renewal_disclosure(
    premium_type: PremiumType,
    billing: RenewalBilling,
    price: &str,
    discount_price: Option<&str>,
) -> String {
    let is_classic = premium_type == PremiumType::Classic;
    match billing {
        RenewalBilling::Yearly => {
            let key = if is_classic { "O7JRza" } else { "0y5kAG" };
            format_plain(key, &[("price", Arg::Str(Cow::Borrowed(price)))])
        }
        RenewalBilling::MonthlyDiscounted => {
            let disc = discount_price.unwrap_or(price);
            format_plain(
                "1/ucvu",
                &[
                    ("discountPrice", Arg::Str(Cow::Borrowed(disc))),
                    ("defaultPrice", Arg::Str(Cow::Borrowed(price))),
                ],
            )
        }
        RenewalBilling::Monthly { interval_count } if interval_count > 1 => format_plain(
            "TmmTgl",
            &[
                ("price", Arg::Str(Cow::Borrowed(price))),
                ("intervalCount", Arg::Num(interval_count as f64)),
            ],
        ),
        RenewalBilling::Monthly { .. } => {
            let key = if is_classic { "rbwRlf" } else { "C9oRCx" };
            format_plain(key, &[("price", Arg::Str(Cow::Borrowed(price)))])
        }
    }
}

/// Confirmation body copy after reinstatement (`231308.js`: `AGf/ye`, `knvOVz`, `aTUr3Z`, `o6hBiR`, `A4THYt`).
pub fn confirmed_copy(premium_type: PremiumType, pending_payment: bool) -> &'static str {
    if pending_payment {
        return match premium_type {
            PremiumType::Classic => dc_intl::t("o6hBiR"),
            _ => dc_intl::t("A4THYt"),
        };
    }
    match premium_type {
        PremiumType::Basic => dc_intl::t("AGf/ye"),
        PremiumType::Classic => dc_intl::t("knvOVz"),
        _ => dc_intl::t("aTUr3Z"),
    }
}

/// One "What You Lose" perk card (`736560.js` `l`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WhatYouLoseCard {
    pub description: String,
    pub icon_name: &'static str,
    pub badge_label: Option<String>,
    pub help_url: &'static str,
}

/// Builds the tier-specific perk card list (`736560.js` `l`).
pub fn what_you_lose_cards(premium_type: PremiumType) -> Vec<WhatYouLoseCard> {
    let custom_emojis = WhatYouLoseCard {
        description: format_plain("couiKJ", &[]),
        icon_name: "nitroplan_custom_emoji_anywhere",
        badge_label: None,
        help_url: "https://support.discord.com/hc/en-us/articles/360036479811",
    };
    let uploads = WhatYouLoseCard {
        description: format_plain("wFWO6D", &[]),
        icon_name: "nitroplan_50mb_uploads",
        badge_label: None,
        help_url: "https://support.discord.com/hc/en-us/articles/115000435108",
    };
    let screen_share = WhatYouLoseCard {
        description: format_plain("wK04T1", &[]),
        icon_name: "nitroplan_hd_video_streaming",
        badge_label: None,
        help_url: "https://support.discord.com/hc/en-us/articles/115000435108",
    };
    match premium_type {
        PremiumType::Basic => vec![custom_emojis, uploads],
        PremiumType::Classic => vec![
            WhatYouLoseCard {
                description: format_plain("xCaYwE", &[]),
                icon_name: "nitroplan_custom_profiles_and_more",
                badge_label: None,
                help_url: "https://support.discord.com/hc/en-us/articles/4403147417623",
            },
            custom_emojis,
            screen_share,
            uploads,
            WhatYouLoseCard {
                description: format_plain("K4Hv69", &[]),
                icon_name: "boost_gem",
                badge_label: None,
                help_url: "https://support.discord.com/hc/en-us/articles/360028038352",
            },
        ],
        _ => vec![
            WhatYouLoseCard {
                description: format_plain("pwkxYF", &[("time", Arg::Num(30.0))]),
                icon_name: "nitroplan_special_nitro_badge_on_your_profile",
                badge_label: None,
                help_url: "https://support.discord.com/hc/en-us/articles/115000435108",
            },
            WhatYouLoseCard {
                description: format_plain("gpqr+n", &[]),
                icon_name: "nitroplan_custom_profiles_and_more",
                badge_label: None,
                help_url: "https://support.discord.com/hc/en-us/articles/4403147417623",
            },
            WhatYouLoseCard {
                description: format_plain("0hUHi6", &[]),
                icon_name: "nitroplan_custom_emoji_anywhere",
                badge_label: None,
                help_url: "https://support.discord.com/hc/en-us/articles/360036479811",
            },
            WhatYouLoseCard {
                description: format_plain("wRxEDW", &[]),
                icon_name: "nitroplan_2_server_boosts",
                badge_label: Some(format_plain("Ou/g/P", &[("boostCount", Arg::Num(2.0))])),
                help_url: "https://support.discord.com/hc/en-us/articles/360028038352",
            },
            screen_share,
            WhatYouLoseCard {
                description: format_plain("GsOFRJ", &[]),
                icon_name: "nitroplan_1gb_uploads",
                badge_label: None,
                help_url: "https://support.discord.com/hc/en-us/articles/115000435108",
            },
        ],
    }
}

/// Builds the `PATCH /users/@me/billing/subscriptions/{id}` request body (`158032.Ir` -> `781300.nV`).
pub fn uncancel_patch_request(modal: &PremiumUncancelModal) -> Request {
    let mut body = json!({
        "status": premium::status::ACTIVE,
        "items": [{"plan_id": modal.plan_id.to_string(), "quantity": 1}],
        "currency": modal.currency,
        "expected_invoice_price": {"amount": 0, "currency": modal.currency},
    });
    if let Some(ps) = modal.payment_source_id.as_deref() {
        body["payment_source_id"] = json!(ps);
    }
    Request::Rest {
        key: Some(format!("{UNCANCEL_KEY}{}", modal.subscription_id)),
        method: reqwest::Method::PATCH,
        route: format!(
            "/users/@me/billing/subscriptions/{}?location=Uncancel%20Winback%20Modal",
            modal.subscription_id
        ),
        body: Some(body),
    }
}

fn format_plain(key: &str, args: &[(&str, Arg<'_>)]) -> String {
    dc_intl::format_runs(key, args)
        .into_iter()
        .map(|(text, _)| text)
        .collect()
}

/// Background gradient for the `PREMIUM_UNCANCEL` notice bar (`596339.js` `colorPremiumTier*_Q02kKo`).
pub(super) fn notice_gradient(premium_type: PremiumType) -> gpui::Background {
    match premium_type {
        PremiumType::Basic => linear_gradient(
            270.,
            linear_color_stop(rgb(0x8547c6), 0.),
            linear_color_stop(rgb(0x4f74e8), 1.),
        ),
        PremiumType::Classic => linear_gradient(
            270.,
            linear_color_stop(rgb(0x5865f2), 0.),
            linear_color_stop(rgb(0x3c45a5), 1.),
        ),
        _ => linear_gradient(
            90.,
            linear_color_stop(rgb(0x8547c6), 0.),
            linear_color_stop(rgb(0xb845c1), 1.),
        ),
    }
}

impl Root {
    /// Opens the `174705` Keep Nitro Winback Modal from the live subscription (or notice bar click).
    pub(super) fn open_premium_uncancel_modal(
        &mut self,
        days_left: i64,
        premium_type: PremiumType,
        cx: &mut Context<Self>,
    ) {
        let sub = self.premium_subscription();
        let (sub_id, plan_id, payment_source_id, currency, billing, price_display) =
            if let Some(sub) = sub.as_ref() {
                let period_end = sub.current_period_end.as_deref();
                dismiss_notice_until(period_end);
                let plan_info = self.nitro.plans.iter().find(|p| p.id == sub.plan_id);
                let billing = match plan_info.map(|p| (p.interval, p.interval_count)) {
                    Some((Interval::Year, _)) => RenewalBilling::Yearly,
                    Some((Interval::Month, n)) => RenewalBilling::Monthly { interval_count: n },
                    _ => RenewalBilling::Monthly { interval_count: 1 },
                };
                let price = plan_info
                    .and_then(|p| {
                        p.price(0, sub.payment_source_id.as_deref())
                            .map(|pr| pr.display())
                    })
                    .unwrap_or_else(|| "$9.99".to_owned());
                (
                    sub.id.clone(),
                    sub.plan_id,
                    sub.payment_source_id.clone(),
                    sub.currency.clone().unwrap_or_else(|| "usd".to_owned()),
                    billing,
                    price,
                )
            } else {
                let (plan_id, price) = match premium_type {
                    PremiumType::Basic => (premium::plan_id::MONTH_TIER_0, "$2.99".to_owned()),
                    PremiumType::Classic => (premium::plan_id::MONTH_TIER_1, "$4.99".to_owned()),
                    _ => (premium::plan_id::MONTH_TIER_2, "$9.99".to_owned()),
                };
                (
                    "0".to_owned(),
                    plan_id,
                    None,
                    "usd".to_owned(),
                    RenewalBilling::Monthly { interval_count: 1 },
                    price,
                )
            };
        self.premium_uncancel_modal = Some(PremiumUncancelModal {
            subscription_id: sub_id,
            premium_type,
            days_left,
            plan_id,
            payment_source_id,
            currency,
            price_display,
            discount_price_display: None,
            billing,
            pending_payment: false,
            submitting: false,
            uncanceled: false,
            error: None,
        });
        cx.notify();
    }

    /// Handles the `PATCH /users/@me/billing/subscriptions/{id}` response (`158032.Ir`).
    pub(super) fn handle_premium_uncancel_response(
        &mut self,
        ok: bool,
        body: Option<&serde_json::Value>,
        cx: &mut Context<Self>,
    ) {
        let Some(modal) = self.premium_uncancel_modal.as_mut() else {
            return;
        };
        modal.submitting = false;
        if ok {
            modal.uncanceled = true;
            modal.error = None;
            self.vv.notices.uncancel_demo = None;
        } else {
            let msg = body
                .and_then(|v| v["message"].as_str())
                .unwrap_or("Failed to reinstate subscription. Please try again.");
            modal.error = Some(msg.to_owned());
        }
        cx.notify();
    }

    /// Renders the `174705` Keep Nitro Winback Modal (initial perk list or `231308` Nitro Activated confirmation).
    pub(super) fn premium_uncancel_layer(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let modal = self.premium_uncancel_modal.as_ref()?;
        let card = if modal.uncanceled {
            self.render_uncancel_confirmed(modal, cx)
        } else {
            self.render_uncancel_offer(modal, cx)
        };
        Some(
            gpui::deferred(
                div()
                    .id("premium-uncancel-backdrop")
                    .occlude()
                    .absolute()
                    .top_0()
                    .left_0()
                    .size_full()
                    .bg(gpui::rgba(0x000000b3))
                    .flex()
                    .items_center()
                    .justify_center()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.premium_uncancel_modal = None;
                        cx.notify();
                    }))
                    .child(card),
            )
            .with_priority(6)
            .into_any_element(),
        )
    }

    fn render_uncancel_offer(
        &self,
        modal: &PremiumUncancelModal,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let (title, subtitle) = modal_header_copy(modal.premium_type, modal.days_left);
        let cards = what_you_lose_cards(modal.premium_type);
        let disclosure = renewal_disclosure(
            modal.premium_type,
            modal.billing,
            &modal.price_display,
            modal.discount_price_display.as_deref(),
        );
        let compact_margin = cards.len() <= 2;
        let submitting = modal.submitting;
        let has_payment_source = modal.payment_source_id.is_some();

        let perk_rows = cards.into_iter().enumerate().map(|(idx, card)| {
            let url = card.help_url;
            div()
                .id(("uncancel-perk", idx))
                .w_full()
                .min_h(px(108.))
                .p(px(12.))
                .rounded(px(8.))
                .bg(color("--background-mod-subtle"))
                .border_1()
                .border_color(color("--border-muted"))
                .flex()
                .items_center()
                .justify_between()
                .gap(px(16.))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .gap(px(6.))
                        .child(
                            div()
                                .text_size(px(14.))
                                .line_height(px(20.))
                                .text_color(color("--text-default"))
                                .child(card.description),
                        )
                        .child(
                            div()
                                .id(("uncancel-perk-learn", idx))
                                .text_size(px(13.))
                                .line_height(px(16.))
                                .font_weight(FontWeight(500.))
                                .text_color(color("--text-link"))
                                .cursor_pointer()
                                .hover(|s| s.underline())
                                .on_click(cx.listener(move |_, _, _, cx| {
                                    cx.stop_propagation();
                                    cx.open_url(url);
                                }))
                                // `hvVgAZ`: "Learn More"
                                .child(dc_intl::t("hvVgAZ")),
                        ),
                )
                // Right 160px preview card (`736560.js` `.whatYouLoseItem_o6yNAQ`).
                .child(
                    div()
                        .flex_none()
                        .w(px(160.))
                        .min_h(px(84.))
                        .p(px(10.))
                        .rounded(px(8.))
                        .bg(color("--background-surface-higher"))
                        .border_1()
                        .border_color(color("--border-subtle"))
                        .flex()
                        .flex_col()
                        .items_center()
                        .justify_center()
                        .gap(px(6.))
                        .child(icon(card.icon_name, 36., "--text-brand"))
                        .children(card.badge_label.map(|lbl| {
                            div()
                                .text_size(px(11.))
                                .line_height(px(14.))
                                .font_weight(FontWeight(600.))
                                .text_color(color("--text-muted"))
                                .text_center()
                                .child(lbl)
                        })),
                )
        });

        div()
            .id("premium-uncancel-modal")
            .w(px(560.))
            .max_h(px(620.))
            .rounded(px(12.))
            .bg(color("--background-surface-high"))
            .border_1()
            .border_color(color("--border-subtle"))
            .overflow_hidden()
            .flex()
            .flex_col()
            .on_click(|_, _, cx| cx.stop_propagation())
            // Header
            .child(
                div()
                    .px(px(24.))
                    .pt(px(22.))
                    .pb(px(12.))
                    .flex()
                    .items_start()
                    .justify_between()
                    .gap(px(12.))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap(px(6.))
                            .child(
                                div()
                                    .text_size(px(20.))
                                    .line_height(px(24.))
                                    .font_weight(FontWeight(700.))
                                    .text_color(color("--text-strong"))
                                    .child(title),
                            )
                            .child(
                                div()
                                    .text_size(px(14.))
                                    .line_height(px(20.))
                                    .text_color(color("--text-subtle"))
                                    .child(subtitle),
                            ),
                    )
                    .child(
                        div()
                            .id("premium-uncancel-close")
                            .size(px(28.))
                            .rounded(px(6.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .cursor_pointer()
                            .hover(|s| s.bg(color("--background-mod-subtle")))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.premium_uncancel_modal = None;
                                cx.notify();
                            }))
                            .child(icon("modal_close", 18., "--interactive-text-default")),
                    ),
            )
            // Scrollable perk list (`736560.js`) + renewal disclosure (`174705.js`)
            .child(
                div()
                    .id("premium-uncancel-body")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .px(px(24.))
                    .py(px(if compact_margin { 4. } else { 8. }))
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .children(perk_rows)
                    .child(
                        div()
                            .mt(px(8.))
                            .pb(px(8.))
                            .text_size(px(12.))
                            .line_height(px(16.))
                            .text_color(color("--text-muted"))
                            .child(disclosure),
                    )
                    .children(modal.error.as_ref().map(|err| {
                        div()
                            .p(px(10.))
                            .rounded(px(6.))
                            .bg(gpui::rgba(0xda373c22))
                            .border_1()
                            .border_color(color("--status-danger"))
                            .text_size(px(13.))
                            .line_height(px(18.))
                            .text_color(color("--text-feedback-critical"))
                            .child(err.clone())
                    })),
            )
            // Footer actions: Dismiss (`XDpS4K`) + Keep Nitro (`2+luBl`)
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
                            .id("premium-uncancel-dismiss")
                            .h(px(38.))
                            .px(px(16.))
                            .rounded(px(8.))
                            .bg(color("--control-secondary-background-default"))
                            .hover(|s| s.bg(color("--control-secondary-background-hover")))
                            .flex()
                            .items_center()
                            .justify_center()
                            .cursor_pointer()
                            .text_size(px(14.))
                            .font_weight(FontWeight(500.))
                            .text_color(color("--control-secondary-text-default"))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.premium_uncancel_modal = None;
                                cx.notify();
                            }))
                            .child(dc_intl::t("XDpS4K")),
                    )
                    .child(
                        div()
                            .id("premium-uncancel-keep")
                            .h(px(38.))
                            .px(px(20.))
                            .rounded(px(8.))
                            .bg(color("--control-primary-background-default"))
                            .when(!submitting, |s| {
                                s.cursor_pointer()
                                    .hover(|h| h.bg(color("--control-primary-background-hover")))
                            })
                            .when(submitting, |s| s.opacity(0.6))
                            .flex()
                            .items_center()
                            .justify_center()
                            .text_size(px(14.))
                            .font_weight(FontWeight(600.))
                            .text_color(color("--control-primary-text-default"))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if submitting {
                                    return;
                                }
                                if !has_payment_source {
                                    // `174705.js`: without a valid saved payment source, open Billing/Checkout.
                                    this.premium_uncancel_modal = None;
                                    this.open_settings_page("billing", cx);
                                    return;
                                }
                                if let Some(m) = this.premium_uncancel_modal.as_mut() {
                                    m.submitting = true;
                                    m.error = None;
                                    let req = uncancel_patch_request(m);
                                    let _ = this.requests.send(req);
                                }
                                cx.notify();
                            }))
                            .child(dc_intl::t("2+luBl")),
                    ),
            )
            .into_any_element()
    }

    /// Success state (`231308.Ay` inside `174705.js`): gradient-bordered `"NITRO ACTIVATED"` pill,
    /// tier confirmation copy, and `"Sweet!"` (`TkTvBz`) button.
    fn render_uncancel_confirmed(
        &self,
        modal: &PremiumUncancelModal,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let copy = confirmed_copy(modal.premium_type, modal.pending_payment);
        div()
            .id("premium-uncancel-confirmed")
            .w(px(440.))
            .rounded(px(12.))
            .bg(color("--background-surface-high"))
            .border_1()
            .border_color(color("--border-subtle"))
            .px(px(32.))
            .pt(px(48.))
            .pb(px(36.))
            .flex()
            .flex_col()
            .items_center()
            .text_center()
            .on_click(|_, _, cx| cx.stop_propagation())
            // `.gradientOuterBorder_JXikcP`: `linear-gradient(90deg, #9b1da5, #1e2353 120%)`, 2px padding.
            .child(
                div()
                    .mb(px(16.))
                    .p(px(2.))
                    .rounded(px(10.))
                    .bg(linear_gradient(
                        90.,
                        linear_color_stop(rgb(0x9b1da5), 0.),
                        linear_color_stop(rgb(0x1e2353), 1.),
                    ))
                    .child(
                        div()
                            .px(px(16.))
                            .py(px(10.))
                            .rounded(px(8.))
                            .bg(color("--background-base-low"))
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .child(icon("nitro_wheel", 20., "--text-brand"))
                            .child(
                                div()
                                    .text_size(px(15.))
                                    .line_height(px(20.))
                                    .font_weight(FontWeight(800.))
                                    .text_color(color("--text-strong"))
                                    .child("NITRO ACTIVATED"),
                            ),
                    ),
            )
            .child(
                div()
                    .max_w(px(360.))
                    .text_size(px(15.))
                    .line_height(px(22.))
                    .font_weight(FontWeight(500.))
                    .text_color(color("--interactive-text-default"))
                    .child(copy),
            )
            .child(
                div()
                    .id("premium-uncancel-sweet")
                    .mt(px(20.))
                    .h(px(38.))
                    .px(px(24.))
                    .rounded(px(8.))
                    .bg(color("--control-primary-background-default"))
                    .hover(|s| s.bg(color("--control-primary-background-hover")))
                    .cursor_pointer()
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_size(px(14.))
                    .font_weight(FontWeight(600.))
                    .text_color(color("--control-primary-text-default"))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.premium_uncancel_modal = None;
                        cx.notify();
                    }))
                    .child(dc_intl::t("TkTvBz")),
            )
            .into_any_element()
    }

    /// Capture-only harness helper: `premium-uncancel <notice|modal|modal-tier1|confirmed|close>`.
    #[cfg(feature = "capture")]
    pub(crate) fn script_premium_uncancel(&mut self, mode: &str, cx: &mut Context<Self>) -> String {
        match mode.trim() {
            "notice" => {
                self.vv.notices.uncancel_demo = Some((3, PremiumType::Nitro));
                self.premium_uncancel_modal = None;
                cx.notify();
                "premium-uncancel: notice (3 days, Nitro)".into()
            }
            "modal" => {
                self.premium_uncancel_modal = Some(PremiumUncancelModal {
                    subscription_id: "1234567890".into(),
                    premium_type: PremiumType::Nitro,
                    days_left: 3,
                    plan_id: premium::plan_id::MONTH_TIER_2,
                    payment_source_id: Some("987654".into()),
                    currency: "usd".into(),
                    price_display: "$9.99".into(),
                    discount_price_display: None,
                    billing: RenewalBilling::Monthly { interval_count: 1 },
                    pending_payment: false,
                    submitting: false,
                    uncanceled: false,
                    error: None,
                });
                cx.notify();
                "premium-uncancel: modal (Nitro, 3 days)".into()
            }
            "modal-tier1" => {
                self.premium_uncancel_modal = Some(PremiumUncancelModal {
                    subscription_id: "1234567890".into(),
                    premium_type: PremiumType::Classic,
                    days_left: 1,
                    plan_id: premium::plan_id::YEAR_TIER_1,
                    payment_source_id: Some("987654".into()),
                    currency: "usd".into(),
                    price_display: "$49.99".into(),
                    discount_price_display: None,
                    billing: RenewalBilling::Yearly,
                    pending_payment: false,
                    submitting: false,
                    uncanceled: false,
                    error: None,
                });
                cx.notify();
                "premium-uncancel: modal (Classic, 1 day)".into()
            }
            "confirmed" => {
                self.premium_uncancel_modal = Some(PremiumUncancelModal {
                    subscription_id: "1234567890".into(),
                    premium_type: PremiumType::Nitro,
                    days_left: 3,
                    plan_id: premium::plan_id::MONTH_TIER_2,
                    payment_source_id: Some("987654".into()),
                    currency: "usd".into(),
                    price_display: "$9.99".into(),
                    discount_price_display: None,
                    billing: RenewalBilling::Monthly { interval_count: 1 },
                    pending_payment: false,
                    submitting: false,
                    uncanceled: true,
                    error: None,
                });
                cx.notify();
                "premium-uncancel: confirmed".into()
            }
            "close" => {
                self.vv.notices.uncancel_demo = None;
                self.premium_uncancel_modal = None;
                cx.notify();
                "premium-uncancel: closed".into()
            }
            other => format!("premium-uncancel: unknown mode {other}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_sub(status: u64, days_until_end: i64, canceled_days_ago: i64) -> Subscription {
        let now = Utc::now();
        Subscription {
            id: "100".into(),
            kind: premium::SUBSCRIPTION_TYPE_PREMIUM,
            user_id: Some(1),
            status,
            items: vec![],
            plan_id: premium::plan_id::MONTH_TIER_2,
            renewal_mutations: None,
            payment_source_id: Some("55".into()),
            trial_id: None,
            trial_ends_at: None,
            created_at: None,
            canceled_at: Some((now - chrono::Duration::days(canceled_days_ago)).to_rfc3339()),
            current_period_start: None,
            current_period_end: Some((now + chrono::Duration::days(days_until_end)).to_rfc3339()),
            streak_started_at: None,
            currency: Some("usd".into()),
            payment_gateway: None,
        }
    }

    #[test]
    fn uncancel_predicate_matches_596339_rules() {
        let now = Utc::now();
        // Canceled 3 days ago, ends in 4 days -> holds.
        let ok = sample_sub(premium::status::CANCELED, 4, 3);
        assert!(uncancel_days_left(&ok, now).is_some());
        // Active subscription -> None.
        let active = sample_sub(premium::status::ACTIVE, 4, 3);
        assert!(uncancel_days_left(&active, now).is_none());
        // Canceled within 1 day -> None.
        let recent = sample_sub(premium::status::CANCELED, 4, 0);
        assert!(uncancel_days_left(&recent, now).is_none());
        // More than 7 days left -> None.
        let far = sample_sub(premium::status::CANCELED, 12, 3);
        assert!(uncancel_days_left(&far, now).is_none());
        // External gateway -> None.
        let mut ext = sample_sub(premium::status::CANCELED, 4, 3);
        ext.payment_gateway = Some(1);
        assert!(uncancel_days_left(&ext, now).is_none());
    }

    #[test]
    fn copy_and_perk_lists_cover_all_three_tiers() {
        assert_eq!(what_you_lose_cards(PremiumType::Basic).len(), 2);
        assert_eq!(what_you_lose_cards(PremiumType::Classic).len(), 5);
        assert_eq!(what_you_lose_cards(PremiumType::Nitro).len(), 6);
        let (title, sub) = modal_header_copy(PremiumType::Nitro, 3);
        assert!(title.contains("3"));
        assert!(!sub.is_empty());
        let disc = renewal_disclosure(
            PremiumType::Nitro,
            RenewalBilling::MonthlyDiscounted,
            "$9.99",
            Some("$4.99"),
        );
        assert!(disc.contains("$4.99") && disc.contains("$9.99"));
        assert!(!confirmed_copy(PremiumType::Nitro, false).is_empty());
        assert!(!confirmed_copy(PremiumType::Classic, true).is_empty());
    }
}
