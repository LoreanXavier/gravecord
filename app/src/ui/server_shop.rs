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
    /// Downloadable attachments `(id, filename, size_bytes)` (module 733230 / 59375).
    pub attachments: Vec<(String, String, u64)>,
    /// Optional included server role `(name, rgb_color)` (module 733230 / 411342).
    pub role: Option<(String, u32)>,
    /// Whether the viewer already owns this product listing.
    pub has_entitlement: bool,
}

/// Server Product Info Modal state (`GUILD_PRODUCT_LISTING_INFO_MODAL`, module 733230).
#[derive(Clone, Debug)]
pub(crate) struct ServerProductInfoModal {
    pub product: ServerShopProduct,
    pub contents_expanded: bool,
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
    /// Active Product Info detail dialog (module 733230).
    pub detail: Option<ServerProductInfoModal>,
}

impl Root {
    pub(crate) fn close_server_shop(&mut self) -> bool {
        if let Some(m) = self.server_shop_modal.as_mut()
            && m.detail.take().is_some()
        {
            return true;
        }
        self.server_shop_modal.take().is_some()
    }

    pub(in crate::ui) fn server_shop_modal_layer(
        &self,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let modal = self.server_shop_modal.as_ref()?;
        if let Some(detail) = modal.detail.as_ref() {
            return Some(self.server_product_info_dialog(
                &modal.guild_name,
                modal.is_creator_preview,
                detail,
                cx,
            ));
        }
        let tab = modal.tab;
        let welcome_header = self.server_shop_welcome_header(modal, cx);

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
                                let clicked = prod.clone();
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
                                    .hover(|s| s.border_color(color("--border-normal")))
                                    .bg(color("--background-base-lower"))
                                    .cursor_pointer()
                                    .flex()
                                    .flex_col()
                                    .gap(px(8.))
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        if let Some(m) = this.server_shop_modal.as_mut() {
                                            m.detail = Some(ServerProductInfoModal {
                                                product: clicked.clone(),
                                                contents_expanded: false,
                                            });
                                        }
                                        cx.notify();
                                    }))
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
                    .px(px(20.))
                    .py(px(14.))
                    .flex()
                    .flex_col()
                    .gap(px(12.))
                    .child(welcome_header)
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

    /// Renders `GUILD_PRODUCT_LISTING_INFO_MODAL` (module 733230 + 571654 + 411342 + 731423 + 179499):
    /// subtitle `xImSei` (`"{productType} offered by {personName}"`), cover hero, expandable
    /// `ESEKua`/`DKNxgB` ("View/Collapse Product Contents") showing `DWYJua` Downloadable attachments
    /// and `o9xphc` Role pill (with lock icon when unentitled), `TNnDJs` "Product Details" description,
    /// and action bar with price + Purchase/Purchased/Download Now/Access Files button.
    fn server_product_info_dialog(
        &self,
        guild_name: &str,
        is_creator_preview: bool,
        detail: &ServerProductInfoModal,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let product = &detail.product;
        let has_att = !product.attachments.is_empty();
        let has_role = product.role.is_some();
        let product_type = match (has_att, has_role) {
            (true, true) => dc_intl::t("ih4QMU"),
            (true, false) => dc_intl::t("DWYJua"),
            _ => dc_intl::t("o9xphc"),
        };
        let subtitle = dc_intl::format(
            "xImSei",
            &[
                ("productType", product_type.into()),
                ("personName", guild_name.into()),
            ],
        );
        let role_pill = product.role.as_ref().map(|(rname, rcol)| {
            let dot_rgba = ((*rcol) << 8) | 0xff;
            let _sr_prefix = dc_intl::t("RnyseL");
            div()
                .self_start()
                .px(px(10.))
                .py(px(5.))
                .rounded(px(6.))
                .bg(color("--background-mod-subtle"))
                .border_1()
                .border_color(color("--border-subtle"))
                .flex()
                .items_center()
                .gap(px(6.))
                .child(div().size(px(10.)).rounded_full().bg(rgba(dot_rgba)))
                .child(
                    div()
                        .text_size(px(13.))
                        .font_weight(FontWeight(500.))
                        .text_color(color("--text-strong"))
                        .child(rname.clone()),
                )
                .when(!product.has_entitlement, |pill| {
                    pill.child(icon("access_lock", 12., "--text-muted"))
                })
                .into_any_element()
        });

        let contents_section: Option<AnyElement> = if has_att {
            let expanded = detail.contents_expanded;
            let toggle_label = if expanded {
                dc_intl::t("DKNxgB")
            } else {
                dc_intl::t("ESEKua")
            };
            let att_rows: Vec<AnyElement> = product
                .attachments
                .iter()
                .map(|(aid, fname, bytes)| {
                    let mb = (*bytes as f64) / (1024. * 1024.);
                    div()
                        .id(SharedString::from(format!("srv-prod-att-{aid}")))
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .text_size(px(13.))
                        .text_color(color("--text-default"))
                        .child(icon("plugin_cloud_download", 14., "--text-muted"))
                        .child(fname.clone())
                        .child(
                            div()
                                .text_color(color("--text-muted"))
                                .child(format!("• {mb:.2} MB")),
                        )
                        .into_any_element()
                })
                .collect();
            Some(
                div()
                    .p(px(12.))
                    .rounded(px(8.))
                    .bg(color("--background-base-lower"))
                    .border_1()
                    .border_color(color("--border-subtle"))
                    .flex()
                    .flex_col()
                    .gap(px(10.))
                    .child(
                        div()
                            .id("srv-prod-contents-toggle")
                            .cursor_pointer()
                            .flex()
                            .items_center()
                            .justify_between()
                            .on_click(cx.listener(|this, _, _, cx| {
                                if let Some(d) = this
                                    .server_shop_modal
                                    .as_mut()
                                    .and_then(|m| m.detail.as_mut())
                                {
                                    d.contents_expanded = !d.contents_expanded;
                                }
                                cx.notify();
                            }))
                            .child(
                                div()
                                    .text_size(px(14.))
                                    .font_weight(FontWeight(600.))
                                    .text_color(color("--text-strong"))
                                    .child(toggle_label),
                            )
                            .child(icon("shop_select_chevron", 14., "--text-muted")),
                    )
                    .when(expanded, |box_col| {
                        box_col
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap(px(6.))
                                    .child(
                                        div()
                                            .text_size(px(11.))
                                            .font_weight(FontWeight(700.))
                                            .text_color(color("--text-muted"))
                                            .child(dc_intl::t("DWYJua").to_uppercase()),
                                    )
                                    .children(att_rows),
                            )
                            .when_some(role_pill, |col, pill| {
                                col.child(div().w_full().h(px(1.)).bg(color("--border-subtle")))
                                    .child(
                                        div()
                                            .flex()
                                            .flex_col()
                                            .gap(px(6.))
                                            .child(
                                                div()
                                                    .text_size(px(11.))
                                                    .font_weight(FontWeight(700.))
                                                    .text_color(color("--text-muted"))
                                                    .child(dc_intl::t("o9xphc").to_uppercase()),
                                            )
                                            .child(pill),
                                    )
                            })
                    })
                    .into_any_element(),
            )
        } else {
            role_pill
        };

        let (cta_label, cta_icon, cta_primary, cta_disabled) = if is_creator_preview {
            (dc_intl::t("xUi3BL"), None, true, true)
        } else if !product.has_entitlement {
            (dc_intl::t("xUi3BL"), None, true, false)
        } else if product.attachments.len() == 1 {
            (
                dc_intl::t("t9bE9b"),
                Some("plugin_cloud_download"),
                true,
                false,
            )
        } else if product.attachments.len() > 1 {
            (dc_intl::t("UyuiUE"), Some("shop_arrow_right"), true, false)
        } else {
            (dc_intl::t("RcTOGF"), None, false, true)
        };

        let dialog = div()
            .id("server-product-info-modal")
            .occlude()
            .w(px(480.))
            .rounded(px(12.))
            .bg(color("--modal-background"))
            .border_1()
            .border_color(color("--border-subtle"))
            .overflow_hidden()
            .flex()
            .flex_col()
            .on_click(cx.listener(|_, _, _, cx| cx.stop_propagation()))
            // Header
            .child(
                div()
                    .px(px(20.))
                    .pt(px(18.))
                    .pb(px(12.))
                    .flex()
                    .items_start()
                    .justify_between()
                    .gap(px(12.))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(2.))
                            .child(
                                div()
                                    .text_size(px(20.))
                                    .line_height(px(24.))
                                    .font_weight(FontWeight(700.))
                                    .text_color(color("--text-strong"))
                                    .child(product.name.clone()),
                            )
                            .child(
                                div()
                                    .text_size(px(13.))
                                    .line_height(px(18.))
                                    .text_color(color("--text-muted"))
                                    .child(subtitle),
                            ),
                    )
                    .child(
                        div()
                            .id("srv-prod-info-close")
                            .size(px(28.))
                            .rounded(px(6.))
                            .cursor_pointer()
                            .hover(|s| s.bg(color("--background-mod-subtle")))
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(icon("modal_close", 16., "--interactive-icon-default"))
                            .on_click(cx.listener(|this, _, _, cx| {
                                if let Some(m) = this.server_shop_modal.as_mut() {
                                    m.detail = None;
                                }
                                cx.notify();
                            })),
                    ),
            )
            // Body
            .child(
                div()
                    .px(px(20.))
                    .pb(px(16.))
                    .flex()
                    .flex_col()
                    .gap(px(14.))
                    // Cover visual placeholder
                    .child(
                        div()
                            .w_full()
                            .h(px(148.))
                            .rounded(px(8.))
                            .bg(color("--background-base-lower"))
                            .border_1()
                            .border_color(color("--border-subtle"))
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(icon(
                                "storefront_store",
                                40.,
                                "--control-primary-background-default",
                            )),
                    )
                    .children(contents_section)
                    .child(div().w_full().h(px(1.)).bg(color("--border-subtle")))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(6.))
                            .child(
                                div()
                                    .text_size(px(16.))
                                    .font_weight(FontWeight(600.))
                                    .text_color(color("--text-strong"))
                                    .child(dc_intl::t("TNnDJs")),
                            )
                            .child(
                                div()
                                    .text_size(px(14.))
                                    .line_height(px(20.))
                                    .text_color(color("--text-muted"))
                                    .child(product.description.clone()),
                            ),
                    ),
            )
            // Action bar
            .child(
                div()
                    .px(px(20.))
                    .py(px(14.))
                    .bg(color("--modal-footer-background"))
                    .border_t_1()
                    .border_color(color("--border-subtle"))
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .text_size(px(20.))
                            .font_weight(FontWeight(700.))
                            .text_color(color("--text-strong"))
                            .child(product.price.clone()),
                    )
                    .child(
                        div()
                            .id("srv-prod-info-cta")
                            .h(px(38.))
                            .px(px(18.))
                            .rounded(px(8.))
                            .bg(if cta_primary {
                                color("--control-primary-background-default")
                            } else {
                                color("--control-secondary-background-default")
                            })
                            .when(cta_disabled, |btn| btn.opacity(0.55))
                            .when(!cta_disabled, |btn| {
                                btn.cursor_pointer().hover(|s| {
                                    s.bg(if cta_primary {
                                        color("--control-primary-background-hover")
                                    } else {
                                        color("--control-secondary-background-hover")
                                    })
                                })
                            })
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .text_size(px(14.))
                            .font_weight(FontWeight(600.))
                            .text_color(if cta_primary {
                                color("--white")
                            } else {
                                color("--text-strong")
                            })
                            .children(cta_icon.map(|g| icon(g, 15., "--white")))
                            .child(cta_label)
                            .when(!cta_disabled, |btn| {
                                btn.on_click(cx.listener(|this, _, _, cx| {
                                    if let Some(m) = this.server_shop_modal.as_mut() {
                                        m.detail = None;
                                    }
                                    cx.notify();
                                }))
                            }),
                    ),
            );

        deferred(
            div()
                .id("server-product-info-backdrop")
                .occlude()
                .absolute()
                .inset_0()
                .bg(rgba(0x000000b3))
                .flex()
                .items_center()
                .justify_center()
                .on_click(cx.listener(|this, _, _, cx| {
                    if let Some(m) = this.server_shop_modal.as_mut() {
                        m.detail = None;
                    }
                    cx.notify();
                }))
                .child(dialog),
        )
        .with_priority(3)
        .into_any_element()
    }

    #[cfg(feature = "capture")]
    pub(crate) fn script_server_shop(&mut self, arg: &str, cx: &mut Context<Self>) -> String {
        let sample_bundle = ServerShopProduct {
            id: "p1".to_owned(),
            name: "Starter Wallpaper Pack".to_owned(),
            price: "$3.99".to_owned(),
            is_bundle: true,
            description: "4K wallpapers plus an exclusive Patron role badge.".to_owned(),
            attachments: vec![
                (
                    "a1".to_owned(),
                    "desktop_4k_pack.zip".to_owned(),
                    14_800_000,
                ),
                (
                    "a2".to_owned(),
                    "mobile_wallpapers.zip".to_owned(),
                    6_250_000,
                ),
            ],
            role: Some(("Shop Patron".to_owned(), 0x5865f2)),
            has_entitlement: arg == "product-owned",
        };
        let products = if arg == "empty" {
            Vec::new()
        } else {
            vec![
                sample_bundle.clone(),
                ServerShopProduct {
                    id: "p2".to_owned(),
                    name: "VIP Color Role".to_owned(),
                    price: "$2.99".to_owned(),
                    is_bundle: false,
                    description: "Stand out in the member list with custom flair.".to_owned(),
                    attachments: Vec::new(),
                    role: Some(("VIP Color".to_owned(), 0xeb459e)),
                    has_entitlement: false,
                },
            ]
        };
        let detail = match arg {
            "product-info" => Some(ServerProductInfoModal {
                product: sample_bundle,
                contents_expanded: false,
            }),
            "product-info-expanded" | "product-owned" => Some(ServerProductInfoModal {
                product: sample_bundle,
                contents_expanded: true,
            }),
            _ => None,
        };
        self.server_shop_modal = Some(ServerShopModal {
            guild_name: "gravecord fixture".to_owned(),
            tab: if arg == "subs" {
                ServerShopTab::Subscriptions
            } else {
                ServerShopTab::Products
            },
            products,
            is_creator_preview: arg != "member" && detail.is_none(),
            is_tab_layout: arg != "page",
            promo_dismissed: false,
            detail,
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

    /// Server Shop Welcome Header (module 43136: `J4cw1q`, `LvXy/H`, `XyqKh8`, `NY/FIW`,
    /// `rtgp7q`, `NZeik9`, `7JwrlH`).
    pub(super) fn server_shop_welcome_header(
        &self,
        modal: &ServerShopModal,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let username = self
            .stores
            .current_user
            .as_ref()
            .map(|u| {
                u.global_name
                    .as_deref()
                    .filter(|s| !s.is_empty())
                    .unwrap_or(&u.username)
                    .to_owned()
            })
            .unwrap_or_else(|| "friend".to_owned());
        let title = if modal.is_creator_preview {
            dc_intl::t("rtgp7q").to_owned()
        } else {
            dc_intl::format(
                "NZeik9",
                &[(
                    "guildName",
                    dc_intl::Arg::Str(std::borrow::Cow::Borrowed(&modal.guild_name)),
                )],
            )
        };
        let greeting = dc_intl::format(
            "7JwrlH",
            &[(
                "username",
                dc_intl::Arg::Str(std::borrow::Cow::Borrowed(&username)),
            )],
        );
        let desc = if modal.is_creator_preview {
            match modal.tab {
                ServerShopTab::Products => dc_intl::t("LvXy/H"),
                ServerShopTab::Subscriptions => dc_intl::t("XyqKh8"),
            }
        } else {
            dc_intl::t("NY/FIW")
        };

        div()
            .id("server-shop-welcome-header")
            .w_full()
            .rounded(px(12.))
            .border_1()
            .border_color(color("--border-subtle"))
            .bg(color("--background-base-lower"))
            .p(px(12.))
            .flex()
            .items_center()
            .justify_between()
            .gap(px(20.))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .items_start()
                    .gap(px(14.))
                    .child(
                        div()
                            .flex_none()
                            .size(px(52.))
                            .rounded(px(14.))
                            .bg(rgba(0x5865f233))
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(icon("storefront_store", 26., "--white")),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap(px(4.))
                            .child(
                                div()
                                    .text_size(px(18.))
                                    .line_height(px(22.))
                                    .font_weight(FontWeight(700.))
                                    .text_color(color("--text-strong"))
                                    .truncate()
                                    .child(title),
                            )
                            .child(
                                div()
                                    .text_size(px(13.))
                                    .font_weight(FontWeight(600.))
                                    .text_color(color("--text-link"))
                                    .child(greeting),
                            )
                            .child(
                                div()
                                    .text_size(px(13.))
                                    .line_height(px(18.))
                                    .text_color(color("--text-default"))
                                    .child(desc),
                            )
                            .child(
                                div()
                                    .id("server-shop-show-more")
                                    .mt(px(2.))
                                    .cursor_pointer()
                                    .flex()
                                    .items_center()
                                    .gap(px(4.))
                                    .text_size(px(12.))
                                    .font_weight(FontWeight(600.))
                                    .text_color(color("--text-strong"))
                                    .child(dc_intl::t("J4cw1q"))
                                    .child(icon("shop_arrow_right", 12., "--text-strong"))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.put_app_toast(
                                            dc_intl::t("NY/FIW").to_owned(),
                                            true,
                                            cx,
                                        );
                                    })),
                            ),
                    ),
            )
            .child(
                div()
                    .flex_none()
                    .w(px(180.))
                    .h(px(88.))
                    .rounded(px(10.))
                    .bg(gpui::linear_gradient(
                        135.,
                        gpui::linear_color_stop(rgba(0x5865f255), 0.),
                        gpui::linear_color_stop(rgba(0xeb459e44), 1.),
                    ))
                    .border_1()
                    .border_color(color("--border-subtle"))
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(icon("storefront_badge", 34., "--white")),
            )
            .into_any_element()
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
