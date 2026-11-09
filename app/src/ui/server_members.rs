//! Server Settings > Members (module 989433 `ps`, requests module 636496): Show Members In Channel List
//! (`ENABLED_MODERATION_EXPERIENCE_FOR_NON_COMMUNITY` in the server's features, saved at once by `tx`), then Recent
//! Members from `POST /guilds/{id}/members-search`, newest first, with the search by username or id (module 859126's
//! split). A 202 reply means the server is still indexing: the capture's "Before searching..." line shows and the
//! request repeats after `retry_after`. Measured in `design/dom/settings/guild-members-darker-cozy.json`.
//! Sort reorders by `sort` in the same request (module 70738's orders); Prune opens module 749872's dialog: the count
//! from `GET /guilds/{id}/prune`, then `POST` with `compute_prune_count: false`.
//! Not drawn yet: the column filters, row selection, Add Roles, Open Mod View and More Options.

use super::{Root, avatar_source, color, icon};
use crate::editor::{Editor, EditorEvent};
use crate::net::Request;
use dc_state::{Id, User};
use gpui::{
    AnyElement, Context, Entity, FontWeight, IntoElement, SharedString, Subscription, div, img,
    prelude::*, px,
};
use serde_json::Value;

pub(super) const MEMBERS_PREFIX: &str = "guild-members-search:";
const MODERATION_FEATURE: &str = "ENABLED_MODERATION_EXPERIENCE_FOR_NON_COMMUNITY";
const PAGE_SIZE: u64 = 250;
const PRUNE_PREFIX: &str = "guild-members-search:prune:";
/// The Sort menu (`design/dom/menus/members-sort-darker-cozy.json`) with module 70738's orders.
const SORTS: [(&str, u64); 4] = [
    ("Member Since (Newest first)", 1),
    ("Member Since (Oldest first)", 2),
    ("Joined Discord (Newest first)", 3),
    ("Joined Discord (Oldest first)", 4),
];

pub(crate) struct MemberView {
    pub(super) guild: Id,
    pub(super) state: MembersState,
}

pub(crate) struct MembersState {
    pub(super) search: Entity<Editor>,
    /// The body last asked for, so a late retry or reply for an older one is ignored.
    last_body: Value,
    /// The server's features, for the switch; `None` until read.
    features: Option<Vec<String>>,
    sort: u64,
    sort_open: bool,
    prune: Option<Prune>,
    _events: Subscription,
}

/// The Prune Members dialog.
struct Prune {
    days: u64,
    roles: Vec<Id>,
    roles_open: bool,
    /// The members it would kick, once the server has counted.
    count: Option<u64>,
}

/// Relative times as the table's cells read: "12 hrs ago", "3 years ago", with moment's rounding thresholds.
pub(super) fn from_now(seconds: i64) -> String {
    let seconds = seconds.max(0) as f64;
    let round = |unit: f64| (seconds / unit).round() as i64;
    let (minutes, hours, days) = (round(60.), round(3600.), round(86400.));
    let months = round(86400. * 30.4375);
    let plural = |n: i64, one: &str, many: &str| {
        if n == 1 {
            format!("1 {one} ago")
        } else {
            format!("{n} {many} ago")
        }
    };
    if seconds < 45. {
        "a few seconds ago".to_owned()
    } else if minutes < 45 {
        plural(minutes.max(1), "min", "mins")
    } else if hours < 22 {
        plural(hours.max(1), "hr", "hrs")
    } else if days < 26 {
        plural(days.max(1), "day", "days")
    } else if months < 11 {
        plural(months.max(1), "month", "months")
    } else {
        plural(round(86400. * 365.25).max(1), "year", "years")
    }
}

/// Module 636496's query: the words as usernames, numeric ids as user ids; one character is no query yet.
fn search_body(query: &str, sort: u64) -> Value {
    let mut and_query = serde_json::Map::new();
    if query.chars().count() > 1 {
        let (words, ids) = super::server_settings_pages::split_query(query);
        if !words.is_empty() {
            and_query.insert("usernames".into(), serde_json::json!({"or_query": words}));
        }
        if !ids.is_empty() {
            and_query.insert("user_id".into(), serde_json::json!({"or_query": ids}));
        }
    }
    serde_json::json!({"or_query": {}, "and_query": and_query, "limit": PAGE_SIZE, "sort": sort})
}

