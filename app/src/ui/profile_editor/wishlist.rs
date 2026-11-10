//! The editor's Wishlist tab: module 927071's wishlist section for the account's own profile. With no hearted items
//! the empty state ("No hearts yet" and the Shop button, `o6`), then the suggestions panel (`oY`): a 4 column grid of
//! 90px product tiles (the Nitro takeover first, then the recommended skus that are not hearted).
//! Measured in `design/dom/settings/profile-editor-wishlist-tab-darker-cozy.json`.

use super::{FETCH_PREFIX, text};
use crate::ui::{Root, color, icon};
use gpui::{Context, Div, IntoElement, Window, div, prelude::*, px, rgba};
use serde_json::Value;

/// `USER_WISHLIST_RECOMMENDATIONS` with the application ids the tab passes: the collectibles shop's own application
/// (`FYj` of module 652215) and this user.
pub(super) fn gifts_route(user: u64) -> String {
    format!(
        "/wishlist/gift-recommendations?application_ids=1096190356233670716&user_ids={user}&max_recommendations=15&localize=true"
    )
}

/// The skus of a wishlist answer, in the order the server listed them.
pub(super) fn wishlist_skus(body: &Value) -> Vec<String> {
    body["wishlist_items"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|item| item["sku"]["id"].as_str().map(str::to_owned))
        .collect()
}

/// The skus of the recommendations answer.
pub(super) fn gift_skus(body: &Value) -> Vec<String> {
    body["skus"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|sku| sku["id"].as_str().map(str::to_owned))
        .collect()
}

/// The grid's skus: the Nitro takeover (an empty sku) first, then the recommendations that are not hearted, at most
/// eight in all.
pub(super) fn suggested(gifts: &[String], hearted: &[String], takeover: bool) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    if takeover {
        out.push(String::new());
    }
    out.extend(gifts.iter().filter(|sku| !hearted.contains(sku)).cloned());
    out.truncate(8);
    out
}

impl Root {
    /// The answers of the wishlist reads: the products to load.
    pub(super) fn profile_editor_wishlist_fetched(&mut self, what: &str) {
        let key = format!("{FETCH_PREFIX}{what}");
        let Some(body) = self.fetched.get(&key).cloned() else {
            return;
        };
        match what {
            "wishlistprofile" => {
                if let Some(id) = crate::ui::shop::data::wishlist_ids(&body)
                    .into_iter()
                    .next()
                {
                    let _ = self.requests.send(crate::net::Request::Fetch {
                        key: format!("{FETCH_PREFIX}wishlist"),
                        route: crate::ui::shop::data::wishlist_route(&id),
                    });
                }
            }
            "wishlist" => self.editor_wishlist_need(&wishlist_skus(&body)),
            "gifts" => self.editor_wishlist_need(&gift_skus(&body)),
            _ => {}
        }
    }

    fn profile_editor_tile(
        &mut self,
        sku: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let art = if sku.is_empty() {
            Some(
                div()
                    .size_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(icon("settings_nitro", 40., "--text-strong"))
                    .into_any_element(),
            )
        } else {
            self.editor_wishlist_preview(sku, 90., window, cx)
        };
        div()
            .relative()
            .size(px(90.))
            .flex_none()
            .rounded(px(8.))
            .overflow_hidden()
            .bg(color("--user-profile-overlay-background"))
            .children(art)
            .child(
                div()
                    .absolute()
                    .inset_0()
                    .rounded(px(8.))
                    .border_1()
                    .border_color(rgba(0x97979f1f)),
            )
    }

    /// The tab's content.
    pub(super) fn profile_editor_wishlist_tab(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let hearted = self
            .fetched
            .get(&format!("{FETCH_PREFIX}wishlist"))
            .map(wishlist_skus)
            .unwrap_or_default();
        let gifts = self
            .fetched
            .get(&format!("{FETCH_PREFIX}gifts"))
            .map(gift_skus)
            .unwrap_or_default();
        let items = suggested(&gifts, &hearted, true);
        let mut rows: Vec<Vec<Div>> = Vec::new();
        for sku in &items {
            let tile = self.profile_editor_tile(sku, window, cx);
            match rows.last_mut() {
                Some(row) if row.len() < 4 => row.push(tile),
                _ => rows.push(vec![tile]),
            }
        }
        let empty = hearted.is_empty().then(|| {
            div()
                .w(px(413.))
                .h(px(268.))
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap(px(24.))
                .child(
                    div()
                        .w(px(328.))
                        .flex()
                        .flex_col()
                        .items_center()
                        .gap(px(8.))
                        .child(text(
                            "No hearts yet \u{1F494}",
                            16.,
                            20.,
                            500.,
                            "--text-strong",
                        ))
                        .child(
                            div()
                                .text_center()
                                .text_size(px(14.))
                                .line_height(px(18.))
                                .text_color(color("--text-default"))
                                .child(
                                    "Browse the Shop and heart the items you love to add them here.",
                                ),
                        ),
                )
                .child(
                    div()
                        .id("profile-editor-wishlist-shop")
                        .h(px(32.))
                        .px(px(12.))
                        .rounded(px(8.))
                        .border_1()
                        .border_color(rgba(0xffffff14))
                        .bg(color("--control-primary-background-default"))
                        .flex()
                        .items_center()
                        .gap(px(4.))
                        .cursor_pointer()
                        .child(icon("shop", 16., "--white"))
                        .child(text("Explore the Shop", 14., 18., 500., "--white"))
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.profile_editor = None;
                            this.close_settings(cx);
                            this.open_shop(cx);
                        })),
                )
        });
        let suggestions = (!rows.is_empty()).then(|| {
            div()
                .mt(px(32.))
                .w(px(413.))
                .p(px(16.))
                .rounded(px(16.))
                .border_1()
                .border_color(rgba(0x97979f0a))
                .bg(color("--user-profile-overlay-background"))
                .flex()
                .flex_col()
                .gap(px(16.))
                .child(text(
                    "Suggested items for you",
                    12.,
                    16.,
                    400.,
                    "--text-subtle",
                ))
                .child(
                    div().flex().flex_col().gap(px(10.)).children(
                        rows.into_iter()
                            .map(|row| div().flex().gap(px(10.)).children(row)),
                    ),
                )
        });
        div()
            .flex()
            .flex_col()
            .children(empty)
            .children(suggestions)
    }
}
