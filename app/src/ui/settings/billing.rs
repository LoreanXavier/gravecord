//! The bodies of the billing pages that hold real account data: Subscriptions, and the payment methods and
//! transaction history of Billing. Measured in `design/dom/settings/{subscriptions,billing}-darker-cozy.json`.
//!
//! Only the empty states are drawn: the captured account has no subscription, credit, payment source or payment.
//! A saved source, a running subscription or a past payment needs the field layout of its API record, which is not in
//! the recorded traffic; those lists draw nothing until it is (`BLOCKED.md`).

use super::{ButtonKind, Root, button, color};
use crate::ui::icon;
use gpui::{AnyElement, FontWeight, IntoElement, div, prelude::*, px};

fn empty(loaded: Option<&serde_json::Value>) -> bool {
    loaded.is_none_or(|body| body.as_array().is_some_and(|rows| rows.is_empty()))
}

/// `You have no active subscriptions`: the dark 62px row with the empty-state glyph.
fn empty_row(id: &'static str, text: &'static str) -> AnyElement {
    div()
        .id(id)
        .w_full()
        .h(px(62.))
        .rounded(px(8.))
        .bg(color("--background-surface-high"))
        .px(px(20.))
        .flex()
        .items_center()
        .gap(px(8.))
        .child(icon("billing_empty", 30., "--text-strong"))
        .child(
            div()
                .text_size(px(16.))
                .line_height(px(20.))
                .font_weight(FontWeight(500.))
                .text_color(color("--text-strong"))
                .child(text),
        )
        .into_any_element()
}

impl Root {
    pub(super) fn billing_custom(
        &self,
        key: &'static str,
        cx: &mut gpui::Context<Self>,
    ) -> Option<AnyElement> {
        Some(match key {
            "premium_guild_subscriptions_setting" => self.server_boost_body(cx),
            "nitro_setting" => self.nitro_settings_body(cx),
            "subscriptions_settings" => self.subscriptions_body(),
            "billing_payment_methods" => self.payment_methods_body(),
            "billing_transaction_history" => self.transactions_body(),
            _ => return self.gift_inventory_custom(key, cx),
        })
    }

    fn subscriptions_body(&self) -> AnyElement {
        let none = empty(self.nitro.subscriptions.as_ref());
        div()
            .w_full()
            .pt(px(9.))
            .pb(px(16.))
            .flex()
            .flex_col()
            .child(
                div()
                    .text_size(px(20.))
                    .line_height(px(24.))
                    .font_weight(FontWeight(600.))
                    .text_color(color("--text-strong"))
                    .child("Your Subscriptions"),
            )
            .child(
                div()
                    .mt(px(16.))
                    .text_size(px(16.))
                    .line_height(px(20.))
                    .text_color(color("--text-default"))
                    .child(
                        "These are your current subscriptions. They will be billed on the same billing cycle. You can update any subscription at any time.",
                    ),
            )
            .child(div().mt(px(16.)).children(
                none.then(|| empty_row("subscriptions-none", "You have no active subscriptions")),
            ))
            .child(
                div()
                    .mt(px(32.))
                    .text_size(px(20.))
                    .line_height(px(24.))
                    .font_weight(FontWeight(500.))
                    .text_color(color("--text-strong"))
                    .child("Subscription Credit"),
            )
            .child(
                div()
                    .mt(px(4.))
                    .text_size(px(14.))
                    .line_height(px(18.))
                    .text_color(color("--text-default"))
                    .child(
                        "When you accept a gift while you have a running subscription, redeem a promotion, or accept a gift that differs from your current subscription, it will appear here as credit.",
                    ),
            )
            .child(div().mt(px(32.)).child(empty_row(
                "subscriptions-no-credit",
                "You have no unused credits",
            )))
            .child(div().mt(px(40.)).h(px(1.)).bg(color("--border-subtle")))
            .into_any_element()
    }

    /// No saved payment source: the notice and "Add Payment Method". The button is drawn but not wired: the
    /// browser page that adds a source on its own is not in the snapshot.
    fn payment_methods_body(&self) -> AnyElement {
        if !empty(self.nitro.sources.as_ref()) {
            return div().h(px(76.)).into_any_element();
        }
        div()
            .h(px(76.))
            .pt(px(29.))
            .flex()
            .items_start()
            .justify_between()
            .child(
                div()
                    .flex()
                    .flex_col()
                    .text_size(px(14.))
                    .line_height(px(18.))
                    .child(
                        div()
                            .text_color(color("--text-default"))
                            .child("No payment method saved"),
                    )
                    .child(
                        div()
                            .mt(px(4.))
                            .text_color(color("--text-muted"))
                            .child("Checkout faster by saving a payment method"),
                    ),
            )
            .child(div().id("billing-add-payment").child(button(
                "billing-add-payment-button",
                "Add Payment Method",
                ButtonKind::Primary,
            )))
            .into_any_element()
    }

    fn transactions_body(&self) -> AnyElement {
        if !empty(self.nitro.payments.as_ref()) {
            return div().h(px(48.)).into_any_element();
        }
        div()
            .h(px(48.))
            .pt(px(9.))
            .text_size(px(14.))
            .line_height(px(18.))
            .text_color(color("--text-muted"))
            .child("You haven't made any transactions yet.")
            .into_any_element()
    }
}
