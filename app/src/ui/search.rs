//! Message search: the header field, its Filters popout, and the results panel, measured in
//! `design/dom/popouts/search-focus-darker-cozy.json` and `channel/search-results-darker-cozy.json`, requests as
//! `TRAFFIC/search.json` records: `GET /guilds/{id}/messages/search?content=…&sort_by=timestamp&sort_order=desc
//! &offset=0` (or `/channels/{id}/messages/search` in a DM). The panel (`searchResultsWrap_QHVbp3`, 418 wide) groups
//! results by channel, highlights the searched words, and opens a result in its channel on click. The filter rows put
//! their operator into the field; `from:`, `mentions:`, `in:` and `has:` are sent as their query parameters.

use super::{Root, avatar_source, color, editor_style, icon};
use crate::editor::{Editor, EditorEvent};
use crate::net::Request;
use dc_state::{Id, Message};
use gpui::{
    AnyElement, Context, Entity, FontWeight, IntoElement, SharedString, StyledText, Subscription,
    TextRun, Window, deferred, div, img, prelude::*, px,
};

pub(super) const RESULTS_KEY: &str = "search-results";
/// `rgba(253, 184, 51, 0.3)` behind matched words in the capture.
const HIGHLIGHT: u32 = 0xfdb8334d;

pub(crate) struct Search {
    pub field: Entity<Editor>,
    _events: Subscription,
    pub focused: bool,
    /// The query the panel shows results for.
    pub shown: Option<String>,
    pub oldest_first: bool,
    /// The method and route of the last search request, for the harness.
    pub last_route: Option<String>,
    /// The offset of the page shown (`getSearchResultsOffset`): 25 times the page index.
    pub offset: u64,
    /// While a page of the same query and sort loads, the count of the answer before it (module 477654 keeps it
    /// under `searchResultsPaginationKey`, `{mode}:{query}`), so the pager stays.
    pub carried_total: Option<u64>,
    /// The page number field a gap of the pager opened.
    pub jump: Option<super::search_pager::Jump>,
    /// The results scroller, back at the top for each page.
    pub scroll: gpui::ScrollHandle,
    /// SearchQueryStore `showBlockedResults`: the results of blocked and ignored people are shown too.
    pub show_blocked: bool,
}

/// The line under the results that hides or shows those of blocked and ignored people (module 36544): what is hidden
/// while they are, and what to hide while they are shown.
/// Byte ranges of `text` where any of `needles` occurs, compared lower-cased, sorted and merged. Lower-casing can
/// change a character's byte length (Turkish `İ` grows from 2 to 3 bytes, the Kelvin sign `K` shrinks from 3 to 1),
/// so offsets found in `text.to_lowercase()` are not offsets of `text`. The search runs on a lower-cased copy that
/// remembers, for each of its bytes, the character of `text` it came from: every range starts and ends on one of
/// `text`'s character boundaries, as a text run needs.
pub(super) fn match_ranges(text: &str, needles: &[&str]) -> Vec<std::ops::Range<usize>> {
    let mut lower = String::with_capacity(text.len());
    let mut origin: Vec<std::ops::Range<usize>> = Vec::with_capacity(text.len());
    for (at, character) in text.char_indices() {
        let span = at..at + character.len_utf8();
        for low in character.to_lowercase() {
            lower.push(low);
            origin.extend(std::iter::repeat_n(span.clone(), low.len_utf8()));
        }
    }
    let mut found: Vec<std::ops::Range<usize>> = Vec::new();
    for needle in needles {
        let needle = needle.to_lowercase();
        if needle.is_empty() {
            continue;
        }
        for (start, matched) in lower.match_indices(needle.as_str()) {
            found.push(origin[start].start..origin[start + matched.len() - 1].end);
        }
    }
    found.sort_by_key(|range| range.start);
    let mut merged: Vec<std::ops::Range<usize>> = Vec::with_capacity(found.len());
    for range in found {
        match merged.last_mut() {
            Some(last) if range.start <= last.end => last.end = last.end.max(range.end),
            _ => merged.push(range),
        }
    }
    merged
}

pub(super) fn blocked_results_text(blocked: u64, ignored: u64, shown: bool) -> String {
    let both = blocked + ignored;
    match (shown, blocked > 0, ignored > 0) {
        (true, true, true) => dc_intl::t!("OvJs9w", count = both),
        (true, true, false) => dc_intl::t!("n/1QFS", count = blocked),
        (true, false, _) => dc_intl::t!("ypezTA", count = ignored),
        (false, true, true) => dc_intl::t!("EJHRcV", count = both),
        (false, true, false) => dc_intl::t!("HTE8JP", count = blocked),
        (false, false, _) => dc_intl::t!("e7f8r9", count = ignored),
    }
}

/// The count of an answer, either search's.
pub(super) fn total_of(fetched: &serde_json::Value) -> Option<u64> {
    results_body(fetched)["total_results"].as_u64()
}

