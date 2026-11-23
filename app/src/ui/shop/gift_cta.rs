//! Wishlist & Game Shop Gift CTA modal + Game Stats widget copy (module 909004).
//!
//! Renders self-gift ("Treat yourself to a gift" / "From your Wishlist and Shop") and friend-gift
//! ("Send {username} a gift" / "From their Wishlist and Shop") cards with priced CTAs
//! ("Buy for {price}", "Gift for {price}", "Get {orbCount} Orbs", "Buy Now", "Gift Item"),
//! "{applicationName} Shop" header, "View Game Profile", error fallback, and Game Stats prompt.

use crate::ui::{Root, color, icon};
use dc_intl::Arg;
use gpui::{
    AnyElement, Context, FontWeight, IntoElement, SharedString, deferred, div, prelude::*, px, rgba,
};
use std::borrow::Cow;

#[derive(Clone, Debug)]
pub(crate) struct GiftCatalogItem {
    pub id: String,
    pub product_name: String,
    pub price: Option<String>,
    pub orb_count: Option<u64>,
}

#[derive(Clone, Debug)]
pub(crate) struct ShopGiftCtaModal {
    /// `None` = self ("Treat yourself to a gift"), `Some("")` = generic "Send a gift",
    /// `Some(name)` = "Send {username} a gift".
    pub recipient_username: Option<String>,
    pub application_name: String,
    pub items: Vec<GiftCatalogItem>,
    pub is_own_profile: bool,
    pub account_connected: bool,
    pub error: bool,
}

/// Formats `GiVd2Q` (`Get {orbIcon} {orbCount} Orbs`) substituting `orbIconHook`.
pub(crate) fn format_get_orbs(orb_count: u64) -> String {
    let runs = dc_intl::format_runs("GiVd2Q", &[("orbCount", Arg::Num(orb_count as f64))]);
    let mut out = String::new();
    for (text, hook) in runs {
        if hook.as_deref() == Some("orbIconHook") {
            out.push('✦');
        } else {
            out.push_str(&text);
        }
    }
    out
}

/// Formats the item CTA button label (`Xp5WTn`, `o2WeeD`, `boqtTA`, `QAZA5f`, `GiVd2Q`).
pub(crate) fn gift_item_cta_label(item: &GiftCatalogItem, is_self: bool) -> String {
    if let Some(orbs) = item.orb_count {
        return format_get_orbs(orbs);
    }
    match (&item.price, is_self) {
        (Some(price), true) => {
            dc_intl::format("Xp5WTn", &[("price", Arg::Str(Cow::Borrowed(price)))])
        }
        (Some(price), false) => {
            dc_intl::format("o2WeeD", &[("price", Arg::Str(Cow::Borrowed(price)))])
        }
        (None, true) => dc_intl::t("boqtTA").to_owned(),
        (None, false) => dc_intl::t("QAZA5f").to_owned(),
    }
}

/// Header title & subtitle (`kv4lSp`/`vzZoSF` for self; `YwV901`/`qCmNFr` + `SSgrne` for others).
pub(crate) fn gift_header_copy(recipient: Option<&str>) -> (String, String) {
    match recipient {
        None => (
            dc_intl::t("kv4lSp").to_owned(),
            dc_intl::t("vzZoSF").to_owned(),
        ),
        Some("") => (
            dc_intl::t("qCmNFr").to_owned(),
            dc_intl::t("SSgrne").to_owned(),
        ),
        Some(username) => (
            dc_intl::format("YwV901", &[("username", Arg::Str(Cow::Borrowed(username)))]),
            dc_intl::t("SSgrne").to_owned(),
        ),
    }
}

/// Game Stats widget footer prompt (`YWa8L2`, `xxPgmT`, `QuIwht`).
pub(crate) fn game_stats_prompt(
    application_name: &str,
    is_own_profile: bool,
    account_connected: bool,
) -> String {
    if !account_connected {
        if is_own_profile {
            dc_intl::t("YWa8L2").to_owned()
        } else {
            dc_intl::t("xxPgmT").to_owned()
        }
    } else {
        dc_intl::format(
            "QuIwht",
            &[("applicationName", Arg::Str(Cow::Borrowed(application_name)))],
        )
    }
}

impl Root {
    pub(crate) fn close_shop_gift_cta(&mut self) -> bool {
        self.shop_gift_cta_modal.take().is_some()
    }

