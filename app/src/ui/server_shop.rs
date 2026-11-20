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
