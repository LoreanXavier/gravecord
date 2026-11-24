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
//! Member Since, Joined Discord, Roles and Signals open their menus of module 920360 (`member_filters.rs`).
//! Not drawn yet: Add Roles, Open Mod View and More Options.

use super::member_filters::FilterMenu;
use super::motion::{PopKind, PopSide, Popout as _};
use super::{Root, avatar_source, color, icon};
use crate::editor::{Editor, EditorEvent};
use crate::net::Request;
use dc_state::{Id, User, permissions};
use gpui::{
    AnyElement, App, Context, Entity, FontWeight, IntoElement, SharedString, Subscription,
    deferred, div, img, prelude::*, px, rgba,
};
use serde_json::Value;
use std::collections::HashSet;

pub(super) const MEMBERS_PREFIX: &str = "guild-members-search:";
/// `GET /guilds/{id}/invites` for the join-method menu's invite rows.
const INVITES_PREFIX: &str = "guild-members-search:invites:";
/// `MANAGE_GUILD`: module 920360 only gives the column headers their filters with it.
pub(super) const MANAGE_GUILD: u64 = 1 << 5;
/// `P` of module 311567: the Other Join Methods view, in its order (Integration and Server Invite are not listed).
const OTHER_JOIN_TYPES: [u64; 6] = [3, 6, 1, 4, 7, 8];
/// The anchor of the Join Method header, under which the menu opens.
const JOIN_HEADER: &str = "server-members-join-method";
const MODERATION_FEATURE: &str = "ENABLED_MODERATION_EXPERIENCE_FOR_NON_COMMUNITY";
const PAGE_SIZE: u64 = 250;
/// Module 749060 / 233693 `MO`: member safety table page-size choices.
pub(super) const PAGE_SIZES: [u64; 4] = [12, 25, 50, 100];
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

/// Module 856768 / 333179: Bulk Ban confirmation dialog state (`POST /guilds/{g}/bulk-ban/v2`).
#[derive(Clone, Debug)]
pub(super) struct BulkBanModal {
    pub(super) delete_seconds: i64,
    pub(super) reason_idx: usize,
    pub(super) submitting: bool,
}

impl Default for BulkBanModal {
    fn default() -> Self {
        Self {
            delete_seconds: 3600,
            reason_idx: 0,
            submitting: false,
        }
    }
}

/// Module 856768 / 333179: message history deletion options (default 1 hour = 3600s).
const BULK_BAN_DELETE_OPTIONS: [(i64, &str); 7] = [
    (0, "Don't Delete Any"),
    (3600, "Previous Hour"),
    (21600, "Previous 6 Hours"),
    (43200, "Previous 12 Hours"),
    (86400, "Previous 24 Hours"),
    (259200, "Previous 3 Days"),
    (604800, "Previous 7 Days"),
];

/// Module 856768 / 333179: bulk ban reason preset keys (`177354.js`).
const BULK_BAN_REASONS: [&str; 4] = ["IrYX1y", "w4Ivys", "8l3W0y", "tamLhT"];

