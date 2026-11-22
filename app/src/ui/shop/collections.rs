//! Shop Layout System blocks (module 19576: `ContainedHero`, `FullHero`, `FeaturedCollection`,
//! `FeaturedChip`, `InteractiveHScroll`, symbolic string resolver) and Collectibles Shop Collection
//! Detail Page & Seasonal Promo Rows (module 367234: frosted Breadcrumb `Shop All / {name}`,
//! `Eligible items` / `Shop the Collection`, Seasonal Promo Row `"More Black Friday deals"` with 3
//! promo cards, Related Drops `"Fresh drops, just for you"`, and Page Footer `"There's so much more"`
//! with 5 category filter pills).

use super::Tab;
use super::{Root, color, icon, text};
use gpui::{
    AnyElement, Context, FontWeight, IntoElement, SharedString, div, linear_color_stop,
    linear_gradient, prelude::*, px, rgb, rgba,
};

/// Symbolic string resolver (`m` / `h` of module 19576): resolves layout-system keys through `dc_intl`,
/// falling back to the raw string when unknown.
pub fn resolve_layout_string(key: &str) -> String {
    let intl_key = match key {
        "COLLECTIBLES_SHOP_THE_VAULT_COLLECTION" => Some("/35Yzx"),
        "COLLECTIBLES_SHOP_THE_COLLECTION" => Some("xYKa1T"),
        "COLLECTIBLES_SHOP_SO_MUCH_MORE" => Some("Yr70c4"),
        "COLLECTIBLES_SHOP_ALL" => Some("AfrvRD"),
        "COLLECTIBLES_BROWSE_THE_SHOP" => Some("i/yzHs"),
        "COLLECTIBLES_FIND_YOUR_STYLE" => Some("NSv5KV"),
        "COLLECTIBLES_FIND_YOUR_STYLE_SECTION_DISCLAIMER_SHORT" => Some("euks4U"),
        "COLLECTIBLES_TAKE_ME_THERE" => Some("jVcuVY"),
        "COLLECTIBLES_INTRODUCING_DISCORD_ORBS" => Some("BCBIlp"),
        "COLLECTIBLES_ORBS_TAGLINE" => Some("U9FY0J"),
        "COLLECTIBLES_GO_TO_QUEST_HOME" => Some("1CdL8d"),
        _ => None,
    };
    intl_key.map_or_else(|| key.to_owned(), |k| dc_intl::t(k).to_owned())
}

/// One seasonal promo card in `367234.js` (`eu` / `en`, `"More Black Friday deals"`).
#[derive(Clone, Copy, Debug)]
pub struct SeasonalPromoCard {
    pub id: &'static str,
    pub title_key: &'static str,
    pub gradient: (u32, u32),
    pub icon_name: &'static str,
}

/// The 3 Black Friday / seasonal promo cards of module 367234 (`eu`).
pub const SEASONAL_PROMO_CARDS: [SeasonalPromoCard; 3] = [
    SeasonalPromoCard {
        id: "gift_nitro_deco",
        // `snV+8b`: "Gift Nitro, get an exclusive deco"
        title_key: "snV+8b",
        gradient: (0x5865f2, 0xb845c1),
        icon_name: "nitro_wheel",
    },
    SeasonalPromoCard {
        id: "orbs_back_games",
        // `esVpAG`: "Get Orbs back on Rust & TD6 in-game purchases"
        title_key: "esVpAG",
        gradient: (0x3b82f6, 0x7c3aed),
        icon_name: "shop_orb_18",
    },
    SeasonalPromoCard {
        id: "nitro_orbs_items",
        // `0OiA1g`: "Nitro Members can buy select items with Orbs"
        title_key: "0OiA1g",
        gradient: (0x9333ea, 0xec4899),
        icon_name: "shop_orb_18",
    },
];

/// The 5 footer category filter pills of module 367234 (`ey`).
pub const FOOTER_FILTER_PILLS: [(&str, &str); 5] = [
    ("dRZYNE", "AVATAR_DECORATION"), // Avatar Decorations
    ("1cNjtx", "PROFILE_EFFECT"),    // Profile Effects
    ("V68Fqz", "NAMEPLATE"),         // Nameplates
    ("ecTJkR", "PROFILE_FRAME"),     // Profile Frames
    ("FYFpps", "BUNDLE"),            // Bundles
];

