//! Server Settings > Members (module 989433 `ps`, requests module 636496): Show Members In Channel List
//! (`ENABLED_MODERATION_EXPERIENCE_FOR_NON_COMMUNITY` in the server's features, saved at once by `tx`), then Recent
//! Members from `POST /guilds/{id}/members-search`, newest first, with the search by username or id (module 859126's
//! split). A 202 reply means the server is still indexing: the capture's "Before searching..." line shows and the
//! request repeats after `retry_after`. Measured in `design/dom/settings/guild-members-darker-cozy.json`.
//! Sort reorders by `sort` in the same request (module 70738's orders); Prune opens module 749872's dialog: the count
//! from `GET /guilds/{id}/prune`, then `POST` with `compute_prune_count: false`.
//! The Join Method column's filter opens module 311567's `members-table-join-method-menu`: invite code search, All,
//! the server's invites and the Other Join Methods view, which narrow the search by `source_invite_code` /
//! `join_source_type` (module 406318's search state, sent by module 636496).
//! Not drawn yet: the other column filters, row selection, Add Roles, Open Mod View and More Options.

use super::motion::{PopKind, PopSide, Popout as _};
use super::{Root, avatar_source, color, icon};
use crate::editor::{Editor, EditorEvent};
use crate::net::Request;
use dc_state::{Id, User};
use gpui::{
    AnyElement, App, Context, Entity, FontWeight, IntoElement, SharedString, Subscription, div,
    img, prelude::*, px,
};
use serde_json::Value;

pub(super) const MEMBERS_PREFIX: &str = "guild-members-search:";
/// `GET /guilds/{id}/invites` for the join-method menu's invite rows.
const INVITES_PREFIX: &str = "guild-members-search:invites:";
/// `MANAGE_GUILD`: module 920360 only gives the column headers their filters with it.
const MANAGE_GUILD: u64 = 1 << 5;
/// `P` of module 311567: the Other Join Methods view, in its order (Integration and Server Invite are not listed).
const OTHER_JOIN_TYPES: [u64; 6] = [3, 6, 1, 4, 7, 8];
/// The anchor of the Join Method header, under which the menu opens.
const JOIN_HEADER: &str = "server-members-join-method";
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
    /// Module 406318's `selectedSourceInviteCode` / `selectedJoinSourceType`.
    join: JoinFilter,
    /// The join-method menu is open, and on its Other Join Methods view (`h` of module 311567).
    join_open: bool,
    join_other: bool,
    /// The menu's invite code box.
    join_search: Entity<Editor>,
    /// Bumped by each keystroke in the box and by closing the menu, so only the last keystroke's 300ms settle applies
    /// (the debounce is cancelled when the menu unmounts).
    join_typed: u64,
    /// The server's invite codes, newest first.
    invites: Vec<String>,
    _events: Subscription,
    _join_events: Subscription,
}

/// The join filter of module 406318's search state; JS `null` and `undefined` are both `None` (every reader tests
/// `== null`).
#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct JoinFilter {
    code: Option<String>,
    kind: Option<u64>,
}

impl JoinFilter {
    /// `k` of module 311567, an invite row, and the box's settle after 300ms: the code trimmed, none when empty, and
    /// any join type cleared.
    fn invite(code: &str) -> Self {
        let code = code.trim();
        Self {
            code: (!code.is_empty()).then(|| code.to_owned()),
            kind: None,
        }
    }

    /// `O`: the box's text as typed, untrimmed (an empty box still sends `""` until it settles).
    fn typed(text: &str) -> Self {
        Self {
            code: Some(text.to_owned()),
            kind: None,
        }
    }

    /// `D`: a join type row, `None` for All; the type already checked clears both.
    fn pick(&self, kind: Option<u64>) -> Self {
        if kind == self.kind {
            Self::default()
        } else {
            Self { code: None, kind }
        }
    }

    /// The header's filtered look (`A || M` in module 920360): a non-empty code or a type.
    fn filtered(&self) -> bool {
        self.code.as_deref().is_some_and(|code| !code.is_empty()) || self.kind.is_some()
    }