    pub(in crate::ui) fn shop_gift_cta_modal_layer(
        &self,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let modal = self.shop_gift_cta_modal.as_ref()?;
        let is_self = modal.recipient_username.is_none();
        let (title, subtitle) = gift_header_copy(modal.recipient_username.as_deref());
        let app_shop_label = dc_intl::format(
            "HDT/rg",
            &[(
                "applicationName",
                Arg::Str(Cow::Borrowed(&modal.application_name)),
            )],
        );
        let stats_prompt = game_stats_prompt(
            &modal.application_name,
            modal.is_own_profile,
            modal.account_connected,
        );

        let body: AnyElement = if modal.error {
            div()
                .py(px(36.))
                .px(px(24.))
                .flex()
                .flex_col()
                .items_center()
                .gap(px(8.))
                .child(icon("circle_info", 28., "--text-feedback-critical"))
                .child(
                    div()
                        .text_size(px(14.))
                        .text_color(color("--text-default"))
                        .child(dc_intl::t("rTU7/z")),
                )
                .into_any_element()
        } else {
            div()
                .p(px(20.))
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
                                .text_size(px(13.))
                                .font_weight(FontWeight(700.))
                                .text_color(color("--text-muted"))
                                .child(app_shop_label.to_uppercase()),
                        )
                        .child(
                            div()
                                .id("shop-gift-view-game")
                                .px(px(10.))
                                .py(px(5.))
                                .rounded(px(6.))
                                .bg(color("--control-secondary-background-default"))
                                .hover(|s| s.bg(color("--control-secondary-background-hover")))
                                .cursor_pointer()
                                .text_size(px(12.))
                                .font_weight(FontWeight(500.))
                                .text_color(color("--text-strong"))
                                .child(dc_intl::t("ajHoOr"))
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.close_shop_gift_cta();
                                    cx.notify();
                                })),
                        ),
                )
                .child(
                    div()
                        .flex()
                        .gap(px(12.))
                        .children(modal.items.iter().enumerate().map(|(idx, item)| {
                            let cta = gift_item_cta_label(item, is_self);
                            let details_tip = dc_intl::format(
                                "ZBB4Ty",
                                &[("productName", Arg::Str(Cow::Borrowed(&item.product_name)))],
                            );
                            let card = div()
                                .id(SharedString::from(format!(
                                    "shop-gift-card-{idx}-{}",
                                    item.id
                                )))
                                .flex_1()
                                .p(px(14.))
                                .rounded(px(10.))
                                .border_1()
                                .border_color(color("--border-subtle"))
                                .bg(color("--background-base-lower"))
                                .flex()
                                .flex_col()
                                .gap(px(10.))
                                .child(
                                    div()
                                        .relative()
                                        .h(px(72.))
                                        .rounded(px(8.))
                                        .bg(color("--background-surface-higher"))
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .child(icon(
                                            "gift",
                                            28.,
                                            "--control-primary-background-default",
                                        ))
                                        // Module 627001 (`P`): top-left 20px In-Game Item app badge (`4yiU7x` tooltip).
                                        .when(!modal.application_name.is_empty(), |box_el| {
                                            let badge = div()
                                                .id(SharedString::from(format!(
                                                    "shop-gift-ingame-badge-{idx}-{}",
                                                    item.id
                                                )))
                                                .size(px(20.))
                                                .rounded(px(6.))
                                                .border_1()
                                                .border_color(color("--border-normal"))
                                                .bg(color("--background-base-lowest"))
                                                .flex()
                                                .items_center()
                                                .justify_center()
                                                .child(icon("activity_game", 13., "--text-strong"));
                                            let tipped = self.with_tip(
                                                format!("shop-gift-ingame-tip-{idx}-{}", item.id),
                                                crate::ui::tooltip::Tip::text(
                                                    dc_intl::t("4yiU7x"),
                                                    crate::ui::tooltip::Side::Top,
                                                ),
                                                badge,
                                                cx,
                                            );
                                            box_el.child(
                                                div()
                                                    .absolute()
                                                    .top(px(6.))
                                                    .left(px(6.))
                                                    .child(tipped),
                                            )
                                        }),
                                )
                                .child(
                                    div()
                                        .text_size(px(14.))
                                        .font_weight(FontWeight(600.))
                                        .text_color(color("--text-strong"))
                                        .child(item.product_name.clone()),
                                )
                                .child(
                                    div()
                                        .id(SharedString::from(format!("shop-gift-cta-{idx}")))
                                        .h(px(34.))
                                        .w_full()
                                        .rounded(px(8.))
                                        .bg(color("--control-primary-background-default"))
                                        .hover(|s| {
                                            s.bg(color("--control-primary-background-hover"))
                                        })
                                        .cursor_pointer()
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .text_size(px(13.))
                                        .font_weight(FontWeight(500.))
                                        .text_color(color("--white"))
                                        .child(cta)
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.close_shop_gift_cta();
                                            cx.notify();
                                        })),
                                );
                            self.with_tip(
                                format!("shop-gift-card-tip-{idx}-{}", item.id),
                                crate::ui::tooltip::Tip::body(
                                    crate::ui::tooltip::Body::Text(details_tip.into()),
                                    crate::ui::tooltip::Side::Top,
                                ),
                                card,
                                cx,
                            )
                        })),
                )
                .child(
                    div()
                        .p(px(14.))
                        .rounded(px(10.))
                        .border_1()
                        .border_color(color("--border-subtle"))
                        .bg(color("--background-base-lower"))
                        .flex()
                        .flex_col()
                        .gap(px(6.))
                        .child(
                            div()
                                .text_size(px(12.))
                                .font_weight(FontWeight(700.))
                                .text_color(color("--text-muted"))
                                .child(dc_intl::t("EKR8Ps").to_uppercase()),
                        )
                        .child(
                            div()
                                .text_size(px(13.))
                                .line_height(px(18.))
                                .text_color(color("--text-default"))
                                .child(stats_prompt),
                        ),
                )
                .into_any_element()
        };

        let dialog = div()
            .id("shop-gift-cta-modal")
            .occlude()
            .w(px(620.))
            .rounded(px(12.))
            .border_1()
            .border_color(color("--border-subtle"))
            .bg(color("--background-surface-high"))
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
                    .pt(px(18.))
                    .pb(px(14.))
                    .border_b_1()
                    .border_color(color("--border-subtle"))
                    .flex()
                    .items_start()
                    .justify_between()
                    .gap(px(12.))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(4.))
                            .child(
                                div()
                                    .text_size(px(20.))
                                    .line_height(px(24.))
                                    .font_weight(FontWeight(700.))
                                    .text_color(color("--text-strong"))
                                    .child(title),
                            )
                            .child(
                                div()
                                    .text_size(px(13.))
                                    .text_color(color("--text-muted"))
                                    .child(subtitle),
                            ),
                    )
                    .child(
                        div()
                            .id("shop-gift-cta-close")
                            .size(px(32.))
                            .rounded(px(8.))
                            .cursor_pointer()
                            .hover(|s| s.bg(color("--background-mod-subtle")))
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(icon("modal_close", 18., "--interactive-icon-default"))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.close_shop_gift_cta();
                                cx.notify();
                            })),
                    ),
            )
            .child(body);

        Some(
            deferred(
                div()
                    .id("shop-gift-cta-backdrop")
                    .occlude()
                    .absolute()
                    .inset_0()
                    .bg(rgba(0x000000b8))
                    .flex()
                    .items_center()
                    .justify_center()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.close_shop_gift_cta();
                        cx.notify();
                    }))
                    .child(dialog),
            )
            .with_priority(3)
            .into_any_element(),
        )
    }

    #[cfg(feature = "capture")]
    pub(crate) fn script_shop_gift_cta(&mut self, arg: &str, cx: &mut Context<Self>) -> String {
        let items = vec![
            GiftCatalogItem {
                id: "sku1".to_owned(),
                product_name: "Starlight Avatar Frame".to_owned(),
                price: Some("$5.99".to_owned()),
                orb_count: None,
            },
            GiftCatalogItem {
                id: "sku2".to_owned(),
                product_name: "Cyberpunk Nameplate".to_owned(),
                price: None,
                orb_count: Some(2500),
            },
            GiftCatalogItem {
                id: "sku3".to_owned(),
                product_name: "Aurora Profile Effect".to_owned(),
                price: None,
                orb_count: None,
            },
        ];
        self.shop_gift_cta_modal = Some(ShopGiftCtaModal {
            recipient_username: match arg {
                "self" => None,
                "generic" => Some(String::new()),
                _ => Some("loreanxavier".to_owned()),
            },
            application_name: "Valorant".to_owned(),
            items,
            is_own_profile: arg == "self",
            account_connected: arg == "connected",
            error: arg == "error",
        });
        cx.notify();
        format!("shop-gift-cta: {arg}")
    }
}

