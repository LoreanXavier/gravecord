//! The App Launcher's home in a text channel (module 621377 with surface `APP_LAUNCHER_IN_TEXT`). Below Recents it
//! lists the application directory's collections for that surface (module 503406 `An`:
//! `GET /application-directory-static/collections?surface=2&active_state=1&platform=4`, asked again after ten
//! minutes), in their `position` order and only those the desktop platform (`WEB`, 4) is in. The Promoted collection
//! (type 5) is a grid of two 224px banner cards across (`sectionTwoColumnContentContainer`); every other collection
//! shows its first four apps as 64px rows in one rounded box, with "View More" in its header when it holds more.
//! Measured in `design/dom/channel/apps-launcher-darker-cozy.json`; a recorded reply is in
//! `design/snapshot/TRAFFIC/app-launcher-collections-text.json`.

use super::activity_shelf::{Shelf, ShelfCard, shelf_key};
use super::{Root, color};
use crate::net::Request;
use gpui::{AnyElement, Context, FontWeight, IntoElement, SharedString, div, prelude::*, px, rgba};
use serde_json::Value;

/// The key of the text launcher's collections in `Root::fetched`.
pub(super) const COLLECTIONS_KEY: &str = "launcher-collections:text";
/// `t < n + 6e5`: collections younger than ten minutes are not asked for again.
const COLLECTIONS_FRESH_MS: i64 = 600_000;
/// `platform`: `WEB` of module 503406.
const PLATFORM_WEB: u64 = 4;
/// Rows a collection shows before "View More".
const ROWS_SHOWN: usize = 4;

/// One app of a collection.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Item {
    pub app_id: String,
    pub name: String,
    pub description: String,
    pub icon: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct Collection {
    pub id: String,
    /// 5 is the Promoted grid; any other type lists rows.
    pub kind: u64,
    pub title: String,
    pub items: Vec<Item>,
}

/// The collections the desktop shows, in `position` order, each with its items in theirs.
pub(super) fn parse_collections(body: &Value) -> Vec<Collection> {
    let mut collections: Vec<(i64, Collection)> = body
        .as_array()
        .into_iter()
        .flatten()
        .filter(|collection| collection["platforms"].as_u64().unwrap_or(0) & PLATFORM_WEB != 0)
        .filter_map(|collection| {
            let mut items: Vec<(i64, Item)> = collection["application_directory_collection_items"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|item| {
                    let application = &item["application"];
                    let app_id = application["id"].as_str()?.to_owned();
                    Some((
                        item["position"].as_i64().unwrap_or(0),
                        Item {
                            app_id,
                            name: application["name"].as_str().unwrap_or_default().to_owned(),
                            description: application["description"]
                                .as_str()
                                .unwrap_or_default()
                                .to_owned(),
                            icon: application["icon"].as_str().map(str::to_owned),
                        },
                    ))
                })
                .collect();
            items.sort_by_key(|(position, _)| *position);
            Some((
                collection["position"].as_i64().unwrap_or(0),
                Collection {
                    id: collection["id"].as_str()?.to_owned(),
                    kind: collection["type"].as_u64().unwrap_or(0),
                    title: collection["title"].as_str().unwrap_or_default().to_owned(),
                    items: items.into_iter().map(|(_, item)| item).collect(),
                },
            ))
        })
        .filter(|(_, collection)| !collection.items.is_empty())
        .collect();
    collections.sort_by_key(|(position, _)| *position);
    collections
        .into_iter()
        .map(|(_, collection)| collection)
        .collect()
}

impl Root {
    /// On opening the launcher: the collections when not fresh, and this server's shelf for the banners.
    pub(super) fn request_launcher_home(&mut self) {
        let now = chrono::Utc::now().timestamp_millis();
        let stale = self
            .launcher_collections_at
            .is_none_or(|at| now - at >= COLLECTIONS_FRESH_MS);
        if stale && !self.requested.contains(COLLECTIONS_KEY) {
            self.requested.insert(COLLECTIONS_KEY.to_owned());
            let _ = self.requests.send(Request::Fetch {
                key: COLLECTIONS_KEY.to_owned(),
                route: "/application-directory-static/collections?surface=2&active_state=1&platform=4&locale=en-US&cache=true"
                    .to_owned(),
            });
        }
        let key = shelf_key(self.guild);
        if !self.fetched.contains_key(&key) && self.requested.insert(key.clone()) {
            let route = match self.guild {
                Some(guild) => format!("/activities/shelf?guild_id={guild}"),
                None => "/activities/shelf".to_owned(),
            };
            let _ = self.requests.send(Request::Fetch { key, route });
        }
    }

