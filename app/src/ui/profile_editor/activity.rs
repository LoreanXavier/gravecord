//! The editor's Activity tab: module 927071's `sc` for the account's own profile. A "Recent activity" heading with the
//! intro line ("Learn more" linked) over one card per entry of the content-inventory outbox (`rQ` of module 637865).
//! Measured in `design/dom/settings/profile-editor-activity-tab-darker-cozy.json`.

use super::{FETCH_PREFIX, text};
use crate::ui::{Root, color, icon};
use gpui::{AnyElement, Div, FontWeight, IntoElement, div, img, prelude::*, px};
use serde_json::Value;

/// `ContentInventoryEntryType`: a played game, a listening session and a launched activity.
const PLAYED_GAME: u64 = 1;
const LISTENED_SESSION: u64 = 5;
const LAUNCHED_ACTIVITY: u64 = 8;

/// Milliseconds since the Unix epoch of a snowflake.
fn snowflake_ms(id: u64) -> i64 {
    (id >> 22) as i64 + 1_420_070_400_000
}

/// `GET /content-inventory/users/<id>/outbox` (`fetchUserOutbox`): the entries the tab lists.
pub(super) fn outbox_route(user: u64) -> String {
    format!("/content-inventory/users/{user}/outbox")
}

/// The entries the tab draws a card for, in the order the server listed them (`$R` of module 20805).
pub(super) fn drawn(outbox: &Value) -> Vec<&Value> {
    outbox["entries"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|entry| {
            matches!(
                entry["content_type"].as_u64(),
                Some(PLAYED_GAME | LISTENED_SESSION | LAUNCHED_ACTIVITY)
            )
        })
        .collect()
}

/// The routes an arrived outbox asks for next: the applications behind the entries (their icons are the art).
pub(super) fn apps_route(outbox: &Value) -> Option<String> {
    let query: Vec<String> = drawn(outbox)
        .into_iter()
        .filter_map(|entry| entry["extra"]["application_id"].as_str())
        .map(|id| format!("application_ids={id}"))
        .collect();
    (!query.is_empty()).then(|| format!("/applications/public?{}", query.join("&")))
}

impl Root {
    /// The answers of the outbox and its applications: the art to load.
    pub(super) fn profile_editor_activity_fetched(&mut self, what: &str) {
        let key = format!("{FETCH_PREFIX}{what}");
        let Some(body) = self.fetched.get(&key).cloned() else {
            return;
        };
        match what {
            "outbox" => {
                if let Some(route) = apps_route(&body) {
                    let _ = self.requests.send(crate::net::Request::Fetch {
                        key: format!("{FETCH_PREFIX}apps"),
                        route,
                    });
                }
                for entry in drawn(&body) {
                    if let Some(art) = entry["extra"]["entries"][0]["media"]["image_url"].as_str() {
                        self.profile_editor_image(art.to_owned(), art.to_owned());
                    }
                }
            }
            "apps" => {
                for app in body.as_array().into_iter().flatten() {
                    if let (Some(id), Some(icon)) = (app["id"].as_str(), app["icon"].as_str()) {
                        self.profile_editor_image(
                            format!("app-icon:{id}"),
                            crate::ui::cdn::app_icon(
                                id,
                                icon,
                                crate::ui::live_activity::ACTIVITY_ART,
                            ),
                        );
                    }
                }
            }
            _ => {}
        }
    }

    /// Asked for through `request_image`, so a failure is retried like every other picture (`image_retry`).
    fn profile_editor_image(&mut self, key: String, url: String) {
        self.request_image(key, url);
    }

    /// One `card_Bu4Al8` of the tab: the 60px art, the title with the artist line of a listening session, and the
    /// badge row with the age of the entry.
    fn profile_editor_activity_card(&self, entry: &Value, now_ms: i64) -> Option<AnyElement> {
        let kind = entry["content_type"].as_u64()?;
        let id: u64 = entry["id"].as_str()?.parse().ok()?;
        let ago = crate::ui::full_profile::time_ago((now_ms - snowflake_ms(id)) / 1_000);
        let app = entry["extra"]["application_id"]
            .as_str()
            .unwrap_or_default();
        let (art, title, subtitle, glyph) = match kind {
            PLAYED_GAME => (
                self.images.get(&format!("app-icon:{app}")).cloned(),
                entry["extra"]["game_name"]
                    .as_str()
                    .unwrap_or_default()
                    .to_owned(),
                None,
                "activity_game",
            ),
            LISTENED_SESSION => {
                let media = &entry["extra"]["entries"][0]["media"];
                let url = media["image_url"].as_str().unwrap_or_default();
                (
                    self.images.get(url).cloned(),
                    media["title"].as_str().unwrap_or_default().to_owned(),
                    media["artists"][0]["name"].as_str().map(str::to_owned),
                    "activity_music",
                )
            }
            _ => (
                self.images.get(&format!("app-icon:{app}")).cloned(),
                entry["extra"]["activity_name"]
                    .as_str()
                    .unwrap_or_default()
                    .to_owned(),
                None,
                "activity_launched",
            ),
        };
        Some(
            div()
                .relative()
                .w(px(413.))
                .h(px(84.))
                .flex_none()
                .p(px(12.))
                .rounded(px(12.))
                .bg(color("--user-profile-overlay-background"))
                .flex()
                .gap(px(12.))
                .child(
                    div()
                        .size(px(60.))
                        .flex_none()
                        .rounded(px(8.))
                        .overflow_hidden()
                        .children(art.map(|picture| img(picture).size(px(60.)).rounded(px(8.)))),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.))
                        .flex()
                        .flex_col()
                        .justify_center()
                        .gap(px(4.))
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .child(
                                    div()
                                        .text_size(px(14.))
                                        .line_height(px(18.))
                                        .font_weight(FontWeight(600.))
                                        .text_color(color("--text-strong"))
                                        .child(title),
                                )
                                .children(subtitle.map(|subtitle| {
                                    text(subtitle, 12., 16., 400., "--text-default")
                                })),
                        )
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(4.))
                                .child(icon(glyph, 12., "--text-subtle"))
                                .child(text(ago, 12., 16., 400., "--text-subtle")),
                        ),
                )
                .child(div().absolute().top(px(12.)).right(px(12.)).child(icon(
                    "more_horizontal",
                    16.,
                    "--text-subtle",
                )))
                .into_any_element(),
        )
    }

    /// The tab: heading, intro and the cards (`headings_NcvRRg` then the list at an 8px gap).
    pub(super) fn profile_editor_activity_tab(&self) -> Div {
        let now_ms = chrono::Utc::now().timestamp_millis();
        let cards: Vec<AnyElement> = self
            .fetched
            .get(&format!("{FETCH_PREFIX}outbox"))
            .map(|outbox| {
                drawn(outbox)
                    .into_iter()
                    .filter_map(|entry| self.profile_editor_activity_card(entry, now_ms))
                    .collect()
            })
            .unwrap_or_default();
        div()
            .flex()
            .flex_col()
            .gap(px(8.))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .child(text("Recent activity", 12., 16., 500., "--text-subtle"))
                    .child(
                        div()
                            .flex()
                            .text_size(px(12.))
                            .line_height(px(16.))
                            .text_color(color("--text-default"))
                            .child("You'll see your activity from the last 30 days here.\u{a0}")
                            .child(text("Learn more", 12., 16., 400., "--text-link")),
                    ),
            )
            .child(div().flex().flex_col().gap(px(8.)).children(cards))
    }
}