/// The words of the query outside operators, which the results highlight.
pub(crate) fn plain_words(query: &str) -> Vec<String> {
    query
        .split_whitespace()
        .filter(|word| !word.contains(':'))
        .map(str::to_lowercase)
        .collect()
}

/// `from:name in:channel has:link mentions:name` and the rest as `content`.
pub(crate) fn operators(query: &str) -> (Vec<(&'static str, String)>, String) {
    let mut found = Vec::new();
    let mut content = Vec::new();
    for word in query.split_whitespace() {
        match word.split_once(':') {
            Some(("from", value)) if !value.is_empty() => found.push(("from", value.to_owned())),
            Some(("mentions", value)) if !value.is_empty() => {
                found.push(("mentions", value.to_owned()))
            }
            Some(("in", value)) if !value.is_empty() => {
                found.push(("in", value.trim_start_matches('#').to_owned()))
            }
            Some(("has", value)) if !value.is_empty() => found.push(("has", value.to_owned())),
            Some((key, value)) if !value.is_empty() => match key.to_lowercase().as_str() {
                "authortype" => found.push(("author_type", value.to_lowercase())),
                "pinned" => found.push(("pinned", value.to_lowercase())),
                "before" => found.push(("before", value.to_owned())),
                "after" => found.push(("after", value.to_owned())),
                "during" => found.push(("during", value.to_owned())),
                _ => content.push(word),
            },
            _ => content.push(word),
        }
    }
    (found, content.join(" "))
}

/// "By default, searching in DMs..." (`isCrossDmSearchEnabled`, settings proto `text_and_images.is_cross_dm_search_enabled`,
/// off by default): its second option searches across all DMs.
const CROSS_DM_SEARCH: &str = "message_search_default_dm_search_behavior";
/// `tc619d`.
const ALL_DMS_SUBTITLE: &str = "in all Direct Messages";
/// `SEARCH_TABS_DMS`.
const DMS_TABS_ROUTE: &str = "/users/@me/messages/search/tabs";
/// `getLimit` of the DM search (module 753806).
const TABS_LIMIT: u64 = 25;

/// `SearchFetchManager.createRequestPayload` (module 956467) for the messages tab: the query's fields under
/// `tabs.messages` with the sort, the offset and the limit, `track_exact_total_hits` set. Parsed queries (module 822382
/// `Zf`) hold `content` as one string, `pinned` as one boolean, `min_id`/`max_id` as ids, and every other field as a
/// list.
fn tabs_payload(params: &[String]) -> serde_json::Value {
    use serde_json::{Map, Value, json};
    let mut tab = Map::new();
    let mut content = Vec::new();
    for param in params {
        let Some((key, value)) = param.split_once('=') else {
            continue;
        };
        let value = decode(value);
        match key {
            "content" => content.push(value),
            "sort_by" | "sort_order" | "min_id" | "max_id" => {
                tab.insert(key.to_owned(), Value::String(value));
            }
            "offset" => {
                tab.insert(key.to_owned(), json!(value.parse::<u64>().unwrap_or(0)));
            }
            "pinned" => {
                tab.insert(key.to_owned(), Value::Bool(value == "true"));
            }
            _ => {
                let list = tab
                    .entry(key.to_owned())
                    .or_insert_with(|| Value::Array(Vec::new()));
                if let Value::Array(list) = list {
                    list.push(Value::String(value));
                }
            }
        }
    }
    let content = content.join(" ").trim().to_owned();
    if !content.is_empty() {
        tab.insert("content".to_owned(), Value::String(content));
    }
    tab.insert("limit".to_owned(), json!(TABS_LIMIT));
    json!({"tabs": {"messages": tab}, "track_exact_total_hits": true})
}

/// The reverse of `encode`.
fn decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%'
            && let Some(byte) = text
                .get(index + 1..index + 3)
                .and_then(|hex| u8::from_str_radix(hex, 16).ok())
        {
            out.push(byte);
            index += 3;
            continue;
        }
        out.push(bytes[index]);
        index += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// The results of either search: a GET's body, or the messages tab of the tabs search's answer (`body.tabs.messages`,
/// which carries `total_results`, `messages`, `threads` as the GET's body does).
fn results_body(fetched: &serde_json::Value) -> &serde_json::Value {
    let body = if fetched.get("ok").is_some() && fetched.get("status").is_some() {
        &fetched["body"]
    } else {
        fetched
    };
    match body.get("tabs") {
        Some(tabs) => &tabs["messages"],
        None => body,
    }
}

pub(super) fn encode(text: &str) -> String {
    text.bytes()
        .map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (byte as char).to_string()
            }
            _ => format!("%{byte:02X}"),
        })
        .collect()
}

impl Root {
    /// The search context of module 775427 `J`: in a DM it is `{type: DMS}` (every DM) when the setting is on, else the
    /// DM itself.
    pub(super) fn search_all_dms(&self) -> bool {
        self.guild.is_none()
            && self.current_dm().is_some()
            && self.pref_index(CROSS_DM_SEARCH, 0) == 1
    }

