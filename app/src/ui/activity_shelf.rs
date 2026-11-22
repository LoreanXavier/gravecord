//! The Start An Activity shelf: the App Launcher modal with the VOICE entry point (modules 126784 and 621377). It
//! fetches `GET /activities/shelf?guild_id=`, keeps the cards the desktop platform ("web") can run, orders them as
//! 994428 does, and shows a Recents row, the Activities grid and a search that matches the name and the description
//! (621377 `lh`). Measured in `design/dom/popouts/voice-panel-activity-darker-cozy.json`.
//!
//! The response has no recorded sample; its field names come from the bundle's consumers (shelf spec: `activities`
//! configs, `applications`, `assets` by application id).

use super::{Root, color, editor_style, icon};
use crate::editor::{Editor, EditorEvent};
use crate::net::Request;
use crate::ui::motion::Posed as _;
use gpui::{
    AnyElement, Context, Entity, Focusable, FontWeight, IntoElement, ObjectFit, SharedString,
    Subscription, Window, deferred, div, img, prelude::*, px, rgba,
};
use serde_json::{Value, json};

pub(super) const SHELF_PREFIX: &str = "activity-shelf:";
/// `EmbeddedActivitiesStore`: a shelf younger than six hours is not fetched again, but the launcher asks every time
/// it opens (621377 `lv`, `force: true`).
const RECENTS_KEY: &str = "activities.recent";
/// Application flag bit 29, `PROMOTED`.
const PROMOTED_FLAG: u64 = 1 << 29;
const RECENTS_SHOWN: usize = 8;

pub(crate) struct ActivityShelf {
    search: Entity<Editor>,
    _events: Subscription,
    query: String,
    guild: Option<u64>,
    scroll: gpui::ScrollHandle,
}

/// One shelf entry, with what the card shows.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ShelfCard {
    pub app_id: String,
    pub name: String,
    pub description: String,
    pub icon: Option<String>,
    /// The asset named `embedded_cover` of this application.
    pub banner: Option<String>,
    pub promoted: bool,
    /// `label_type` while its window is open: 1 NEW, 2 UPDATED, else 0.
    pub label: u64,
    pub requires_age_gate: bool,
}

/// What the shelf holds.
pub(crate) enum Shelf {
    Loading,
    Failed,
    Cards(Vec<ShelfCard>),
}

impl Shelf {
    /// A fetched shelf reply: its cards, or the failure.
    pub(super) fn from_body(body: &Value) -> Self {
        if body["error"].as_bool() == Some(true) {
            Shelf::Failed
        } else {
            Shelf::Cards(parse_shelf(body, chrono::Utc::now()))
        }
    }
}

/// The key of a guild's shelf in `Root::fetched`: `K(guildId) = guildId ?? "0"`.
pub(super) fn shelf_key(guild: Option<u64>) -> String {
    format!("{SHELF_PREFIX}{}", guild.unwrap_or(0))
}

fn flag_bits(application: &Value) -> u64 {
    match &application["flags"] {
        Value::Number(number) => number.as_u64().unwrap_or(0),
        Value::String(text) => text.parse().unwrap_or(0),
        _ => 0,
    }
}

/// Module 697675 (`ActivityNotificationLabel`): returns the uppercase badge label for an activity
/// shelf `label_type` (`1` = `"NEW"`, `2` = `/qdhkk` `"UPDATED"`, else `None`).
pub(crate) fn activity_notification_label_text(label_type: u64) -> Option<String> {
    match label_type {
        1 => Some("NEW".to_owned()),
        2 => Some(dc_intl::t("/qdhkk").to_uppercase()),
        _ => None,
    }
}