impl Root {
    /// The members table's state: the search box and the order, shared by the settings page and the Members view.
    fn new_members_state(guild: Id, cx: &mut Context<Self>) -> MembersState {
        let search = cx.new(|cx| {
            let mut editor = Editor::new(super::editor_style(), cx);
            editor.set_placeholder("Search by username or id".to_owned());
            editor
        });
        let events = cx.subscribe(&search, move |this, editor, event: &EditorEvent, cx| {
            match event {
                EditorEvent::Edited => {
                    let query = editor.read(cx).text().to_owned();
                    this.search_members(guild, &query);
                }
                EditorEvent::Cancel => {
                    if this.server_settings.is_some() {
                        this.server_settings = None;
                    }
                }
                EditorEvent::Submit(text) => {
                    let text = text.clone();
                    editor.update(cx, |editor, _| editor.set_text(&text));
                }
            }
            cx.notify();
        });
        MembersState {
            search,
            last_body: Value::Null,
            features: None,
            sort: 1,
            sort_open: false,
            prune: None,
            _events: events,
        }
    }

    /// The table being driven now: the settings page's while it is open, else the Members view's.
    pub(super) fn members_state(&self) -> Option<&MembersState> {
        match self
            .server_settings
            .as_ref()
            .and_then(|open| open.members.as_ref())
        {
            Some(state) => Some(state),
            None => self.member_view.as_ref().map(|view| &view.state),
        }
    }

    fn members_state_mut(&mut self) -> Option<&mut MembersState> {
        if self
            .server_settings
            .as_ref()
            .is_some_and(|open| open.members.is_some())
        {
            return self
                .server_settings
                .as_mut()
                .and_then(|open| open.members.as_mut());
        }
        self.member_view.as_mut().map(|view| &mut view.state)
    }

    /// The server whose table that is.
    fn members_guild(&self) -> Option<Id> {
        match self.server_settings.as_ref() {
            Some(open) if open.members.is_some() => Some(open.guild),
            _ => self.member_view.as_ref().map(|view| view.guild),
        }
    }

    pub(super) fn request_members_page(&mut self, guild: Id, cx: &mut Context<Self>) {
        if self
            .server_settings
            .as_ref()
            .is_some_and(|open| open.members.is_none())
        {
            let state = Self::new_members_state(guild, cx);
            if let Some(open) = self.server_settings.as_mut() {
                open.members = Some(state);
            }
        }
        self.search_members(guild, "");
        let _ = self.requests.send(Request::Fetch {
            key: format!("{MEMBERS_PREFIX}guild:{guild}"),
            route: format!("/guilds/{guild}"),
        });
    }

    /// The channel list's Members row: the server's Members view replaces the chat.
    pub(super) fn open_member_view(&mut self, guild: Id, cx: &mut Context<Self>) {
        let state = Self::new_members_state(guild, cx);
        self.member_view = Some(MemberView { guild, state });
        self.search_members(guild, "");
        cx.notify();
    }

    fn search_members(&mut self, guild: Id, query: &str) {
        let sort = self.members_state().map_or(1, |state| state.sort);
        let body = search_body(query, sort);
        if let Some(state) = self.members_state_mut() {
            state.last_body = body.clone();
        }
        let _ = self.requests.send(Request::Rest {
            key: Some(format!("{MEMBERS_PREFIX}{guild}")),
            method: reqwest::Method::POST,
            route: format!("/guilds/{guild}/members-search"),
            body: Some(body),
        });
    }