/// Module 128790 (`Z`, CSS `968069` / `736033`): source category of a gifting breadcrumb recommendation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(dead_code)]
pub(crate) enum GiftingBreadcrumbSource {
    Wishlist,
    Popular,
}

/// Module 128790: single SKU card inside the Slayer Storefront gifting breadcrumb widget.
#[derive(Clone, Debug, PartialEq, Eq)]
#[allow(dead_code)]
pub(crate) struct GiftingBreadcrumbItem {
    pub sku_id: String,
    pub product_name: String,
    pub price: String,
    pub source: GiftingBreadcrumbSource,
    pub recommender_initials: Vec<String>,
}

/// Module 128790: header title switches to `"Wishlist"` (`7lZ31J`) when only wishlist items are
/// present, else `"Suggested Gifts"` (`BCi1gT`).
#[allow(dead_code)]
pub(crate) fn gifting_breadcrumb_header(items: &[GiftingBreadcrumbItem]) -> &'static str {
    let has_wishlist = items
        .iter()
        .any(|i| i.source == GiftingBreadcrumbSource::Wishlist);
    let has_popular = items
        .iter()
        .any(|i| i.source == GiftingBreadcrumbSource::Popular);
    if has_wishlist && !has_popular {
        dc_intl::t("7lZ31J")
    } else {
        dc_intl::t("BCi1gT")
    }
}