pub(crate) struct MembersState {
    pub(super) search: Entity<Editor>,
    /// The body last asked for, so a late retry or reply for an older one is ignored.
    last_body: Value,
    /// The server's features, for the switch; `None` until read.
    features: Option<Vec<String>>,
    pub(super) sort: u64,
    sort_open: bool,
    prune: Option<Prune>,
    /// Module 406318's `selectedSourceInviteCode` / `selectedJoinSourceType`.
    join: JoinFilter,
    /// The join-method menu is open, and on its Other Join Methods view (`h` of module 311567).
    pub(super) join_open: bool,
    join_other: bool,
    /// Keyboard focus state for `members-table-join-method-menu` (`165862` menu navigation).
    join_focus_idx: Option<usize>,
    join_focus: gpui::FocusHandle,
    /// The menu's invite code box.
    join_search: Entity<Editor>,
    /// Bumped by each keystroke in the box and by closing the menu, so only the last keystroke's 300ms settle applies
    /// (the debounce is cancelled when the menu unmounts).
    join_typed: u64,
    /// The server's invite codes, newest first.
    invites: Vec<String>,
    /// Module 749060 pagination state (`pageSize`, `currentPage`, page-size dropdown, loading).
    pub(super) page_size: u64,
    pub(super) current_page: u64,
    page_size_open: bool,
    loading: bool,
    /// Module 856768 / 587426: selected actionable member IDs for bulk moderation actions.
    pub(super) selected_user_ids: HashSet<Id>,
    /// Module 856768 / 333179: active Bulk Ban confirmation modal.
    pub(super) bulk_ban: Option<BulkBanModal>,
    /// Capture-harness override allowing the bulk action bar to preview even on read-only fixtures.
    #[allow(dead_code)]
    pub(super) bulk_demo_override: bool,
    /// Module 406318's date, role and safety filters, and the column menu open over the table.
    pub(super) filters: super::member_filters::Filters,
    pub(super) filter_menu: Option<super::member_filters::MenuState>,
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
        // Module 509402: Manual Verification icon.
        7 => Some("manual_verification"),
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

/// `R` of module 920360 (`noResultsContainer_ZMTSV1`: a centred column 16px apart, at least 300 tall, 36px in): the
/// empty search's picture (module 636670, 188x90) over "No members match these search results.", or the indexing line
/// alone; `body-md/normal` in `text-muted`.
pub(super) fn no_results_of(report: Option<&Value>, empty: bool) -> Option<AnyElement> {
    let report = report?;
    if report["status"] == 202 {
        Some(no_results(false))
    } else if report["ok"] == true && empty {
        Some(no_results(true))
    } else {
        None
    }
}

fn no_results(empty: bool) -> AnyElement {
    div()
        .w_full()
        .min_h(px(300.))
        .p(px(36.))
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap(px(16.))
        .when(empty, |this| {
            this.child(img("icons/members_no_results.svg").w(px(188.)).h(px(90.)))
        })
        .child(
            div()
                .text_size(px(16.))
                .line_height(px(20.))
                .text_color(color("--text-muted"))
                .child(dc_intl::t(if empty { "wdyR52" } else { "AXPbZr" })),
        )
        .into_any_element()
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
fn search_body(
    query: &str,
    sort: u64,
    join: &JoinFilter,
    filters: &super::member_filters::Filters,
    now_ms: i64,
) -> Value {
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
    let mut or_query = serde_json::Map::new();
    filters.apply(&mut and_query, &mut or_query, now_ms);
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
    serde_json::json!({"or_query": or_query, "and_query": and_query, "limit": PAGE_SIZE, "sort": sort})
}

impl Root {
    /// Escape: Bulk Ban, Prune Members role list, the dialog and the sort/page-size lists close one at a time (top first).
    pub(super) fn close_members_layer(&mut self) -> bool {
        let Some(state) = self.members_state_mut() else {
            return false;
        };
        if state.bulk_ban.take().is_some() {
            return true;
        }
        if let Some(prune) = state.prune.as_mut() {
            if std::mem::take(&mut prune.roles_open) {
                return true;
            }
            state.prune = None;
            return true;
        }
        if state.filter_menu.take().is_some() {
            return true;
        }
        if std::mem::take(&mut state.join_open) {
            state.join_typed += 1;
            return true;
        }
        if std::mem::take(&mut state.page_size_open) {
            return true;
        }
        std::mem::take(&mut state.sort_open)
    }

    /// The members table's state: the search box and the order, shared by the settings page and the Members view.
    fn new_members_state(guild: Id, cx: &mut Context<Self>) -> MembersState {
        let search = cx.new(|cx| {
            // `body-sm/normal` in the small search bar.
            let mut style = super::editor_style();
            style.size = 14.;
            style.line_height = 18.;
            style.text = super::hsla("--input-text-default");
            style.caret = super::hsla("--input-text-default");
            style.placeholder = super::hsla("--input-placeholder-text-default");
            let mut editor = Editor::new(style, cx);
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
            join_focus_idx: None,
            join_focus: cx.focus_handle(),
            join_search,
            join_typed: 0,
            invites: Vec::new(),
            page_size: PAGE_SIZES[0],
            current_page: 1,
            page_size_open: false,
            loading: false,
            selected_user_ids: HashSet::new(),
            bulk_ban: None,
            bulk_demo_override: false,
            filters: Default::default(),
            filter_menu: None,
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

    pub(super) fn members_state_mut(&mut self) -> Option<&mut MembersState> {
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
    pub(super) fn members_guild(&self) -> Option<Id> {
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
        let (sort, join, page_size, filters) = self.members_state().map_or_else(
            || (1, JoinFilter::default(), PAGE_SIZES[0], Default::default()),
            |state| {
                (
                    state.sort,
                    state.join.clone(),
                    state.page_size,
                    state.filters.clone(),
                )
            },
        );
        let now = chrono::Utc::now().timestamp_millis();
        let mut body = search_body(query, sort, &join, &filters, now);
        // Module 636496: chunk size = `Math.max(5 * pageSize, 250)`.
        body["limit"] = serde_json::json!((page_size.saturating_mul(5)).max(PAGE_SIZE));
        if let Some(state) = self.members_state_mut() {
            state.last_body = body.clone();
            state.loading = true;
            state.current_page = 1;
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
    pub(super) fn research_members(&mut self, guild: Id, cx: &App) {
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
        state.filter_menu = None;
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
        if let Some(state) = self.members_state_mut() {
            state.loading = false;
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
        // `R` of module 920360 in the table body: `SUCCESS_STILL_INDEXING` says the server is being indexed,
        // `SUCCESS_EMPTY` (a search that found nobody) shows the picture over its line.
        let members: Vec<Value> = report
            .and_then(|report| report["body"]["members"].as_array().cloned())
            .unwrap_or_default();
        let no_results = no_results_of(report, members.is_empty());
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
        let box_mark = |ticked: bool, disabled: bool| {
            div()
                .size(px(20.))
                .rounded(px(4.))
                .border_1()
                .when(ticked, |this| {
                    this.border_color(color("--control-primary-background-default"))
                        .bg(color("--control-primary-background-default"))
                })
                .when(!ticked, |this| {
                    this.border_color(gpui::rgba(0x97979fa3))
                        .bg(gpui::rgba(0x00000014))
                })
                .when(disabled, |this| this.opacity(0.45))
                .flex()
                .items_center()
                .justify_center()
                .when(ticked, |this| {
                    this.child(icon("checkbox_check", 16., "--white"))
                })
        };
        let owner = self.stores.guild(guild).and_then(|g| g.owner_id);
        let me = self.stores.me().map(|u| u.id);
        let page_size = state.page_size.max(1) as usize;
        let skip = (state.current_page.saturating_sub(1) as usize).saturating_mul(page_size);
        let visible_actionable: Vec<Id> = members
            .iter()
            .skip(skip)
            .take(page_size)
            .filter_map(|entry| entry["member"]["user"]["id"].as_str()?.parse::<Id>().ok())
            .filter(|uid| Some(*uid) != owner && Some(*uid) != me)
            .collect();
        let all_visible_selected = !visible_actionable.is_empty()
            && visible_actionable
                .iter()
                .all(|uid| state.selected_user_ids.contains(uid));
        let rows: Vec<AnyElement> = members
            .iter()
            .skip(skip)
            .take(page_size)
            .map(|entry| {
                let member = &entry["member"];
                let user = User::from_value(&member["user"]);
                let picture = user
                    .as_ref()
                    .and_then(|user| self.images.get(&avatar_source(user).0).cloned());
                let id = member["user"]["id"].as_str().unwrap_or_default();
                let uid_opt = id.parse::<Id>().ok();
                let actionable = uid_opt.is_some_and(|uid| Some(uid) != owner && Some(uid) != me);
                let checked = uid_opt.is_some_and(|uid| state.selected_user_ids.contains(&uid));
                let joined_server = member["joined_at"]
                    .as_str()
                    .and_then(|at| chrono::DateTime::parse_from_rfc3339(at).ok())
                    .map_or(0, |at| at.timestamp());
                let created = id
                    .parse::<u64>()
                    .map_or(0, |id| ((id >> 22) as i64 + 1_420_070_400_000) / 1000);
                let nick = member["nick"]
                    .as_str()
                    .filter(|s| !s.is_empty())
                    .map(str::to_owned);
                let name = nick.unwrap_or_else(|| {
                    user.as_ref()
                        .map(|user| self.concealed_display_name(user).0)
                        .unwrap_or_default()
                });
                let username = user
                    .as_ref()
                    .map(|user| self.profile_username(user).0)
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
                    .child(
                        column(36.).child(
                            div()
                                .id(SharedString::from(format!("server-member-chk-{id}")))
                                .when(actionable, |d| d.cursor_pointer())
                                .child(box_mark(checked, !actionable))
                                .when(actionable, |d| {
                                    d.on_click(cx.listener(move |this, _, _, cx| {
                                        if let Some(uid) = uid_opt
                                            && let Some(st) = this.members_state_mut()
                                        {
                                            if !st.selected_user_ids.remove(&uid) {
                                                st.selected_user_ids.insert(uid);
                                            }
                                            cx.notify();
                                        }
                                    }))
                                }),
                        ),
                    )
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
        // `contentColumnDefault_jnk11m` at 1000: 40px in on the left only (`header_mg3aTC` and `tableContainer_mg3aTC`
        // are 960 wide from there).
        div()
            .min_w(px(1000.))
            .w_full()
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
                                            // `sm_0PLWhS`: the 22px icon box 4px in (11px glass), the input 4px after it.
                                            .pl(px(4.))
                                            .flex()
                                            .items_center()
                                            .gap(px(4.))
                                            .child(
                                                div()
                                                    .size(px(22.))
                                                    .flex_none()
                                                    .flex()
                                                    .items_center()
                                                    .justify_center()
                                                    .child(icon("picker_search", 11., "--input-text-default")),
                                            )
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
                                        // `tableHeaderCellContainer_JHplLf`: 8px above and 16px below the cells.
                                        div()
                                            .h(px(60.))
                                            .pt(px(8.))
                                            .pb(px(16.))
                                            .flex()
                                            .items_center()
                                            .child(
                                                column(36.).child(
                                                    div()
                                                        .id("server-members-select-all")
                                                        .when(!visible_actionable.is_empty(), |d| {
                                                            d.cursor_pointer()
                                                        })
                                                        .child(box_mark(
                                                            all_visible_selected,
                                                            visible_actionable.is_empty(),
                                                        ))
                                                        .on_click(cx.listener(move |this, _, _, cx| {
                                                            let Some(st) = this.members_state_mut()
                                                            else {
                                                                return;
                                                            };
                                                            if all_visible_selected {
                                                                for uid in &visible_actionable {
                                                                    st.selected_user_ids.remove(uid);
                                                                }
                                                            } else {
                                                                for uid in &visible_actionable {
                                                                    st.selected_user_ids.insert(*uid);
                                                                }
                                                            }
                                                            cx.notify();
                                                        })),
                                                ),
                                            )
                                            .child(column(195.).child(head("Name")))
                                            .child(column(100.).child(self.members_filter_header(guild, FilterMenu::Joined, "xcKP1P", cx)))
                                            .child(column(100.).child(self.members_filter_header(guild, FilterMenu::Account, "sPph4O", cx)))
                                            .child(column(100.).child(self.members_join_header(guild, cx)))
                                            .child(column(190.).child(self.members_filter_header(guild, FilterMenu::Roles, "2SZsWX", cx)))
                                            .child(column(100.).child(self.members_filter_header(guild, FilterMenu::Signals, "7V3759", cx))),
                                    )
                                    .children(no_results)
                                    .children(rows),
                            )
                            .when(!indexing, |this| {
                                this.child(self.member_safety_pagination_footer(guild, total, cx))
                            }),
                    ),
            )
            .children(self.member_safety_bulk_bar(guild, cx))
            .children(self.member_safety_bulk_ban_modal(guild, cx))
            .into_any_element()
    }

    /// Module 856768 / `134413.vA`: bulk moderation requires `MANAGE_GUILD` + `BAN_MEMBERS` (or `ADMINISTRATOR`).
    pub(super) fn can_bulk_ban(&self, guild: Id) -> bool {
        let Some((known, me)) = self.stores.guild(guild).zip(self.stores.me()) else {
            return false;
        };
        let perms = permissions::guild_permissions(known, me);
        let is_admin = perms & permissions::ADMINISTRATOR == permissions::ADMINISTRATOR;
        let has_manage_and_ban = perms & permissions::MANAGE_GUILD == permissions::MANAGE_GUILD
            && perms & permissions::BAN_MEMBERS == permissions::BAN_MEMBERS;
        is_admin || has_manage_and_ban
    }

    /// Module 856768 (`MemberSafetyPageActionNotice`): floating 660px bottom notice bar when ≥1
    /// member is selected and user has bulk-ban permission (`TstoSZ`, `yW6ZdE`, `2a50fF`).
    pub(super) fn member_safety_bulk_bar(
        &self,
        guild: Id,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let state = self.members_state()?;
        let count = state.selected_user_ids.len();
        if count == 0 || (!state.bulk_demo_override && !self.can_bulk_ban(guild)) {
            return None;
        }
        let selected_label = dc_intl::format("TstoSZ", &[("count", (count as f64).into())]);
        let bar = div()
            .id("member-safety-bulk-bar")
            .occlude()
            .w(px(660.))
            .py(px(10.))
            .pl(px(16.))
            .pr(px(10.))
            .rounded(px(8.))
            .bg(color("--background-surface-highest"))
            .border_1()
            .border_color(color("--border-subtle"))
            .shadow(vec![gpui::BoxShadow {
                color: gpui::hsla(0., 0., 0., 0.35),
                offset: gpui::point(px(0.), px(8.)),
                blur_radius: px(24.),
                spread_radius: px(0.),
                inset: false,
            }])
            .flex()
            .items_center()
            .justify_between()
            .gap(px(12.))
            .child(
                div()
                    .max_w(px(432.))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(icon("members_nav", 22., "--text-muted"))
                    .child(
                        div()
                            .text_size(px(14.))
                            .line_height(px(18.))
                            .font_weight(FontWeight(600.))
                            .text_color(color("--text-default"))
                            .child(SharedString::from(selected_label)),
                    )
                    .child(
                        div()
                            .id("member-safety-bulk-clear")
                            .cursor_pointer()
                            .text_size(px(14.))
                            .line_height(px(18.))
                            .font_weight(FontWeight(500.))
                            .text_color(color("--text-link"))
                            .child(dc_intl::t("yW6ZdE"))
                            .on_click(cx.listener(|this, _, _, cx| {
                                if let Some(st) = this.members_state_mut() {
                                    st.selected_user_ids.clear();
                                    st.bulk_ban = None;
                                    st.bulk_demo_override = false;
                                }
                                cx.notify();
                            })),
                    ),
            )
            .child(
                div()
                    .id("member-safety-bulk-ban")
                    .h(px(36.))
                    .px(px(14.))
                    .rounded(px(8.))
                    .bg(color("--control-critical-primary-background-default"))
                    .hover(|s| s.bg(color("--control-critical-primary-background-hover")))
                    .cursor_pointer()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(icon("oauth_entry_hammer", 18., "--white"))
                    .child(
                        div()
                            .text_size(px(14.))
                            .line_height(px(18.))
                            .font_weight(FontWeight(600.))
                            .text_color(color("--white"))
                            .child(dc_intl::t("2a50fF")),
                    )
                    .on_click(cx.listener(|this, _, _, cx| {
                        if let Some(st) = this.members_state_mut() {
                            st.bulk_ban = Some(BulkBanModal::default());
                        }
                        cx.notify();
                    })),
            );
        Some(
            deferred(
                div()
                    .id("member-safety-bulk-bar-anchor")
                    .absolute()
                    .bottom(px(20.))
                    .left_0()
                    .right_0()
                    .flex()
                    .justify_center()
                    .child(bar),
            )
            .with_priority(3)
            .into_any_element(),
        )
    }

    /// Module 856768 / 333179: Bulk Ban confirm modal (`HVJexf`, `POST /guilds/{g}/bulk-ban/v2`).
    pub(super) fn member_safety_bulk_ban_modal(
        &self,
        guild: Id,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let state = self.members_state()?;
        let modal = state.bulk_ban.as_ref()?;
        let count = state.selected_user_ids.len();
        if count == 0 {
            return None;
        }
        let title = dc_intl::format("HVJexf", &[("count", (count as f64).into())]);
        let delete_seconds = modal.delete_seconds;
        let reason_idx = modal.reason_idx;
        let submitting = modal.submitting;
        let card = div()
            .id("member-safety-bulk-ban-modal")
            .occlude()
            .w(px(440.))
            .rounded(px(12.))
            .border_1()
            .border_color(color("--border-subtle"))
            .bg(color("--background-surface-high"))
            .p(px(24.))
            .flex()
            .flex_col()
            .gap(px(14.))
            .on_click(|_, _, cx| cx.stop_propagation())
            .child(
                div()
                    .text_size(px(20.))
                    .line_height(px(24.))
                    .font_weight(FontWeight(700.))
                    .text_color(color("--text-strong"))
                    .child(SharedString::from(title)),
            )
            .child(div().flex().flex_col().gap(px(6.)).children(
                BULK_BAN_REASONS.iter().enumerate().map(|(idx, &key)| {
                    let active = idx == reason_idx;
                    div()
                        .id(SharedString::from(format!("bulk-ban-reason-{idx}")))
                        .px(px(12.))
                        .py(px(8.))
                        .rounded(px(8.))
                        .bg(if active {
                            color("--background-mod-strong")
                        } else {
                            color("--background-base-lower")
                        })
                        .border_1()
                        .border_color(if active {
                            color("--control-primary-background-default")
                        } else {
                            color("--border-subtle")
                        })
                        .cursor_pointer()
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .text_size(px(14.))
                        .text_color(color("--text-strong"))
                        .child(dc_intl::t(key))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if let Some(m) =
                                this.members_state_mut().and_then(|s| s.bulk_ban.as_mut())
                            {
                                m.reason_idx = idx;
                            }
                            cx.notify();
                        }))
                }),
            ))
            .child(
                div().flex().flex_wrap().gap(px(6.)).children(
                    BULK_BAN_DELETE_OPTIONS
                        .iter()
                        .enumerate()
                        .map(|(idx, &(secs, label))| {
                            let active = secs == delete_seconds;
                            div()
                                .id(SharedString::from(format!("bulk-ban-del-{idx}")))
                                .px(px(10.))
                                .h(px(28.))
                                .rounded(px(6.))
                                .bg(if active {
                                    color("--control-primary-background-default")
                                } else {
                                    color("--background-mod-subtle")
                                })
                                .cursor_pointer()
                                .flex()
                                .items_center()
                                .text_size(px(12.))
                                .font_weight(FontWeight(500.))
                                .text_color(if active {
                                    color("--white")
                                } else {
                                    color("--text-default")
                                })
                                .child(label)
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    if let Some(m) =
                                        this.members_state_mut().and_then(|s| s.bulk_ban.as_mut())
                                    {
                                        m.delete_seconds = secs;
                                    }
                                    cx.notify();
                                }))
                        }),
                ),
            )
            .child(
                div()
                    .flex()
                    .justify_end()
                    .gap(px(8.))
                    .mt(px(4.))
                    .child(
                        div()
                            .id("bulk-ban-cancel")
                            .h(px(36.))
                            .px(px(16.))
                            .rounded(px(8.))
                            .bg(color("--control-secondary-background-default"))
                            .hover(|s| s.bg(color("--control-secondary-background-hover")))
                            .cursor_pointer()
                            .flex()
                            .items_center()
                            .text_size(px(14.))
                            .font_weight(FontWeight(600.))
                            .text_color(color("--control-secondary-text-default"))
                            .child(dc_intl::t("ETE/oC"))
                            .on_click(cx.listener(|this, _, _, cx| {
                                if let Some(st) = this.members_state_mut() {
                                    st.bulk_ban = None;
                                }
                                cx.notify();
                            })),
                    )
                    .child(
                        div()
                            .id("bulk-ban-confirm")
                            .h(px(36.))
                            .px(px(16.))
                            .rounded(px(8.))
                            .bg(color("--control-critical-primary-background-default"))
                            .when(!submitting, |d| {
                                d.hover(|s| {
                                    s.bg(color("--control-critical-primary-background-hover"))
                                })
                                .cursor_pointer()
                            })
                            .when(submitting, |d| d.opacity(0.6))
                            .flex()
                            .items_center()
                            .text_size(px(14.))
                            .font_weight(FontWeight(600.))
                            .text_color(color("--white"))
                            .child(dc_intl::t("5MBJ5M"))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                let Some(st) = this.members_state_mut() else {
                                    return;
                                };
                                let ids: Vec<Id> = st.selected_user_ids.iter().copied().collect();
                                let reason_key = BULK_BAN_REASONS
                                    .get(reason_idx)
                                    .copied()
                                    .unwrap_or("IrYX1y");
                                let reason = dc_intl::t(reason_key);
                                st.selected_user_ids.clear();
                                st.bulk_ban = None;
                                st.bulk_demo_override = false;
                                let _ =
                                    this.requests.send(super::member_actions::bulk_ban_request(
                                        guild,
                                        &ids,
                                        delete_seconds,
                                        reason,
                                        format!("guild-bulk-ban:{guild}"),
                                    ));
                                cx.notify();
                            })),
                    ),
            );
        Some(
            deferred(
                div()
                    .id("member-safety-bulk-ban-backdrop")
                    .occlude()
                    .absolute()
                    .inset_0()
                    .bg(rgba(0x000000b3))
                    .flex()
                    .items_center()
                    .justify_center()
                    .on_click(cx.listener(|this, _, _, cx| {
                        if let Some(st) = this.members_state_mut() {
                            st.bulk_ban = None;
                        }
                        cx.notify();
                    }))
                    .child(card),
            )
            .with_priority(95)
            .into_any_element(),
        )
    }

    /// Capture-harness helper for module 856768 (`member-bulk-bar [open|modal|close]`).
    #[cfg(feature = "capture")]
    pub(crate) fn script_member_bulk_bar(&mut self, arg: &str, cx: &mut Context<Self>) -> String {
        let mode = arg.trim();
        if mode == "close" {
            if let Some(st) = self.members_state_mut() {
                st.selected_user_ids.clear();
                st.bulk_ban = None;
                st.bulk_demo_override = false;
            }
            cx.notify();
            return "member-bulk-bar: closed".to_owned();
        }
        let guild = self
            .guild
            .or_else(|| self.stores.guilds.first().map(|g| g.id))
            .unwrap_or(100);
        self.open_member_view(guild, cx);
        if let Some(st) = self.members_state_mut() {
            st.bulk_demo_override = true;
            st.selected_user_ids.insert(10001);
            st.selected_user_ids.insert(10002);
            st.selected_user_ids.insert(10003);
            st.bulk_ban = (mode == "modal").then(BulkBanModal::default);
        }
        cx.notify();
        format!("member-bulk-bar: {mode}")
    }
}