    /// All is checked when neither is set.
    fn all(&self) -> bool {
        self.code.is_none() && self.kind.is_none()
    }

    /// The type shown under Other Join Methods (`C`: set and not `UNSPECIFIED`).
    fn shown_kind(&self) -> Option<u64> {
        self.kind.filter(|kind| *kind != 0)
    }
}

/// The invite codes of `GET /guilds/{id}/invites`, newest `created_at` first as module 311567 sorts them.
fn invite_codes(body: &Value) -> Vec<String> {
    let mut invites: Vec<(i64, String)> = body
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|invite| {
            let code = invite["code"].as_str()?.to_owned();
            let at = invite["created_at"]
                .as_str()
                .and_then(|at| chrono::DateTime::parse_from_rfc3339(at).ok())
                .map_or(0, |at| at.timestamp_millis());
            Some((at, code))
        })
        .collect();
    invites.sort_by_key(|(at, _)| std::cmp::Reverse(*at));
    invites.into_iter().map(|(_, code)| code).collect()
}

/// `CI` of module 11541 with the server's vanity code: the label of a join type; Streamer Mode's Hide Invite Links
/// hides the vanity code.
fn join_type_label(kind: u64, vanity: Option<&str>, hide: bool) -> String {
    let key = match kind {
        1 => "HumZAi",
        2 => "gmCUFw",
        3 => "Ql/e9Y",
        4 => "Op8B3O",
        5 => "/3vIRd",
        6 => match vanity {
            Some(vanity) if !hide => {
                return dc_intl::format("EIUjRy", &[("vanityUrl", vanity.into())]);
            }
            _ => "dGiD1O",
        },
        7 => "vdu7oS",
        8 => "9/ZreX",
        _ => "DvMBkS",
    };
    dc_intl::t(key).to_owned()
}

/// `w` of module 311567: the 16px icon before a join type's label.
fn join_type_icon(kind: u64) -> Option<&'static str> {
    match kind {
        1 => Some("oauth_entry_robot"),
        2 => Some("integrations_webhook"),
        3 => Some("discover"),
        4 => Some("discover_hub"),
        5 | 6 => Some("member_invite_link"),
        // GAP: Manual Verification's icon (module 509402, a pencil in a box) has no asset in app/assets/icons yet.
        7 => None,
        8 => Some("system_game_controller"),
        _ => None,
    }
}

/// A row of the join-method menu (`labelContainer_M3qk4j`, the sort menu's measured 36px rows).
fn join_row(id: SharedString) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .flex_none()
        .min_h(px(36.))
        .p(px(8.))
        .rounded(px(2.))
        .cursor_pointer()
        .hover(|style| {
            style
                .bg(color("--interactive-background-hover"))
                .rounded(px(4.))
        })
        .flex()
        .items_center()
        .justify_between()
        .text_size(px(14.))
        .line_height(px(18.))
        .font_weight(FontWeight(500.))
        .text_color(color("--text-strong"))
}