    pub(super) fn members_fetched(&mut self, key: &str, cx: &mut Context<Self>) {
        let Some(rest) = key.strip_prefix(MEMBERS_PREFIX) else {
            return;
        };
        if key.starts_with(PRUNE_PREFIX) {
            let count = self
                .fetched
                .get(key)
                .and_then(|body| body["pruned"].as_u64());
            if let Some(prune) = self
                .members_state_mut()
                .and_then(|state| state.prune.as_mut())
            {
                prune.count = count;
            }
            cx.notify();
            return;
        }
        if let Some(guild) = rest.strip_prefix("guild:") {
            let features: Vec<String> = self
                .fetched
                .get(key)
                .and_then(|body| body["features"].as_array().cloned())
                .unwrap_or_default()
                .into_iter()
                .filter_map(|feature| feature.as_str().map(str::to_owned))
                .collect();
            if guild.parse::<Id>().is_ok()
                && let Some(state) = self.members_state_mut()
            {
                state.features = Some(features);
            }
            cx.notify();
            return;
        }
        let Ok(guild) = rest.parse::<Id>() else {
            return;
        };
        let report = self.fetched.get(key).cloned().unwrap_or_default();
        // Still indexing: ask again after the wait the server names.
        if report["status"] == 202 {
            let wait = report["body"]["retry_after"].as_f64().unwrap_or(5.);
            let body = self.members_state().map(|state| state.last_body.clone());
            cx.spawn(async move |this, cx| {
                cx.background_executor()
                    .timer(std::time::Duration::from_secs_f64(wait))
                    .await;
                let _ = this.update(cx, |this, _| {
                    if let Some(body) = body {
                        let _ = this.requests.send(Request::Rest {
                            key: Some(format!("{MEMBERS_PREFIX}{guild}")),
                            method: reqwest::Method::POST,
                            route: format!("/guilds/{guild}/members-search"),
                            body: Some(body),
                        });
                    }
                });
            })
            .detach();
            cx.notify();
            return;
        }
        let users: Vec<User> = report["body"]["members"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|entry| User::from_value(&entry["member"]["user"]))
            .collect();
        for user in users {
            let (key, url) = avatar_source(&user);
            self.request_image(key, url);
        }
        cx.notify();
    }

    /// `tx`: the feature is added or dropped and saved straight away.
    fn toggle_moderation_list(&mut self, guild: Id, cx: &mut Context<Self>) {
        let Some(state) = self.members_state_mut() else {
            return;
        };
        let Some(features) = state.features.as_mut() else {
            return;
        };
        if let Some(at) = features
            .iter()
            .position(|feature| feature == MODERATION_FEATURE)
        {
            features.remove(at);
        } else {
            features.push(MODERATION_FEATURE.to_owned());
        }
        let body = serde_json::json!({"features": features});
        let _ = self.requests.send(Request::Rest {
            key: None,
            method: reqwest::Method::PATCH,
            route: format!("/guilds/{guild}"),
            body: Some(body),
        });
        cx.notify();
    }

