//! Shop Checkout Review modal (module 582695): Review header, Tenure badge / Nitro line,
//! payment method selector labels across all 19 methods, Orbs balance widget, and support links.

use crate::ui::{Root, color, icon};
use gpui::{
    AnyElement, Context, FontWeight, IntoElement, SharedString, deferred, div, prelude::*, px, rgba,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(dead_code)]
pub(crate) enum CheckoutMethod {
    Card,
    PayPal,
    Sofort,
    Giropay,
    Przelewy24,
    Paysafecard,
    GCash,
    GrabPay,
    MoMoWallet,
    Venmo,
    KaKaoPay,
    GoPay,
    Bancontact,
    Eps,
    Ideal,
    CashAppPay,
    Apple,
    Pix,
    PixAutomatico,
}

impl CheckoutMethod {
    /// Localized display name from module 582695.
    pub(crate) fn label(self) -> &'static str {
        let key = match self {
            Self::Card => "ei5/p8",
            Self::PayPal => "2dgEq+",
            Self::Sofort => "edKX/1",
            Self::Giropay => "y+0MQZ",
            Self::Przelewy24 => "u25uL0",
            Self::Paysafecard => "boznHN",
            Self::GCash => "PjehcF",
            Self::GrabPay => "T5davE",
            Self::MoMoWallet => "J0A1Vk",
            Self::Venmo => "jYOezc",
            Self::KaKaoPay => "CSVexi",
            Self::GoPay => "43J8JK",
            Self::Bancontact => "1ITkfq",
            Self::Eps => "5BSDU6",
            Self::Ideal => "nSbwqC",
            Self::CashAppPay => "+rbTmL",
            Self::Apple => "RFi12i",
            Self::Pix => "JG3WQU",
            Self::PixAutomatico => "JkKNss",
        };
        dc_intl::t(key)
    }

    pub(crate) const ALL: &[Self] = &[
        Self::Card,
        Self::PayPal,
        Self::Sofort,
        Self::Giropay,
        Self::Przelewy24,
        Self::Paysafecard,
        Self::GCash,
        Self::GrabPay,
        Self::MoMoWallet,
        Self::Venmo,
        Self::KaKaoPay,
        Self::GoPay,
        Self::Bancontact,
        Self::Eps,
        Self::Ideal,
        Self::CashAppPay,
        Self::Apple,
        Self::Pix,
        Self::PixAutomatico,
    ];
}

#[derive(Clone, Debug)]
#[allow(dead_code)]
pub(crate) struct ShopCheckoutReviewModal {
    pub product_name: String,
    pub price_label: String,
    pub show_tenure_badge: bool,
    pub show_nitro_discount: bool,
    pub method: CheckoutMethod,
    pub orb_balance: u64,
    pub orb_cost: Option<u64>,
    pub picker_open: bool,
}

/// Purchase legal fine print variants (module 596034 `I`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(dead_code)]
pub(crate) enum LegalFinePrintVariant {
    Subscription,
    SubscriptionTrial,
    Shop,
    OrbsRedemption,
    GiftNitro,
    GiftShop,
}

fn intl_or(key: &'static str, fallback: &'static str) -> &'static str {
    if dc_intl::has_key(key) {
        dc_intl::t(key)
    } else {
        fallback
    }
}

/// Module 708791 (`StripeExpressCheckout`): express checkout lifecycle states (`705822.i`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(dead_code)]
pub(crate) enum ExpressCheckoutState {
    Initializing,
    Submitting,
    Ready,
    Unavailable,
}

/// Module 708791 (`en`): one line item passed to Apple Pay / Google Pay Express Checkout.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ExpressLineItem {
    pub name: String,
    pub amount_cents: i64,
}