/// `w`: a join type's icon and label; `labelWithIconContainer_vPFJvh` with the icon's 4px margin.
fn join_label(kind: u64, text: String) -> gpui::Div {
    div()
        .flex_1()
        .flex()
        .items_center()
        .whitespace_nowrap()
        .children(join_type_icon(kind).map(|name| icon(name, 16., "--text-strong").mr(px(4.))))
        .child(text)
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

/// Module 636496's query: the words as usernames, numeric ids as user ids; one character is no query yet. The join
/// filter adds `source_invite_code` and `join_source_type`, each as a one-value `or_query`.
fn search_body(query: &str, sort: u64, join: &JoinFilter) -> Value {
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
    if let Some(code) = &join.code {
        and_query.insert(
            "source_invite_code".into(),
            serde_json::json!({"or_query": [code]}),
        );
    }
    if let Some(kind) = join.kind {
        and_query.insert(
            "join_source_type".into(),
            serde_json::json!({"or_query": [kind]}),
        );
    }
    serde_json::json!({"or_query": {}, "and_query": and_query, "limit": PAGE_SIZE, "sort": sort})
}

impl Root {
    /// Escape: the Prune Members role list, the dialog and the sort list close one at a time (top first).
    pub(super) fn close_members_layer(&mut self) -> bool {
        let Some(state) = self.members_state_mut() else {
            return false;
        };
        if let Some(prune) = state.prune.as_mut() {
            if std::mem::take(&mut prune.roles_open) {
                return true;
            }
            state.prune = None;
            return true;
        }
        if std::mem::take(&mut state.join_open) {
            state.join_typed += 1;
            return true;
        }
        std::mem::take(&mut state.sort_open)
    }

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
        let join_search = cx.new(|cx| {
            let mut editor = Editor::new(super::editor_style(), cx);
            editor.set_placeholder(dc_intl::t("YwJnGz").to_owned());
            editor
        });
        let join_events = cx.subscribe(
            &join_search,
            move |this, editor, event: &EditorEvent, cx| match event {
                EditorEvent::Edited => {
                    let text = editor.read(cx).text().to_owned();
                    this.type_invite_code(guild, text, cx);
                }
                EditorEvent::Cancel => {
                    this.close_join_menu();
                    cx.notify();
                }
                EditorEvent::Submit(text) => {
                    let text = text.clone();
                    editor.update(cx, |editor, _| editor.set_text(&text));
                }
            },
        );
        MembersState {
            search,
            last_body: Value::Null,
            features: None,
            sort: 1,
            sort_open: false,
            prune: None,
            join: JoinFilter::default(),
            join_open: false,
            join_other: false,
            join_search,
            join_typed: 0,
            invites: Vec::new(),
            _events: events,
            _join_events: join_events,
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
        let (sort, join) = self
            .members_state()
            .map_or((1, JoinFilter::default()), |state| {
                (state.sort, state.join.clone())
            });
        let body = search_body(query, sort, &join);
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

    /// The search again with the table's current text, after the join filter changed (`MEMBER_SAFETY_SEARCH_STATE_UPDATE`
    /// sends module 636496's request at once).
    fn research_members(&mut self, guild: Id, cx: &App) {
        let query = self
            .members_state()
            .map(|state| state.search.read(cx).text().to_owned())
            .unwrap_or_default();
        self.search_members(guild, &query);
    }

    /// The menu closes; a pending settle of the box is dropped with it.
    fn close_join_menu(&mut self) {
        if let Some(state) = self.members_state_mut() {
            state.join_open = false;
            state.join_other = false;
            state.join_typed += 1;
        }
    }

    /// The Join Method header: the menu opens on its main view with the box showing the code, and the server's invites
    /// are asked for again (module 311567 fetches them on each mount).
    fn toggle_join_menu(&mut self, guild: Id, cx: &mut Context<Self>) {
        let Some(state) = self.members_state_mut() else {
            return;
        };
        if state.join_open {
            self.close_join_menu();
            cx.notify();
            return;
        }
        state.join_open = true;
        state.join_other = false;
        let code = state.join.code.clone().unwrap_or_default();
        let editor = state.join_search.clone();
        editor.update(cx, |editor, _| editor.set_text(&code));
        let _ = self.requests.send(Request::Fetch {
            key: format!("{INVITES_PREFIX}{guild}"),
            route: format!("/guilds/{guild}/invites"),
        });
        cx.notify();
    }

    /// A row of the menu was picked: the filter is stored and the search goes out. Radio items stay open (module
    /// 165862's `radio` rows get no `onClose`); `D` returns to the main view, and the box follows the stored code.
    fn pick_join_filter(&mut self, guild: Id, filter: JoinFilter, cx: &mut Context<Self>) {
        let Some(state) = self.members_state_mut() else {
            return;
        };
        let shown = filter.code.clone().unwrap_or_default();
        state.join = filter;
        state.join_other = false;
        let editor = state.join_search.clone();
        editor.update(cx, |editor, _| editor.set_text(&shown));
        self.research_members(guild, cx);
        cx.notify();
    }

    /// `O` of module 311567: each keystroke searches with the raw text, and 300ms after the last one the trimmed text
    /// replaces it (searching again only when that changes the filter).
    fn type_invite_code(&mut self, guild: Id, text: String, cx: &mut Context<Self>) {
        let Some(state) = self.members_state_mut() else {
            return;
        };
        state.join = JoinFilter::typed(&text);
        state.join_typed += 1;
        let typed = state.join_typed;
        self.research_members(guild, cx);
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(std::time::Duration::from_millis(300))
                .await;
            let _ = this.update(cx, |this, cx| {
                let Some(state) = this.members_state_mut() else {
                    return;
                };
                if state.join_typed != typed {
                    return;
                }
                let settled = JoinFilter::invite(&text);
                if state.join == settled {
                    return;
                }
                // The box is bound to the stored code (`query: m ?? ""`), so it shows the trimmed text too.
                let shown = settled.code.clone().unwrap_or_default();
                state.join = settled;
                if shown != text {
                    let editor = state.join_search.clone();
                    editor.update(cx, |editor, _| editor.set_text(&shown));
                }
                this.research_members(guild, cx);
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    pub(super) fn members_fetched(&mut self, key: &str, cx: &mut Context<Self>) {
        let Some(rest) = key.strip_prefix(MEMBERS_PREFIX) else {
            return;
        };
        if key.starts_with(INVITES_PREFIX) {
            let codes = self.fetched.get(key).map(invite_codes).unwrap_or_default();
            if let Some(state) = self.members_state_mut() {
                state.invites = codes;
            }
            cx.notify();
            return;
        }
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
                            .child(join_method(entry)),
                    )
                    .into_any_element()
            })
            .collect();
        div()
            .min_w(px(1000.))
            .w_full()
            .flex_none()
            .pl(px(40.))
            .pr(px(16.))
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
                                    .flex_1()
                                    .min_w(px(0.))
                                    .max_w(px(572.))
                                    .mr(px(16.))
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
                                            .child(column(100.).child(self.members_join_header(guild, cx)))
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

    /// The Join Method header (`eM` of module 920360, `tableHeaderCell_JHplLf tableHeaderCellWithFilter_JHplLf` in
    /// `design/dom/settings/guild-members-darker-cozy.json`): the 14/18 medium label, `text-strong` while filtered, and
    /// the 16px filter icon turned 90 degrees, brand while filtered. Without Manage Server it is the bare label.
    pub(super) fn members_join_header(&self, guild: Id, cx: &mut Context<Self>) -> AnyElement {
        let filtered = self
            .members_state()
            .is_some_and(|state| state.join.filtered());
        let label = div()
            .flex_1()
            .min_w(px(0.))
            .text_size(px(14.))
            .line_height(px(18.))
            .font_weight(FontWeight(500.))
            .text_color(color(if filtered {
                "--text-strong"
            } else {
                "--text-default"
            }))
            .child(dc_intl::t("yn0w1+"));
        if !self.my_permission(guild, None, MANAGE_GUILD) {
            return div().flex().child(label).into_any_element();
        }
        div()
            .id(JOIN_HEADER)
            .relative()
            .child(self.anchor_probe(JOIN_HEADER))
            .cursor_pointer()
            .flex()
            .items_center()
            .gap(px(4.))
            .child(label)
            // `filterIconButton_JHplLf`: 16 wide with 2px of padding, the icon overflowing it.
            .child(
                div().size(px(16.)).flex_none().pl(px(2.)).child(
                    icon(
                        "member_filter",
                        16.,
                        if filtered {
                            "--control-brand-foreground"
                        } else {
                            "--text-muted"
                        },
                    )
                    .with_transformation(gpui::Transformation::rotate(gpui::percentage(0.25))),
                ),
            )
            .on_click(cx.listener(move |this, _, _, cx| this.toggle_join_menu(guild, cx)))
            .into_any_element()
    }

    /// Module 311567's `members-table-join-method-menu` ("Server Join Method Options"), under the Join Method header
    /// (position bottom, align left, spacing 4). The main view: the invite code box, All, the server's invites (none under
    /// Streamer Mode's Hide Invite Links) and Other Join Methods with the picked type as its subtext; that row swaps in
    /// the second view: back, then the types of `P`. Picking a row keeps it open.
    pub(super) fn members_join_layer(
        &self,
        window: &gpui::Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let state = self.members_state()?;
        if !state.join_open {
            return None;
        }
        let guild = self.members_guild()?;
        let anchor = self.tip_anchors.borrow().get(JOIN_HEADER).copied()?;
        let hide = self.streamer_hides_invites();
        let vanity = self
            .stores
            .guilds
            .iter()
            .find(|known| known.id == guild)
            .and_then(|known| known.vanity_url_code.clone());
        let join = state.join.clone();
        let separator = || {
            div()
                .my(px(8.))
                .h(px(1.))
                .flex_none()
                .bg(color("--border-subtle"))
                .into_any_element()
        };
        let radio = |checked: bool| super::create_channel::radio(checked).ml(px(8.));
        let other = || dc_intl::t("Kz/cho");
        let mut rows: Vec<AnyElement> = Vec::new();
        if state.join_other {
            rows.push(
                join_row("server-members-join-back".into())
                    .justify_start()
                    .child(icon("chevron_small_left", 20., "--interactive-text-default").mr(px(8.)))
                    .child(div().flex_1().whitespace_nowrap().child(other()))
                    .on_click(cx.listener(|this, _, _, cx| {
                        if let Some(state) = this.members_state_mut() {
                            state.join_other = false;
                        }
                        cx.notify();
                    }))
                    .into_any_element(),
            );
            for kind in OTHER_JOIN_TYPES {
                let filter = join.pick(Some(kind));
                rows.push(
                    join_row(format!("join-source-type-option-{kind}").into())
                        .child(join_label(
                            kind,
                            join_type_label(kind, vanity.as_deref(), hide),
                        ))
                        .child(radio(join.kind == Some(kind)))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.pick_join_filter(guild, filter.clone(), cx)
                        }))
                        .into_any_element(),
                );
            }
        } else {
            // `members-table-invite-code-search`: the table's own small SearchBar.
            // GAP: the menu control item's own padding around the box is not in the CSS or a capture.
            rows.push(
                div()
                    .id("members-table-invite-code-search")
                    .flex_none()
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
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .child(state.join_search.clone()),
                    )
                    .into_any_element(),
            );
            rows.push(separator());
            let all = join.pick(None);
            rows.push(
                join_row("join-source-type-option-all".into())
                    .child(
                        div()
                            .flex_1()
                            .whitespace_nowrap()
                            .child(dc_intl::t("an9Ry3")),
                    )
                    .child(radio(join.all()))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.pick_join_filter(guild, all.clone(), cx)
                    }))
                    .into_any_element(),
            );
            if !hide {
                for code in &state.invites {
                    let filter = JoinFilter::invite(code);
                    rows.push(
                        join_row(format!("join-source-type-option-{code}").into())
                            .child(join_label(5, code.clone()))
                            .child(radio(join.code.as_deref() == Some(code.as_str())))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.pick_join_filter(guild, filter.clone(), cx)
                            }))
                            .into_any_element(),
                    );
                }
            }
            rows.push(separator());
            let subtext = join
                .shown_kind()
                .map(|kind| join_type_label(kind, vanity.as_deref(), hide));
            rows.push(
                join_row("other-join-methods".into())
                    .child(
                        div()
                            .flex_1()
                            .flex()
                            .flex_col()
                            .child(div().whitespace_nowrap().child(other()))
                            .children(subtext.map(|text| {
                                // `subtext_M3qk4j`: 2px under the label, `text-muted`.
                                div()
                                    .mt(px(2.))
                                    .text_size(px(12.))
                                    .line_height(px(16.))
                                    .font_weight(FontWeight(400.))
                                    .text_color(color("--text-muted"))
                                    .child(text)
                            })),
                    )
                    // GAP: the trailing chevron's colour is not measured; the context menu's caret colour is used.
                    .child(
                        icon("inventory_chevron_right", 20., "--interactive-text-default")
                            .ml(px(8.)),
                    )
                    .on_click(cx.listener(|this, _, _, cx| {
                        if let Some(state) = this.members_state_mut() {
                            state.join_other = true;
                        }
                        cx.notify();
                    }))
                    .into_any_element(),
            );
        }
        Some(
            gpui::deferred(
                div()
                    .id("members-table-join-method-menu")
                    .popout(
                        "members-table-join-method-menu",
                        PopKind::Fade,
                        PopSide::Bottom,
                    )
                    .occlude()
                    .absolute()
                    .left(anchor.left())
                    .top(anchor.bottom() + px(4.))
                    // `menu_M3qk4j flexible_M3qk4j`: 188 to 320 wide, as wide as its rows.
                    .min_w(px(188.))
                    .max_w(px(320.))
                    .p(px(8.))
                    .rounded(px(8.))
                    .border_1()
                    .border_color(color("--border-subtle"))
                    .bg(color("--background-surface-higher"))
                    .shadow(vec![gpui::BoxShadow {
                        color: gpui::hsla(0., 0., 0., 0.24),
                        offset: gpui::point(px(0.), px(12.)),
                        blur_radius: px(24.),
                        spread_radius: px(0.),
                        inset: false,
                    }])
                    // GAP: the layer's `--reference-position-layer-max-height` is not measured; the menu stops 16px
                    // (`--custom-menu-viewport-padding`) short of the window's bottom and its rows scroll.
                    .max_h(px((f32::from(window.viewport_size().height)
                        - f32::from(anchor.bottom())
                        - 20.)
                        .max(120.)))
                    .flex()
                    .flex_col()
                    .on_mouse_down_out(cx.listener(|this, event: &gpui::MouseDownEvent, _, cx| {
                        // A press on the header is its own toggle.
                        let on_header = this
                            .tip_anchors
                            .borrow()
                            .get(JOIN_HEADER)
                            .is_some_and(|bounds| bounds.contains(&event.position));
                        if !on_header {
                            this.close_join_menu();
                            cx.notify();
                        }
                    }))
                    .child(
                        div()
                            .id("members-table-join-method-scroll")
                            .min_h(px(0.))
                            .overflow_y_scroll()
                            .flex()
                            .flex_col()
                            .children(rows),
                    ),
            )
            .with_priority(2)
            .into_any_element(),
        )
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
                    .popout("server-members-sort-menu", PopKind::Fade, PopSide::Bottom)
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