/// Module 749060 / 233693: recalculates current page when `pageSize` changes:
/// `Math.max(Math.ceil(oldPageSize / newPageSize * currentPage), 1)`.
pub(super) fn recalculate_page_for_size(old_size: u64, new_size: u64, current_page: u64) -> u64 {
    let old = old_size.max(1) as f64;
    let new = new_size.max(1) as f64;
    let cur = current_page.max(1) as f64;
    ((old / new) * cur).ceil().max(1.0) as u64
}

/// Module 749060 / 662203 `Paginator` (`maxVisiblePages: 7`, `disablePaginationGap: true`, `hideMaxPage: true`):
/// returns up to `max_visible` contiguous 1-based page numbers centered around `current_page`.
pub(super) fn visible_page_window(
    current_page: u64,
    total_pages: u64,
    max_visible: u64,
) -> Vec<u64> {
    if total_pages == 0 || max_visible == 0 {
        return Vec::new();
    }
    let count = total_pages.min(max_visible);
    let cur = current_page.clamp(1, total_pages);
    let half = count / 2;
    let start = cur
        .saturating_sub(half)
        .max(1)
        .min(total_pages.saturating_sub(count) + 1);
    (start..start + count).collect()
}

impl Root {
    /// Module 749060 (`paginationContainer_nkD26S`): shared Member Safety & Server Settings Members
    /// table pagination footer with `GZpwME` (`<= 12`), or `jNwLu2` + `[12, 25, 50, 100]` selector +
    /// `RNDnQ/` (`members of {count}` / `"..."` + `ZTNur7` loading) + right-aligned round-14px `Paginator`.
    pub(super) fn member_safety_pagination_footer(
        &self,
        guild: Id,
        total: u64,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let (page_size, current_page, page_size_open, loading) =
            self.members_state()
                .map_or((PAGE_SIZES[0], 1, false, false), |s| {
                    (
                        s.page_size.max(1),
                        s.current_page.max(1),
                        s.page_size_open,
                        s.loading,
                    )
                });
        let total_pages = total.max(1).div_ceil(page_size);
        let cur = current_page.min(total_pages);
        let show_selector = total > PAGE_SIZES[0];
        let show_paginator = total > page_size || loading;

        let left: AnyElement = if !show_selector {
            let text = dc_intl::t!("GZpwME", count = total).replace("**", "");
            div()
                .flex()
                .items_center()
                .py(px(12.))
                .text_size(px(16.))
                .line_height(px(20.))
                .text_color(color("--text-muted"))
                .child(text)
                .into_any_element()
        } else {
            let count_str = if loading {
                "...".to_owned()
            } else {
                total.to_string()
            };
            let members_of =
                dc_intl::format("RNDnQ/", &[("count", dc_intl::Arg::Str(count_str.into()))])
                    .replace("**", "");
            let selector = div()
                .id("member-safety-page-size")
                .relative()
                .flex_none()
                .min_w(px(72.))
                .h(px(32.))
                .px(px(10.))
                .rounded(px(8.))
                .border_1()
                .border_color(color("--input-border-default"))
                .bg(color("--input-background-default"))
                .cursor_pointer()
                .flex()
                .items_center()
                .justify_between()
                .gap(px(6.))
                .text_size(px(14.))
                .line_height(px(18.))
                .text_color(color("--text-default"))
                .child(page_size.to_string())
                .child(icon("chevron_down", 12., "--icon-subtle"))
                .on_click(cx.listener(|this, _, _, cx| {
                    if let Some(state) = this.members_state_mut() {
                        state.page_size_open = !state.page_size_open;
                    }
                    cx.notify();
                }))
                .when(page_size_open, |el| {
                    el.child(
                        gpui::deferred(
                            div()
                                .id("member-safety-page-size-menu")
                                .occlude()
                                .absolute()
                                .bottom(px(36.))
                                .left(px(0.))
                                .w(px(96.))
                                .p(px(4.))
                                .rounded(px(8.))
                                .border_1()
                                .border_color(color("--border-subtle"))
                                .bg(color("--background-surface-higher"))
                                .shadow(vec![super::shadow_high()])
                                .on_click(|_, _, cx| cx.stop_propagation())
                                .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                                    if let Some(state) = this.members_state_mut() {
                                        state.page_size_open = false;
                                        cx.notify();
                                    }
                                }))
                                .flex()
                                .flex_col()
                                .gap(px(2.))
                                .children(PAGE_SIZES.into_iter().map(|sz| {
                                    let selected = sz == page_size;
                                    div()
                                        .id(SharedString::from(format!("member-page-size-{sz}")))
                                        .h(px(28.))
                                        .px(px(8.))
                                        .rounded(px(4.))
                                        .cursor_pointer()
                                        .when(selected, |r| r.bg(color("--background-mod-subtle")))
                                        .hover(|s| s.bg(color("--background-mod-subtle")))
                                        .flex()
                                        .items_center()
                                        .justify_between()
                                        .text_size(px(14.))
                                        .text_color(color("--text-default"))
                                        .child(sz.to_string())
                                        .when(selected, |r| {
                                            r.child(icon(
                                                "circle_check",
                                                14.,
                                                "--control-primary-background-default",
                                            ))
                                        })
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            cx.stop_propagation();
                                            let mut changed = false;
                                            if let Some(state) = this.members_state_mut() {
                                                if state.page_size != sz {
                                                    let old = state.page_size;
                                                    state.current_page = recalculate_page_for_size(
                                                        old,
                                                        sz,
                                                        state.current_page,
                                                    );
                                                    state.page_size = sz;
                                                    changed = true;
                                                }
                                                state.page_size_open = false;
                                            }
                                            if changed {
                                                let next_page = this
                                                    .members_state()
                                                    .map_or(1, |s| s.current_page);
                                                this.research_members(guild, cx);
                                                if let Some(state) = this.members_state_mut() {
                                                    state.current_page = next_page;
                                                }
                                            }
                                            cx.notify();
                                        }))
                                })),
                        )
                        .with_priority(3),
                    )
                });
            div()
                .flex()
                .items_center()
                .gap(px(8.))
                .py(px(8.))
                .child(
                    div()
                        .text_size(px(16.))
                        .line_height(px(20.))
                        .text_color(color("--text-muted"))
                        .child(dc_intl::t("jNwLu2")),
                )
                .child(selector)
                .child(
                    div()
                        .text_size(px(16.))
                        .line_height(px(20.))
                        .text_color(color("--text-muted"))
                        .child(if loading {
                            format!("{members_of} ({})", dc_intl::t("ZTNur7"))
                        } else {
                            members_of
                        }),
                )
                .into_any_element()
        };

        let pages = visible_page_window(cur, total_pages, 7);
        let paginator: Option<AnyElement> = show_paginator.then(|| {
            let can_back = cur > 1;
            let can_next = cur < total_pages;
            div()
                .flex_1()
                .flex()
                .items_center()
                .justify_end()
                .gap(px(4.))
                .child(
                    div()
                        .id("member-page-prev")
                        .px(px(8.))
                        .h(px(28.))
                        .rounded(px(14.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_size(px(13.))
                        .font_weight(FontWeight(600.))
                        .text_color(color(if can_back {
                            "--text-strong"
                        } else {
                            "--text-muted"
                        }))
                        .when(can_back, |b| {
                            b.cursor_pointer()
                                .hover(|s| s.bg(color("--background-mod-subtle")))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    if let Some(state) = this.members_state_mut() {
                                        state.current_page =
                                            state.current_page.saturating_sub(1).max(1);
                                    }
                                    cx.notify();
                                }))
                        })
                        .child("Back"),
                )
                .children(pages.into_iter().map(|page| {
                    let active = page == cur;
                    div()
                        .id(SharedString::from(format!("member-page-btn-{page}")))
                        .min_w(px(28.))
                        .h(px(28.))
                        .px(px(6.))
                        .rounded(px(14.))
                        .cursor_pointer()
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_size(px(13.))
                        .font_weight(FontWeight(600.))
                        .when(active, |b| {
                            b.bg(color("--brand-500")).text_color(color("--white"))
                        })
                        .when(!active, |b| {
                            b.text_color(color("--text-strong"))
                                .hover(|s| s.bg(color("--background-mod-subtle")))
                        })
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if let Some(state) = this.members_state_mut() {
                                state.current_page = page;
                            }
                            cx.notify();
                        }))
                        .child(page.to_string())
                }))
                .child(
                    div()
                        .id("member-page-next")
                        .px(px(8.))
                        .h(px(28.))
                        .rounded(px(14.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_size(px(13.))
                        .font_weight(FontWeight(600.))
                        .text_color(color(if can_next {
                            "--text-strong"
                        } else {
                            "--text-muted"
                        }))
                        .when(can_next, |b| {
                            b.cursor_pointer()
                                .hover(|s| s.bg(color("--background-mod-subtle")))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    if let Some(state) = this.members_state_mut() {
                                        state.current_page =
                                            (state.current_page + 1).min(total_pages);
                                    }
                                    cx.notify();
                                }))
                        })
                        .child("Next"),
                )
                .into_any_element()
        });

        div()
            .min_h(px(48.))
            .px(px(16.))
            .border_t_1()
            .border_color(color("--border-subtle"))
            .flex()
            .items_center()
            .gap(px(12.))
            .child(left)
            .children(paginator)
            .into_any_element()
    }

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

    /// Module 311567 / 165862 keyboard navigation for the Join Method menu rows.
    fn members_join_key_down(
        &mut self,
        guild: Id,
        event: &gpui::KeyDownEvent,
        cx: &mut Context<Self>,
    ) {
        let hide = self.streamer_hides_invites();
        let (other, count, cur, invites, join) = {
            let Some(state) = self.members_state() else {
                return;
            };
            let count = if state.join_other {
                1 + OTHER_JOIN_TYPES.len()
            } else {
                let inv_count = if hide { 0 } else { state.invites.len() };
                1 + inv_count + 1
            };
            (
                state.join_other,
                count,
                state.join_focus_idx,
                if hide {
                    Vec::new()
                } else {
                    state.invites.clone()
                },
                state.join.clone(),
            )
        };
        match event.keystroke.key.as_str() {
            "escape" => {
                self.close_join_menu();
                cx.stop_propagation();
                cx.notify();
            }
            "left" | "backspace" if other => {
                if let Some(state) = self.members_state_mut() {
                    state.join_other = false;
                    state.join_focus_idx = None;
                }
                cx.stop_propagation();
                cx.notify();
            }
            "down" => {
                if let Some(state) = self.members_state_mut() {
                    let next = cur.map_or(0, |i| (i + 1) % count.max(1));
                    state.join_focus_idx = Some(next);
                }
                cx.stop_propagation();
                cx.notify();
            }
            "up" => {
                if let Some(state) = self.members_state_mut() {
                    let next = cur.map_or(count.saturating_sub(1), |i| {
                        if i == 0 {
                            count.saturating_sub(1)
                        } else {
                            i - 1
                        }
                    });
                    state.join_focus_idx = Some(next);
                }
                cx.stop_propagation();
                cx.notify();
            }
            "enter" | "space" => {
                let Some(idx) = cur else {
                    return;
                };
                if other {
                    if idx == 0 {
                        if let Some(state) = self.members_state_mut() {
                            state.join_other = false;
                            state.join_focus_idx = None;
                        }
                        cx.notify();
                    } else if let Some(&kind) = OTHER_JOIN_TYPES.get(idx - 1) {
                        self.pick_join_filter(guild, join.pick(Some(kind)), cx);
                    }
                } else if idx == 0 {
                    self.pick_join_filter(guild, join.pick(None), cx);
                } else if idx <= invites.len() {
                    let filter = JoinFilter::invite(&invites[idx - 1]);
                    self.pick_join_filter(guild, filter, cx);
                } else if let Some(state) = self.members_state_mut() {
                    state.join_other = true;
                    state.join_focus_idx = Some(0);
                    cx.notify();
                }
                cx.stop_propagation();
            }
            _ => {}
        }
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
        let focused_idx = state.join_focus_idx;
        let focus_handle = state.join_focus.clone();
        let highlight = |row: gpui::Stateful<gpui::Div>, idx: usize| {
            row.when(focused_idx == Some(idx), |d| {
                d.bg(color("--interactive-background-hover"))
                    .rounded(px(4.))
            })
        };
        let mut rows: Vec<AnyElement> = Vec::new();
        if state.join_other {
            rows.push(
                highlight(join_row("server-members-join-back".into()), 0)
                    .justify_start()
                    .child(icon("chevron_small_left", 20., "--interactive-text-default").mr(px(8.)))
                    .child(div().flex_1().whitespace_nowrap().child(other()))
                    .on_click(cx.listener(|this, _, _, cx| {
                        if let Some(state) = this.members_state_mut() {
                            state.join_other = false;
                            state.join_focus_idx = None;
                        }
                        cx.notify();
                    }))
                    .into_any_element(),
            );
            for (i, kind) in OTHER_JOIN_TYPES.into_iter().enumerate() {
                let filter = join.pick(Some(kind));
                rows.push(
                    highlight(
                        join_row(format!("join-source-type-option-{kind}").into()),
                        i + 1,
                    )
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
            let mut nav_idx: usize = 0;
            let all = join.pick(None);
            rows.push(
                highlight(join_row("join-source-type-option-all".into()), nav_idx)
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
            nav_idx += 1;
            if !hide {
                for code in &state.invites {
                    let filter = JoinFilter::invite(code);
                    rows.push(
                        highlight(
                            join_row(format!("join-source-type-option-{code}").into()),
                            nav_idx,
                        )
                        .child(join_label(5, code.clone()))
                        .child(radio(join.code.as_deref() == Some(code.as_str())))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.pick_join_filter(guild, filter.clone(), cx)
                        }))
                        .into_any_element(),
                    );
                    nav_idx += 1;
                }
            }
            rows.push(separator());
            let subtext = join
                .shown_kind()
                .map(|kind| join_type_label(kind, vanity.as_deref(), hide));
            rows.push(
                highlight(join_row("other-join-methods".into()), nav_idx)
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
                            state.join_focus_idx = Some(0);
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
                    .aria_label(dc_intl::t("u/7Rdc"))
                    .track_focus(&focus_handle)
                    .on_key_down(cx.listener(move |this, event: &gpui::KeyDownEvent, _, cx| {
                        this.members_join_key_down(guild, event, cx);
                    }))
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
                    .on_mouse_down_out(cx.listener(
                        move |this, event: &gpui::MouseDownEvent, _, cx| {
                            if !anchor.contains(&event.position)
                                && let Some(state) = this.members_state_mut()
                            {
                                state.sort_open = false;
                                cx.notify();
                            }
                        },
                    ))
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
                    .relative()
                    .child(
                        div()
                            .id("server-prune-roles")
                            .relative()
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
                            .child(self.anchor_probe("server-prune-roles"))
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
                            gpui::deferred(
                                div()
                                    .id("server-prune-role-list")
                                    .occlude()
                                    .absolute()
                                    .left_0()
                                    .right_0()
                                    .top(px(44.))
                                    .max_h(px(160.))
                                    .p(px(4.))
                                    .rounded(px(8.))
                                    .border_1()
                                    .border_color(color("--border-subtle"))
                                    .bg(color("--background-surface-higher"))
                                    .shadow(vec![super::shadow_high()])
                                    .overflow_y_scroll()
                                    .flex()
                                    .flex_col()
                                    .on_click(|_, _, cx| cx.stop_propagation())
                                    .on_mouse_down_out(cx.listener(
                                        |this, event: &gpui::MouseDownEvent, _, cx| {
                                            let on_trigger = this
                                                .tip_anchors
                                                .borrow()
                                                .get("server-prune-roles")
                                                .is_some_and(|b| b.contains(&event.position));
                                            if !on_trigger
                                                && let Some(prune) = this
                                                    .members_state_mut()
                                                    .and_then(|state| state.prune.as_mut())
                                            {
                                                prune.roles_open = false;
                                                cx.notify();
                                            }
                                        },
                                    ))
                                    .children(role_rows),
                            )
                            .with_priority(99),
                        )
                    }),
            )
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