    /// The page, 1000 wide inside its horizontal scroller (`settingsHorizontalScroll`).
    pub(super) fn server_members_page(&self, guild: Id, cx: &mut Context<Self>) -> AnyElement {
        let Some(state) = self.members_state() else {
            return div().into_any_element();
        };
        let report = self.fetched.get(&format!("{MEMBERS_PREFIX}{guild}"));
        let indexing = report.is_none_or(|report| report["status"] == 202);
        let members: Vec<Value> = report
            .and_then(|report| report["body"]["members"].as_array().cloned())
            .unwrap_or_default();
        let total = report
            .and_then(|report| report["body"]["total_result_count"].as_u64())
            .unwrap_or(members.len() as u64);
        let now = chrono::Utc::now().timestamp();
        let on = state
            .features
            .as_ref()
            .is_some_and(|features| features.iter().any(|feature| feature == MODERATION_FEATURE));
        let column = |width: f32| div().w(px(width)).flex_none().px(px(8.));
        let head = |text: &'static str| {
            div()
                .text_size(px(14.))
                .line_height(px(18.))
                .font_weight(FontWeight(600.))
                .text_color(color("--text-strong"))
                .child(text)
        };
        let box_mark = |ticked: bool| {
            div()
                .size(px(20.))
                .rounded(px(4.))
                .border_1()
                .when(ticked, |this| {
                    this.border_color(gpui::rgba(0x97979f1f))
                        .bg(color("--control-primary-background-default"))
                        .opacity(0.5)
                })
                .when(!ticked, |this| {
                    this.border_color(gpui::rgba(0x97979fa3))
                        .bg(gpui::rgba(0x00000014))
                })
                .flex()
                .items_center()
                .justify_center()
                .when(ticked, |this| {
                    this.child(icon("checkbox_check", 18., "--white"))
                })
        };
        let rows: Vec<AnyElement> = members
            .iter()
            .map(|entry| {
                let member = &entry["member"];
                let user = User::from_value(&member["user"]);
                let picture = user
                    .as_ref()
                    .and_then(|user| self.images.get(&avatar_source(user).0).cloned());
                let id = member["user"]["id"].as_str().unwrap_or_default();
                let joined_server = member["joined_at"]
                    .as_str()
                    .and_then(|at| chrono::DateTime::parse_from_rfc3339(at).ok())
                    .map_or(0, |at| at.timestamp());
                let created = id
                    .parse::<u64>()
                    .map_or(0, |id| ((id >> 22) as i64 + 1_420_070_400_000) / 1000);
                let name = user
                    .as_ref()
                    .map(|user| user.display_name().to_owned())
                    .unwrap_or_default();
                let username = user
                    .as_ref()
                    .map(|user| user.username.clone())
                    .unwrap_or_default();
                let cell = |text: String| {
                    div()
                        .text_size(px(14.))
                        .line_height(px(18.))
                        .font_weight(FontWeight(500.))
                        .text_color(color("--text-default"))
                        .child(text)
                };
                div()
                    .id(SharedString::from(format!("server-member-{id}")))
                    .h(px(43.))
                    .rounded(px(4.))
                    .hover(|style| style.bg(color("--background-mod-subtle")))
                    .flex()
                    .items_center()
                    .child(column(36.).child(box_mark(false)))
                    .child(
                        div()
                            .w(px(195.))
                            .flex_none()
                            .pl(px(4.))
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .child(div().size(px(32.)).flex_none().rounded_full().children(
                                picture.map(|picture| img(picture).size(px(32.)).rounded_full()),
                            ))
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
                                            .child(name),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(12.))
                                            .line_height(px(16.))
                                            .font_weight(FontWeight(500.))
                                            .text_color(color("--text-default"))
                                            .child(username),
                                    ),
                            ),
                    )
                    .child(
                        div()
                            .w(px(100.))
                            .flex_none()
                            .pl(px(4.))
                            .child(cell(from_now(now - joined_server))),
                    )
                    .child(
                        div()
                            .w(px(100.))
                            .flex_none()
                            .pl(px(4.))
                            .child(cell(from_now(now - created))),
                    )
                    .child(
                        div()
                            .w(px(100.))
                            .flex_none()
                            .pl(px(4.))
                            .text_size(px(14.))
                            .line_height(px(18.))
                            .font_weight(FontWeight(500.))
                            .text_color(color("--text-muted"))
                            .child("Unknown"),
                    )
                    .into_any_element()
            })
            .collect();
        div()
            .w(px(1000.))
            .flex_none()
            .pl(px(40.))
            .pt(px(60.))
            .pb(px(60.))
            .flex()
            .flex_col()
            .child(
                div()
                    .p(px(8.))
                    .text_size(px(20.))
                    .line_height(px(24.))
                    .font_weight(FontWeight(600.))
                    .text_color(color("--text-strong"))
                    .child("Server Members"),
            )
            .child(
                div()
                    .p(px(8.))
                    .child(
                        div()
                            .mt(px(16.))
                            .flex()
                            .justify_between()
                            .child(
                                div()
                                    .w(px(572.))
                                    .flex()
                                    .flex_col()
                                    .gap(px(4.))
                                    .child(div().text_size(px(16.)).line_height(px(20.)).font_weight(FontWeight(500.)).text_color(color("--text-strong")).child("Show Members In Channel List"))
                                    .child(
                                        div()
                                            .text_size(px(14.))
                                            .line_height(px(18.))
                                            .text_color(color("--text-subtle"))
                                            .child("Enabling this will show the members page in the channel list, allowing you to quickly see who's recently joined your server, and find any users flagged for unusual activity."),
                                    ),
                            )
                            .child(
                                div()
                                    .id("server-members-channel-list")
                                    .cursor_pointer()
                                    .child(super::create_channel::switch(on))
                                    .on_click(cx.listener(move |this, _, _, cx| this.toggle_moderation_list(guild, cx))),
                            ),
                    )
                    .child(
                        div()
                            .mt(px(24.))
                            .rounded(px(12.))
                            .bg(color("--background-base-lower"))
                            .flex()
                            .flex_col()
                            .child(
                                div()
                                    .h(px(48.))
                                    .px(px(12.))
                                    .flex()
                                    .items_center()
                                    .gap(px(8.))
                                    .child(div().flex_1().text_size(px(16.)).line_height(px(20.)).font_weight(FontWeight(500.)).text_color(color("--text-strong")).child("Recent Members"))
                                    .child(
                                        div()
                                            .w(px(250.))
                                            .h(px(32.))
                                            .rounded(px(8.))
                                            .border_1()
                                            .border_color(color("--input-border-default"))
                                            .bg(color("--input-background-default"))
                                            .pl(px(8.))
                                            .pr(px(8.))
                                            .flex()
                                            .items_center()
                                            .gap(px(8.))
                                            .child(icon("picker_search", 14., "--input-text-default"))
                                            .child(div().flex_1().min_w(px(0.)).child(state.search.clone())),
                                    )
                                    .child(self.members_sort_button(cx))
                                    .child(self.members_prune_button(guild, cx)),
                            )
                            .child(
                                div()
                                    .border_t_1()
                                    .border_color(color("--border-subtle"))
                                    .px(px(8.))
                                    .pb(px(8.))
                                    .child(
                                        div()
                                            .h(px(60.))
                                            .flex()
                                            .items_center()
                                            .child(column(36.).child(box_mark(true)))
                                            .child(column(195.).child(head("Name")))
                                            .child(column(100.).child(head("Member Since")))
                                            .child(column(100.).child(head("Joined Discord")))
                                            .child(column(100.).child(head("Join Method")))
                                            .child(column(190.).child(head("Roles")))
                                            .child(column(100.).child(head("Signals"))),
                                    )
                                    .when(indexing, |this| {
                                        this.child(
                                            div()
                                                .h(px(240.))
                                                .flex()
                                                .items_center()
                                                .justify_center()
                                                .text_size(px(16.))
                                                .line_height(px(20.))
                                                .text_color(color("--text-muted"))
                                                .child("Before searching, we need to index this server. Give us a bit."),
                                        )
                                    })
                                    .children(rows),
                            )
                            .when(!indexing, |this| {
                                this.child(
                                    div()
                                        .h(px(44.))
                                        .px(px(16.))
                                        .border_t_1()
                                        .border_color(color("--border-subtle"))
                                        .flex()
                                        .items_center()
                                        .text_size(px(16.))
                                        .line_height(px(20.))
                                        .text_color(color("--text-muted"))
                                        .child("Showing\u{a0}")
                                        .child(div().font_weight(FontWeight(600.)).text_color(color("--text-default")).child(total.to_string()))
                                        .child(if total == 1 { "\u{a0}member" } else { "\u{a0}members" }),
                                )
                            }),
                    ),
            )
            .into_any_element()
    }
}

