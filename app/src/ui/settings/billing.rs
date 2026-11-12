//! The bodies of the billing pages that hold real account data: Subscriptions, and the payment methods and
//! transaction history of Billing. Measured in `design/dom/settings/{subscriptions,billing}-darker-cozy.json`.
//!
//! Only the empty states are drawn: the captured account has no subscription, credit, payment source or payment.
//! A saved source, a running subscription or a past payment needs the field layout of its API record, which is not in
//! the recorded traffic; those lists draw nothing until it is (`BLOCKED.md`).

use super::{ButtonKind, Root, button, color};
use crate::ui::icon;
use dc_state::premium::{self, PremiumType, Subscription, status};
use gpui::{
    AnyElement, FontWeight, IntoElement, div, linear_color_stop, linear_gradient, prelude::*, px,
};

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
            "billing_payment_methods" => self.payment_methods_body(cx),
            "billing_transaction_history" => self.transactions_body(),
            _ => return self.gift_inventory_custom(key, cx),
        })
    }

    fn subscriptions_body(&self) -> AnyElement {
        let none = empty(self.nitro.subscriptions.as_ref());
        let row = self.premium_subscription();
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
            .child(
                div()
                    .mt(px(16.))
                    .children(none.then(|| {
                        empty_row("subscriptions-none", "You have no active subscriptions")
                    }))
                    .children(row.map(|sub| self.subscription_row(&sub))),
            )
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

    /// The subscription's banner (module 358787 `$`, `banner_TJpZiO`: 16px padding, 4px radius, the tier's gradient;
    /// canceled is orange, past due red), its plan name, the renewal sentence of `getBillingInformationString` and
    /// Cancel and Switch Plans. Built from the module with no capture; the price comes from the plan list, the
    /// module's fallback when no invoice preview is loaded. Cancel and Switch Plans open flows that are not built.
    fn subscription_row(&self, sub: &Subscription) -> AnyElement {
        let tier = sub.premium_type().unwrap_or(PremiumType::Nitro);
        let price = premium::plan_sku(sub.plan_id).and_then(|sku| {
            let plan = self
                .nitro
                .plans
                .iter()
                .find(|plan| plan.id == sub.plan_id)?;
            let price = self.plan_price(sku, plan.is_yearly())?.display();
            // `CE` of module 722178: the price with its interval ("TRY 104.99/month").
            Some(
                premium::price_with_interval(&price, plan.interval, plan.interval_count)
                    .unwrap_or(price),
            )
        });
        let date = sub.current_period_end.as_deref().and_then(medium_date);
        let segments = info_segments(sub.status, date.as_deref(), price.as_deref());
        let canceled = sub.status == status::CANCELED;
        let ink = if canceled { "--black" } else { "--white" };
        let gradient = |from: &str, to: &str| {
            linear_gradient(
                90.,
                linear_color_stop(color(from), 0.),
                linear_color_stop(color(to), 1.),
            )
        };
        let banner = div()
            .id("subscription-row")
            .relative()
            .w_full()
            .rounded(px(4.))
            .p(px(16.))
            .flex()
            .flex_col()
            .text_color(color(ink));
        let banner = if canceled {
            banner.bg(linear_gradient(
                90.,
                linear_color_stop(gpui::rgb(0xf18623), 0.),
                linear_color_stop(gpui::rgb(0xfab930), 1.),
            ))
        } else if sub.status == status::PAST_DUE {
            banner.bg(linear_gradient(
                270.,
                linear_color_stop(gpui::rgb(0xe82427), 0.),
                linear_color_stop(gpui::rgb(0xc81031), 1.),
            ))
        } else {
            match tier {
                PremiumType::Basic => banner.bg(gradient(
                    "--premium-tier-0-purple-for-gradients",
                    "--premium-tier-0-blue-for-gradients",
                )),
                PremiumType::Classic => banner.bg(gradient(
                    "--premium-tier-1-blue-for-gradients",
                    "--premium-tier-1-dark-blue-for-gradients",
                )),
                _ => banner.bg(gradient(
                    "--premium-tier-2-purple-for-gradients",
                    "--premium-tier-2-pink-for-gradients",
                )),
            }
        };
        let info = div()
            .mt(px(8.))
            .text_size(px(16.))
            .line_height(px(20.))
            .flex()
            .flex_wrap()
            .children(segments.into_iter().map(|(text, bold)| {
                div()
                    .font_weight(FontWeight(if bold { 700. } else { 400. }))
                    .child(text)
            }));
        let tools = div()
            .absolute()
            .top(px(16.))
            .right(px(20.))
            .flex()
            .items_center()
            .gap(px(16.))
            .children((!canceled).then(|| {
                div()
                    .id("subscription-cancel")
                    .text_size(px(14.))
                    .line_height(px(16.))
                    .font_weight(FontWeight(500.))
                    .child("Cancel")
            }))
            .child(
                div()
                    .id("subscription-switch")
                    .h(px(32.))
                    .px(px(16.))
                    .rounded(px(3.))
                    .bg(gpui::rgb(0xffffff))
                    .text_color(gpui::rgb(0x000000))
                    .flex()
                    .items_center()
                    .text_size(px(14.))
                    .line_height(px(16.))
                    .font_weight(FontWeight(500.))
                    .child(if canceled {
                        "Re-subscribe"
                    } else {
                        "Switch Plans"
                    }),
            );
        banner
            .child(
                div()
                    .text_size(px(24.))
                    .line_height(px(28.))
                    .font_weight(FontWeight(800.))
                    .font_family(crate::ui::nitro::NITRO_FONT)
                    .child(sub.display_name().unwrap_or("Nitro").to_uppercase()),
            )
            .child(info)
            .child(tools)
            .into_any_element()
    }

    /// No saved payment source: the notice and "Add Payment Method". The button is drawn but not wired: the
    /// browser page that adds a source on its own is not in the snapshot.
    fn payment_methods_body(&self, cx: &mut gpui::Context<Self>) -> AnyElement {
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
            .child(
                div()
                    .id("billing-add-payment")
                    .cursor_pointer()
                    // A money flow: only its first dialog opens here (`actions::Dialog::Account("payment")`).
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.settings_extra.dialog =
                            Some(super::actions::Dialog::Account("payment"));
                        cx.notify();
                    }))
                    .child(button(
                        "billing-add-payment-button",
                        "Add Payment Method",
                        ButtonKind::Primary,
                    )),
            )
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