/// Module 697675 (`ActivityNotificationLabel`): renders either a 10x10 circular notification dot
/// (`--badge-notification-background`) when collapsed, or a bordered uppercase `"NEW"` / `"UPDATED"`
/// (`--badge-primary-*`) pill when expanded.
pub(crate) fn activity_notification_label(label_type: u64, collapsed: bool) -> Option<AnyElement> {
    let text = activity_notification_label_text(label_type)?;
    if collapsed {
        return Some(
            div()
                .size(px(10.))
                .rounded_full()
                .bg(color("--badge-notification-background"))
                .into_any_element(),
        );
    }
    Some(
        div()
            .h(px(20.))
            .px(px(6.))
            .rounded(px(4.))
            .border_1()
            .flex()
            .items_center()
            .bg(color("--badge-primary-background"))
            .border_color(color("--badge-primary-border"))
            .text_color(color("--badge-primary-text"))
            .text_size(px(12.))
            .line_height(px(16.))
            .font_weight(FontWeight(600.))
            .child(text)
            .into_any_element(),
    )
}

/// Whether `now` is inside a `client_platform_config.web` label window (`label_from`, `label_until`).
fn label_active(web: &Value, now: chrono::DateTime<chrono::Utc>) -> bool {
    let at = |name: &str| {
        web[name]
            .as_str()
            .and_then(|text| chrono::DateTime::parse_from_rfc3339(text).ok())
            .map(|time| time.with_timezone(&chrono::Utc))
    };
    // `D` (735991): both ends have to be set, and `now` strictly between them.
    matches!((at("label_from"), at("label_until")), (Some(from), Some(until)) if now > from && now < until)
}

/// 994428: the configs in server order, resolved against their applications; kept when the application supports
/// the desktop platform ("web") and is not age-gated for an account that may not see it (here the account's
/// `nsfw_allowed` is unknown, which passes, as the bundle does); then each NEW or UPDATED item whose label is
/// current moves forward to `shelf_rank - 1`. Promoted items get no special place.
pub(crate) fn parse_shelf(body: &Value, now: chrono::DateTime<chrono::Utc>) -> Vec<ShelfCard> {
    let applications = body["applications"].as_array().cloned().unwrap_or_default();
    let assets = &body["assets"];
    let mut kept: Vec<(ShelfCard, Option<usize>)> = Vec::new();
    for config in body["activities"].as_array().into_iter().flatten() {
        let Some(id) = config["application_id"].as_str() else {
            continue;
        };
        let Some(application) = applications.iter().find(|a| a["id"].as_str() == Some(id)) else {
            continue;
        };
        let platforms = config["supported_platforms"].as_array();
        if !platforms.is_some_and(|list| list.iter().any(|p| p.as_str() == Some("web"))) {
            continue;
        }
        let web = &config["client_platform_config"]["web"];
        let labelled = matches!(web["label_type"].as_u64(), Some(1 | 2)) && label_active(web, now);
        let rank = labelled
            .then(|| config["shelf_rank"].as_u64())
            .flatten()
            .map(|rank| rank.saturating_sub(1) as usize);
        let banner = assets[id].as_array().and_then(|list| {
            list.iter()
                .find(|asset| asset["name"].as_str() == Some("embedded_cover"))
                .and_then(|asset| asset["id"].as_str())
                .map(str::to_owned)
        });
        kept.push((
            ShelfCard {
                app_id: id.to_owned(),
                name: application["name"].as_str().unwrap_or_default().to_owned(),
                description: application["description"]
                    .as_str()
                    .unwrap_or_default()
                    .to_owned(),
                icon: application["icon"].as_str().map(str::to_owned),
                banner,
                promoted: flag_bits(application) & PROMOTED_FLAG != 0,
                label: if labelled {
                    web["label_type"].as_u64().unwrap_or(0)
                } else {
                    0
                },
                requires_age_gate: config["requires_age_gate"].as_bool().unwrap_or(false),
            },
            rank,
        ));
    }
    // "Moved to index shelf_rank - 1 if that is lower than its current index", in the order the list is walked.
    let mut order: Vec<(ShelfCard, Option<usize>)> = kept;
    let mut index = 0;
    while index < order.len() {
        match order[index].1 {
            Some(target) if target < index => {
                let item = order.remove(index);
                order.insert(target, item);
            }
            _ => {}
        }
        index += 1;
    }
    order.into_iter().map(|(card, _)| card).collect()
}