impl Root {
    /// The Sort button: it opens the menu of `members_sort_layer` under it.
    pub(super) fn members_sort_button(&self, cx: &mut Context<Self>) -> AnyElement {
        div()
            .id("server-members-sort")
            .relative()
            .child(self.anchor_probe("server-members-sort"))
            .h(px(32.))
            .px(px(11.))
            .rounded(px(8.))
            .border_1()
            .border_color(color("--control-secondary-border-default"))
            .bg(color("--control-secondary-background-default"))
            .cursor_pointer()
            .flex()
            .items_center()
            .gap(px(4.))
            .text_size(px(14.))
            .line_height(px(18.))
            .font_weight(FontWeight(500.))
            .text_color(color("--control-secondary-text-default"))
            .child(icon(
                "members_sort",
                16.,
                "--control-secondary-text-default",
            ))
            .child("Sort")
            .on_click(cx.listener(|this, _, _, cx| {
                if let Some(state) = this.members_state_mut() {
                    state.sort_open = !state.sort_open;
                }
                cx.notify();
            }))
            .into_any_element()
    }

    /// The Prune button.
    pub(super) fn members_prune_button(&self, guild: Id, cx: &mut Context<Self>) -> AnyElement {
        div()
            .id("server-members-prune")
            .h(px(32.))
            .px(px(11.))
            .rounded(px(8.))
            .border_1()
            .border_color(color("--control-critical-secondary-border-default"))
            .bg(color("--control-critical-secondary-background-default"))
            .cursor_pointer()
            .flex()
            .items_center()
            .text_size(px(14.))
            .line_height(px(18.))
            .font_weight(FontWeight(500.))
            .text_color(color("--control-critical-secondary-text-default"))
            .child("Prune")
            .on_click(cx.listener(move |this, _, _, cx| this.open_prune(guild, cx)))
            .into_any_element()
    }

