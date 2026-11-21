//! Server Shop storefront modal/view (module 38137): Products & Server Subscriptions tabs,
//! Downloadable + Role cards, creator promo state, and preview removal banner.

use super::{Root, color, icon};
use gpui::{
    AnyElement, Context, FontWeight, IntoElement, SharedString, deferred, div, prelude::*, px, rgba,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ServerShopTab {
    Products,
    Subscriptions,
}

#[derive(Clone, Debug)]
#[allow(dead_code)]
pub(crate) struct ServerShopProduct {
    pub id: String,
    pub name: String,
    pub price: String,
    pub is_bundle: bool,
    pub description: String,
}

#[derive(Clone, Debug)]
#[allow(dead_code)]
pub(crate) struct ServerShopModal {
    pub guild_name: String,
    pub tab: ServerShopTab,
    pub products: Vec<ServerShopProduct>,
    pub is_creator_preview: bool,
    pub is_tab_layout: bool,
    pub promo_dismissed: bool,
}

impl Root {
    pub(crate) fn close_server_shop(&mut self) -> bool {
        self.server_shop_modal.take().is_some()
    }

    pub(in crate::ui) fn server_shop_modal_layer(
        &self,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let modal = self.server_shop_modal.as_ref()?;
        let tab = modal.tab;

        let tab_btn = |id: &'static str,
                       title_key: &'static str,
                       sub_key: &'static str,
                       which: ServerShopTab| {
            let active = tab == which;
            div()
                .id(id)
                .flex_1()
                .p(px(12.))
                .rounded(px(8.))
                .border_1()
                .border_color(if active {
                    color("--control-primary-background-default")
                } else {
                    color("--border-subtle")
                })
                .bg(if active {
                    color("--background-mod-subtle")
                } else {
                    color("--background-base-lower")
                })
                .cursor_pointer()
                .flex()
                .flex_col()
                .gap(px(2.))
                .child(
                    div()
                        .text_size(px(14.))
                        .font_weight(FontWeight(600.))
                        .text_color(color("--text-strong"))
                        .child(dc_intl::t(title_key)),
                )
                .child(
                    div()
                        .text_size(px(12.))
                        .line_height(px(16.))
                        .text_color(color("--text-muted"))
                        .child(dc_intl::t(sub_key)),
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    if let Some(m) = this.server_shop_modal.as_mut() {
                        m.tab = which;
                    }
                    cx.notify();
                }))
        };

        let preview_banner = modal.is_creator_preview.then(|| {
            let msg = if modal.is_tab_layout {
                dc_intl::t("F4Z0nV")
            } else {
                dc_intl::t("pzTFpM")
            };
            let btn = if modal.is_tab_layout {
                dc_intl::t("YF35pV")
            } else {
                dc_intl::t("r30bky")
            };
            div()
                .px(px(16.))
                .py(px(10.))
                .bg(color("--background-base-lower"))
                .border_b_1()
                .border_color(color("--border-subtle"))
                .flex()
                .items_center()
                .justify_between()
                .gap(px(12.))
                .child(
                    div()
                        .text_size(px(12.))
                        .text_color(color("--text-default"))
                        .child(msg),
                )
                .child(
                    div()
                        .id("server-shop-remove-preview")
                        .px(px(10.))
                        .py(px(4.))
                        .rounded(px(6.))
                        .bg(color("--control-secondary-background-default"))
                        .hover(|s| s.bg(color("--control-secondary-background-hover")))
                        .cursor_pointer()
                        .text_size(px(12.))
                        .font_weight(FontWeight(500.))
                        .text_color(color("--text-strong"))
                        .child(btn)
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.close_server_shop();
                            cx.notify();
                        })),
                )
        });

        let content: AnyElement = match tab {
            ServerShopTab::Products => {
                if modal.products.is_empty() {
                    div()
                        .p(px(28.))
                        .flex()
                        .flex_col()
                        .items_center()
                        .gap(px(10.))
                        .child(
                            div()
                                .text_size(px(18.))
                                .font_weight(FontWeight(700.))
                                .text_color(color("--text-strong"))
                                .child(dc_intl::t("U2hvDo")),
                        )
                        .child(
                            div()
                                .text_size(px(14.))
                                .line_height(px(20.))
                                .text_color(color("--text-default"))
                                .text_center()
                                .child(dc_intl::t("w/TNGT")),
                        )
                        .into_any_element()
                } else {
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(12.))
                        .when(!modal.promo_dismissed, |col| {
                            col.child(
                                div()
                                    .p(px(12.))
                                    .rounded(px(8.))
                                    .bg(color("--background-base-lower"))
                                    .border_1()
                                    .border_color(color("--border-subtle"))
                                    .flex()
                                    .items_center()
                                    .justify_between()
                                    .gap(px(12.))
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
                                                    .child(dc_intl::t("827i8I")),
                                            )
                                            .child(
                                                div()
                                                    .text_size(px(12.))
                                                    .text_color(color("--text-muted"))
                                                    .child(dc_intl::t("GF433I")),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .id("server-shop-dismiss-promo")
                                            .px(px(10.))
                                            .py(px(5.))
                                            .rounded(px(6.))
                                            .bg(color("--control-secondary-background-default"))
                                            .cursor_pointer()
                                            .text_size(px(12.))
                                            .text_color(color("--text-strong"))
                                            .child(dc_intl::t("WAI6xu"))
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                if let Some(m) = this.server_shop_modal.as_mut() {
                                                    m.promo_dismissed = true;
                                                }
                                                cx.notify();
                                            })),
                                    ),
                            )
                        })
                        .child(
                            div()
                                .text_size(px(12.))
                                .font_weight(FontWeight(700.))
                                .text_color(color("--text-muted"))
                                .child(dc_intl::t("qe4kTa").to_uppercase()),
                        )
                        .child(div().flex().gap(px(12.)).children(
                            modal.products.iter().enumerate().map(|(i, prod)| {
                                div()
                                    .id(SharedString::from(format!(
                                        "srv-shop-prod-{i}-{}",
                                        prod.id
                                    )))
                                    .flex_1()
                                    .p(px(14.))
                                    .rounded(px(10.))
                                    .border_1()
                                    .border_color(color("--border-subtle"))
                                    .bg(color("--background-base-lower"))
                                    .flex()
                                    .flex_col()
                                    .gap(px(8.))
                                    .when(prod.is_bundle, |card| {
                                        card.child(
                                            div()
                                                .self_start()
                                                .px(px(6.))
                                                .py(px(2.))
                                                .rounded(px(4.))
                                                .bg(color("--control-primary-background-default"))
                                                .text_size(px(11.))
                                                .font_weight(FontWeight(600.))
                                                .text_color(color("--white"))
                                                .child(dc_intl::t("ih4QMU")),
                                        )
                                    })
                                    .child(
                                        div()
                                            .text_size(px(15.))
                                            .font_weight(FontWeight(600.))
                                            .text_color(color("--text-strong"))
                                            .child(prod.name.clone()),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(12.))
                                            .line_height(px(17.))
                                            .text_color(color("--text-muted"))
                                            .child(prod.description.clone()),
                                    )
                                    .child(
                                        div()
                                            .mt(px(4.))
                                            .text_size(px(15.))
                                            .font_weight(FontWeight(700.))
                                            .text_color(color("--text-strong"))
                                            .child(prod.price.clone()),
                                    )
                            }),
                        ))
                        .into_any_element()
                }
            }
            ServerShopTab::Subscriptions => {
                let notice = dc_intl::format("ycs4oO", &[]);
                div()
                    .p(px(20.))
                    .rounded(px(10.))
                    .border_1()
                    .border_color(color("--border-subtle"))
                    .bg(color("--background-base-lower"))
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(px(10.))
                    .child(icon(
                        "boost_gem",
                        32.,
                        "--control-primary-background-default",
                    ))
                    .child(
                        div()
                            .text_size(px(15.))
                            .font_weight(FontWeight(600.))
                            .text_color(color("--text-strong"))
                            .child(dc_intl::t("KzCF/6")),
                    )
                    .child(
                        div()
                            .text_size(px(13.))
                            .text_color(color("--text-muted"))
                            .child(notice),
                    )
                    .into_any_element()
            }
        };

        let learn_more = dc_intl::format("umfZ8q", &[]);

        let dialog = div()
            .id("server-shop-modal")
            .occlude()
            .w(px(660.))
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
            .children(preview_banner)
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
                                    .child(dc_intl::t("al5EXL")),
                            )
                            .child(
                                div()
                                    .text_size(px(13.))
                                    .text_color(color("--text-muted"))
                                    .child(format!("· {}", modal.guild_name)),
                            ),
                    )
                    .child(
                        div()
                            .id("server-shop-close")
                            .size(px(30.))
                            .rounded(px(8.))
                            .cursor_pointer()
                            .hover(|s| s.bg(color("--background-mod-subtle")))
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(icon("modal_close", 18., "--interactive-icon-default"))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.close_server_shop();
                                cx.notify();
                            })),
                    ),
            )
            .child(
                div()
                    .p(px(20.))
                    .flex()
                    .flex_col()
                    .gap(px(16.))
                    .child(
                        div()
                            .flex()
                            .gap(px(12.))
                            .child(tab_btn(
                                "server-shop-tab-products",
                                "odvTUp",
                                "0JFpRW",
                                ServerShopTab::Products,
                            ))
                            .child(tab_btn(
                                "server-shop-tab-subs",
                                "KzCF/6",
                                "jqz+ly",
                                ServerShopTab::Subscriptions,
                            )),
                    )
                    .child(content)
                    .child(
                        div()
                            .text_size(px(12.))
                            .text_color(color("--text-muted"))
                            .child(learn_more),
                    ),
            );

        Some(
            deferred(
                div()
                    .id("server-shop-backdrop")
                    .occlude()
                    .absolute()
                    .inset_0()
                    .bg(rgba(0x000000b8))
                    .flex()
                    .items_center()
                    .justify_center()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.close_server_shop();
                        cx.notify();
                    }))
                    .child(dialog),
            )
            .with_priority(3)
            .into_any_element(),
        )
    }

    #[cfg(feature = "capture")]
    pub(crate) fn script_server_shop(&mut self, arg: &str, cx: &mut Context<Self>) -> String {
        let products = if arg == "empty" {
            Vec::new()
        } else {
            vec![
                ServerShopProduct {
                    id: "p1".to_owned(),
                    name: "Starter Wallpaper Pack".to_owned(),
                    price: "$3.99".to_owned(),
                    is_bundle: true,
                    description: "4K wallpapers plus an exclusive Patron role badge.".to_owned(),
                },
                ServerShopProduct {
                    id: "p2".to_owned(),
                    name: "VIP Color Role".to_owned(),
                    price: "$2.99".to_owned(),
                    is_bundle: false,
                    description: "Stand out in the member list with custom flair.".to_owned(),
                },
            ]
        };
        self.server_shop_modal = Some(ServerShopModal {
            guild_name: "gravecord fixture".to_owned(),
            tab: if arg == "subs" {
                ServerShopTab::Subscriptions
            } else {
                ServerShopTab::Products
            },
            products,
            is_creator_preview: arg != "member",
            is_tab_layout: arg != "page",
            promo_dismissed: false,
        });
        cx.notify();
        format!("server-shop: {arg}")
    }
}