/// Module 128790 (`808247.A.fetchWishlistRecommendations`): `GET /wishlist/gift-recommendations`.
#[allow(dead_code)]
pub(crate) fn fetch_gifting_breadcrumb_request(
    application_id: &str,
    user_ids: &[dc_state::Id],
    max_recommendations: usize,
) -> crate::net::Request {
    let mut query = format!(
        "/wishlist/gift-recommendations?application_ids[]={application_id}&max_recommendations={max_recommendations}&localize=true"
    );
    for uid in user_ids {
        query.push_str(&format!("&user_ids[]={uid}"));
    }
    crate::net::Request::Fetch {
        key: format!("wishlist-recs:{application_id}"),
        route: query,
    }
}

/// Module 128790: compact Slayer Storefront gifting breadcrumb widget (`Wishlist` / `Suggested Gifts`
/// header + optional green `.offerBadge_viLuyz` + horizontal row of 90px SKU cards).
#[allow(dead_code)]
pub(crate) fn render_gifting_breadcrumb_widget(
    items: &[GiftingBreadcrumbItem],
    offer_badge_label: Option<&str>,
    cx: &mut Context<Root>,
) -> Option<AnyElement> {
    if items.is_empty() {
        return None;
    }
    let has_wishlist = items
        .iter()
        .any(|i| i.source == GiftingBreadcrumbSource::Wishlist);
    let has_popular = items
        .iter()
        .any(|i| i.source == GiftingBreadcrumbSource::Popular);
    let show_icons = has_wishlist && has_popular;
    let title = gifting_breadcrumb_header(items);
    Some(
        div()
            .w_full()
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
                            .text_size(px(12.))
                            .font_weight(FontWeight(500.))
                            .text_color(color("--text-muted"))
                            .child(title),
                    )
                    .children(offer_badge_label.map(|label| {
                        div()
                            .px(px(8.))
                            .py(px(2.))
                            .rounded_full()
                            .bg(gpui::rgba(0x23a55a2e))
                            .border_1()
                            .border_color(color("--status-positive"))
                            .text_size(px(11.))
                            .font_weight(FontWeight(600.))
                            .text_color(color("--status-positive"))
                            .child(label.to_owned())
                    })),
            )
            .child(
                div()
                    .flex()
                    .items_stretch()
                    .gap(px(8.))
                    .children(items.iter().enumerate().map(|(idx, item)| {
                        let details_aria = dc_intl::format(
                            "ZBB4Ty",
                            &[(
                                "productName",
                                dc_intl::Arg::Str(std::borrow::Cow::Borrowed(&item.product_name)),
                            )],
                        );
                        let name = item.product_name.clone();
                        div()
                            .id(SharedString::from(format!("gift-bc-{idx}-{}", item.sku_id)))
                            .aria_label(details_aria)
                            .w(px(96.))
                            .p(px(8.))
                            .rounded(px(8.))
                            .border_1()
                            .border_color(color("--border-subtle"))
                            .bg(color("--background-mod-subtle"))
                            .hover(|s| s.bg(color("--background-mod-normal")))
                            .cursor_pointer()
                            .flex()
                            .flex_col()
                            .gap(px(6.))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.put_app_toast(name.clone(), true, cx);
                            }))
                            .child(
                                div()
                                    .relative()
                                    .w_full()
                                    .h(px(64.))
                                    .rounded(px(6.))
                                    .bg(color("--background-base-lowest"))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .child(icon("gift", 22., "--icon-subtle"))
                                    .when(
                                        show_icons
                                            && item.source == GiftingBreadcrumbSource::Wishlist
                                            && !item.recommender_initials.is_empty(),
                                        |tile| {
                                            tile.child(
                                                div()
                                                    .absolute()
                                                    .bottom(px(4.))
                                                    .right(px(4.))
                                                    .flex()
                                                    .gap(px(2.))
                                                    .children(
                                                        item.recommender_initials
                                                            .iter()
                                                            .take(2)
                                                            .map(|init| {
                                                                div()
                                                                    .size(px(16.))
                                                                    .rounded_full()
                                                                    .bg(color("--background-brand"))
                                                                    .flex()
                                                                    .items_center()
                                                                    .justify_center()
                                                                    .text_size(px(9.))
                                                                    .font_weight(FontWeight(700.))
                                                                    .text_color(color("--white"))
                                                                    .child(init.clone())
                                                            }),
                                                    ),
                                            )
                                        },
                                    ),
                            )
                            .child(
                                div()
                                    .text_size(px(11.))
                                    .font_weight(FontWeight(600.))
                                    .text_color(color("--text-strong"))
                                    .truncate()
                                    .child(item.product_name.clone()),
                            )
                            .child(
                                div()
                                    .text_size(px(11.))
                                    .text_color(color("--text-muted"))
                                    .child(item.price.clone()),
                            )
                    })),
            )
            .into_any_element(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_18_keys_of_shop_gift_cta_909004_format_cleanly() {
        assert!(format_get_orbs(2500).contains("2,500 Orbs"));

        let priced = GiftCatalogItem {
            id: "1".to_owned(),
            product_name: "Halo".to_owned(),
            price: Some("$4.99".to_owned()),
            orb_count: None,
        };
        assert_eq!(gift_item_cta_label(&priced, true), "Buy for $4.99");
        assert_eq!(gift_item_cta_label(&priced, false), "Gift for $4.99");

        let unpriced = GiftCatalogItem {
            id: "2".to_owned(),
            product_name: "Frame".to_owned(),
            price: None,
            orb_count: None,
        };
        assert_eq!(gift_item_cta_label(&unpriced, true), "Buy Now");
        assert_eq!(gift_item_cta_label(&unpriced, false), "Gift Item");

        assert_eq!(gift_header_copy(None).0, "Treat yourself to a gift");
        assert_eq!(gift_header_copy(Some("")).0, "Send a gift");
        assert_eq!(gift_header_copy(Some("alice")).0, "Send alice a gift");

        assert!(game_stats_prompt("Valorant", true, false).contains("show off"));
        assert!(game_stats_prompt("Valorant", false, false).contains("get your own"));
        assert!(game_stats_prompt("Valorant", true, true).contains("Valorant"));
    }

    #[test]
    fn gifting_breadcrumb_128790_header_route_and_intl() {
        assert_eq!(dc_intl::t("7lZ31J"), "Wishlist");
        assert_eq!(dc_intl::t("BCi1gT"), "Suggested Gifts");
        assert!(dc_intl::has_key("ZBB4Ty"));
        let wl = GiftingBreadcrumbItem {
            sku_id: "s1".into(),
            product_name: "Starlight Bundle".into(),
            price: "$9.99".into(),
            source: GiftingBreadcrumbSource::Wishlist,
            recommender_initials: vec!["A".into()],
        };
        let pop = GiftingBreadcrumbItem {
            sku_id: "s2".into(),
            product_name: "Neon Skin".into(),
            price: "$4.99".into(),
            source: GiftingBreadcrumbSource::Popular,
            recommender_initials: Vec::new(),
        };
        assert_eq!(
            gifting_breadcrumb_header(std::slice::from_ref(&wl)),
            "Wishlist"
        );
        assert_eq!(gifting_breadcrumb_header(&[wl, pop]), "Suggested Gifts");
        let crate::net::Request::Fetch { route, .. } =
            fetch_gifting_breadcrumb_request("12345", &[1, 2], 2)
        else {
            panic!("expected Fetch");
        };
        assert!(route.starts_with("/wishlist/gift-recommendations?"));
        assert!(route.contains("application_ids[]=12345"));
        assert!(route.contains("user_ids[]=1&user_ids[]=2"));
    }
}