    fn request_prune_count(&mut self, guild: Id) {
        let Some(prune) = self.members_state().and_then(|state| state.prune.as_ref()) else {
            return;
        };
        let mut route = format!("/guilds/{guild}/prune?days={}", prune.days);
        for role in &prune.roles {
            route.push_str(&format!("&include_roles={role}"));
        }
        let _ = self.requests.send(Request::Fetch {
            key: format!("{PRUNE_PREFIX}{guild}"),
            route,
        });
    }

    fn open_prune(&mut self, guild: Id, cx: &mut Context<Self>) {
        if let Some(state) = self.members_state_mut() {
            state.prune = Some(Prune {
                days: 7,
                roles: Vec::new(),
                roles_open: false,
                count: None,
            });
        }
        self.request_prune_count(guild);
        cx.notify();
    }

    /// The Sort menu under its button.
    pub(super) fn members_sort_layer(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let state = self.members_state()?;
        if !state.sort_open {
            return None;
        }
        let guild = self.members_guild()?;
        let anchor = self
            .tip_anchors
            .borrow()
            .get("server-members-sort")
            .copied()?;
        let current = state.sort;
        let rows = SORTS.iter().map(|(label, value)| {
            let value = *value;
            div()
                .id(SharedString::from(format!("server-members-sort-{value}")))
                .h(px(36.))
                .px(px(8.))
                .rounded(px(4.))
                .cursor_pointer()
                .hover(|style| style.bg(color("--interactive-background-hover")))
                .flex()
                .items_center()
                .justify_between()
                .gap(px(16.))
                .text_size(px(14.))
                .line_height(px(18.))
                .font_weight(FontWeight(500.))
                .text_color(color("--text-default"))
                .child(*label)
                .child(super::create_channel::radio(current == value).size(px(16.)))
                .on_click(cx.listener(move |this, _, _, cx| {
                    let mut query = String::new();
                    if let Some(state) = this.members_state_mut() {
                        state.sort = value;
                        state.sort_open = false;
                        query = state.search.read(cx).text().to_owned();
                    }
                    this.search_members(guild, &query);
                    cx.notify();
                }))
        });
        Some(
            gpui::deferred(
                div()
                    .id("server-members-sort-menu")
                    .occlude()
                    .absolute()
                    .left(anchor.right() - px(240.))
                    .top(anchor.bottom() + px(8.))
                    .w(px(240.))
                    .p(px(8.))
                    .rounded(px(8.))
                    .border_1()
                    .border_color(color("--border-subtle"))
                    .bg(color("--background-surface-high"))
                    .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                        if let Some(state) = this.members_state_mut() {
                            state.sort_open = false;
                        }
                        cx.notify();
                    }))
                    .children(rows),
            )
            .with_priority(2)
            .into_any_element(),
        )
    }

    /// "Prune Members\u{2014}{server}" (module 749872): Last Seen, roles to include, the count, Cancel and Prune.
    pub(super) fn members_prune_modal(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let prune = self.members_state()?.prune.as_ref()?;
        let guild = self.members_guild()?;
        let known = self.stores.guilds.iter().find(|known| known.id == guild)?;
        let days = prune.days;
        let radio_row =
            |id: &'static str, label: &'static str, value: u64, cx: &mut Context<Self>| {
                div()
                    .id(id)
                    .h(px(36.))
                    .cursor_pointer()
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .child(super::create_channel::radio(days == value))
                    .child(
                        div()
                            .text_size(px(16.))
                            .line_height(px(20.))
                            .text_color(color("--text-default"))
                            .child(label),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if let Some(prune) = this
                            .members_state_mut()
                            .and_then(|state| state.prune.as_mut())
                        {
                            prune.days = value;
                            prune.count = None;
                        }
                        this.request_prune_count(guild);
                        cx.notify();
                    }))
            };
        let label = |text: &'static str| {
            div()
                .mb(px(8.))
                .text_size(px(16.))
                .line_height(px(20.))
                .font_weight(FontWeight(500.))
                .text_color(color("--text-strong"))
                .child(text)
        };
        let count = prune.count;
        let members = match count {
            None => "[calculating...] members".to_owned(),
            Some(1) => "1 member".to_owned(),
            Some(n) => format!("{n} members"),
        };
        let with_roles = !prune.roles.is_empty();
        let (before, after) = (
            "Pruning will kick ",
            if with_roles {
                " and are assigned to just the roles you've selected. Members who are not assigned to any roles are still included. They can rejoin the server using a new invite."
            } else {
                " and are not assigned to any roles. They can rejoin the server using a new invite."
            },
        );
        let span = format!("{days} days");
        let middle = " who have not been seen on Discord in ";
        let sentence = format!("{before}{members}{middle}{span}{after}");
        let first = before.len()..before.len() + members.len();
        let second = first.end + middle.len()..first.end + middle.len() + span.len();
        let bold = gpui::HighlightStyle {
            font_weight: Some(FontWeight(600.)),
            ..Default::default()
        };
        let names: Vec<String> = prune
            .roles
            .iter()
            .filter_map(|role| known.roles.get(role).cloned())
            .collect();
        let shown = if names.is_empty() {
            "Select...".to_owned()
        } else {
            names.join(", ")
        };
        let picked = prune.roles.clone();
        let role_rows: Vec<AnyElement> = if prune.roles_open {
            let mut roles: Vec<(&Id, &String)> =
                known.roles.iter().filter(|(id, _)| **id != guild).collect();
            roles.sort_by_key(|(_, name)| name.to_lowercase());
            roles
                .into_iter()
                .map(|(id, name)| {
                    let id = *id;
                    div()
                        .id(SharedString::from(format!("server-prune-role-{id}")))
                        .flex_none()
                        .h(px(36.))
                        .px(px(8.))
                        .rounded(px(4.))
                        .cursor_pointer()
                        .hover(|style| style.bg(color("--background-mod-normal")))
                        .flex()
                        .items_center()
                        .justify_between()
                        .text_size(px(16.))
                        .line_height(px(20.))
                        .text_color(color("--text-strong"))
                        .child(name.clone())
                        .when(picked.contains(&id), |this| {
                            this.child(icon("select_check", 20., "--text-brand"))
                        })
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if let Some(prune) = this
                                .members_state_mut()
                                .and_then(|state| state.prune.as_mut())
                            {
                                if let Some(at) = prune.roles.iter().position(|known| *known == id)
                                {
                                    prune.roles.remove(at);
                                } else {
                                    prune.roles.push(id);
                                }
                                prune.count = None;
                            }
                            this.request_prune_count(guild);
                            cx.notify();
                        }))
                        .into_any_element()
                })
                .collect()
        } else {
            Vec::new()
        };
        let body = div()
            .flex()
            .flex_col()
            .child(label("Last Seen"))
            .child(radio_row("server-prune-7", "more than 7 days ago", 7, cx))
            .child(radio_row(
                "server-prune-30",
                "more than 30 days ago",
                30,
                cx,
            ))
            .child(
                div()
                    .mt(px(12.))
                    .child(label("Also include members with these roles")),
            )
            .child(
                div()
                    .id("server-prune-roles")
                    .h(px(40.))
                    .pl(px(12.))
                    .rounded(px(8.))
                    .border_1()
                    .border_color(color("--input-border-default"))
                    .bg(color("--input-background-default"))
                    .cursor_pointer()
                    .flex()
                    .items_center()
                    .child(
                        div()
                            .flex_1()
                            .text_size(px(16.))
                            .line_height(px(20.))
                            .text_color(if names.is_empty() {
                                color("--input-placeholder-text-default")
                            } else {
                                color("--text-default")
                            })
                            .child(shown),
                    )
                    .child(div().w(px(36.)).flex().justify_center().child(icon(
                        "select_chevron",
                        20.,
                        "--interactive-text-active",
                    )))
                    .on_click(cx.listener(|this, _, _, cx| {
                        if let Some(prune) = this
                            .members_state_mut()
                            .and_then(|state| state.prune.as_mut())
                        {
                            prune.roles_open = !prune.roles_open;
                        }
                        cx.notify();
                    })),
            )
            .when(!role_rows.is_empty(), |this| {
                this.child(
                    div()
                        .id("server-prune-role-list")
                        .mt(px(4.))
                        .max_h(px(160.))
                        .p(px(4.))
                        .rounded(px(8.))
                        .border_1()
                        .border_color(color("--border-subtle"))
                        .bg(color("--background-surface-higher"))
                        .overflow_y_scroll()
                        .flex()
                        .flex_col()
                        .children(role_rows),
                )
            })
            .child(
                div()
                    .mt(px(16.))
                    .text_size(px(14.))
                    .line_height(px(18.))
                    .font_weight(FontWeight(500.))
                    .text_color(color("--text-default"))
                    .child(
                        gpui::StyledText::new(sentence)
                            .with_highlights([(first, bold), (second, bold)]),
                    ),
            )
            .into_any_element();
        let close = |this: &mut Root| {
            if let Some(state) = this.members_state_mut() {
                state.prune = None;
            }
        };
        let name = known.name.clone();
        Some(super::modal::render(
            super::modal::Spec {
                id: "server-prune-modal",
                title: format!("Prune Members\u{2014}{name}").into(),
                subtitle: None,
                body: Some(body),
                buttons: vec![
                    super::modal::button(
                        "server-prune-cancel",
                        "Cancel",
                        super::modal::Tone::Secondary,
                        move |this, _, _| close(this),
                    ),
                    super::modal::button(
                        "server-prune-confirm",
                        "Prune",
                        super::modal::Tone::Primary,
                        move |this, _, _| {
                            let Some(prune) =
                                this.members_state().and_then(|state| state.prune.as_ref())
                            else {
                                return;
                            };
                            let roles: Vec<String> =
                                prune.roles.iter().map(|role| role.to_string()).collect();
                            let days = prune.days;
                            let _ = this.requests.send(Request::Rest {
                            key: None,
                            method: reqwest::Method::POST,
                            route: format!("/guilds/{guild}/prune"),
                            body: Some(serde_json::json!({"days": days, "compute_prune_count": false, "include_roles": roles})),
                        });
                            close(this);
                        },
                    ),
                ],
                close: Box::new(move |this, _, _| close(this)),
            },
            cx,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn searches_build_module_636496s_query() {
        assert_eq!(from_now(12 * 3600), "12 hrs ago");
        assert_eq!(from_now(3 * 365 * 86400), "3 years ago");
        assert_eq!(search_body("", 1)["sort"], 1);
        assert_eq!(search_body("", 1)["and_query"], serde_json::json!({}));
        assert_eq!(search_body("a", 1)["and_query"], serde_json::json!({}));
        assert_eq!(
            search_body("ono", 1)["and_query"]["usernames"]["or_query"],
            serde_json::json!(["ono"])
        );
        assert_eq!(
            search_body("903573240566399026", 1)["and_query"]["user_id"]["or_query"],
            serde_json::json!(["903573240566399026"])
        );
    }
}