/// `medium` date of the ICU message format (en-US): "Nov 12, 2026", read from an ISO 8601 timestamp.
fn medium_date(iso: &str) -> Option<String> {
    const MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    let mut parts = iso.get(..10)?.split('-');
    let year: u32 = parts.next()?.parse().ok()?;
    let month: usize = parts.next()?.parse().ok()?;
    let day: u32 = parts.next()?.parse().ok()?;
    Some(format!(
        "{} {day}, {year}",
        MONTHS.get(month.checked_sub(1)?)?
    ))
}

/// The sentence of `getBillingInformationString` (module 722178) by status, as (text, bold) runs: active renews
/// ("Q18lRK"), canceled ends ("Whp/qk"), past due asks for payment info ("qEIzyi").
fn info_segments(state: u64, date: Option<&str>, price: Option<&str>) -> Vec<(String, bool)> {
    let date = date.unwrap_or("").to_owned();
    let price = price.unwrap_or("").to_owned();
    let run = |text: &str| (text.to_owned(), false);
    if state == status::CANCELED {
        vec![
            run("Your subscription will end on "),
            (date, true),
            run("."),
        ]
    } else if state == status::PAST_DUE {
        vec![
            run("Your "),
            (price, true),
            run(
                " subscription is past due. Please update your payment info or your subscription will end on ",
            ),
            (date, true),
            run("."),
        ]
    } else {
        vec![
            run("Your subscriptions will automatically renew on "),
            (date, true),
            run(" and you'll be charged "),
            (price, true),
            run("."),
        ]
    }
}

#[cfg(test)]
mod subscription_row_tests {
    use super::{info_segments, medium_date};
    use dc_state::premium::status;

    #[test]
    fn dates_read_like_icu_medium() {
        assert_eq!(
            medium_date("2026-11-12T08:00:00.000000+00:00").as_deref(),
            Some("Nov 12, 2026")
        );
        assert_eq!(medium_date("2027-01-03").as_deref(), Some("Jan 3, 2027"));
        assert_eq!(medium_date("soon"), None);
    }

    #[test]
    fn the_sentence_follows_the_status() {
        let line = |state| {
            info_segments(state, Some("Dec 1, 2026"), Some("TRY 104.99/month"))
                .into_iter()
                .map(|(text, _)| text)
                .collect::<String>()
        };
        assert_eq!(
            line(status::ACTIVE),
            "Your subscriptions will automatically renew on Dec 1, 2026 and you'll be charged TRY 104.99/month."
        );
        assert_eq!(
            line(status::CANCELED),
            "Your subscription will end on Dec 1, 2026."
        );
        assert!(line(status::PAST_DUE).contains("is past due"));
    }
}