/// 621377 `lA`: the cards whose name or description holds the query, in four buckets (name starts with, name
/// contains, description starts with, description contains); the first bucket that matches wins, and the order inside
/// a bucket is the recents score, then the name.
pub(crate) fn search_cards(cards: &[ShelfCard], recents: &[String], query: &str) -> Vec<ShelfCard> {
    let query = query.trim().trim_start_matches('/').to_lowercase();
    if query.is_empty() {
        return cards.to_vec();
    }
    let mut buckets: [Vec<&ShelfCard>; 4] = Default::default();
    for card in cards {
        let name = card.name.to_lowercase();
        let description = card.description.to_lowercase();
        let bucket = if name.starts_with(&query) {
            0
        } else if name.contains(&query) {
            1
        } else if description.starts_with(&query) {
            2
        } else if description.contains(&query) {
            3
        } else {
            continue;
        };
        buckets[bucket].push(card);
    }
    let recency = |card: &ShelfCard| {
        recents
            .iter()
            .position(|id| *id == card.app_id)
            .map_or(0, |at| recents.len() - at)
    };
    buckets
        .into_iter()
        .flat_map(|mut bucket| {
            bucket.sort_by(|a, b| {
                recency(b)
                    .cmp(&recency(a))
                    .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
            });
            bucket.into_iter().cloned()
        })
        .collect()
}