    pub(super) fn launcher_collections_fetched(&mut self, body: Value, cx: &mut Context<Self>) {
        self.requested.remove(COLLECTIONS_KEY);
        if crate::net::is_fetch_failure(&body) {
            cx.notify();
            return;
        }
        self.launcher_collections_at = Some(chrono::Utc::now().timestamp_millis());
        for collection in parse_collections(&body) {
            for item in collection.items {
                if let Some(hash) = &item.icon {
                    self.request_image(
                        format!("shelf-icon:{}", item.app_id),
                        format!(
                            "https://cdn.discordapp.com/app-icons/{}/{hash}.png?size=128",
                            item.app_id
                        ),
                    );
                }
            }
        }
        self.fetched.insert(COLLECTIONS_KEY.to_owned(), body);
        cx.notify();
    }

    /// Activities launched lately that this server's shelf still has, as 48px icons (Recents also lists them in the
    /// official launcher, which keeps one frecency list for commands and activities; here they follow the command
    /// apps).
    pub(super) fn launcher_recent_activities(
        &self,
        limit: usize,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let cards = match self.fetched.get(&shelf_key(self.guild)) {
            Some(body) => match Shelf::from_body(body) {
                Shelf::Cards(cards) => cards,
                _ => return Vec::new(),
            },
            None => return Vec::new(),
        };
        super::activity_shelf::stored_recents()
            .into_iter()
            .filter(|id| !self.slash.recents.iter().any(|(app, _, _)| app == id))
            .filter_map(|id| cards.iter().find(|card| card.app_id == id).cloned())
            .take(limit)
            .map(|card| {
                let picture = self
                    .images
                    .get(&format!("shelf-icon:{}", card.app_id))
                    .cloned();
                let app_id = card.app_id.clone();
                div()
                    .id(SharedString::from(format!(
                        "launcher-recent-{}",
                        card.app_id
                    )))
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.slash.launcher = None;
                        this.launch_activity(&app_id, cx);
                    }))
                    .child(self.shelf_icon(picture, 48.))
                    .into_any_element()
            })
            .collect()
    }

    /// The card an app of the Promoted grid draws: its shelf entry (with the banner) when this server's shelf has it.
    fn promoted_card(&self, item: &Item) -> ShelfCard {
        let shelf = match self.fetched.get(&shelf_key(self.guild)) {
            Some(body) => match Shelf::from_body(body) {
                Shelf::Cards(cards) => cards,
                _ => Vec::new(),
            },
            None => Vec::new(),
        };
        let mut card = shelf
            .into_iter()
            .find(|card| card.app_id == item.app_id)
            .unwrap_or_else(|| ShelfCard {
                app_id: item.app_id.clone(),
                name: item.name.clone(),
                description: item.description.clone(),
                icon: item.icon.clone(),
                banner: None,
                promoted: false,
                label: 0,
                requires_age_gate: false,
            });
        // The text launcher's grid draws no PROMOTED or NEW pill (captured Promoted section).
        card.promoted = false;
        card.label = 0;
        card
    }

    /// `header_o-oxSF`: 4px above the 20px title, and "View More" (brand text) at the right.
    pub(super) fn launcher_header(&self, title: String, more: bool) -> AnyElement {
        div()
            .h(px(24.))
            .pt(px(4.))
            .flex()
            .justify_between()
            .text_size(px(16.))
            .line_height(px(20.))
            .font_weight(FontWeight(500.))
            .child(div().text_color(color("--text-strong")).child(title))
            .when(more, |this| {
                this.child(div().text_color(color("--text-brand")).child("View More"))
            })
            .into_any_element()
    }

    /// One 64px row (`rowContainer_LekV6N`): a 40px icon, the name and one line of description, a divider under it but
    /// the last.
    fn launcher_row(
        &self,
        item: &Item,
        first: bool,
        last: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let picture = self
            .images
            .get(&format!("shelf-icon:{}", item.app_id))
            .cloned();
        let app_id = item.app_id.clone();
        div()
            .id(SharedString::from(format!("launcher-row-{}", item.app_id)))
            .relative()
            .h(px(64.))
            .p(px(12.))
            .flex()
            .items_center()
            .gap(px(12.))
            .bg(color("--background-surface-high"))
            .when(first, |this| this.rounded_t(px(12.)))
            .when(last, |this| this.rounded_b(px(12.)))
            .cursor_pointer()
            .hover(|style| style.bg(color("--interactive-background-hover")))
            .on_click(cx.listener(move |this, _, _, cx| {
                this.slash.launcher = None;
                this.launch_activity(&app_id, cx);
            }))
            .child(self.shelf_icon(picture, 40.))
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .h(px(20.))
                            .truncate()
                            .text_size(px(16.))
                            .line_height(px(20.))
                            .font_weight(FontWeight(600.))
                            .text_color(color("--text-strong"))
                            .child(item.name.clone()),
                    )
                    .child(
                        div()
                            .h(px(18.))
                            .truncate()
                            .text_size(px(14.))
                            .line_height(px(18.))
                            .text_color(color("--text-subtle"))
                            .child(item.description.clone()),
                    ),
            )
            .when(!last, |this| {
                this.child(
                    div()
                        .absolute()
                        .left_0()
                        .right_0()
                        .bottom_0()
                        .h(px(1.))
                        .bg(rgba(0x97979f1f)),
                )
            })
            .into_any_element()
    }

    /// The collections under Recents, each a header with its content 12px below and 32px of space after.
    pub(super) fn launcher_collections(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let Some(body) = self.fetched.get(COLLECTIONS_KEY) else {
            return Vec::new();
        };
        let collections = parse_collections(body);
        let mut out = Vec::new();
        for collection in &collections {
            let content = if collection.kind == 5 {
                div()
                    .flex()
                    .flex_wrap()
                    .gap(px(12.))
                    .children(collection.items.iter().map(|item| {
                        let card = self.promoted_card(item);
                        self.shelf_card(
                            &card,
                            SharedString::from(format!("launcher-promoted-{}", item.app_id)),
                            cx,
                        )
                    }))
                    .into_any_element()
            } else {
                let shown = collection.items.len().min(ROWS_SHOWN);
                div()
                    .flex()
                    .flex_col()
                    .children(collection.items.iter().take(shown).enumerate().map(
                        |(index, item)| self.launcher_row(item, index == 0, index + 1 == shown, cx),
                    ))
                    .into_any_element()
            };
            out.push(
                div()
                    .id(SharedString::from(format!(
                        "launcher-collection-{}",
                        collection.id
                    )))
                    .flex()
                    .flex_col()
                    .child(self.launcher_header(
                        collection.title.clone(),
                        collection.kind != 5 && collection.items.len() > ROWS_SHOWN,
                    ))
                    .child(div().mt(px(8.)).mb(px(32.)).child(content))
                    .into_any_element(),
            );
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::parse_collections;

    #[test]
    fn collections_follow_their_positions_and_the_desktop_platform() {
        let body: serde_json::Value = serde_json::from_str(
            r#"[
                {"id": "b", "type": 4, "position": 2, "platforms": 7, "title": "Puzzle Games",
                 "application_directory_collection_items": [
                    {"position": 2, "application": {"id": "2", "name": "Two", "description": "d2"}},
                    {"position": 1, "application": {"id": "1", "name": "One", "description": "d1", "icon": "h"}}]},
                {"id": "a", "type": 5, "position": 1, "platforms": 7, "title": "Promoted",
                 "application_directory_collection_items": [{"position": 1, "application": {"id": "3", "name": "Three"}}]},
                {"id": "m", "type": 4, "position": 3, "platforms": 3, "title": "Mobile only",
                 "application_directory_collection_items": [{"position": 1, "application": {"id": "4", "name": "Four"}}]}
            ]"#,
        )
        .unwrap();
        let collections = parse_collections(&body);
        assert_eq!(
            collections
                .iter()
                .map(|c| c.title.as_str())
                .collect::<Vec<_>>(),
            ["Promoted", "Puzzle Games"]
        );
        assert_eq!(
            collections[1]
                .items
                .iter()
                .map(|i| i.name.as_str())
                .collect::<Vec<_>>(),
            ["One", "Two"]
        );
        assert_eq!(collections[1].items[0].icon.as_deref(), Some("h"));
    }
}