impl Root {
    pub(super) fn close_server_prune_role_select(&mut self) -> bool {
        if let Some(prune) = self
            .members_state_mut()
            .and_then(|state| state.prune.as_mut())
            && prune.roles_open
        {
            prune.roles_open = false;
            true
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `R` of module 920360 only for a still-indexing reply or a successful empty one; nothing while loading or after
    /// an error.
    #[test]
    fn the_empty_state_follows_the_search_state() {
        let ok = serde_json::json!({"ok": true, "status": 200});
        let indexing = serde_json::json!({"ok": true, "status": 202});
        let failed = serde_json::json!({"ok": false, "status": 500});
        assert!(no_results_of(None, true).is_none());
        assert!(no_results_of(Some(&ok), true).is_some());
        assert!(no_results_of(Some(&ok), false).is_none());
        assert!(no_results_of(Some(&indexing), false).is_some());
        assert!(no_results_of(Some(&failed), true).is_none());
    }

    /// The column filters join the text and the join method in one body: flags under `or_query`, the rest under
    /// `and_query`, the account age merged into the id search.
    #[test]
    fn column_filters_join_the_search_body() {
        let filters = super::super::member_filters::Filters {
            roles: vec![7],
            quarantined: true,
            account: super::super::member_filters::DateFilter {
                option: 3,
                after: Some(1_420_070_400_000 + 2),
                before: None,
            },
            ..Default::default()
        };
        let body = search_body(
            "903573240566399026",
            1,
            &JoinFilter::invite("abc"),
            &filters,
            50,
        );
        assert_eq!(
            body["or_query"],
            serde_json::json!({"safety_signals": {"automod_quarantined_username": true}})
        );
        assert_eq!(
            body["and_query"],
            serde_json::json!({
                "user_id": {"or_query": ["903573240566399026"], "range": {"gte": (2u64 << 22).to_string()}},
                "role_ids": {"and_query": ["7"]},
                "source_invite_code": {"or_query": ["abc"]},
            })
        );
    }

    #[test]
    fn searches_build_module_636496s_query() {
        assert_eq!(from_now(12 * 3600), "12 hrs ago");
        assert_eq!(from_now(3 * 365 * 86400), "3 years ago");
        let none = JoinFilter::default();
        assert_eq!(search_body("", 1, &none, &Default::default(), 0)["sort"], 1);
        assert_eq!(
            search_body("", 1, &none, &Default::default(), 0)["and_query"],
            serde_json::json!({})
        );
        assert_eq!(
            search_body("a", 1, &none, &Default::default(), 0)["and_query"],
            serde_json::json!({})
        );
        assert_eq!(
            search_body("ono", 1, &none, &Default::default(), 0)["and_query"]["usernames"]["or_query"],
            serde_json::json!(["ono"])
        );
        assert_eq!(
            search_body("903573240566399026", 1, &none, &Default::default(), 0)["and_query"]["user_id"]
                ["or_query"],
            serde_json::json!(["903573240566399026"])
        );
    }

    #[test]
    fn the_join_filter_joins_module_636496s_and_query() {
        use serde_json::json;
        let code = JoinFilter::invite(" abc ");
        assert_eq!(
            search_body("", 1, &code, &Default::default(), 0)["and_query"],
            json!({"source_invite_code": {"or_query": ["abc"]}})
        );
        let kind = JoinFilter::default().pick(Some(3));
        assert_eq!(
            search_body("ono", 2, &kind, &Default::default(), 0)["and_query"],
            json!({"usernames": {"or_query": ["ono"]}, "join_source_type": {"or_query": [3]}})
        );
        // While typing, the raw text goes out, an empty box as `""`.
        assert_eq!(
            search_body("", 1, &JoinFilter::typed(""), &Default::default(), 0)["and_query"],
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

    #[test]
    fn member_safety_pagination_footer_math_749060() {
        assert_eq!(PAGE_SIZES, [12, 25, 50, 100]);
        // Module 233693 formula: Math.max(Math.ceil(oldPageSize / newPageSize * currentPage), 1).
        assert_eq!(recalculate_page_for_size(12, 25, 1), 1);
        assert_eq!(recalculate_page_for_size(12, 25, 3), 2);
        assert_eq!(recalculate_page_for_size(50, 25, 2), 4);
        assert_eq!(recalculate_page_for_size(100, 12, 1), 9);

        // Max 7 visible pages centered around currentPage.
        assert_eq!(visible_page_window(1, 3, 7), vec![1, 2, 3]);
        assert_eq!(visible_page_window(1, 20, 7), vec![1, 2, 3, 4, 5, 6, 7]);
        assert_eq!(
            visible_page_window(10, 20, 7),
            vec![7, 8, 9, 10, 11, 12, 13]
        );
        assert_eq!(
            visible_page_window(20, 20, 7),
            vec![14, 15, 16, 17, 18, 19, 20]
        );

        for key in ["jNwLu2", "RNDnQ/", "ZTNur7", "GZpwME"] {
            assert!(!dc_intl::t(key).is_empty(), "missing intl key {key}");
        }
    }

    #[test]
    fn member_safety_bulk_bar_856768_request_and_intl() {
        for key in ["TstoSZ", "yW6ZdE", "2a50fF", "HVJexf", "ETE/oC", "5MBJ5M"] {
            assert!(dc_intl::has_key(key), "missing 856768 key {key}");
        }
        let one = dc_intl::format("TstoSZ", &[("count", 1.0.into())]);
        let many = dc_intl::format("TstoSZ", &[("count", 3.0.into())]);
        assert!(one.contains("1"), "got {one}");
        assert!(many.contains("3"), "got {many}");
        let ban_title = dc_intl::format("HVJexf", &[("count", 3.0.into())]);
        assert!(ban_title.contains("3"), "got {ban_title}");
        let req = super::super::member_actions::bulk_ban_request(
            77,
            &[101, 202],
            3600,
            "Raid accounts",
            "bulk:77".to_owned(),
        );
        let Request::Rest {
            method,
            route,
            body,
            ..
        } = req
        else {
            panic!("expected Rest");
        };
        assert_eq!(method, reqwest::Method::POST);
        assert!(route.starts_with("/guilds/77/bulk-ban/v2"));
        assert_eq!(body.as_ref().unwrap()["delete_message_seconds"], 3600);
        assert_eq!(
            body.as_ref().unwrap()["user_ids"],
            serde_json::json!(["101", "202"])
        );
    }
}