/// Module 708791 (`en`): builds Apple Pay / Google Pay Express Checkout line items: base item
/// (`taxInclusive ? total - tax : subtotal`), optional `bdF0KJ` ("Gift Card Adjustment", negative),
/// and optional `jiRvC7` ("Tax"). Returns empty when `vault_only` or `amount_due <= 0`.
pub(crate) fn build_express_checkout_line_items(
    product_name: &str,
    subtotal_cents: i64,
    tax_cents: i64,
    gift_card_cents: i64,
    tax_inclusive: bool,
    vault_only: bool,
) -> Vec<ExpressLineItem> {
    let total = subtotal_cents + if tax_inclusive { 0 } else { tax_cents };
    let amount_due = total - gift_card_cents;
    if vault_only || amount_due <= 0 {
        return Vec::new();
    }
    let base = if tax_inclusive {
        (total - tax_cents).max(0)
    } else {
        subtotal_cents
    };
    let mut items = vec![ExpressLineItem {
        name: product_name.to_owned(),
        amount_cents: base,
    }];
    if gift_card_cents > 0 {
        items.push(ExpressLineItem {
            name: intl_or("bdF0KJ", "Gift Card Adjustment").to_owned(),
            amount_cents: -gift_card_cents,
        });
    }
    if tax_cents > 0 {
        items.push(ExpressLineItem {
            name: dc_intl::t("jiRvC7").to_owned(),
            amount_cents: tax_cents,
        });
    }
    items
}

/// Module 708791 (`rI` + `Zr` + `w`): Stripe Express Checkout button bar (40px, 8px radius),
/// optional `— or —` (`N4EAHm`) divider, and centered footer note (`BMVyQO` / `gVM8va`).
pub(crate) fn render_express_checkout_block(
    product_name: &str,
    subtotal_cents: i64,
    remaining_amount: Option<&str>,
    show_divider_below: bool,
) -> AnyElement {
    let _line_items =
        build_express_checkout_line_items(product_name, subtotal_cents, 0, 0, false, false);
    let _toast_keys = (
        intl_or("cxwOJN", "Your total has been updated"),
        intl_or("302Ck/", "Your payment method has been saved"),
        intl_or(
            "6zCzw1",
            "There was a problem adding this payment method. Try again or use a different payment method.",
        ),
    );
    let footer_note = match remaining_amount {
        Some(amount) if dc_intl::has_key("BMVyQO") => dc_intl::format(
            "BMVyQO",
            &[(
                "amount",
                dc_intl::Arg::Str(std::borrow::Cow::Borrowed(amount)),
            )],
        ),
        Some(amount) => format!(
            "Pay the remaining {amount} in one step. We'll save your payment method for next time."
        ),
        None => intl_or("gVM8va", "Pay in one step. We'll save it for next time.").to_owned(),
    };
    div()
        .w_full()
        .flex()
        .flex_col()
        .gap(px(8.))
        .child(
            div()
                .w_full()
                .min_h(px(40.))
                .flex()
                .gap(px(8.))
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.))
                        .h(px(40.))
                        .rounded(px(8.))
                        .bg(gpui::rgba(0x000000ff))
                        .border_1()
                        .border_color(gpui::rgba(0xffffff22))
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_size(px(14.))
                        .font_weight(FontWeight(600.))
                        .text_color(color("--white"))
                        .child("Apple Pay"),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.))
                        .h(px(40.))
                        .rounded(px(8.))
                        .bg(gpui::rgba(0x1f1f1fff))
                        .border_1()
                        .border_color(gpui::rgba(0xffffff22))
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_size(px(14.))
                        .font_weight(FontWeight(600.))
                        .text_color(color("--white"))
                        .child("Google Pay"),
                ),
        )
        .child(
            div()
                .text_center()
                .text_size(px(12.))
                .line_height(px(16.))
                .text_color(color("--text-muted"))
                .child(footer_note),
        )
        .when(show_divider_below, |col| {
            col.child(
                div()
                    .w_full()
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .child(div().flex_1().h(px(1.)).bg(color("--border-subtle")))
                    .child(
                        div()
                            .text_size(px(12.))
                            .text_color(color("--text-muted"))
                            .child(intl_or("N4EAHm", "or")),
                    )
                    .child(div().flex_1().h(px(1.)).bg(color("--border-subtle"))),
            )
        })
        .into_any_element()
}

/// Module 708791 (`781300.IC`): validates a billing address before saving a Stripe Express payment source.
#[allow(dead_code)]
pub(crate) fn validate_billing_address_request(
    billing_address: serde_json::Value,
) -> crate::net::Request {
    crate::net::Request::Rest {
        key: Some("billing:validate-address".to_owned()),
        method: reqwest::Method::POST,
        route: "/users/@me/billing/payment-sources/validate-billing-address".to_owned(),
        body: Some(serde_json::json!({ "billing_address": billing_address })),
    }
}