/// The Join Method cell (`CI` in module 11541): the member's `join_source_type`, and for a vanity URL its code.
fn join_method(entry: &Value) -> String {
    match entry["join_source_type"].as_u64() {
        Some(1) => "Bot Invite".to_owned(),
        Some(2) => "Integration".to_owned(),
        Some(3) => "Server Discovery".to_owned(),
        Some(4) => "Student Hub".to_owned(),
        Some(5) => "Server Invite".to_owned(),
        Some(6) => match entry["source_invite_code"].as_str() {
            Some(code) => format!("{code} (Vanity)"),
            None => "Vanity URL".to_owned(),
        },
        Some(7) => "Manual Verification".to_owned(),
        Some(8) => "Linked Channel".to_owned(),
        _ => "Unknown".to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn searches_build_module_636496s_query() {
        assert_eq!(from_now(12 * 3600), "12 hrs ago");
        assert_eq!(from_now(3 * 365 * 86400), "3 years ago");
        let none = JoinFilter::default();
        assert_eq!(search_body("", 1, &none)["sort"], 1);
        assert_eq!(
            search_body("", 1, &none)["and_query"],
            serde_json::json!({})
        );
        assert_eq!(
            search_body("a", 1, &none)["and_query"],
            serde_json::json!({})
        );
        assert_eq!(
            search_body("ono", 1, &none)["and_query"]["usernames"]["or_query"],
            serde_json::json!(["ono"])
        );
        assert_eq!(
            search_body("903573240566399026", 1, &none)["and_query"]["user_id"]["or_query"],
            serde_json::json!(["903573240566399026"])
        );
    }

    #[test]
    fn the_join_filter_joins_module_636496s_and_query() {
        use serde_json::json;
        let code = JoinFilter::invite(" abc ");
        assert_eq!(
            search_body("", 1, &code)["and_query"],
            json!({"source_invite_code": {"or_query": ["abc"]}})
        );
        let kind = JoinFilter::default().pick(Some(3));
        assert_eq!(
            search_body("ono", 2, &kind)["and_query"],
            json!({"usernames": {"or_query": ["ono"]}, "join_source_type": {"or_query": [3]}})
        );
        // While typing, the raw text goes out, an empty box as `""`.
        assert_eq!(
            search_body("", 1, &JoinFilter::typed(""))["and_query"],
            json!({"source_invite_code": {"or_query": [""]}})
        );
    }

    #[test]
    fn the_join_menu_rows_set_module_406318s_search_state() {
        let none = JoinFilter::default();
        assert!(none.all() && !none.filtered());
        // A type row sets the type and clears the code; the checked one clears both.
        let typed = JoinFilter::typed("ab");
        let bot = typed.pick(Some(1));
        assert_eq!(
            bot,
            JoinFilter {
                code: None,
                kind: Some(1)
            }
        );
        assert!(bot.filtered() && !bot.all());
        assert_eq!(bot.pick(Some(1)), none);
        // All clears both, whatever was set.
        assert_eq!(bot.pick(None), none);
        assert_eq!(typed.pick(None), none);
        // An invite row or the settled box: the trimmed code, the type cleared; blank is no filter.
        assert_eq!(
            JoinFilter::invite(" gf "),
            JoinFilter {
                code: Some("gf".into()),
                kind: None
            }
        );
        assert_eq!(JoinFilter::invite("   "), none);
        // An empty box is not "filtered" yet, but All is not checked until it settles.
        let empty = JoinFilter::typed("");
        assert!(!empty.filtered() && !empty.all());
        // Other Join Methods' subtext skips UNSPECIFIED.
        assert_eq!(
            JoinFilter {
                code: None,
                kind: Some(0)
            }
            .shown_kind(),
            None
        );
        assert_eq!(bot.shown_kind(), Some(1));
    }

    #[test]
    fn invites_list_newest_first_and_types_read_like_ci() {
        use serde_json::json;
        let body = json!([
            {"code": "old", "created_at": "2024-01-01T00:00:00+00:00"},
            {"code": "new", "created_at": "2025-06-01T12:00:00.000000+00:00"},
            {"code": "mid", "created_at": "2024-08-01T00:00:00+00:00"},
            {"created_at": "2025-01-01T00:00:00+00:00"}
        ]);
        assert_eq!(invite_codes(&body), vec!["new", "mid", "old"]);
        assert!(invite_codes(&json!({"message": "Missing Access"})).is_empty());
        assert_eq!(join_type_label(1, None, false), "Bot Invite");
        assert_eq!(join_type_label(3, None, false), "Server Discovery");
        assert_eq!(join_type_label(6, Some("gf"), false), "gf (Vanity)");
        assert_eq!(join_type_label(6, Some("gf"), true), "Vanity URL");
        assert_eq!(join_type_label(6, None, false), "Vanity URL");
        assert_eq!(join_type_label(8, None, false), "Linked Channel");
        assert_eq!(join_type_label(42, None, false), "Unknown");
        assert_eq!(OTHER_JOIN_TYPES, [3, 6, 1, 4, 7, 8]);
    }

    #[test]
    fn the_join_method_reads_the_join_source() {
        use serde_json::json;
        assert_eq!(
            join_method(&json!({"join_source_type": 5})),
            "Server Invite"
        );
        assert_eq!(
            join_method(&json!({"join_source_type": 6, "source_invite_code": "gf"})),
            "gf (Vanity)"
        );
        assert_eq!(join_method(&json!({"join_source_type": 6})), "Vanity URL");
        assert_eq!(join_method(&json!({})), "Unknown");
    }
}
