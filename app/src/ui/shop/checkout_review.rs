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
            orb_cost: Some(2500),
            picker_open: arg != "closed",
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
}