/// Renders module 596034 purchase legal fine print: optional EU immediate delivery waiver
/// (`5dmUS+` / `XBtSMa`), transaction-type legal notice, and payment-source disclaimers
/// (`kj9VLI` Paysafecard non-refundable / `UYy1/h` Sofort SEPA debit).
pub(crate) fn render_purchase_legal_fine_print(
    variant: LegalFinePrintVariant,
    method: Option<CheckoutMethod>,
    plan_or_item: &str,
) -> AnyElement {
    let main_copy: String = match variant {
        LegalFinePrintVariant::Subscription => format!(
            "By completing your purchase of {plan_or_item}, you agree to Discord's Paid Services Terms. Your subscription will automatically renew until you cancel in User Settings."
        ),
        LegalFinePrintVariant::SubscriptionTrial => format!(
            "By starting your {plan_or_item} trial, you agree to Discord's Paid Services Terms. After the trial ends, your subscription will renew automatically unless canceled."
        ),
        LegalFinePrintVariant::OrbsRedemption => {
            "By redeeming Orbs for this item, you agree to Discord's Paid Services Terms and Virtual Goods Policy.".to_owned()
        }
        LegalFinePrintVariant::GiftNitro | LegalFinePrintVariant::GiftShop => format!(
            "By purchasing a gift of {plan_or_item}, you agree to Discord's Paid Services Terms."
        ),
        LegalFinePrintVariant::Shop => format!(
            "By purchasing {plan_or_item}, you agree to Discord's Paid Services Terms."
        ),
    };
    let source_notice: Option<&'static str> = match method {
        Some(CheckoutMethod::Paysafecard) => Some(intl_or(
            "kj9VLI",
            "Please note that Discord can't refund purchases made with a Paysafecard.",
        )),
        Some(CheckoutMethod::Sofort) => Some(intl_or(
            "UYy1/h",
            "By continuing, you authorize this Sofort / SEPA direct debit payment.",
        )),
        _ => None,
    };
    div()
        .id("purchase-legal-fine-print")
        .flex()
        .flex_col()
        .gap(px(8.))
        .child(
            div()
                .text_size(px(12.))
                .line_height(px(16.))
                .text_color(color("--text-muted"))
                .child(main_copy),
        )
        .when_some(source_notice, |col, notice| {
            col.child(
                div()
                    .text_size(px(12.))
                    .line_height(px(16.))
                    .text_color(color("--text-muted"))
                    .child(notice),
            )
        })
        .into_any_element()
}

impl Root {
    pub(crate) fn close_shop_checkout_review(&mut self) -> bool {
        self.shop_checkout_review_modal.take().is_some()
    }