/// Server Shop Product Purchase Success modal (module 779457 + 179499 + 228105).
#[derive(Clone, Debug)]
#[allow(dead_code)]
pub(crate) struct ServerShopPurchaseSuccessModal {
    pub guild_id: u64,
    pub guild_name: String,
    pub product_id: String,
    pub product_name: String,
    pub product_type: String,
    pub total_paid: String,
    pub attachments: Vec<(String, String)>,
    pub role_name: Option<String>,
    pub loading: bool,
    pub error: bool,
}

impl Root {
    /// Renders module `779457` Server Shop Product Purchase Success modal.
    pub(in crate::ui) fn server_shop_purchase_success_layer(
        &self,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let modal = self.server_shop_purchase_success.as_ref()?;
        let _extra_keys = (dc_intl::t("R0RpRX"), dc_intl::t("eAn6z2"));
        let card: AnyElement = if modal.error {
            // Product refresh failure alert (`OCwKlu` / `VQ0lXY` / `NX+WJN`)
            div()
                .id("server-shop-success-err")
                .w(px(420.))
                .rounded(px(12.))
                .bg(color("--background-surface-high"))
                .border_1()
                .border_color(color("--border-subtle"))
                .p(px(24.))
                .flex()
                .flex_col()
                .gap(px(12.))
                .on_click(|_, _, cx| cx.stop_propagation())
                .child(
                    div()
                        .text_size(px(18.))
                        .font_weight(FontWeight(700.))
                        .text_color(color("--text-strong"))
                        .child(dc_intl::t("OCwKlu")),
                )
                .child(
                    div()
                        .text_size(px(14.))
                        .line_height(px(20.))
                        .text_color(color("--text-muted"))
                        .child(dc_intl::t("VQ0lXY")),
                )
                .child(
                    div().flex().justify_end().child(
                        div()
                            .id("server-shop-success-err-ok")
                            .h(px(36.))
                            .px(px(18.))
                            .rounded(px(8.))
                            .bg(color("--control-primary-background-default"))
                            .cursor_pointer()
                            .flex()
                            .items_center()
                            .text_size(px(14.))
                            .font_weight(FontWeight(600.))
                            .text_color(color("--white"))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.server_shop_purchase_success = None;
                                cx.notify();
                            }))
                            .child(dc_intl::t("NX+WJN")),
                    ),
                )
                .into_any_element()
        } else {
            let subtitle = dc_intl::format(
                "X6yvvo",
                &[(
                    "productName",
                    dc_intl::Arg::Str(std::borrow::Cow::Borrowed(&modal.product_name)),
                )],
            );
            let role_granted = modal.role_name.as_deref().map(|role| {
                dc_intl::format(
                    "IeJDKs",
                    &[(
                        "roleName",
                        dc_intl::Arg::Str(std::borrow::Cow::Borrowed(role)),
                    )],
                )
            });
            let has_attachments = !modal.attachments.is_empty();
            let single_attachment = modal.attachments.len() == 1;
            let gid = modal.guild_id;
            let pid = modal.product_id.clone();
            let first_aid = modal.attachments.first().map(|(id, _)| id.clone());

            div()
                .id("server-shop-purchase-success-modal")
                .w(px(500.))
                .rounded(px(12.))
                .bg(color("--background-surface-high"))
                .border_1()
                .border_color(color("--border-subtle"))
                .p(px(24.))
                .flex()
                .flex_col()
                .gap(px(14.))
                .on_click(|_, _, cx| cx.stop_propagation())
                // Header (`dhAgv3` "You're all set." + `X6yvvo`)
                .child(
                    div()
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
                                .gap(px(4.))
                                .child(
                                    div()
                                        .text_size(px(20.))
                                        .font_weight(FontWeight(700.))
                                        .text_color(color("--text-strong"))
                                        .child(dc_intl::t("dhAgv3")),
                                )
                                .child(
                                    div()
                                        .text_size(px(14.))
                                        .line_height(px(20.))
                                        .text_color(color("--text-muted"))
                                        .child(SharedString::from(subtitle)),
                                ),
                        )
                        .child(
                            div()
                                .id("server-shop-success-close")
                                .size(px(28.))
                                .flex_none()
                                .cursor_pointer()
                                .flex()
                                .items_center()
                                .justify_center()
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.server_shop_purchase_success = None;
                                    cx.notify();
                                }))
                                .child(icon("modal_close", 16., "--icon-subtle")),
                        ),
                )
                // Dynamic description (`A/RnkK`, `IeJDKs`, `9S3QHF`)
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(6.))
                        .text_size(px(13.))
                        .line_height(px(18.))
                        .text_color(color("--text-default"))
                        .when(has_attachments, |col| col.child(dc_intl::t("A/RnkK")))
                        .when_some(role_granted, |col, msg| col.child(SharedString::from(msg)))
                        .child(dc_intl::t("9S3QHF")),
                )
                // Receipt summary card (`5dWOZX`: "Total paid")
                .child(
                    div()
                        .p(px(14.))
                        .rounded(px(10.))
                        .bg(color("--background-base-lower"))
                        .border_1()
                        .border_color(color("--border-subtle"))
                        .flex()
                        .items_center()
                        .justify_between()
                        .gap(px(12.))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .flex()
                                .flex_col()
                                .gap(px(2.))
                                .child(
                                    div()
                                        .text_size(px(15.))
                                        .font_weight(FontWeight(700.))
                                        .text_color(color("--text-strong"))
                                        .child(SharedString::from(modal.product_name.clone())),
                                )
                                .child(
                                    div()
                                        .text_size(px(12.))
                                        .text_color(color("--text-muted"))
                                        .child(SharedString::from(format!(
                                            "{} · {}",
                                            modal.product_type, modal.guild_name
                                        ))),
                                ),
                        )
                        .child(
                            div()
                                .flex_none()
                                .flex()
                                .flex_col()
                                .items_end()
                                .child(
                                    div()
                                        .text_size(px(16.))
                                        .font_weight(FontWeight(700.))
                                        .text_color(color("--text-strong"))
                                        .child(SharedString::from(modal.total_paid.clone())),
                                )
                                .child(
                                    div()
                                        .text_size(px(11.))
                                        .text_color(color("--text-muted"))
                                        .child(dc_intl::t("5dWOZX")),
                                ),
                        ),
                )
                // Divider
                .child(div().h(px(1.)).bg(color("--border-subtle")))
                // Unlocked benefits: Downloadable (`HOkHPB`) + Role (`gWBNet`)
                .when(has_attachments, |col| {
                    col.child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .gap(px(10.))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .flex()
                                    .flex_col()
                                    .gap(px(4.))
                                    .child(
                                        div()
                                            .text_size(px(13.))
                                            .font_weight(FontWeight(700.))
                                            .text_color(color("--text-strong"))
                                            .child(dc_intl::t("HOkHPB")),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(12.))
                                            .text_color(color("--text-muted"))
                                            .child(SharedString::from(
                                                modal
                                                    .attachments
                                                    .iter()
                                                    .map(|(_, n)| n.as_str())
                                                    .collect::<Vec<_>>()
                                                    .join(", "),
                                            )),
                                    ),
                            )
                            .child(
                                div()
                                    .id("server-shop-download-btn")
                                    .flex_none()
                                    .h(px(34.))
                                    .px(px(14.))
                                    .rounded(px(8.))
                                    .bg(color("--control-primary-background-default"))
                                    .cursor_pointer()
                                    .flex()
                                    .items_center()
                                    .text_size(px(13.))
                                    .font_weight(FontWeight(600.))
                                    .text_color(color("--white"))
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        if let Some(aid) = first_aid.as_deref() {
                                            let _ = this.requests.send(
                                                crate::net::Request::Rest {
                                                    key: None,
                                                    method: reqwest::Method::POST,
                                                    route: format!(
                                                        "/guilds/{gid}/products/listings/{pid}/attachments/{aid}/download"
                                                    ),
                                                    body: None,
                                                },
                                            );
                                        }
                                        cx.notify();
                                    }))
                                    .child(dc_intl::t(if single_attachment {
                                        "t9bE9b"
                                    } else {
                                        "UyuiUE"
                                    })),
                            ),
                    )
                })
                .when_some(modal.role_name.clone(), |col, role| {
                    col.child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .child(
                                div()
                                    .text_size(px(13.))
                                    .font_weight(FontWeight(700.))
                                    .text_color(color("--text-strong"))
                                    .child(dc_intl::t("gWBNet")),
                            )
                            .child(
                                div()
                                    .px(px(10.))
                                    .py(px(4.))
                                    .rounded(px(12.))
                                    .bg(color("--background-base-lower"))
                                    .border_1()
                                    .border_color(color("--border-subtle"))
                                    .text_size(px(12.))
                                    .font_weight(FontWeight(600.))
                                    .text_color(color("--text-default"))
                                    .child(SharedString::from(role)),
                            ),
                    )
                })
                .into_any_element()
        };

        Some(
            deferred(
                div()
                    .id("server-shop-purchase-success-backdrop")
                    .occlude()
                    .absolute()
                    .inset_0()
                    .bg(rgba(0x000000b8))
                    .flex()
                    .items_center()
                    .justify_center()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.server_shop_purchase_success = None;
                        cx.notify();
                    }))
                    .child(card),
            )
            .with_priority(6)
            .into_any_element(),
        )
    }

    #[cfg(feature = "capture")]
    pub(crate) fn script_server_shop_success(
        &mut self,
        mode: &str,
        cx: &mut Context<Self>,
    ) -> String {
        if mode.trim() == "close" {
            self.server_shop_purchase_success = None;
            cx.notify();
            return "server-shop-success: closed".into();
        }
        self.server_shop_purchase_success = Some(ServerShopPurchaseSuccessModal {
            guild_id: 100,
            guild_name: "gravecord fixture".into(),
            product_id: "p1".into(),
            product_name: "Starter Wallpaper Pack".into(),
            product_type: "Bundle".into(),
            total_paid: "$3.99".into(),
            attachments: vec![("a1".into(), "4k-wallpapers.zip".into())],
            role_name: Some("Patron".into()),
            loading: false,
            error: mode.trim() == "error",
        });
        cx.notify();
        format!("server-shop-success: {}", mode.trim())
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn all_18_keys_of_server_shop_38137_resolve() {
        for key in [
            "qe4kTa", "WAI6xu", "odvTUp", "0JFpRW", "KzCF/6", "jqz+ly", "ycs4oO", "827i8I",
            "GF433I", "U2hvDo", "w/TNGT", "ih4QMU", "umfZ8q", "F4Z0nV", "pzTFpM", "YF35pV",
            "r30bky", "al5EXL",
        ] {
            assert!(!dc_intl::t(key).is_empty(), "empty key {key}");
        }
    }
}