    /// Capture only: `search last` names the last request; `search answer <total>` answers it as the server would
    /// (no messages), in the shape of the route it went to.
    #[cfg(feature = "capture")]
    pub(crate) fn script_search(&mut self, words: &[&str]) -> String {
        match words {
            ["last"] => self
                .search
                .as_ref()
                .and_then(|search| search.last_route.clone())
                .unwrap_or_else(|| "no search".to_owned()),
            ["answer", total] => {
                let total = total.parse::<u64>().unwrap_or(0);
                let tabs = self
                    .search
                    .as_ref()
                    .and_then(|search| search.last_route.as_deref())
                    .is_some_and(|route| route.starts_with("POST"));
                let body = serde_json::json!({"total_results": total, "messages": []});
                let answer = if tabs {
                    serde_json::json!({"ok": true, "status": 200, "body": {"tabs": {"messages": body}}})
                } else {
                    body
                };
                self.fetched.insert(RESULTS_KEY.to_owned(), answer);
                format!("answered with {total}")
            }
            _ => "usage: search last | search answer <total>".to_owned(),
        }
    }

    fn ensure_search(&mut self, cx: &mut Context<Self>) -> Entity<Editor> {
        if let Some(search) = self.search.as_ref() {
            return search.field.clone();
        }
        let field = cx.new(|cx| {
            let mut style = editor_style();
            style.size = 14.;
            style.line_height = 22.;
            style.font.weight = FontWeight(500.);
            style.placeholder = color("--input-placeholder-text-default").into();
            let mut field = Editor::new(style, cx);
            field.set_ellipsis_placeholder(true);
            field
        });
        let events = cx.subscribe(&field, |this, editor, event: &EditorEvent, cx| {
            match event {
                EditorEvent::Submit(text) => {
                    editor.update(cx, |editor, _| editor.set_text(text));
                    this.run_search(text.clone(), cx);
                }
                EditorEvent::Cancel => this.clear_search(cx),
                EditorEvent::Edited => {}
            }
            cx.notify();
        });
        self.search = Some(Search {
            field: field.clone(),
            _events: events,
            focused: false,
            shown: None,
            oldest_first: false,
            last_route: None,
            offset: 0,
            carried_total: None,
            jump: None,
            scroll: gpui::ScrollHandle::new(),
            show_blocked: false,
        });
        field
    }

    pub(super) fn search_field(
        &mut self,
        placeholder: String,
        cx: &mut Context<Self>,
    ) -> Entity<Editor> {
        let field = self.ensure_search(cx);
        field.update(cx, |editor, _| editor.set_placeholder(placeholder));
        field
    }

