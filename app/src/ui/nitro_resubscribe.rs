//! Resubscribe to Nitro 2-step confirmation modal (module 674809).
//!
//! Opened from User Settings -> Subscriptions (`settings/billing.rs`) when clicking
//! "Re-subscribe" (`iIvF2z`) on a canceled Nitro subscription.
//! Step 1 (`Confirm`): displays reactivation terms (`dbGGui` standard, or `G8IxyE`/`kXtIIn` when
//! Nitro gift credit applies), `UQolSy` footnote, error banner (`5mlOCW`), and `Confirm` (`cY+Oob`)
//! calling `PATCH /users/@me/billing/subscriptions/{id}` with `{ "status": 1 }`.
//! Step 2 (`Success`): displays `FCsTCk` ("Welcome back!...") and `TkTvBz` ("Sweet!").

use super::{Root, color, icon};
use crate::net::Request;
use dc_intl::Arg;
use gpui::{
    AnyElement, Context, FontWeight, HighlightStyle, IntoElement, StyledText, deferred, div,
    prelude::*, px, rgba,
};
use serde_json::{Value, json};
use std::borrow::Cow;

pub(crate) const RESUBSCRIBE_KEY: &str = "billing:resubscribe-nitro";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ResubscribeStep {
    Confirm,
    Success,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(dead_code)]
pub(crate) enum ResubscribePricing {
    Standard,
    GiftCreditNoTax,
    GiftCreditPlusTax,
}

#[derive(Clone, Debug)]
pub(crate) struct NitroResubscribeModal {
    pub subscription_id: String,
    pub price: String,
    pub discounted_price: String,
    pub renewal_date: String,
    pub billing_period: String,
    pub pricing: ResubscribePricing,
    pub step: ResubscribeStep,
    pub busy: bool,
    pub error: Option<String>,
}

impl NitroResubscribeModal {
    pub(crate) fn new(
        subscription_id: String,
        price: String,
        renewal_date: String,
        billing_period: String,
    ) -> Self {
        Self {
            subscription_id,
            discounted_price: "$0.00".to_owned(),
            price,
            renewal_date,
            billing_period,
            pricing: ResubscribePricing::Standard,
            step: ResubscribeStep::Confirm,
            busy: false,
            error: None,
        }
    }

    pub(crate) fn body_runs(&self) -> Vec<(String, Option<Box<str>>)> {
        match self.pricing {
            ResubscribePricing::Standard => dc_intl::format_runs(
                "dbGGui",
                &[
                    ("price", Arg::Str(Cow::Borrowed(&self.price))),
                    ("date", Arg::Str(Cow::Borrowed(&self.renewal_date))),
                ],
            ),
            ResubscribePricing::GiftCreditNoTax => dc_intl::format_runs(
                "G8IxyE",
                &[
                    (
                        "discountedPrice",
                        Arg::Str(Cow::Borrowed(&self.discounted_price)),
                    ),
                    ("date", Arg::Str(Cow::Borrowed(&self.renewal_date))),
                    ("regularPrice", Arg::Str(Cow::Borrowed(&self.price))),
                    (
                        "billingPeriod",
                        Arg::Str(Cow::Borrowed(&self.billing_period)),
                    ),
                ],
            ),
            ResubscribePricing::GiftCreditPlusTax => dc_intl::format_runs(
                "kXtIIn",
                &[
                    (
                        "discountedPrice",
                        Arg::Str(Cow::Borrowed(&self.discounted_price)),
                    ),
                    ("date", Arg::Str(Cow::Borrowed(&self.renewal_date))),
                    ("regularPrice", Arg::Str(Cow::Borrowed(&self.price))),
                    (
                        "billingPeriod",
                        Arg::Str(Cow::Borrowed(&self.billing_period)),
                    ),
                ],
            ),
        }
    }
}

/// `PATCH /users/@me/billing/subscriptions/{id}` `{ status: 1 }` (module 674809 / 323082).
pub(crate) fn resubscribe_request(subscription_id: &str) -> Request {
    Request::Rest {
        key: Some(RESUBSCRIBE_KEY.to_owned()),
        method: reqwest::Method::PATCH,
        route: format!("/users/@me/billing/subscriptions/{subscription_id}"),
        body: Some(json!({ "status": 1 })),
    }
}

fn styled_bold_runs(runs: Vec<(String, Option<Box<str>>)>) -> StyledText {
    let mut text = String::new();
    let mut highlights = Vec::new();
    for (chunk, hook) in runs {
        let start = text.len();
        text.push_str(&chunk);
        if hook.as_deref() == Some("$b") && start < text.len() {
            highlights.push((
                start..text.len(),
                HighlightStyle {
                    font_weight: Some(FontWeight(700.)),
                    color: Some(color("--text-strong").into()),
                    ..HighlightStyle::default()
                },
            ));
        }
    }
    StyledText::new(text).with_highlights(highlights)
}

