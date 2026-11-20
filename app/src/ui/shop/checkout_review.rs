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