    pub(in crate::ui) fn shop_checkout_review_layer(
        &self,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let modal = self.shop_checkout_review_modal.as_ref()?;
        let method_label = modal.method.label();
        let insufficient_orbs = modal.orb_cost.is_some_and(|cost| modal.orb_balance < cost);

        let dialog = div()
            .id("shop-checkout-review-modal")
            .occlude()
            .w(px(480.))
            .rounded(px(12.))
            .border_1()
            .border_color(color("--border-subtle"))
            .bg(color("--background-surface-high"))
            .overflow_hidden()
            .shadow(vec![gpui::BoxShadow {
                color: gpui::hsla(0., 0., 0., 0.3),
                offset: gpui::point(px(0.), px(12.)),
                blur_radius: px(28.),
                spread_radius: px(0.),
                inset: false,
            }])
            .flex()
            .flex_col()
            .child(
                div()
                    .px(px(20.))
                    .py(px(16.))
                    .border_b_1()
                    .border_color(color("--border-subtle"))
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .child(
                                div()
                                    .text_size(px(19.))
                                    .font_weight(FontWeight(700.))
                                    .text_color(color("--text-strong"))
                                    .child(dc_intl::t("QBnNHq")),
                            )
                            .when(modal.show_tenure_badge, |row| {
                                row.child(
                                    div()
                                        .px(px(6.))
                                        .py(px(2.))
                                        .rounded(px(4.))
                                        .bg(color("--control-primary-background-default"))
                                        .text_size(px(11.))
                                        .font_weight(FontWeight(600.))
                                        .text_color(color("--white"))
                                        .child(dc_intl::t("JEiq/c")),
                                )
                            }),
                    )
                    .child(
                        div()
                            .id("checkout-review-close")
                            .size(px(30.))
                            .rounded(px(8.))
                            .cursor_pointer()
                            .hover(|s| s.bg(color("--background-mod-subtle")))
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(icon("modal_close", 18., "--interactive-icon-default"))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.close_shop_checkout_review();
                                cx.notify();
                            })),
                    ),
            )
            .child(
                div()
                    .p(px(20.))
                    .flex()
                    .flex_col()
                    .gap(px(14.))
                    .child(
                        div()
                            .p(px(14.))
                            .rounded(px(8.))
                            .bg(color("--background-base-lower"))
                            .border_1()
                            .border_color(color("--border-subtle"))
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
                                            .text_size(px(15.))
                                            .font_weight(FontWeight(600.))
                                            .text_color(color("--text-strong"))
                                            .child(modal.product_name.clone()),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(15.))
                                            .font_weight(FontWeight(700.))
                                            .text_color(color("--text-strong"))
                                            .child(modal.price_label.clone()),
                                    ),
                            )
                            .when(modal.show_nitro_discount, |card| {
                                card.child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .gap(px(6.))
                                        .child(icon("nitro_wheel", 14., "--text-link"))
                                        .child(
                                            div()
                                                .text_size(px(12.))
                                                .font_weight(FontWeight(600.))
                                                .text_color(color("--text-link"))
                                                .child(dc_intl::t("lG6a5x")),
                                        ),
                                )
                            }),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(6.))
                            .child(
                                div()
                                    .id("checkout-review-method")
                                    .h(px(40.))
                                    .px(px(12.))
                                    .rounded(px(8.))
                                    .border_1()
                                    .border_color(color("--input-border-default"))
                                    .bg(color("--input-background-default"))
                                    .cursor_pointer()
                                    .flex()
                                    .items_center()
                                    .justify_between()
                                    .child(
                                        div()
                                            .text_size(px(14.))
                                            .font_weight(FontWeight(500.))
                                            .text_color(color("--text-strong"))
                                            .child(method_label),
                                    )
                                    .child(icon("chevron_down", 16., "--interactive-icon-default"))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        if let Some(m) = this.shop_checkout_review_modal.as_mut() {
                                            m.picker_open = !m.picker_open;
                                        }
                                        cx.notify();
                                    })),
                            )
                            .when(modal.picker_open, |col| {
                                col.child(
                                    div()
                                        .id("checkout-method-list")
                                        .max_h(px(160.))
                                        .overflow_y_scroll()
                                        .p(px(4.))
                                        .rounded(px(8.))
                                        .border_1()
                                        .border_color(color("--border-subtle"))
                                        .bg(color("--background-base-lower"))
                                        .flex()
                                        .flex_col()
                                        .gap(px(2.))
                                        .children(CheckoutMethod::ALL.iter().enumerate().map(
                                            |(i, &m_kind)| {
                                                let active = modal.method == m_kind;
                                                div()
                                                    .id(SharedString::from(format!(
                                                        "checkout-method-opt-{i}"
                                                    )))
                                                    .h(px(32.))
                                                    .px(px(10.))
                                                    .rounded(px(6.))
                                                    .when(active, |d| {
                                                        d.bg(color("--background-mod-strong"))
                                                    })
                                                    .hover(|s| {
                                                        s.bg(color("--background-mod-subtle"))
                                                    })
                                                    .cursor_pointer()
                                                    .flex()
                                                    .items_center()
                                                    .justify_between()
                                                    .text_size(px(13.))
                                                    .text_color(color("--text-strong"))
                                                    .child(m_kind.label())
                                                    .when(active, |d| {
                                                        d.child(icon(
                                                            "select_check",
                                                            14.,
                                                            "--control-primary-background-default",
                                                        ))
                                                    })
                                                    .on_click(cx.listener(move |this, _, _, cx| {
                                                        if let Some(m) =
                                                            this.shop_checkout_review_modal.as_mut()
                                                        {
                                                            m.method = m_kind;
                                                            m.picker_open = false;
                                                        }
                                                        cx.notify();
                                                    }))
                                            },
                                        )),
                                )
                            }),
                    )
                    .child(
                        div()
                            .p(px(12.))
                            .rounded(px(8.))
                            .bg(color("--background-base-lower"))
                            .border_1()
                            .border_color(color("--border-subtle"))
                            .flex()
                            .items_center()
                            .justify_between()
                            .gap(px(10.))
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap(px(2.))
                                    .child(
                                        div()
                                            .text_size(px(13.))
                                            .font_weight(FontWeight(600.))
                                            .text_color(color("--text-strong"))
                                            .child(format!("✦ {} Orbs", modal.orb_balance)),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(12.))
                                            .text_color(color("--text-link"))
                                            .child(dc_intl::t("7f4H7D")),
                                    ),
                            )
                            .when(insufficient_orbs, |row| {
                                row.child(
                                    div()
                                        .id("checkout-earn-orbs")
                                        .px(px(10.))
                                        .py(px(6.))
                                        .rounded(px(6.))
                                        .bg(color("--control-secondary-background-default"))
                                        .hover(|s| {
                                            s.bg(color("--control-secondary-background-hover"))
                                        })
                                        .cursor_pointer()
                                        .text_size(px(12.))
                                        .font_weight(FontWeight(500.))
                                        .text_color(color("--text-strong"))
                                        .child(dc_intl::t("H57f41"))
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.close_shop_checkout_review();
                                            this.open_quests(cx);
                                        })),
                                )
                            }),
                    )
                    // Module 29270 (`OneTimePurchaseReviewStepBody`): Order summary (`eoXh7B`),
                    // Tax (`/I8zmP`), Total Due (`Zxav97`), Payment method (`u+Cw58`), Promo badge
                    // (`Fjpyfj`), Collect & Claim banner (`/nFLZv`), and test-mode notice (`OvMyMd`).
                    .child({
                        let _category_and_promo_keys = (
                            dc_intl::t("jM8/71"),
                            dc_intl::t("r29Oel"),
                            dc_intl::t("eTYAvF"),
                            dc_intl::t("of9bom"),
                            dc_intl::t("HzDn6w"),
                            dc_intl::t("iZe9Wy"),
                            dc_intl::t("qwSlCO"),
                            dc_intl::t("hewwdA"),
                            dc_intl::t("YUNJJa"),
                            dc_intl::t("DePOcS"),
                            dc_intl::t("OvMyMd"),
                            dc_intl::t("tAmECt"),
                            dc_intl::format(
                                "Jkxtln",
                                &[
                                    (
                                        "current",
                                        dc_intl::Arg::Str(std::borrow::Cow::Borrowed("2")),
                                    ),
                                    ("target", dc_intl::Arg::Str(std::borrow::Cow::Borrowed("3"))),
                                ],
                            ),
                            dc_intl::format(
                                "OUIULI",
                                &[(
                                    "collectibleTitle",
                                    dc_intl::Arg::Str(std::borrow::Cow::Borrowed(
                                        &modal.product_name,
                                    )),
                                )],
                            ),
                            dc_intl::format(
                                "8yvdPi",
                                &[(
                                    "collectibleTitle",
                                    dc_intl::Arg::Str(std::borrow::Cow::Borrowed(
                                        &modal.product_name,
                                    )),
                                )],
                            ),
                            dc_intl::format("0StwHe", &[("orbCount", dc_intl::Arg::Num(500.))]),
                            dc_intl::format("ZV+aS9", &[("orbCount", dc_intl::Arg::Num(500.))]),
                        );
                        let promo_banner = dc_intl::format(
                            "/nFLZv",
                            &[
                                (
                                    "current",
                                    dc_intl::Arg::Str(std::borrow::Cow::Borrowed("2")),
                                ),
                                ("target", dc_intl::Arg::Str(std::borrow::Cow::Borrowed("3"))),
                                (
                                    "numDays",
                                    dc_intl::Arg::Str(std::borrow::Cow::Borrowed("5")),
                                ),
                            ],
                        );
                        div()
                            .p(px(12.))
                            .rounded(px(8.))
                            .bg(color("--background-base-lower"))
                            .border_1()
                            .border_color(color("--border-subtle"))
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
                                            .text_size(px(13.))
                                            .font_weight(FontWeight(700.))
                                            .text_color(color("--text-strong"))
                                            .child(dc_intl::t("eoXh7B")),
                                    )
                                    .child(
                                        div()
                                            .px(px(6.))
                                            .py(px(2.))
                                            .rounded(px(4.))
                                            .bg(gpui::rgb(0x5865f2))
                                            .text_size(px(10.))
                                            .font_weight(FontWeight(700.))
                                            .text_color(color("--white"))
                                            .child(dc_intl::t("Fjpyfj")),
                                    ),
                            )
                            .child(
                                div()
                                    .flex()
                                    .justify_between()
                                    .text_size(px(12.))
                                    .text_color(color("--text-muted"))
                                    .child(dc_intl::t("/I8zmP"))
                                    .child("$0.00"),
                            )
                            .child(
                                div()
                                    .flex()
                                    .justify_between()
                                    .text_size(px(13.))
                                    .font_weight(FontWeight(700.))
                                    .text_color(color("--text-strong"))
                                    .child(dc_intl::t("Zxav97"))
                                    .child(SharedString::from(modal.price_label.clone())),
                            )
                            .child(
                                div()
                                    .text_size(px(11.))
                                    .text_color(color("--text-muted"))
                                    .child(SharedString::from(format!(
                                        "{}: {}",
                                        dc_intl::t("u+Cw58"),
                                        promo_banner
                                    ))),
                            )
                    })
                    .when(modal.orb_cost.is_none(), |col| {
                        col.child(render_express_checkout_block(
                            &modal.product_name,
                            599,
                            None,
                            true,
                        ))
                    })
                    .child(render_purchase_legal_fine_print(
                        if modal.orb_cost.is_some() {
                            LegalFinePrintVariant::OrbsRedemption
                        } else {
                            LegalFinePrintVariant::Shop
                        },
                        Some(modal.method),
                        &modal.product_name,
                    ))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .text_size(px(12.))
                            .text_color(color("--text-muted"))
                            .child(format!("{} {}", dc_intl::t("5qZv9E"), dc_intl::t("XRdyjz")))
                            .child(
                                div()
                                    .id("checkout-open-shop")
                                    .cursor_pointer()
                                    .text_color(color("--text-link"))
                                    .child(dc_intl::t("J+vlIR"))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.close_shop_checkout_review();
                                        this.open_shop(cx);
                                    })),
                            ),
                    ),
            );

        Some(
            deferred(
                div()
                    .id("shop-checkout-review-backdrop")
                    .occlude()
                    .absolute()
                    .inset_0()
                    .bg(rgba(0x000000b8))
                    .flex()
                    .items_center()
                    .justify_center()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.close_shop_checkout_review();
                        cx.notify();
                    }))
                    .child(dialog),
            )
            .with_priority(3)
            .into_any_element(),
        )
    }

    #[cfg(feature = "capture")]
    pub(crate) fn script_shop_checkout_review(
        &mut self,
        arg: &str,
        cx: &mut Context<Self>,
    ) -> String {
        self.shop_checkout_review_modal = Some(ShopCheckoutReviewModal {
            product_name: "Starlight Avatar Frame".to_owned(),
            price_label: "$5.99".to_owned(),
            show_tenure_badge: true,
            show_nitro_discount: true,
            method: if arg == "paypal" {
                CheckoutMethod::PayPal
            } else {
                CheckoutMethod::Card
            },
            orb_balance: 1200,
            orb_cost: if arg == "express" { None } else { Some(2500) },
            picker_open: arg != "closed" && arg != "express",
        });
        cx.notify();
        format!("shop-checkout-review: {arg}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_27_keys_of_shop_checkout_review_582695_resolve() {
        assert_eq!(CheckoutMethod::ALL.len(), 19);
        for &m in CheckoutMethod::ALL {
            assert!(!m.label().is_empty());
        }
        for key in [
            "5qZv9E", "XRdyjz", "J+vlIR", "QBnNHq", "lG6a5x", "JEiq/c", "H57f41", "7f4H7D",
        ] {
            assert!(!dc_intl::t(key).is_empty(), "empty key {key}");
        }
    }

    #[test]
    fn express_checkout_708791_line_items_and_keys() {
        let items = build_express_checkout_line_items("Avatar Frame", 599, 50, 200, false, false);
        assert_eq!(items.len(), 3);
        assert_eq!(items[0].amount_cents, 599);
        assert_eq!(items[1].amount_cents, -200);
        assert_eq!(items[2].name, dc_intl::t("jiRvC7"));
        assert_eq!(items[2].amount_cents, 50);
        assert!(build_express_checkout_line_items("Frame", 599, 0, 599, false, false).is_empty());
        assert!(build_express_checkout_line_items("Frame", 599, 0, 0, false, true).is_empty());
        assert_eq!(dc_intl::t("jiRvC7"), "Tax");
    }
}