impl Root {
    /// Opens the Resubscribe to Nitro modal (`674809`) from Billing Settings.
    pub(super) fn open_nitro_resubscribe_modal(
        &mut self,
        subscription_id: String,
        price: String,
        renewal_date: String,
        billing_period: String,
        cx: &mut Context<Self>,
    ) {
        self.nitro_resubscribe_modal = Some(NitroResubscribeModal::new(
            subscription_id,
            price,
            renewal_date,
            billing_period,
        ));
        cx.notify();
    }

    /// Handles the `PATCH /users/@me/billing/subscriptions/{id}` response.
    pub(super) fn handle_nitro_resubscribe_response(
        &mut self,
        body: &Value,
        cx: &mut Context<Self>,
    ) {
        let Some(modal) = self.nitro_resubscribe_modal.as_mut() else {
            return;
        };
        modal.busy = false;
        let failed = body["error"].is_string() || body["status"].as_u64().is_some_and(|s| s >= 400);
        if failed {
            modal.error = Some(dc_intl::t("5mlOCW").to_owned());
        } else {
            modal.error = None;
            modal.step = ResubscribeStep::Success;
        }
        cx.notify();
    }

    /// Modal layer for Resubscribe to Nitro (`674809`).
    pub(super) fn nitro_resubscribe_layer(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let modal = self.nitro_resubscribe_modal.as_ref()?;
        let sub_id = modal.subscription_id.clone();
        let busy = modal.busy;

        let card = match modal.step {
            ResubscribeStep::Confirm => div()
                .id("nitro-resubscribe-modal")
                .w(px(460.))
                .rounded(px(12.))
                .border_1()
                .border_color(color("--border-subtle"))
                .bg(color("--background-surface-high"))
                .flex()
                .flex_col()
                .overflow_hidden()
                .on_click(|_, _, cx| cx.stop_propagation())
                .child(
                    div()
                        .px(px(24.))
                        .pt(px(20.))
                        .pb(px(12.))
                        .flex()
                        .items_center()
                        .justify_between()
                        .gap(px(12.))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .text_size(px(20.))
                                .line_height(px(24.))
                                .font_weight(FontWeight(700.))
                                .text_color(color("--text-strong"))
                                .child(dc_intl::t("fYEWlq")),
                        )
                        .child(
                            div()
                                .id("nitro-resubscribe-close")
                                .flex_none()
                                .size(px(32.))
                                .rounded(px(6.))
                                .cursor_pointer()
                                .flex()
                                .items_center()
                                .justify_center()
                                .child(icon("modal_close", 18., "--icon-subtle"))
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.nitro_resubscribe_modal = None;
                                    cx.notify();
                                })),
                        ),
                )
                .child(
                    div()
                        .px(px(24.))
                        .pb(px(20.))
                        .flex()
                        .flex_col()
                        .gap(px(12.))
                        .child(
                            div()
                                .text_size(px(14.))
                                .line_height(px(20.))
                                .text_color(color("--text-default"))
                                .child(styled_bold_runs(modal.body_runs())),
                        )
                        .child(
                            div()
                                .text_size(px(12.))
                                .line_height(px(16.))
                                .text_color(color("--text-muted"))
                                .child(dc_intl::t("UQolSy")),
                        )
                        .children(modal.error.as_ref().map(|err| {
                            div()
                                .rounded(px(6.))
                                .border_1()
                                .border_color(color("--status-danger"))
                                .bg(rgba(0xda373c1a))
                                .px(px(12.))
                                .py(px(8.))
                                .text_size(px(13.))
                                .text_color(color("--text-feedback-critical"))
                                .child(err.clone())
                        })),
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
                                .id("nitro-resubscribe-cancel")
                                .cursor_pointer()
                                .px(px(14.))
                                .h(px(38.))
                                .rounded(px(6.))
                                .flex()
                                .items_center()
                                .text_size(px(14.))
                                .font_weight(FontWeight(500.))
                                .text_color(color("--text-default"))
                                .child(dc_intl::tr("Cancel"))
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.nitro_resubscribe_modal = None;
                                    cx.notify();
                                })),
                        )
                        .child(
                            div()
                                .id("nitro-resubscribe-confirm")
                                .when(!busy, |d| d.cursor_pointer())
                                .when(busy, |d| d.opacity(0.6))
                                .px(px(20.))
                                .h(px(38.))
                                .rounded(px(6.))
                                .bg(color("--control-primary-background-default"))
                                .flex()
                                .items_center()
                                .text_size(px(14.))
                                .font_weight(FontWeight(600.))
                                .text_color(color("--white"))
                                .child(dc_intl::t("cY+Oob"))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    if busy {
                                        return;
                                    }
                                    if let Some(m) = this.nitro_resubscribe_modal.as_mut() {
                                        m.busy = true;
                                        m.error = None;
                                    }
                                    let _ = this.requests.send(resubscribe_request(&sub_id));
                                    cx.notify();
                                })),
                        ),
                ),
            ResubscribeStep::Success => div()
                .id("nitro-resubscribe-success")
                .w(px(440.))
                .rounded(px(12.))
                .border_1()
                .border_color(color("--border-subtle"))
                .bg(color("--background-surface-high"))
                .flex()
                .flex_col()
                .overflow_hidden()
                .on_click(|_, _, cx| cx.stop_propagation())
                .child(
                    div()
                        .px(px(24.))
                        .pt(px(28.))
                        .pb(px(20.))
                        .flex()
                        .flex_col()
                        .items_center()
                        .gap(px(14.))
                        .child(
                            div()
                                .size(px(64.))
                                .rounded_full()
                                .bg(rgba(0x5865f22a))
                                .flex()
                                .items_center()
                                .justify_center()
                                .child(icon("nitro_wheel", 34., "--white")),
                        )
                        .child(
                            div()
                                .text_size(px(20.))
                                .line_height(px(24.))
                                .font_weight(FontWeight(700.))
                                .text_color(color("--text-strong"))
                                .child(dc_intl::t("fYEWlq")),
                        )
                        .child(
                            div()
                                .text_center()
                                .text_size(px(14.))
                                .line_height(px(20.))
                                .text_color(color("--text-default"))
                                .child(dc_intl::t("FCsTCk")),
                        ),
                )
                .child(
                    div()
                        .px(px(24.))
                        .py(px(16.))
                        .bg(color("--background-base-lower"))
                        .border_t_1()
                        .border_color(color("--border-subtle"))
                        .flex()
                        .justify_center()
                        .child(
                            div()
                                .id("nitro-resubscribe-sweet")
                                .cursor_pointer()
                                .w_full()
                                .h(px(38.))
                                .rounded(px(6.))
                                .bg(color("--control-primary-background-default"))
                                .flex()
                                .items_center()
                                .justify_center()
                                .text_size(px(14.))
                                .font_weight(FontWeight(600.))
                                .text_color(color("--white"))
                                .child(dc_intl::t("TkTvBz"))
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.nitro_resubscribe_modal = None;
                                    cx.notify();
                                })),
                        ),
                ),
        };

        Some(
            deferred(
                div()
                    .id("nitro-resubscribe-backdrop")
                    .occlude()
                    .absolute()
                    .inset_0()
                    .bg(rgba(0x000000b3))
                    .flex()
                    .items_center()
                    .justify_center()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.nitro_resubscribe_modal = None;
                        cx.notify();
                    }))
                    .child(card),
            )
            .with_priority(96)
            .into_any_element(),
        )
    }

    /// Capture harness: `nitro-resubscribe [confirm|gift|gift-tax|error|success|close]`.
    #[cfg(feature = "capture")]
    pub(crate) fn script_nitro_resubscribe(&mut self, arg: &str, cx: &mut Context<Self>) -> String {
        let mut modal = NitroResubscribeModal::new(
            "sub_demo_1".to_owned(),
            "$9.99".to_owned(),
            "Dec 15, 2026".to_owned(),
            "month".to_owned(),
        );
        match arg.trim() {
            "" | "confirm" => {
                self.nitro_resubscribe_modal = Some(modal);
                cx.notify();
                "nitro-resubscribe: confirm".to_owned()
            }
            "gift" => {
                modal.pricing = ResubscribePricing::GiftCreditNoTax;
                self.nitro_resubscribe_modal = Some(modal);
                cx.notify();
                "nitro-resubscribe: gift".to_owned()
            }
            "gift-tax" => {
                modal.pricing = ResubscribePricing::GiftCreditPlusTax;
                self.nitro_resubscribe_modal = Some(modal);
                cx.notify();
                "nitro-resubscribe: gift-tax".to_owned()
            }
            "error" => {
                modal.error = Some(dc_intl::t("5mlOCW").to_owned());
                self.nitro_resubscribe_modal = Some(modal);
                cx.notify();
                "nitro-resubscribe: error".to_owned()
            }
            "success" => {
                modal.step = ResubscribeStep::Success;
                self.nitro_resubscribe_modal = Some(modal);
                cx.notify();
                "nitro-resubscribe: success".to_owned()
            }
            "close" => {
                self.nitro_resubscribe_modal = None;
                cx.notify();
                "nitro-resubscribe: closed".to_owned()
            }
            other => format!("nitro-resubscribe: unknown {other}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resubscribe_copy_formats_all_variants() {
        let mut m = NitroResubscribeModal::new(
            "s1".to_owned(),
            "$9.99".to_owned(),
            "Dec 15, 2026".to_owned(),
            "month".to_owned(),
        );
        let text: String = m.body_runs().into_iter().map(|(s, _)| s).collect();
        assert!(text.contains("$9.99"));
        assert!(text.contains("Dec 15, 2026"));

        m.pricing = ResubscribePricing::GiftCreditPlusTax;
        let text2: String = m.body_runs().into_iter().map(|(s, _)| s).collect();
        assert!(text2.contains("plus any applicable taxes"));
        assert!(text2.contains("month"));
    }
}