    pub(super) fn focus_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let field = self.ensure_search(cx);
        window.focus(&gpui::Focusable::focus_handle(field.read(cx), cx), cx);
        if let Some(search) = self.search.as_mut() {
            search.focused = true;
        }
        cx.notify();
    }

    pub(super) fn clear_search(&mut self, cx: &mut Context<Self>) {
        if let Some(search) = self.search.as_mut() {
            search.field.update(cx, |editor, _| editor.set_text(""));
            search.shown = None;
            search.focused = false;
            // `cleanUpSearchState`: the choice to show blocked results goes with the search.
            search.show_blocked = false;
        }
        cx.notify();
    }

    pub(super) fn run_search(&mut self, query: String, cx: &mut Context<Self>) {
        self.run_search_at(query, 0, cx);
    }

    /// A search for one page of results: `offset` is 25 times the page index.
    pub(super) fn run_search_at(&mut self, query: String, offset: u64, cx: &mut Context<Self>) {
        if query.trim().is_empty() {
            return;
        }
        // `searchResultsPaginationKey`: the same query in the same sort keeps the count while the page loads.
        let carried = self.search.as_ref().and_then(|search| {
            (search.shown.as_deref() == Some(query.as_str()))
                .then(|| {
                    self.fetched
                        .get(RESULTS_KEY)
                        .and_then(total_of)
                        .or(search.carried_total)
                })
                .flatten()
        });
        let (found, content) = operators(&query);
        let mut params: Vec<String> = Vec::new();
        if !content.is_empty() {
            params.push(format!("content={}", encode(&content)));
        }
        for (name, value) in found {
            let lower = value.to_lowercase();
            match name {
                "from" | "mentions" => {
                    if let Some(user) = self.stores.users.values().find(|user| {
                        user.username.to_lowercase() == lower
                            || user.display_name().to_lowercase() == lower
                    }) {
                        let key = if name == "from" {
                            "author_id"
                        } else {
                            "mentions"
                        };
                        params.push(format!("{key}={}", user.id));
                    }
                }
                "in" => {
                    if let Some(channel) = self.current_guild().and_then(|guild| {
                        guild
                            .channels
                            .iter()
                            .find(|channel| channel.name.to_lowercase() == lower)
                    }) {
                        params.push(format!("channel_id={}", channel.id));
                    }
                }
                "author_type" => params.push(format!("author_type={}", encode(&lower))),
                "pinned" => params.push(format!("pinned={}", encode(&lower))),
                "before" | "after" | "during" => {
                    let kind = match name {
                        "before" => super::search_filters::DateKind::Before,
                        "after" => super::search_filters::DateKind::After,
                        _ => super::search_filters::DateKind::During,
                    };
                    if let Some(date) = super::search_filters::parse_date(&value) {
                        let (min, max) =
                            super::search_filters::date_bounds(&chrono::Local, kind, date);
                        if let Some(min) = min {
                            params.push(format!("min_id={min}"));
                        }
                        if let Some(max) = max {
                            params.push(format!("max_id={max}"));
                        }
                    }
                }
                "has" if lower == "forward" => params.push("has=snapshot".to_owned()),
                _ => params.push(format!("has={}", encode(&lower))),
            }
        }
        let oldest_first = self
            .search
            .as_ref()
            .is_some_and(|search| search.oldest_first);
        // Not in the pinned client: SearchFix past offset 5000 (`plugin_hooks/search_fix.rs`).
        let (sent_oldest_first, sent_offset) =
            super::plugin_hooks::search_fix(oldest_first, offset);
        params.push("sort_by=timestamp".to_owned());
        params.push(format!(
            "sort_order={}",
            if sent_oldest_first { "asc" } else { "desc" }
        ));
        params.push(format!("offset={sent_offset}"));
        self.fetched.remove(RESULTS_KEY);
        let all_dms = self.search_all_dms();
        if let Some(search) = self.search.as_mut() {
            search.offset = offset;
            search.carried_total = carried;
            search.jump = None;
            search.scroll.set_offset(gpui::point(px(0.), px(0.)));
            search.last_route = Some(if all_dms {
                format!("POST {DMS_TABS_ROUTE}")
            } else {
                match (self.guild, self.channel) {
                    (Some(guild), _) => format!("GET /guilds/{guild}/messages/search"),
                    (None, Some(channel)) => format!("GET /channels/{channel}/messages/search"),
                    _ => String::new(),
                }
            });
        }
        if all_dms {
            // `{type: DMS}` has no GET route: the tabs search (`SearchTabFetcher`, module 177068) posts the query.
            let _ = self.requests.send(Request::Rest {
                key: Some(RESULTS_KEY.to_owned()),
                method: reqwest::Method::POST,
                route: DMS_TABS_ROUTE.to_owned(),
                body: Some(tabs_payload(&params)),
            });
        } else {
            let route = match (self.guild, self.channel) {
                (Some(guild), _) => {
                    format!("/guilds/{guild}/messages/search?{}", params.join("&"))
                }
                (None, Some(channel)) => {
                    format!("/channels/{channel}/messages/search?{}", params.join("&"))
                }
                _ => return,
            };
            let _ = self.requests.send(Request::Fetch {
                key: RESULTS_KEY.to_owned(),
                route,
            });
        }
        if let Some(search) = self.search.as_mut() {
            search.shown = Some(query);
            search.focused = false;
        }
        cx.notify();
    }

    /// The Filters popout under the focused, empty field: each row puts its operator into the field.
    pub(super) fn search_popout(
        &self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let search = self.search.as_ref()?;
        let focused = gpui::Focusable::focus_handle(search.field.read(cx), cx).is_focused(window);
        if !focused || !search.field.read(cx).text().is_empty() {
            return None;
        }
        let anchor = self.tip_anchors.borrow().get("header-search").copied()?;
        let row = |name: &'static str,
                   title: &'static str,
                   operator: &'static str,
                   hint: &'static str| {
            div()
                .id(SharedString::from(format!("search-filter-{name}")))
                .h(px(44.))
                .px(px(8.))
                .py(px(4.))
                .rounded(px(8.))
                .cursor_pointer()
                .flex()
                .items_center()
                .hover(|style| style.bg(color("--interactive-background-hover")))
                .on_click(cx.listener(move |this, _, window, cx| {
                    if operator.is_empty() {
                        this.open_search_filters(window, cx);
                        return;
                    }
                    if let Some(search) = this.search.as_ref() {
                        search
                            .field
                            .update(cx, |editor, cx| editor.insert(&format!("{operator} "), cx));
                    }
                    this.focus_search(window, cx);
                }))
                .child(icon(name, 20., "--text-muted"))
                .child(
                    div()
                        .ml(px(8.))
                        .flex()
                        .flex_col()
                        .child(
                            div()
                                .text_size(px(14.))
                                .line_height(px(18.))
                                .font_weight(FontWeight(500.))
                                .text_color(color("--text-strong"))
                                .child(title),
                        )
                        .child(
                            div()
                                .flex()
                                .gap(px(4.))
                                .text_size(px(14.))
                                .line_height(px(18.))
                                .when(!operator.is_empty(), |this| {
                                    this.child(
                                        div()
                                            .font_weight(FontWeight(600.))
                                            .text_color(color("--text-subtle"))
                                            .child(operator),
                                    )
                                })
                                .child(
                                    div()
                                        .font_weight(FontWeight(500.))
                                        .text_color(color("--text-muted"))
                                        .child(hint),
                                ),
                        ),
                )
        };
        Some(
            deferred(
                div()
                    .absolute()
                    .left((anchor.right() - px(360.)).max(px(8.)))
                    .top(anchor.bottom() + px(8.))
                    .child(
                        div()
                            .id("search-popout")
                            .occlude()
                            .w(px(356_f32.min((self.viewport.0 - 16.).max(260.))))
                            .pt(px(12.))
                            .pb(px(12.))
                            .px(px(8.))
                            .rounded(px(8.))
                            .border_1()
                            .border_color(color("--border-subtle"))
                            .bg(color("--background-surface-high"))
                            .shadow(vec![super::shadow_high()])
                            .on_mouse_down_out(cx.listener(
                                |this, event: &gpui::MouseDownEvent, window, cx| {
                                    let in_input = this
                                        .tip_anchors
                                        .borrow()
                                        .get("header-search")
                                        .is_some_and(|b| b.contains(&event.position));
                                    if !in_input {
                                        window.focus(&this.focus, cx);
                                        cx.notify();
                                    }
                                },
                            ))
                            .flex()
                            .flex_col()
                            .gap(px(4.))
                            .child(
                                div()
                                    .h(px(24.))
                                    .px(px(8.))
                                    .py(px(4.))
                                    .text_size(px(12.))
                                    .line_height(px(16.))
                                    .font_weight(FontWeight(600.))
                                    .text_color(color("--text-subtle"))
                                    .child("Filters"),
                            )
                            .child(row("search_from", "From a specific user", "from:", "user"))
                            .child(row(
                                "search_in",
                                "Sent in a specific channel",
                                "in:",
                                "channel",
                            ))
                            .child(row(
                                "search_has",
                                "Includes a specific type of data",
                                "has:",
                                "link, embed or file",
                            ))
                            .child(row(
                                "search_mentions",
                                "Mentions a specific user",
                                "mentions:",
                                "user",
                            ))
                            .child(row(
                                "search_more",
                                "More filters",
                                "",
                                "dates, author type, and more",
                            )),
                    ),
            )
            .with_priority(2)
            .into_any_element(),
        )
    }

    /// A message's text with the searched words on the highlight colour.
    fn highlighted(&self, text: &str, words: &[String], size: f32, weight: f32) -> AnyElement {
        let needles: Vec<&str> = words.iter().map(String::as_str).collect();
        let mut base = gpui::font(super::SANS);
        base.weight = FontWeight(weight);
        let run = |len: usize, marked: bool| TextRun {
            len,
            font: base.clone(),
            color: color("--text-default").into(),
            background_color: marked.then(|| gpui::rgba(HIGHLIGHT).into()),
            underline: None,
            strikethrough: None,
        };
        let mut runs: Vec<TextRun> = Vec::new();
        let mut index = 0;
        for found in match_ranges(text, &needles) {
            if found.start > index {
                runs.push(run(found.start - index, false));
            }
            runs.push(run(found.len(), true));
            index = found.end;
        }
        if index < text.len() {
            runs.push(run(text.len() - index, false));
        }
        div()
            .text_size(px(size))
            .line_height(px(22.))
            .child(StyledText::new(text.to_owned()).with_runs(runs))
            .into_any_element()
    }

    fn search_result(
        &self,
        message: &Message,
        words: &[String],
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let avatar = self.images.get(&avatar_source(&message.author).0).cloned();
        let target = message.clone();
        div()
            .id(SharedString::from(format!("search-result-{}", message.id)))
            .mb(px(8.))
            .rounded(px(8.))
            .border_1()
            .border_color(color("--border-subtle"))
            .bg(color("--background-surface-high"))
            .cursor_pointer()
            .hover(|style| style.bg(color("--background-surface-higher")))
            .on_click(cx.listener(move |this, _, _, cx| {
                // Module 36544: `k.A(e, n) && n()`, the check of module 81437 before the route changes.
                if !this.jump_allowed(&target) {
                    cx.notify();
                    return;
                }
                if let Some(guild) = this.guild {
                    this.select(guild, Some(target.channel_id));
                }
                this.jump_to_message(&target, cx);
            }))
            .child(
                div()
                    .relative()
                    .py(px(8.))
                    .pl(px(72.))
                    .pr(px(16.))
                    .child(
                        div()
                            .absolute()
                            .left(px(16.))
                            .top(px(10.))
                            .size(px(40.))
                            .children(avatar.map(|image| img(image).size(px(40.)).rounded_full())),
                    )
                    .child(
                        div()
                            .h(px(22.))
                            .flex()
                            .items_center()
                            .child(
                                div()
                                    .font_weight(FontWeight(500.))
                                    .text_color(color("--text-strong"))
                                    .child(message.author.display_name().to_owned()),
                            )
                            .child(
                                div()
                                    .ml(px(4.))
                                    .text_size(px(12.))
                                    .font_weight(FontWeight(500.))
                                    .text_color(color("--text-muted"))
                                    .child(super::header_time(
                                        message.local_time(),
                                        chrono::Local::now(),
                                    )),
                            ),
                    )
                    .child(self.highlighted(&message.content, words, 16., 400.))
                    // A voice message result shows its player under the header, as the message row does.
                    .children(message.is_voice_message().then(|| {
                        div().mt(px(8.)).flex().children(
                            message
                                .attachments
                                .iter()
                                .map(|attachment| self.voice_pill(message, attachment)),
                        )
                    })),
            )
            .into_any_element()
    }

    /// `resultsBlocked_3Zk3yP` (stylesheet 1b45d33a96dd1b9c): `--background-mod-normal` with a 1px
    /// `--background-base-lowest` border, 3px corners, 6px by 18px, 14px `--text-muted` (`--text-default` under the
    /// pointer), after the 32px `resultsBlockedImage_3Zk3yP` 20px before the text. A press shows or hides those results
    /// (`SEARCH_SET_SHOW_BLOCKED_RESULTS`) and the list scrolls to its end. The image (`abbc14555963e723.svg` in a dark
    /// theme) is not in the snapshot: its place is kept empty.
    fn blocked_results_line(
        &self,
        blocked: u64,
        ignored: u64,
        shown: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        div()
            .id("search-results-blocked")
            .flex()
            .items_center()
            .px(px(18.))
            .py(px(6.))
            .rounded(px(3.))
            .border_1()
            .border_color(color("--background-base-lowest"))
            .bg(color("--background-mod-normal"))
            .text_size(px(14.))
            .text_color(color("--text-muted"))
            .cursor_pointer()
            .hover(|style| style.text_color(color("--text-default")))
            .on_click(cx.listener(|this, _, _, cx| {
                if let Some(search) = this.search.as_mut() {
                    search.show_blocked = !search.show_blocked;
                    search.scroll.scroll_to_bottom();
                }
                cx.notify();
            }))
            .child(div().size(px(32.)).flex_none().mr(px(20.)))
            .child(blocked_results_text(blocked, ignored, shown))
            .into_any_element()
    }

    /// `searchResultsWrap_QHVbp3`: the count, Filters and Sort, then results grouped by channel.
    pub(super) fn search_panel(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let search = self.search.as_ref()?;
        let query = search.shown.clone()?;
        let words = plain_words(&query);
        let body = self.fetched.get(RESULTS_KEY).map(results_body);
        let total = body.and_then(|body| body["total_results"].as_u64());
        // `el` of module 252715: across all DMs the count gets "in all Direct Messages" under it (`tc619d`).
        // While searching (`er`) there is no subtitle.
        let subtitle = (self.search_all_dms() && total.is_some()).then_some(ALL_DMS_SUBTITLE);
        let threads: Vec<(Id, String, Id)> = body
            .and_then(|body| body["threads"].as_array())
            .into_iter()
            .flatten()
            .filter_map(|thread| {
                Some((
                    thread["id"].as_str()?.parse().ok()?,
                    thread["name"].as_str()?.to_owned(),
                    thread["parent_id"].as_str()?.parse().ok()?,
                ))
            })
            .collect();
        let results: Vec<Message> = body
            .and_then(|body| body["messages"].as_array())
            .into_iter()
            .flatten()
            .filter_map(|group| group.as_array().and_then(|group| group.first()))
            .filter_map(Message::from_value)
            .collect();
        // Module 956972: how many results are blocked (or else ignored), and module 36544 leaves them out until the
        // line under the results shows them.
        let (mut blocked, mut ignored) = (0u64, 0u64);
        let results: Vec<Message> = results
            .into_iter()
            .filter(|message| {
                let (is_blocked, is_ignored) = self.hidden_for_message(message);
                if is_blocked {
                    blocked += 1;
                } else if is_ignored {
                    ignored += 1;
                }
                search.show_blocked || !(is_blocked || is_ignored)
            })
            .collect();
        let guild = self.current_guild();
        let channel_name = |id: Id| -> (String, bool, String) {
            if let Some((_, name, parent)) = threads.iter().find(|(thread, _, _)| *thread == id) {
                let parent_name = guild
                    .and_then(|guild| guild.channels.iter().find(|channel| channel.id == *parent))
                    .map(|channel| channel.name.clone())
                    .unwrap_or_default();
                return (name.clone(), true, parent_name);
            }
            let channel =
                guild.and_then(|guild| guild.channels.iter().find(|channel| channel.id == id));
            let category = channel
                .and_then(|channel| channel.parent_id)
                .and_then(|parent| {
                    guild.and_then(|guild| guild.channels.iter().find(|known| known.id == parent))
                })
                .map(|category| category.name.clone())
                .unwrap_or_default();
            (
                channel
                    .map(|channel| channel.name.clone())
                    .unwrap_or_default(),
                false,
                category,
            )
        };
        let mut groups: Vec<AnyElement> = Vec::new();
        let mut index = 0;
        while index < results.len() {
            let channel = results[index].channel_id;
            let mut end = index;
            while end < results.len() && results[end].channel_id == channel {
                end += 1;
            }
            let (name, thread, parent) = channel_name(channel);
            groups.push(
                div()
                    .mb(px(24.))
                    .child(
                        div()
                            .mb(px(8.))
                            .h(px(20.))
                            .flex()
                            .items_center()
                            .child(div().pr(px(4.)).child(icon(
                                if thread {
                                    "search_thread"
                                } else {
                                    "text_channel"
                                },
                                16.,
                                "--text-strong",
                            )))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w(px(0.))
                                    .child(self.highlighted(&name, &words, 16., 500.)),
                            )
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .child(div().ml(px(4.)).pr(px(4.)).child(icon(
                                        if thread {
                                            "text_channel"
                                        } else {
                                            "search_category"
                                        },
                                        12.,
                                        "--text-default",
                                    )))
                                    .child(
                                        div()
                                            .text_size(px(12.))
                                            .line_height(px(16.))
                                            .font_weight(FontWeight(500.))
                                            .text_color(color("--text-default"))
                                            .child(parent),
                                    ),
                            ),
                    )
                    .children(
                        results[index..end]
                            .iter()
                            .map(|message| self.search_result(message, &words, cx)),
                    )
                    .into_any_element(),
            );
            index = end;
        }
        let oldest_first = search.oldest_first;
        // Module 477654: the count the pager pages and whether page 400's warning stands.
        let pager_total = self.search_pager_total(total);
        let page_warning =
            total.is_some() && super::search_pager::limits(pager_total, search.offset).warning;
        let button = |id: &'static str, name: &'static str, label: &'static str| {
            div()
                .id(id)
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
                .child(icon(name, 16., "--control-secondary-text-default"))
                .child(label)
        };
        let shown = query.clone();
        Some(
            div()
                .flex_none()
                .w(px(419.))
                .h_full()
                .border_l_1()
                .border_color(color("--border-subtle"))
                .bg(color("--background-base-lowest"))
                .flex()
                .flex_col()
                .child(
                    div()
                        .map(|header| match subtitle {
                            // `searchHeaderWithSubtitle_DAJqRF`: at least 58px.
                            Some(_) => header.min_h(px(58.)),
                            None => header.h(px(49.)),
                        })
                        .px(px(16.))
                        .flex()
                        .items_center()
                        .child(
                            div()
                                .flex_1()
                                .line_height(px(20.))
                                .font_weight(FontWeight(500.))
                                .text_color(color("--text-strong"))
                                // `totalResultsWithSubtitle_DAJqRF`: a column, 2px above and below.
                                .when(subtitle.is_some(), |count| {
                                    count.flex().flex_col().py(px(2.))
                                })
                                .child(match total {
                                    Some(1) => "1 Result".to_owned(),
                                    Some(count) => format!("{count} Results"),
                                    None => "Searching\u{2026}".to_owned(),
                                })
                                .children(subtitle.map(|subtitle| {
                                    // `body-xs/medium` in `--text-subtle`.
                                    div()
                                        .text_size(px(12.))
                                        .line_height(px(16.))
                                        .font_weight(FontWeight(500.))
                                        .text_color(color("--text-subtle"))
                                        .child(subtitle)
                                })),
                        )
                        .child(
                            div()
                                .flex()
                                .gap(px(8.))
                                .child(
                                    button("search-filters", "search_filters", "Filters").on_click(
                                        cx.listener(|this, _, window, cx| {
                                            if let Some(search) = this.search.as_ref() {
                                                search
                                                    .field
                                                    .update(cx, |editor, _| editor.set_text(""));
                                            }
                                            this.focus_search(window, cx);
                                        }),
                                    ),
                                )
                                // Sort flips between newest and oldest first and searches again.
                                .child(button("search-sort", "search_sort", "Sort").on_click(
                                    cx.listener(move |this, _, _, cx| {
                                        if let Some(search) = this.search.as_mut() {
                                            search.oldest_first = !oldest_first;
                                            // Another sort is another pagination key: no count is carried.
                                            search.carried_total = None;
                                        }
                                        this.fetched.remove(RESULTS_KEY);
                                        this.run_search(shown.clone(), cx);
                                    }),
                                )),
                        ),
                )
                .child(
                    div()
                        .id("search-results")
                        .flex_1()
                        .min_h(px(0.))
                        .overflow_y_scroll()
                        .track_scroll(&search.scroll)
                        // `scroller_QHVbp3`: 16px padding, plus the `thin` scroller's stable 8px gutter at the end.
                        .pl(px(16.))
                        .pr(px(24.))
                        .pt(px(16.))
                        // `w && !s.isSearching`: the 400-page warning over the results.
                        .when(page_warning, |this| this.child(self.search_page_warning()))
                        .children(groups)
                        .children((blocked > 0 || ignored > 0).then(|| {
                            self.blocked_results_line(blocked, ignored, search.show_blocked, cx)
                        })),
                )
                .children(self.search_pager(pager_total, search.offset, cx))
                .into_any_element(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::match_ranges;

    /// Every range of a match is on a character boundary of the shown text, whatever lower-casing did to the bytes.
    fn assert_on_boundaries(text: &str, ranges: &[std::ops::Range<usize>]) {
        for range in ranges {
            assert!(range.end <= text.len(), "{range:?} past {text:?}");
            assert!(text.is_char_boundary(range.start) && text.is_char_boundary(range.end));
        }
    }

    #[test]
    fn matches_are_offsets_of_the_original_text_when_lower_casing_changes_lengths() {
        // `İ` (2 bytes) lower-cases to `i̇` (3 bytes): offsets in the lower-cased copy run one byte ahead after it.
        let text = "İstanbul'da buluşalım";
        let ranges = match_ranges(text, &["buluş"]);
        assert_eq!(
            ranges.iter().map(|r| &text[r.clone()]).collect::<Vec<_>>(),
            ["buluş"]
        );
        assert_on_boundaries(text, &ranges);
        // The Kelvin sign (3 bytes) lower-cases to `k` (1 byte).
        let text = "\u{212a}elvin kedi";
        let ranges = match_ranges(text, &["kedi", "k"]);
        assert_eq!(
            ranges.iter().map(|r| &text[r.clone()]).collect::<Vec<_>>(),
            ["\u{212a}", "kedi"]
        );
        assert_on_boundaries(text, &ranges);
    }

    #[test]
    fn matches_inside_emoji_and_isolates_stay_on_boundaries() {
        let text = "\u{2068}Çağrı\u{2069} 🎉🎉 ŞİMDİ şimdi";
        let ranges = match_ranges(text, &["çağrı", "🎉", "şi\u{307}mdi\u{307}", "şimdi"]);
        assert_on_boundaries(text, &ranges);
        let shown: Vec<&str> = ranges.iter().map(|r| &text[r.clone()]).collect();
        assert_eq!(shown, ["Çağrı", "🎉🎉", "ŞİMDİ", "şimdi"]);
    }

    #[test]
    #[allow(clippy::single_range_in_vec_init)]
    fn overlapping_words_merge_and_empty_words_match_nothing() {
        assert_eq!(match_ranges("abcdef", &["abc", "cde", ""]), [0..5]);
        assert!(match_ranges("abc", &[]).is_empty());
        assert!(match_ranges("", &["a"]).is_empty());
    }

    use super::{
        blocked_results_text, decode, encode, operators, plain_words, results_body, tabs_payload,
    };

    #[test]
    fn the_blocked_results_line_names_what_it_hides_or_shows() {
        assert_eq!(
            blocked_results_text(2, 0, false),
            "We\u{2019}ve hidden 2 results from users you\u{2019}ve blocked."
        );
        assert_eq!(
            blocked_results_text(1, 2, false),
            "We\u{2019}ve hidden 3 messages from users you\u{2019}ve blocked and ignored."
        );
        assert_eq!(
            blocked_results_text(0, 1, true),
            "Hide 1 message from ignored users"
        );
        assert_eq!(
            blocked_results_text(4, 0, true),
            "Hide 4 messages from blocked users"
        );
    }
    use serde_json::json;

    #[test]
    fn the_dm_wide_search_posts_the_query_under_the_messages_tab() {
        let params = vec![
            format!("content={}", encode("hello world")),
            "author_id=42".to_owned(),
            "has=link".to_owned(),
            "has=image".to_owned(),
            "pinned=true".to_owned(),
            "min_id=100".to_owned(),
            "sort_by=timestamp".to_owned(),
            "sort_order=desc".to_owned(),
            "offset=0".to_owned(),
        ];
        assert_eq!(
            tabs_payload(&params),
            json!({"tabs": {"messages": {
                "content": "hello world", "author_id": ["42"], "has": ["link", "image"],
                "pinned": true, "min_id": "100", "sort_by": "timestamp", "sort_order": "desc",
                "offset": 0, "limit": 25
            }}, "track_exact_total_hits": true})
        );
        assert_eq!(decode(&encode("a b%ç")), "a b%ç");
    }

    #[test]
    fn both_answers_give_the_same_results() {
        let get = json!({"total_results": 3, "messages": []});
        assert_eq!(results_body(&get)["total_results"], 3);
        let tabs = json!({"ok": true, "status": 200,
            "body": {"tabs": {"messages": {"total_results": 5, "messages": []}}}});
        assert_eq!(results_body(&tabs)["total_results"], 5);
        let refused = json!({"ok": false, "status": 0, "body": null, "refused": true});
        assert!(results_body(&refused)["total_results"].is_null());
    }

    #[test]
    fn operators_leave_the_content() {
        let (found, content) = operators("from:onugeccen hello in:#general has:link world");
        assert_eq!(content, "hello world");
        assert_eq!(found.len(), 3);
        assert_eq!(found[1], ("in", "general".to_owned()));
        assert_eq!(plain_words("from:x Fixture"), vec!["fixture".to_owned()]);
    }
}
