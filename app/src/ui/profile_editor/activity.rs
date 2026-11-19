//! The editor's Activity tab: module 927071's `sc` for the account's own profile. A "Recent activity" heading with the
//! intro line ("Learn more" linked) over one card per entry of the content-inventory outbox (`rQ` of module 637865).
//! Measured in `design/dom/settings/profile-editor-activity-tab-darker-cozy.json`.

use super::FETCH_PREFIX;
use crate::ui::Root;
use gpui::{AnyElement, IntoElement, div};
use serde_json::Value;

/// `ContentInventoryEntryType`: a played game, a listening session and a launched activity.
const PLAYED_GAME: u64 = 1;
const LISTENED_SESSION: u64 = 5;
const LAUNCHED_ACTIVITY: u64 = 8;

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
                let games: Vec<String> = drawn(&body)
                    .into_iter()
                    .filter_map(|entry| {
                        entry["extra"]["application_id"].as_str().map(str::to_owned)
                    })
                    .collect();
                for game in &games {
                    self.request_game_record(game);
                }
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
                if let Some(me) = self.stores.current_user.as_ref().map(|me| me.id) {
                    self.fetched.insert(format!("apps:{me}"), body.clone());
                }
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

    /// The tab: module 927071's `sc` as the full profile draws it (`profile_activity_panel`): Current
    /// activity, then Recent activity with its intro line, the cards of 637865, or the empty state of `sa`.
    pub(super) fn profile_editor_activity_tab(
        &self,
        palette: &crate::ui::dm_profile::Palette,
        cx: &mut gpui::Context<Self>,
    ) -> AnyElement {
        let Some(me) = self.stores.current_user.as_ref().map(|me| me.id) else {
            return div().into_any_element();
        };
        let name = self
            .stores
            .current_user
            .as_ref()
            .map(|me| me.display_name().to_owned())
            .unwrap_or_default();
        let outbox = self.fetched.get(&format!("{FETCH_PREFIX}outbox")).cloned();
        self.profile_activity_panel(me, true, outbox.as_ref(), &name, palette, 0., cx)
    }
}