pub(super) fn stored_recents() -> Vec<String> {
    crate::storage::get(RECENTS_KEY)
        .and_then(|value| value.as_array().cloned())
        .map(|list| {
            list.iter()
                .filter_map(|id| id.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default()
}

/// Puts an application first in the recents list that Recents shows.
pub(crate) fn remember_activity(app_id: &str) {
    let mut recents = stored_recents();
    recents.retain(|id| id != app_id);
    recents.insert(0, app_id.to_owned());
    recents.truncate(100);
    crate::storage::set(RECENTS_KEY, json!(recents));
}

impl Root {
    /// Opens the shelf for the call's guild (the Start An Activity button, the Apps tile, Choose Activity).
    pub(super) fn open_activity_shelf(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        tracing::info!("activity shelf opened");
        let guild = self
            .call
            .as_ref()
            .map(|call| call.guild)
            .filter(|guild| *guild != 0)
            .or(self.guild);
        let key = shelf_key(guild);
        // The launcher asks again on every open.
        self.fetched.remove(&key);
        self.requested.remove(&key);
        self.requested.insert(key.clone());
        let route = match guild {
            Some(guild) => format!("/activities/shelf?guild_id={guild}"),
            None => "/activities/shelf".to_owned(),
        };
        let _ = self.requests.send(Request::Fetch { key, route });
        let search = cx.new(|cx| {
            let mut editor = Editor::new(editor_style(), cx);
            editor.set_placeholder("Search".to_owned());
            editor
        });
        let events = cx.subscribe(&search, |this, editor, event: &EditorEvent, cx| {
            match event {
                EditorEvent::Edited => {
                    if let Some(shelf) = this.activity_shelf.as_mut() {
                        shelf.query = editor.read(cx).text().to_owned();
                        shelf.scroll.set_offset(gpui::point(px(0.), px(0.)));
                    }
                }
                EditorEvent::Submit(_) => {}
                EditorEvent::Cancel => this.close_activity_shelf(cx),
            }
            cx.notify();
        });
        window.focus(&search.read(cx).focus_handle(cx), cx);
        self.activity_shelf = Some(ActivityShelf {
            search,
            _events: events,
            query: String::new(),
            guild,
            scroll: gpui::ScrollHandle::new(),
        });
        cx.notify();
    }

    /// A card or a Recents tile: launching is `launch_activity`.
    fn activity_card_clicked(&mut self, app_id: &str, cx: &mut Context<Self>) {
        self.launch_activity(app_id, cx);
    }

    /// `APP_LAUNCHER_DISMISS`.
    pub(super) fn close_activity_shelf(&mut self, cx: &mut Context<Self>) {
        if self.activity_shelf.take().is_some() {
            cx.notify();
        }
    }

    /// The Fetch result of the shelf: asks for the pictures the cards need.
    pub(super) fn activity_shelf_fetched(&mut self, key: &str, cx: &mut Context<Self>) {
        let Shelf::Cards(cards) = self.shelf_state(key) else {
            cx.notify();
            return;
        };
        for card in cards {
            self.activities
                .names
                .insert(card.app_id.clone(), card.name.clone());
            if let Some(hash) = &card.icon {
                self.request_image(
                    format!("shelf-icon:{}", card.app_id),
                    format!(
                        "https://cdn.discordapp.com/app-icons/{}/{hash}.png?size=128",
                        card.app_id
                    ),
                );
            }
            if let Some(asset) = &card.banner {
                self.request_image(
                    format!("shelf-banner:{}", card.app_id),
                    format!(
                        "https://cdn.discordapp.com/app-assets/{}/{asset}.webp?size=600",
                        card.app_id
                    ),
                );
            }
        }
        cx.notify();
    }

    fn shelf_state(&self, key: &str) -> Shelf {
        match self.fetched.get(key) {
            None => Shelf::Loading,
            Some(body) => Shelf::from_body(body),
        }
    }

    /// Escape: true when the shelf was open.
    pub(super) fn activity_shelf_escape(&mut self, cx: &mut Context<Self>) -> bool {
        if self.activity_shelf.is_none() {
            return false;
        }
        self.close_activity_shelf(cx);
        true
    }

    /// `opens_page`: the text launcher's card opens the app's page; the voice shelf's starts the activity.
    pub(super) fn shelf_card(
        &self,
        card: &ShelfCard,
        id: SharedString,
        opens_page: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let banner = self
            .images
            .get(&format!("shelf-banner:{}", card.app_id))
            .cloned();
        let picture = self
            .images
            .get(&format!("shelf-icon:{}", card.app_id))
            .cloned();
        let app_id = card.app_id.clone();
        div()
            .id(id)
            .w(px(224.))
            .h(px(180.))
            .rounded(px(12.))
            .overflow_hidden()
            .bg(color("--background-surface-high"))
            .cursor_pointer()
            .hover(|style| style.bg(color("--interactive-background-hover")))
            .active(|style| style.bg(color("--interactive-background-active")))
            .on_click(cx.listener(move |this, _, _, cx| {
                if opens_page {
                    this.open_launcher_app(app_id.clone(), cx);
                } else {
                    this.activity_card_clicked(&app_id, cx);
                }
            }))
            .flex()
            .flex_col()
            .child(
                div()
                    .relative()
                    .w_full()
                    .h(px(126.))
                    .flex_none()
                    .overflow_hidden()
                    .bg(color("--background-surface-highest"))
                    .children(
                        banner.map(|banner| img(banner).size_full().object_fit(ObjectFit::Cover)),
                    )
                    .when(card.promoted || card.label != 0, |this| {
                        // `bannerUpperRightContainer`: a row, 4px apart, 8px from the top and the right.
                        let pill = |text: gpui::SharedString, background, border, ink| {
                            div()
                                .h(px(16.))
                                .px(px(8.))
                                .rounded_full()
                                .flex()
                                .items_center()
                                .bg(color(background))
                                .border_color(color(border))
                                .text_color(color(ink))
                                .text_size(px(12.))
                                .line_height(px(16.))
                                .font_weight(FontWeight(600.))
                                .child(text)
                        };
                        this.child(
                            div()
                                .absolute()
                                .top(px(8.))
                                .right(px(8.))
                                .flex()
                                .gap(px(4.))
                                .when(card.promoted, |row| {
                                    row.child(pill(
                                        "PROMOTED".into(),
                                        "--badge-expressive-background",
                                        "--badge-expressive-border",
                                        "--badge-expressive-text",
                                    ))
                                })
                                .children(activity_notification_label(card.label, false)),
                        )
                    }),
            )
            .child(
                div()
                    .h(px(54.))
                    .p(px(8.))
                    .flex()
                    .items_center()
                    .child(self.shelf_icon(picture, 38.))
                    .child(
                        div()
                            .ml(px(12.))
                            .w(px(158.))
                            .flex_shrink(1.)
                            .overflow_hidden()
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
                                    .child(card.name.clone()),
                            )
                            .child(
                                div()
                                    .h(px(18.))
                                    .truncate()
                                    .text_size(px(14.))
                                    .line_height(px(18.))
                                    .text_color(color("--text-subtle"))
                                    .child(card.description.clone()),
                            ),
                    ),
            )
            .into_any_element()
    }

    pub(super) fn shelf_icon(
        &self,
        picture: Option<std::sync::Arc<gpui::Image>>,
        size: f32,
    ) -> AnyElement {
        // The picture is clipped by its own radius: 12px outside, 11px inside the 1px border.
        div()
            .size(px(size))
            .flex_none()
            .rounded(px(12.))
            .border_1()
            .border_color(color("--border-subtle"))
            .children(picture.map(|picture| img(picture).size_full().rounded(px(11.))))
            .into_any_element()
    }

    fn shelf_header(title: &'static str) -> AnyElement {
        div()
            .h(px(24.))
            .pt(px(4.))
            .text_size(px(16.))
            .line_height(px(20.))
            .font_weight(FontWeight(500.))
            .text_color(color("--text-strong"))
            .child(title)
            .into_any_element()
    }

    /// The modal: the search field over the Recents row and the Activities grid.
    pub(super) fn activity_shelf_layer(
        &self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let shelf = self.activity_shelf.as_ref()?;
        let state = self.shelf_state(&shelf_key(shelf.guild));
        let recents = stored_recents();
        let searching = !shelf.query.trim().is_empty();
        let height = f32::from(window.viewport_size().height).min(680.);
        let mut sections: Vec<AnyElement> = Vec::new();
        match &state {
            Shelf::Loading => {
                // 4 placeholder cards per section.
                sections.push(Self::shelf_header("Activities"));
                sections.push(
                    div()
                        .mt(px(8.))
                        .flex()
                        .flex_wrap()
                        .gap(px(12.))
                        .children((0..4).map(|_| {
                            div()
                                .w(px(224.))
                                .h(px(180.))
                                .rounded(px(12.))
                                .bg(color("--background-surface-high"))
                        }))
                        .into_any_element(),
                );
            }
            Shelf::Failed => sections.push(Self::shelf_empty("Failed to fetch activities")),
            Shelf::Cards(cards) => {
                if searching {
                    let found = search_cards(cards, &recents, &shelf.query);
                    if found.is_empty() {
                        sections.push(Self::shelf_empty("No activities match your search"));
                    } else {
                        sections.push(Self::shelf_header("Activities"));
                        sections.push(self.shelf_grid(&found, "shelf-found", cx));
                    }
                } else {
                    let recent: Vec<&ShelfCard> = recents
                        .iter()
                        .filter_map(|id| cards.iter().find(|card| &card.app_id == id))
                        .take(RECENTS_SHOWN)
                        .collect();
                    if !recent.is_empty() {
                        sections.push(Self::shelf_header("Recents"));
                        sections.push(
                            div()
                                .mt(px(8.))
                                .mb(px(32.))
                                .flex()
                                .gap(px(12.))
                                .children(recent.iter().enumerate().map(|(index, card)| {
                                    let app_id = card.app_id.clone();
                                    let picture = self
                                        .images
                                        .get(&format!("shelf-icon:{}", card.app_id))
                                        .cloned();
                                    div()
                                        .id(SharedString::from(format!("shelf-recent-{index}")))
                                        .size(px(48.))
                                        .rounded(px(12.))
                                        .overflow_hidden()
                                        .bg(color("--background-surface-high"))
                                        .cursor_pointer()
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            this.activity_card_clicked(&app_id, cx);
                                        }))
                                        .child(self.shelf_icon(picture, 48.))
                                }))
                                .into_any_element(),
                        );
                    }
                    if cards.is_empty() {
                        sections.push(Self::shelf_empty("Failed to fetch activities"));
                    } else {
                        sections.push(Self::shelf_header("Activities"));
                        sections.push(self.shelf_grid(cards, "shelf-card", cx));
                    }
                }
            }
        }
        let field = div()
            .h(px(40.))
            .w_full()
            // The focused ring is 2px (golden: the 1px border and the focus ring), inside the same 40px.
            .pl(px(3.))
            .rounded(px(8.))
            .border_2()
            .border_color(color("--input-border-active"))
            .bg(color("--input-background-default"))
            .flex()
            .items_center()
            .child(
                div()
                    .size(px(30.))
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(icon("search", 16., "--text-default")),
            )
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .pr(px(12.))
                    .text_size(px(16.))
                    .child(shelf.search.clone()),
            );
        let content = div()
            .size_full()
            .rounded(px(8.))
            .overflow_hidden()
            .bg(color("--background-base-lower"))
            .flex()
            .flex_col()
            .child(div().h(px(72.)).flex_none().p(px(16.)).child(field))
            .child(
                div()
                    .id("shelf-scroll")
                    .flex_1()
                    .min_h(px(0.))
                    .px(px(16.))
                    .pb(px(16.))
                    .overflow_y_scroll()
                    .track_scroll(&shelf.scroll)
                    .flex()
                    .flex_col()
                    .children(sections),
            );
        // The legacy modal root (`root__KoJTP` in the capture, `EO` in module 935462).
        let pose =
            crate::ui::motion::legacy_modal_pose("activity-shelf", "activity-shelf-layer", false);
        Some(
            deferred(
                div()
                    .id("activity-shelf-layer")
                    .occlude()
                    .absolute()
                    .inset_0()
                    .bg(pose.dim(rgba(0x000000b8)))
                    .flex()
                    .items_center()
                    .justify_center()
                    .on_click(cx.listener(|this, _, _, cx| {
                        tracing::info!("activity shelf closed by its backdrop");
                        this.close_activity_shelf(cx)
                    }))
                    .child(
                        div()
                            .posed(pose)
                            .id("activity-shelf")
                            .occlude()
                            .w(px(502.))
                            .h(px(height + 2.))
                            .rounded(px(12.))
                            .border_1()
                            .border_color(color("--border-normal"))
                            .bg(color("--modal-background"))
                            .shadow(vec![gpui::BoxShadow {
                                color: gpui::hsla(0., 0., 0., 0.16),
                                offset: gpui::point(px(0.), px(4.)),
                                blur_radius: px(8.),
                                spread_radius: px(0.),
                                inset: false,
                            }])
                            .child(content),
                    ),
            )
            .with_priority(1)
            .into_any_element(),
        )
    }

    fn shelf_empty(text: &'static str) -> AnyElement {
        div()
            .pt(px(32.))
            .flex()
            .justify_center()
            .text_size(px(16.))
            .line_height(px(20.))
            .text_color(color("--text-subtle"))
            .child(text)
            .into_any_element()
    }

    fn shelf_grid(
        &self,
        cards: &[ShelfCard],
        prefix: &'static str,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        div()
            .mt(px(8.))
            .mb(px(32.))
            .flex()
            .flex_wrap()
            .gap(px(12.))
            .children(cards.iter().enumerate().map(|(index, card)| {
                self.shelf_card(
                    card,
                    SharedString::from(format!("{prefix}-{index}")),
                    false,
                    cx,
                )
            }))
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn now() -> chrono::DateTime<chrono::Utc> {
        chrono::Utc.with_ymd_and_hms(2026, 11, 9, 0, 0, 0).unwrap()
    }

    fn config(id: &str, platforms: &[&str], rank: u64, label: u64) -> Value {
        json!({
            "application_id": id,
            "supported_platforms": platforms,
            "shelf_rank": rank,
            "client_platform_config": {"web": {"label_type": label, "label_from": "2026-01-01T00:00:00+00:00", "label_until": "2027-01-01T00:00:00+00:00"}},
        })
    }

    fn application(id: &str, name: &str, description: &str, flags: u64) -> Value {
        json!({"id": id, "name": name, "description": description, "icon": "ic", "flags": flags})
    }

    fn body() -> Value {
        json!({
            "activities": [
                config("1", &["web"], 1, 0),
                config("2", &["ios"], 2, 0),
                config("3", &["web", "ios"], 3, 0),
                config("4", &["web"], 2, 1),
            ],
            "applications": [
                application("1", "Wordle", "Guess the word", 1 << 29),
                application("2", "Mobile Only", "x", 0),
                application("3", "Watch Together", "Watch videos with friends", 0),
                application("4", "Chess", "Play a word game of chess", 0),
            ],
            "assets": {"1": [{"id": "a1", "name": "embedded_cover", "type": 1}, {"id": "a2", "name": "other", "type": 1}]},
        })
    }

    #[test]
    fn the_shelf_keeps_desktop_activities_in_order_and_moves_labelled_ones_forward() {
        let cards = parse_shelf(&body(), now());
        let names: Vec<&str> = cards.iter().map(|card| card.name.as_str()).collect();
        // Mobile Only is dropped; Chess carries a NEW label with rank 2, so it moves to index 1.
        assert_eq!(names, ["Wordle", "Chess", "Watch Together"]);
        assert!(cards[0].promoted);
        assert_eq!(cards[0].banner.as_deref(), Some("a1"));
        assert!(!cards[1].promoted && cards[1].banner.is_none());
    }

    #[test]
    fn a_label_needs_both_ends_of_its_window() {
        let mut shelf = body();
        shelf["activities"][3]["client_platform_config"]["web"]["label_from"] = json!(null);
        let names: Vec<String> = parse_shelf(&shelf, now())
            .into_iter()
            .map(|card| card.name)
            .collect();
        assert_eq!(names, ["Wordle", "Watch Together", "Chess"]);
        let open = parse_shelf(&body(), now());
        assert_eq!(
            open.iter().map(|card| card.label).collect::<Vec<_>>(),
            [0, 1, 0]
        );
    }

    #[test]
    fn an_expired_label_does_not_reorder() {
        let mut shelf = body();
        shelf["activities"][3]["client_platform_config"]["web"]["label_until"] =
            json!("2026-01-01T00:00:00+00:00");
        let cards = parse_shelf(&shelf, now());
        let names: Vec<&str> = cards.iter().map(|card| card.name.as_str()).collect();
        assert_eq!(names, ["Wordle", "Watch Together", "Chess"]);
    }

    #[test]
    fn search_buckets_are_name_start_name_contains_description_start_description_contains() {
        let card = |name: &str, description: &str| ShelfCard {
            app_id: name.to_owned(),
            name: name.to_owned(),
            description: description.to_owned(),
            icon: None,
            banner: None,
            promoted: false,
            label: 0,
            requires_age_gate: false,
        };
        let cards = vec![
            card("Zed", "plays chess"),
            card("Grand Chess", "x"),
            card("Chess Club", "x"),
            card("Cards", "chess for two"),
            card("Other", "x"),
        ];
        let found = search_cards(&cards, &[], "/Chess");
        let names: Vec<&str> = found.iter().map(|card| card.name.as_str()).collect();
        assert_eq!(names, ["Chess Club", "Grand Chess", "Cards", "Zed"]);
        assert!(search_cards(&cards, &[], "nothing here").is_empty());
        assert_eq!(search_cards(&cards, &[], "  ").len(), 5);
    }

    #[test]
    fn recent_use_orders_inside_a_bucket() {
        let card = |name: &str| ShelfCard {
            app_id: name.to_owned(),
            name: name.to_owned(),
            description: String::new(),
            icon: None,
            banner: None,
            promoted: false,
            label: 0,
            requires_age_gate: false,
        };
        let cards = vec![card("Chess A"), card("Chess B"), card("Chess C")];
        let found = search_cards(
            &cards,
            &["Chess C".to_owned(), "Chess B".to_owned()],
            "chess",
        );
        let names: Vec<&str> = found.iter().map(|card| card.name.as_str()).collect();
        assert_eq!(names, ["Chess C", "Chess B", "Chess A"]);
    }

    #[test]
    fn the_shelf_key_names_the_guild_or_zero() {
        assert_eq!(shelf_key(Some(7)), "activity-shelf:7");
        assert_eq!(shelf_key(None), "activity-shelf:0");
    }

    #[test]
    fn activity_notification_label_697675_modes() {
        assert_eq!(activity_notification_label_text(0), None);
        assert_eq!(activity_notification_label_text(1).as_deref(), Some("NEW"));
        assert_eq!(
            activity_notification_label_text(2).as_deref(),
            Some("UPDATED")
        );
        assert!(activity_notification_label(0, false).is_none());
        assert!(activity_notification_label(1, true).is_some());
        assert!(activity_notification_label(2, false).is_some());
    }
}