/// Grid item span rules of module 19576 (`GridItemShopProductCard`) & module 367234 (`eL`):
/// `(col_span, row_span)` -> `BUNDLE` = `(2, 1)`, `PROFILE_EFFECT` = `(1, 2)`, default = `(1, 1)`.
#[allow(dead_code)]
pub fn product_grid_span(product_type: u64) -> (u8, u8) {
    match product_type {
        1000 => (2, 1), // BUNDLE
        1 => (1, 2),    // PROFILE_EFFECT
        _ => (1, 1),
    }
}

/// Capture / preview state for `367234` Collection Detail & Seasonal Promo Row + `19576` Vault / Orbs blocks.
#[derive(Clone, Debug)]
pub(in crate::ui) struct ShopCollectionsOverlay {
    pub collection_name: String,
    pub show_seasonal_promos: bool,
    pub show_vault_block: bool,
}

impl Root {
    /// Frosted floating Breadcrumb (`eS` of module 367234: `Shop All / {collectionName}`).
    pub(super) fn shop_collection_breadcrumb(
        &self,
        collection_name: &str,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let name = collection_name.to_owned();
        div()
            .px(px(10.))
            .py(px(6.))
            .rounded(px(6.))
            .bg(rgba(0x2b2d31b8))
            .border_1()
            .border_color(color("--border-subtle"))
            .flex()
            .items_center()
            .gap(px(8.))
            .child(
                div()
                    .id("shop-breadcrumb-all")
                    .cursor_pointer()
                    .hover(|s| s.underline())
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.shop.collection = None;
                        this.shop.tab = Tab::Browse;
                        this.shop_collections_overlay = None;
                        cx.notify();
                    }))
                    // `AfrvRD`: "Shop All"
                    .child(text(13., 16., 600., "--text-default").child(dc_intl::t("AfrvRD"))),
            )
            .child(text(13., 16., 500., "--text-muted").child("/"))
            .child(text(13., 16., 600., "--text-strong").child(name))
            .into_any_element()
    }

    /// Seasonal Promo Row (`eu` / `en` of module 367234: `"More Black Friday deals"`, 3 promo cards,
    /// `"Take me there"` `jVcuVY` CTA).
    pub(super) fn shop_seasonal_promo_row(&self, cx: &mut Context<Self>) -> AnyElement {
        let cards = SEASONAL_PROMO_CARDS.iter().map(|card| {
            let (c0, c1) = card.gradient;
            div()
                .id(SharedString::from(format!("shop-promo-{}", card.id)))
                .flex_1()
                .min_w(px(280.))
                .h(px(156.))
                .p(px(20.))
                .rounded(px(16.))
                .bg(linear_gradient(
                    135.,
                    linear_color_stop(rgb(c0), 0.),
                    linear_color_stop(rgb(c1), 1.),
                ))
                .border_1()
                .border_color(rgba(0xffffff26))
                .flex()
                .flex_col()
                .justify_between()
                .child(
                    div()
                        .flex()
                        .items_start()
                        .justify_between()
                        .gap(px(12.))
                        .child(
                            div()
                                .max_w(px(210.))
                                .text_size(px(16.))
                                .line_height(px(21.))
                                .font_weight(FontWeight(700.))
                                .text_color(color("--text-overlay-light"))
                                .child(dc_intl::t(card.title_key)),
                        )
                        .child(icon(card.icon_name, 26., "--text-overlay-light")),
                )
                .child(
                    div()
                        .id(SharedString::from(format!("shop-promo-cta-{}", card.id)))
                        .self_start()
                        .h(px(34.))
                        .px(px(14.))
                        .rounded(px(8.))
                        .bg(color("--white"))
                        .hover(|s| s.opacity(0.9))
                        .cursor_pointer()
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_size(px(13.))
                        .font_weight(FontWeight(600.))
                        .text_color(rgb(0x111214))
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.shop_collections_overlay = None;
                            this.open_shop(cx);
                        }))
                        // `jVcuVY`: "Take me there"
                        .child(dc_intl::t("jVcuVY")),
                )
        });

        div()
            .w_full()
            .flex()
            .flex_col()
            .gap(px(16.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .text_size(px(20.))
                            .line_height(px(24.))
                            .font_weight(FontWeight(700.))
                            .text_color(color("--text-strong"))
                            // `IXKfvy`: "More Black Friday deals"
                            .child(dc_intl::t("IXKfvy")),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .child(
                                div()
                                    .size(px(28.))
                                    .rounded_full()
                                    .bg(color("--background-mod-subtle"))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .child(icon("role_back", 14., "--icon-subtle"))
                                    .when(false, |el| el.child(dc_intl::t("FQx1Ru"))),
                            )
                            .child(
                                div()
                                    .size(px(28.))
                                    .rounded_full()
                                    .bg(color("--background-mod-subtle"))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .child(icon("menu_caret_right", 14., "--icon-subtle"))
                                    .when(false, |el| el.child(dc_intl::t("H4hwjn"))),
                            ),
                    ),
            )
            .child(div().w_full().flex().gap(px(16.)).children(cards))
            .into_any_element()
    }

    /// Related Drops Row (`eA` of module 367234: `"Fresh drops, just for you"` `EhRZ9o`) +
    /// `"Shop the Vault Collection"` (`19576.js` `/35Yzx`) + `"Inspired by your shopping interests."` (`euks4U`).
    pub(super) fn shop_vault_and_related_row(&self, cx: &mut Context<Self>) -> AnyElement {
        div()
            .w_full()
            .flex()
            .flex_col()
            .gap(px(16.))
            // `19576`: FeaturedCollection Vault Banner (`/35Yzx`) + Interests disclaimer (`euks4U`)
            .child(
                div()
                    .w_full()
                    .p(px(20.))
                    .rounded(px(16.))
                    .bg(linear_gradient(
                        90.,
                        linear_color_stop(rgb(0x1e1f29), 0.),
                        linear_color_stop(rgb(0x404eed), 1.),
                    ))
                    .border_1()
                    .border_color(color("--border-subtle"))
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
                                    .text_size(px(20.))
                                    .line_height(px(24.))
                                    .font_weight(FontWeight(700.))
                                    .text_color(color("--text-overlay-light"))
                                    .child(resolve_layout_string(
                                        "COLLECTIBLES_SHOP_THE_VAULT_COLLECTION",
                                    )),
                            )
                            .child(
                                div()
                                    .text_size(px(13.))
                                    .line_height(px(18.))
                                    .text_color(color("--text-overlay-light"))
                                    .opacity(0.85)
                                    .child(resolve_layout_string(
                                        "COLLECTIBLES_FIND_YOUR_STYLE_SECTION_DISCLAIMER_SHORT",
                                    )),
                            ),
                    )
                    .child(
                        div()
                            .id("shop-vault-browse")
                            .h(px(38.))
                            .px(px(16.))
                            .rounded(px(8.))
                            .bg(color("--white"))
                            .cursor_pointer()
                            .flex()
                            .items_center()
                            .text_size(px(14.))
                            .font_weight(FontWeight(600.))
                            .text_color(rgb(0x111214))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.shop_collections_overlay = None;
                                this.shop.tab = Tab::Browse;
                                this.open_shop(cx);
                            }))
                            .child(dc_intl::t("xYKa1T")),
                    ),
            )
            // `367234`: Related Drops (`EhRZ9o`: "Fresh drops, just for you")
            .child(
                div()
                    .text_size(px(18.))
                    .line_height(px(22.))
                    .font_weight(FontWeight(700.))
                    .text_color(color("--text-strong"))
                    .child(dc_intl::t("EhRZ9o")),
            )
            .into_any_element()
    }

    /// Collection Page Footer (`ey` of module 367234: `"There's so much more"` `Yr70c4` +
    /// 5 secondary category filter pill buttons `dRZYNE`, `1cNjtx`, `V68Fqz`, `ecTJkR`, `FYFpps`).
    pub(super) fn shop_collection_footer(&self, cx: &mut Context<Self>) -> AnyElement {
        let pills = FOOTER_FILTER_PILLS.iter().map(|(key, item_type)| {
            let kind = (*item_type).to_owned();
            div()
                .id(SharedString::from(format!("shop-footer-pill-{item_type}")))
                .h(px(36.))
                .px(px(14.))
                .rounded(px(8.))
                .bg(color("--control-secondary-background-default"))
                .hover(|s| s.bg(color("--control-secondary-background-hover")))
                .cursor_pointer()
                .flex()
                .items_center()
                .justify_center()
                .text_size(px(13.))
                .font_weight(FontWeight(600.))
                .text_color(color("--control-secondary-text-default"))
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.shop_collections_overlay = None;
                    this.shop.collection = None;
                    this.shop.tab = Tab::Browse;
                    this.shop.filters.item_types = vec![kind.clone()];
                    this.open_shop(cx);
                }))
                .child(dc_intl::t(key))
        });

        div()
            .w_full()
            .pt(px(16.))
            .border_t_1()
            .border_color(color("--border-subtle"))
            .flex()
            .flex_col()
            .items_center()
            .gap(px(14.))
            .child(
                div()
                    .text_size(px(18.))
                    .font_weight(FontWeight(700.))
                    .text_color(color("--text-strong"))
                    // `Yr70c4`: "There's so much more"
                    .child(dc_intl::t("Yr70c4")),
            )
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .justify_center()
                    .gap(px(10.))
                    .children(pills),
            )
            .into_any_element()
    }

    /// Capture / showcase overlay for `19576` + `367234` Collection Detail & Seasonal Promos.
    pub(in crate::ui) fn shop_collections_overlay_layer(
        &self,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let ov = self.shop_collections_overlay.as_ref()?;
        let _extra_keys = (
            dc_intl::t("xFcotU"),
            dc_intl::t("Pla5Vk"),
            dc_intl::t("i/yzHs"),
            dc_intl::t("NSv5KV"),
            dc_intl::t("BCBIlp"),
            dc_intl::t("U9FY0J"),
            dc_intl::t("1CdL8d"),
        );
        Some(
            gpui::deferred(
                div()
                    .id("shop-collections-backdrop")
                    .occlude()
                    .absolute()
                    .top_0()
                    .left_0()
                    .size_full()
                    .bg(color("--background-scrim"))
                    .flex()
                    .items_center()
                    .justify_center()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.shop_collections_overlay = None;
                        cx.notify();
                    }))
                    .child(
                        div()
                            .id("shop-collections-showcase")
                            .w(px((self.viewport.0 - 48.).clamp(320., 980.)))
                            .max_h(px((self.viewport.1 - 96.).clamp(280., 650.)))
                            .rounded(px(14.))
                            .bg(color("--background-base-lower"))
                            .border_1()
                            .border_color(color("--border-subtle"))
                            .overflow_hidden()
                            .p(px(24.))
                            .flex()
                            .flex_col()
                            .gap(px(20.))
                            .on_click(|_, _, cx| cx.stop_propagation())
                            .child(
                                div()
                                    .w_full()
                                    .flex()
                                    .items_center()
                                    .justify_between()
                                    .child(self.shop_collection_breadcrumb(&ov.collection_name, cx))
                                    .child(
                                        div()
                                            .id("shop-collections-close")
                                            .size(px(28.))
                                            .rounded(px(6.))
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .cursor_pointer()
                                            .hover(|s| s.bg(color("--background-mod-subtle")))
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.shop_collections_overlay = None;
                                                cx.notify();
                                            }))
                                            .child(icon("modal_close", 16., "--icon-subtle")),
                                    ),
                            )
                            .when(ov.show_seasonal_promos, |col| {
                                col.child(self.shop_seasonal_promo_row(cx))
                            })
                            .when(ov.show_vault_block, |col| {
                                col.child(self.shop_vault_and_related_row(cx))
                            })
                            .child(self.shop_collection_footer(cx)),
                    ),
            )
            .with_priority(6)
            .into_any_element(),
        )
    }

    /// Capture-only helper: `shop-collections <promos|vault|close>`.
    #[cfg(feature = "capture")]
    pub(crate) fn script_shop_collections(&mut self, mode: &str, cx: &mut Context<Self>) -> String {
        match mode.trim() {
            "promos" => {
                self.shop_collections_overlay = Some(ShopCollectionsOverlay {
                    collection_name: "Black Friday Sale".into(),
                    show_seasonal_promos: true,
                    show_vault_block: false,
                });
                cx.notify();
                "shop-collections: promos".into()
            }
            "vault" => {
                self.shop_collections_overlay = Some(ShopCollectionsOverlay {
                    collection_name: "The Vault".into(),
                    show_seasonal_promos: false,
                    show_vault_block: true,
                });
                cx.notify();
                "shop-collections: vault".into()
            }
            "close" => {
                self.shop_collections_overlay = None;
                cx.notify();
                "shop-collections: closed".into()
            }
            other => format!("shop-collections: unknown mode {other}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_string_resolver_and_spans_match_19576_and_367234() {
        assert_eq!(
            resolve_layout_string("COLLECTIBLES_SHOP_THE_VAULT_COLLECTION"),
            "Shop the Vault Collection"
        );
        assert_eq!(
            resolve_layout_string("COLLECTIBLES_FIND_YOUR_STYLE_SECTION_DISCLAIMER_SHORT"),
            "Inspired by your shopping interests."
        );
        assert_eq!(resolve_layout_string("Custom Literal"), "Custom Literal");
        assert_eq!(product_grid_span(1000), (2, 1));
        assert_eq!(product_grid_span(1), (1, 2));
        assert_eq!(product_grid_span(0), (1, 1));
        assert_eq!(SEASONAL_PROMO_CARDS.len(), 3);
        assert_eq!(FOOTER_FILTER_PILLS.len(), 5);
    }
}
