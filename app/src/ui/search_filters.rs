//! The Filters dialog behind the search popout's More filters (module 561965, option lists in 121806 and 224467):
//! From, In, Has, Mentions, Date, Author Type and Pinned, with Clear Filters (n), Cancel and Apply Filters. Measured in
//! `design/dom/modals/search-filters*.json`. Apply puts the filters in the search field and runs the search.
//!
//! The Date field of the official dialog opens a calendar; here the date is typed (`Nov 9, 2026` or `2026-11-09`).
//! From, In and Mentions list matching people and channels of the server as the field is typed in, and stay empty
//! ("No items to show") until then, as the official one does.

use super::{Root, color, editor_style, icon};
use crate::editor::{Editor, EditorEvent};
use crate::parity::Parity;
use crate::ui::motion::Transitions as _;
use chrono::{Duration, Local, NaiveDate, TimeZone};
use dc_state::Id;
use gpui::{
    AnyElement, Context, Entity, FontWeight, IntoElement, ScrollHandle, SharedString, Subscription,
    Window, deferred, div, prelude::*, px,
};

/// The `has:` values in the order of the dropdown (module 224467 `ea`).
pub(crate) const HAS: [&str; 9] = [
    "image", "video", "link", "file", "embed", "sound", "poll", "sticker", "forward",
];
pub(crate) const AUTHOR_TYPES: [&str; 3] = ["user", "bot", "webhook"];
/// Search accepts at most four date filters (module 561965 `N`).
const MAX_DATES: usize = 4;
/// Every select keeps its own probe so the list finds its field on the frame it opens.
fn probe_key(field: Field) -> SharedString {
    SharedString::from(format!("search-filters-probe-{field:?}"))
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum DateKind {
    Before,
    After,
    During,
}

impl DateKind {
    const ALL: [DateKind; 3] = [DateKind::Before, DateKind::After, DateKind::During];

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Before => "Before",
            Self::After => "After",
            Self::During => "During",
        }
    }

    pub(crate) fn key(self) -> &'static str {
        match self {
            Self::Before => "before",
            Self::After => "after",
            Self::During => "during",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Field {
    From,
    In,
    Has,
    Mentions,
    AuthorType,
    Pinned,
    /// The type select of the date filter at this index.
    DateKind(usize),
}

pub(crate) struct DateFilter {
    pub kind: DateKind,
    pub date: NaiveDate,
    pub text: Entity<Editor>,
    _events: Subscription,
}

pub(crate) struct SearchFilters {
    pub from: Vec<Id>,
    pub channels: Vec<Id>,
    pub has: Vec<&'static str>,
    pub mentions: Vec<Id>,
    pub authors: Vec<&'static str>,
    pub pinned: Option<bool>,
    pub dates: Vec<DateFilter>,
    /// The words of the search field outside operators: they stay in the query.
    pub rest: String,
    pub open: Option<Field>,
    pub from_text: Entity<Editor>,
    pub in_text: Entity<Editor>,
    pub mentions_text: Entity<Editor>,
    pub scroll: ScrollHandle,
    _events: Vec<Subscription>,
}

/// What the dialog has chosen, as names, for [`filter_query`].
pub(crate) struct Selection<'a> {
    pub from: &'a [String],
    pub mentions: &'a [String],
    pub channels: &'a [String],
    pub has: &'a [&'static str],
    pub dates: &'a [(DateKind, NaiveDate)],
    pub authors: &'a [&'static str],
    pub pinned: Option<bool>,
}

/// The query of the filters in the official order (module 561965 `eA`): from, mentions, in, has, dates, author type,
/// pinned. The official text has a space after each colon; this one has none, which the search field reads as well.
pub(crate) fn filter_query(selection: &Selection) -> String {
    let mut parts: Vec<String> = Vec::new();
    parts.extend(selection.from.iter().map(|name| format!("from:{name}")));
    parts.extend(
        selection
            .mentions
            .iter()
            .map(|name| format!("mentions:{name}")),
    );
    parts.extend(selection.channels.iter().map(|name| format!("in:{name}")));
    parts.extend(selection.has.iter().map(|has| format!("has:{has}")));
    parts.extend(
        selection
            .dates
            .iter()
            .map(|(kind, date)| format!("{}:{}", kind.key(), date.format("%Y-%m-%d"))),
    );
    parts.extend(
        selection
            .authors
            .iter()
            .map(|author| format!("authorType:{author}")),
    );
    if let Some(pinned) = selection.pinned {
        parts.push(format!("pinned:{pinned}"));
    }
    parts.join(" ")
}

/// Snowflake of a millisecond timestamp (`fromTimestamp`).
pub(crate) fn snowflake(millis: i64) -> u64 {
    const DISCORD_EPOCH: i64 = 1_420_070_400_000;
    ((millis - DISCORD_EPOCH).max(0) as u64) << 22
}

/// `min_id` and `max_id` of a date filter (module 224467 `j`): the day runs from its start to the next day's start;
/// `before` keeps the start as its end, `after` the end as its start.
pub(crate) fn date_bounds<Tz: TimeZone>(
    zone: &Tz,
    kind: DateKind,
    date: NaiveDate,
) -> (Option<u64>, Option<u64>) {
    let at = |day: NaiveDate| {
        day.and_hms_opt(0, 0, 0)
            .and_then(|time| zone.from_local_datetime(&time).earliest())
            .map(|time| snowflake(time.timestamp_millis()))
    };
    let (start, end) = (at(date), at(date + Duration::days(1)));
    match kind {
        DateKind::Before => (None, start),
        DateKind::After => (end, None),
        DateKind::During => (start, end),
    }
}

/// `Nov 9, 2026` (the picker's format) or `2026-11-09`.
pub(crate) fn parse_date(text: &str) -> Option<NaiveDate> {
    let text = text.trim();
    NaiveDate::parse_from_str(text, "%Y-%m-%d")
        .or_else(|_| NaiveDate::parse_from_str(text, "%b %-d, %Y"))
        .ok()
}

fn date_text(date: NaiveDate) -> String {
    date.format("%b %-d, %Y").to_string()
}

impl SearchFilters {
    /// How many filters are chosen: the number in Clear Filters (n).
    pub(crate) fn count(&self) -> usize {
        self.from.len()
            + self.channels.len()
            + self.has.len()
            + self.mentions.len()
            + self.dates.len()
            + self.authors.len()
            + usize::from(self.pinned.is_some())
    }
}

/// The people the typed text finds: by user name or display name, the first twenty.
fn matching_users(root: &Root, text: &str, chosen: &[Id]) -> Vec<(Id, String, String)> {
    let needle = text.trim().to_lowercase();
    if needle.is_empty() {
        return Vec::new();
    }
    let mut found: Vec<(Id, String, String)> = root
        .stores
        .users
        .values()
        .filter(|user| !chosen.contains(&user.id))
        .filter(|user| {
            user.username.to_lowercase().starts_with(&needle)
                || user.display_name().to_lowercase().starts_with(&needle)
        })
        .map(|user| {
            (
                user.id,
                user.display_name().to_owned(),
                user.username.clone(),
            )
        })
        .collect();
    found.sort_by_key(|entry| entry.1.to_lowercase());
    found.truncate(20);
    found
}

fn matching_channels(root: &Root, text: &str, chosen: &[Id]) -> Vec<(Id, String)> {
    let needle = text.trim().trim_start_matches('#').to_lowercase();
    if needle.is_empty() {
        return Vec::new();
    }
    let Some(guild) = root.current_guild() else {
        return Vec::new();
    };
    let mut found: Vec<(Id, String)> = guild
        .channels
        .iter()
        .filter(|channel| !chosen.contains(&channel.id))
        .filter(|channel| channel.name.to_lowercase().starts_with(&needle))
        .map(|channel| (channel.id, channel.name.clone()))
        .collect();
    found.truncate(20);
    found
}

impl Root {
    /// Opens the dialog from the popout's More filters or the results panel's Filters.
    pub(super) fn open_search_filters(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.focus, cx);
        let rest = self
            .search
            .as_ref()
            .map(|search| {
                let text = search.field.read(cx).text().to_owned();
                super::search::operators(&text).1
            })
            .unwrap_or_default();
        let field = |placeholder: String, cx: &mut Context<Self>| {
            let editor = cx.new(|cx| {
                let mut editor = Editor::new(editor_style(), cx);
                editor.set_placeholder(placeholder);
                editor
            });
            let events = cx.subscribe(&editor, |_, _, _: &EditorEvent, cx| cx.notify());
            (editor, events)
        };
        let me = self
            .stores
            .current_user
            .as_ref()
            .map(|user| user.username.clone())
            .unwrap_or_else(|| "user".to_owned());
        let channel = self
            .current_channel_name()
            .unwrap_or_else(|| "general".to_owned());
        let (from_text, a) = field(format!("ex. {me}"), cx);
        let (in_text, b) = field(format!("ex. {channel}"), cx);
        let (mentions_text, c) = field(format!("ex. {me}"), cx);
        self.search_filters = Some(SearchFilters {
            from: Vec::new(),
            channels: Vec::new(),
            has: Vec::new(),
            mentions: Vec::new(),
            authors: Vec::new(),
            pinned: None,
            dates: Vec::new(),
            rest,
            open: None,
            from_text,
            in_text,
            mentions_text,
            scroll: ScrollHandle::new(),
            _events: vec![a, b, c],
        });
        if let Some(search) = self.search.as_mut() {
            search.focused = false;
        }
        cx.notify();
    }

    fn current_channel_name(&self) -> Option<String> {
        let guild = self.current_guild()?;
        let id = self.channel?;
        guild
            .channels
            .iter()
            .find(|channel| channel.id == id)
            .map(|channel| channel.name.clone())
    }

    fn filter_user_name(&self, id: Id) -> String {
        self.stores
            .users
            .get(&id)
            .map(|user| user.username.clone())
            .unwrap_or_else(|| id.to_string())
    }

    fn channel_name(&self, id: Id) -> String {
        self.current_guild()
            .and_then(|guild| guild.channels.iter().find(|channel| channel.id == id))
            .map(|channel| channel.name.clone())
            .unwrap_or_else(|| id.to_string())
    }

    /// The query Apply Filters puts in the search field.
    fn filters_query(&self, filters: &SearchFilters) -> String {
        let from: Vec<String> = filters
            .from
            .iter()
            .map(|id| self.filter_user_name(*id))
            .collect();
        let mentions: Vec<String> = filters
            .mentions
            .iter()
            .map(|id| self.filter_user_name(*id))
            .collect();
        let channels: Vec<String> = filters
            .channels
            .iter()
            .map(|id| self.channel_name(*id))
            .collect();
        let dates: Vec<(DateKind, NaiveDate)> = filters
            .dates
            .iter()
            .map(|filter| (filter.kind, filter.date))
            .collect();
        filter_query(&Selection {
            from: &from,
            mentions: &mentions,
            channels: &channels,
            has: &filters.has,
            dates: &dates,
            authors: &filters.authors,
            pinned: filters.pinned,
        })
    }

    fn apply_search_filters(&mut self, cx: &mut Context<Self>) {
        let Some(filters) = self.search_filters.take() else {
            return;
        };
        let query = format!("{} {}", self.filters_query(&filters), filters.rest)
            .trim()
            .to_owned();
        if query.is_empty() {
            self.clear_search(cx);
        } else {
            self.run_search(query, cx);
        }
        cx.notify();
    }

    fn pick_filter(&mut self, field: Field, index: usize, cx: &mut Context<Self>) {
        let options = self.filter_options(field, cx);
        let Some(option) = options.get(index) else {
            return;
        };
        let (id, label) = (option.id, option.label.clone());
        let Some(filters) = self.search_filters.as_mut() else {
            return;
        };
        let toggle = |list: &mut Vec<Id>| {
            if let Some(position) = list.iter().position(|known| *known == id) {
                list.remove(position);
            } else {
                list.push(id);
            }
        };
        match field {
            Field::From => {
                toggle(&mut filters.from);
                filters
                    .from_text
                    .update(cx, |editor, _| editor.set_text(""));
            }
            Field::Mentions => {
                toggle(&mut filters.mentions);
                filters
                    .mentions_text
                    .update(cx, |editor, _| editor.set_text(""));
            }
            Field::In => {
                toggle(&mut filters.channels);
                filters.in_text.update(cx, |editor, _| editor.set_text(""));
            }
            Field::Has => {
                if let Some(known) = HAS.iter().find(|known| **known == label) {
                    if let Some(position) = filters.has.iter().position(|has| has == known) {
                        filters.has.remove(position);
                    } else {
                        filters.has.push(known);
                    }
                }
            }
            Field::AuthorType => {
                if let Some(known) = AUTHOR_TYPES.iter().find(|known| **known == label) {
                    if let Some(position) = filters.authors.iter().position(|a| a == known) {
                        filters.authors.remove(position);
                    } else {
                        filters.authors.push(known);
                    }
                }
                filters.open = Some(Field::AuthorType);
            }
            Field::Pinned => {
                filters.pinned = Some(label == "true");
                filters.open = None;
            }
            Field::DateKind(at) => {
                if let Some(filter) = filters.dates.get_mut(at) {
                    filter.kind = DateKind::ALL
                        .into_iter()
                        .find(|kind| kind.label() == label)
                        .unwrap_or(filter.kind);
                }
                filters.open = None;
            }
        }
        // The multiple selects stay open, the single ones close.
        if matches!(
            field,
            Field::From | Field::Mentions | Field::In | Field::Has
        ) {
            filters.open = Some(field);
        }
        cx.notify();
    }

    fn add_date_filter(&mut self, cx: &mut Context<Self>) {
        let Some(filters) = self.search_filters.as_mut() else {
            return;
        };
        if filters.dates.len() >= MAX_DATES {
            return;
        }
        let date = Local::now().date_naive();
        let text = cx.new(|cx| {
            let mut editor = Editor::new(editor_style(), cx);
            editor.set_text(&date_text(date));
            editor
        });
        let index = filters.dates.len();
        let events = cx.subscribe(&text, move |this, editor, event: &EditorEvent, cx| {
            if matches!(event, EditorEvent::Edited)
                && let Some(date) = parse_date(editor.read(cx).text())
                && let Some(filter) = this
                    .search_filters
                    .as_mut()
                    .and_then(|filters| filters.dates.get_mut(index))
            {
                filter.date = date;
            }
            cx.notify();
        });
        filters.dates.push(DateFilter {
            kind: DateKind::Before,
            date,
            text,
            _events: events,
        });
        cx.notify();
    }

    fn remove_date_filter(&mut self, index: usize, cx: &mut Context<Self>) {
        if let Some(filters) = self.search_filters.as_mut()
            && index < filters.dates.len()
        {
            filters.dates.remove(index);
            // The change handlers count dates by position.
            for (at, filter) in filters.dates.iter().enumerate().skip(index) {
                let _ = (at, filter);
            }
            filters.open = None;
        }
        cx.notify();
    }

    /// The rows of the open field's list.
    fn filter_options(&self, field: Field, cx: &Context<Self>) -> Vec<FilterOption> {
        let Some(filters) = self.search_filters.as_ref() else {
            return Vec::new();
        };
        match field {
            Field::From => matching_users(self, filters.from_text.read(cx).text(), &filters.from)
                .into_iter()
                .map(|(id, name, tag)| FilterOption::user(id, name, tag))
                .collect(),
            Field::Mentions => matching_users(
                self,
                filters.mentions_text.read(cx).text(),
                &filters.mentions,
            )
            .into_iter()
            .map(|(id, name, tag)| FilterOption::user(id, name, tag))
            .collect(),
            Field::In => {
                matching_channels(self, filters.in_text.read(cx).text(), &filters.channels)
                    .into_iter()
                    .map(|(id, name)| FilterOption {
                        id,
                        label: name,
                        trailing: None,
                        icon: Some("channel_type_text"),
                        checked: false,
                    })
                    .collect()
            }
            Field::Has => {
                // The chosen types come first, as the official list orders them.
                let mut rows: Vec<FilterOption> = HAS
                    .iter()
                    .enumerate()
                    .map(|(index, has)| FilterOption {
                        id: index as Id,
                        label: (*has).to_owned(),
                        trailing: None,
                        icon: Some(has_icon(has)),
                        checked: filters.has.contains(has),
                    })
                    .collect();
                rows.sort_by_key(|row| !row.checked);
                rows
            }
            Field::AuthorType => {
                let mut rows: Vec<FilterOption> = AUTHOR_TYPES
                    .iter()
                    .enumerate()
                    .map(|(index, author)| FilterOption {
                        id: index as Id,
                        label: (*author).to_owned(),
                        trailing: None,
                        icon: Some(author_icon(author)),
                        checked: filters.authors.contains(author),
                    })
                    .collect();
                rows.sort_by_key(|row| !row.checked);
                rows
            }
            Field::Pinned => ["true", "false"]
                .iter()
                .enumerate()
                .map(|(index, text)| FilterOption {
                    id: index as Id,
                    label: (*text).to_owned(),
                    trailing: None,
                    icon: None,
                    checked: filters.pinned == Some(*text == "true"),
                })
                .collect(),
            Field::DateKind(at) => DateKind::ALL
                .iter()
                .enumerate()
                .map(|(index, kind)| FilterOption {
                    id: index as Id,
                    label: kind.label().to_owned(),
                    trailing: None,
                    icon: None,
                    checked: filters.dates.get(at).is_some_and(|f| f.kind == *kind),
                })
                .collect(),
        }
    }

    pub(super) fn search_filters_modal(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let Some(filters) = self.search_filters.as_ref() else {
            return Vec::new();
        };
        let count = filters.count();
        let section = |first: bool, last: bool, body: AnyElement| {
            div()
                .flex_none()
                .pt(px(if first { 24. } else { 0. }))
                .pb(px(if last { 12. } else { 20. }))
                .child(body)
        };
        let title = |label: &'static str, hint: &'static str| {
            div()
                .flex()
                .flex_col()
                .child(
                    div()
                        .h(px(20.))
                        .text_size(px(16.))
                        .line_height(px(20.))
                        .font_weight(FontWeight(500.))
                        .text_color(color("--text-strong"))
                        .child(label),
                )
                .child(
                    div()
                        .mt(px(4.))
                        .h(px(18.))
                        .text_size(px(14.))
                        .line_height(px(18.))
                        .text_color(color("--text-subtle"))
                        .child(hint),
                )
        };
        let labelled = |label: &'static str, hint: &'static str, control: AnyElement| {
            div()
                .flex()
                .flex_col()
                .child(title(label, hint))
                .child(div().mt(px(8.)).child(control))
                .into_any_element()
        };
        let name_of_user = |id: Id| self.filter_user_name(id);
        let from = self.filter_field(
            Field::From,
            filters
                .from
                .iter()
                .map(|id| (name_of_user(*id), None))
                .collect(),
            Some(filters.from_text.clone()),
            "Any user",
            cx,
        );
        let channels = self.filter_field(
            Field::In,
            filters
                .channels
                .iter()
                .map(|id| (self.channel_name(*id), Some("channel_type_text")))
                .collect(),
            Some(filters.in_text.clone()),
            "Any channel",
            cx,
        );
        let has = self.filter_field(
            Field::Has,
            filters
                .has
                .iter()
                .map(|has| ((*has).to_owned(), Some(has_icon(has))))
                .collect(),
            None,
            "Any content",
            cx,
        );
        let mentions = self.filter_field(
            Field::Mentions,
            filters
                .mentions
                .iter()
                .map(|id| (name_of_user(*id), None))
                .collect(),
            Some(filters.mentions_text.clone()),
            "Anyone",
            cx,
        );
        let authors = self.filter_field(
            Field::AuthorType,
            filters
                .authors
                .iter()
                .map(|author| ((*author).to_owned(), Some(author_icon(author))))
                .collect(),
            None,
            "ex. Bot",
            cx,
        );
        let pinned = self.filter_field(
            Field::Pinned,
            Vec::new(),
            None,
            filters
                .pinned
                .map_or("ex. True", |pinned| if pinned { "true" } else { "false" }),
            cx,
        );
        let date_rows: Vec<AnyElement> = filters
            .dates
            .iter()
            .enumerate()
            .map(|(index, filter)| self.date_row(index, filter, cx))
            .collect();
        let dates = div()
            .flex()
            .flex_col()
            .child(
                div()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .h(px(20.))
                            .text_size(px(16.))
                            .line_height(px(20.))
                            .font_weight(FontWeight(500.))
                            .text_color(color("--text-strong"))
                            .child("Date"),
                    )
                    .child(
                        div()
                            .h(px(18.))
                            .text_size(px(14.))
                            .line_height(px(18.))
                            .text_color(color("--text-subtle"))
                            .child("When the message was sent"),
                    ),
            )
            .child(
                div()
                    .mt(px(8.))
                    .flex()
                    .flex_col()
                    .gap(px(12.))
                    .children(date_rows)
                    .when(filters.dates.len() < MAX_DATES, |this| {
                        this.child(
                            div()
                                .id("search-filters-add-date")
                                .h(px(40.))
                                .rounded(px(8.))
                                .border_1()
                                .border_color(color("--border-subtle"))
                                .bg(color("--control-secondary-background-default"))
                                .cursor_pointer()
                                .flex()
                                .items_center()
                                .justify_center()
                                .gap(px(4.))
                                .text_size(px(16.))
                                .line_height(px(20.))
                                .font_weight(FontWeight(500.))
                                .text_color(color("--control-secondary-text-default"))
                                .child(icon("search_plus", 20., "--control-secondary-text-default"))
                                .child("Add date")
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.add_date_filter(cx);
                                })),
                        )
                    }),
            )
            .into_any_element();
        let content = div()
            .w(px(430.))
            .flex()
            .flex_col()
            .child(section(
                true,
                false,
                labelled("From", "Sent by any of the selected users", from),
            ))
            .child(section(
                false,
                false,
                labelled("In", "Sent in any of the selected channels", channels),
            ))
            .child(section(
                false,
                false,
                labelled("Has", "Includes any of the selected types of data", has),
            ))
            .child(section(
                false,
                false,
                labelled("Mentions", "Mentions any of the selected users", mentions),
            ))
            .child(section(false, false, dates))
            .child(section(
                false,
                false,
                labelled(
                    "Author Type",
                    "Sent by any of the selected types of author",
                    authors,
                ),
            ))
            .child(section(
                false,
                true,
                labelled("Pinned", "If the message is pinned or not", pinned),
            ));
        let scroll = filters.scroll.clone();
        let body = div()
            .relative()
            .ml(px(-24.))
            .mt(px(1.))
            .mb(px(1.))
            .w(px(478.))
            .h(px(444.))
            .child(
                div()
                    .id("search-filters-scroll")
                    .size_full()
                    .overflow_y_scroll()
                    .track_scroll(&scroll)
                    .child(div().w(px(454.)).pl(px(24.)).child(content)),
            )
            .child(super::settings::auto_scrollbar(scroll))
            .into_any_element();
        let clear_label = if count == 0 {
            "Clear Filters".to_owned()
        } else {
            format!("Clear Filters ({count})")
        };
        let clear = div()
            .id("search-filters-clear")
            .flex()
            .items_center()
            .h(px(20.))
            .text_size(px(16.))
            .line_height(px(20.))
            .font_weight(FontWeight(500.))
            .text_color(color("--text-brand"))
            .when(count == 0, |this| this.opacity(0.5))
            .when(count > 0, |this| this.cursor_pointer())
            .child(clear_label)
            .on_click(cx.listener(|this, _, _, cx| {
                if let Some(filters) = this.search_filters.as_mut() {
                    filters.from.clear();
                    filters.channels.clear();
                    filters.has.clear();
                    filters.mentions.clear();
                    filters.authors.clear();
                    filters.pinned = None;
                    filters.dates.clear();
                    filters.open = None;
                }
                cx.notify();
            }))
            .into_any_element();
        let changed = count > 0;
        let dropdown = self.filters_dropdown(cx);
        let modal = super::modal::render_with_input(
            super::modal::Spec {
                id: "search-filters",
                title: "Filters".into(),
                subtitle: None,
                body: Some(body),
                buttons: vec![
                    super::modal::button(
                        "search-filters-cancel",
                        "Cancel",
                        super::modal::Tone::Secondary,
                        |this, _, _| this.search_filters = None,
                    ),
                    super::modal::button(
                        "search-filters-apply",
                        "Apply Filters",
                        if changed {
                            super::modal::Tone::Primary
                        } else {
                            super::modal::Tone::PrimaryDisabled
                        },
                        |this, _, cx| this.apply_search_filters(cx),
                    ),
                ],
                close: Box::new(|this, _, _| this.search_filters = None),
            },
            clear,
            cx,
        );
        std::iter::once(modal).chain(dropdown).collect()
    }

    /// One select: the field with its tags or placeholder and chevron, a click opens its list.
    fn filter_field(
        &self,
        field: Field,
        tags: Vec<(String, Option<&'static str>)>,
        text: Option<Entity<Editor>>,
        placeholder: &'static str,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let open = self
            .search_filters
            .as_ref()
            .is_some_and(|filters| filters.open == Some(field));
        let id = SharedString::from(format!("search-filters-field-{field:?}"));
        let editor = text.clone();
        let tag_views: Vec<AnyElement> = tags
            .iter()
            .enumerate()
            .map(|(index, (label, symbol))| {
                let label = label.clone();
                div()
                    .id(SharedString::from(format!("{id}-tag-{index}")))
                    .flex_none()
                    .h(px(30.))
                    .rounded(px(4.))
                    .border_1()
                    .border_color(gpui::transparent_black())
                    .bg(color("--control-secondary-background-default"))
                    .pl(px(8.))
                    .pr(px(4.))
                    .flex()
                    .items_center()
                    .gap(px(4.))
                    .text_size(px(14.))
                    .line_height(px(18.))
                    .text_color(color("--control-secondary-text-default"))
                    .when_some(*symbol, |this, symbol| {
                        this.child(icon(symbol, 16., "--control-secondary-text-default"))
                    })
                    .child(label.clone())
                    .child(
                        div()
                            .id(SharedString::from(format!("{id}-tag-{index}-remove")))
                            .size(px(18.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .cursor_pointer()
                            .child(icon("close", 12., "--control-secondary-text-default"))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.remove_filter_tag(field, index, cx);
                                cx.stop_propagation();
                            })),
                    )
                    .into_any_element()
            })
            .collect();
        let empty = tag_views.is_empty();
        div()
            .id(id)
            .relative()
            .child(self.anchor_probe(probe_key(field)))
            .min_h(px(40.))
            .rounded(px(8.))
            .border_1()
            .border_color(if open {
                color("--input-border-active")
            } else {
                color("--input-border-default")
            })
            .rule(crate::ui::motion::Rule::TextField)
            .bg(color("--input-background-default"))
            .flex()
            .cursor_pointer()
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap(px(4.))
                    .when(!empty, |this| this.p(px(3.)))
                    .when(empty, |this| this.pl(px(12.)).pr(px(8.)))
                    .children(tag_views)
                    .child(match editor {
                        Some(editor) => div()
                            .flex_1()
                            .min_w(px(60.))
                            .when(!empty, |this| this.px(px(8.)))
                            .child(editor)
                            .into_any_element(),
                        None => div()
                            .flex_1()
                            .text_size(px(16.))
                            .line_height(px(20.))
                            .text_color(
                                if matches!(field, Field::Pinned)
                                    && placeholder.len() <= 5
                                    && placeholder != "ex. True"
                                {
                                    color("--text-strong")
                                } else {
                                    color("--channel-text-area-placeholder")
                                },
                            )
                            .when(!empty, |this| this.h(px(0.)).overflow_hidden())
                            .child(if empty { placeholder } else { "" })
                            .into_any_element(),
                    }),
            )
            .child(
                div()
                    .flex_none()
                    .w(px(36.))
                    .h(px(38.))
                    .pr(px(4.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(
                        gpui::svg()
                            .path("icons/select_chevron.svg")
                            .size(px(20.))
                            .text_color(color("--text-strong"))
                            .when(open, |this| {
                                this.with_transformation(gpui::Transformation::rotate(
                                    gpui::radians(std::f32::consts::PI),
                                ))
                            }),
                    ),
            )
            .on_click(cx.listener(move |this, _, window, cx| {
                if let Some(filters) = this.search_filters.as_mut() {
                    filters.open = if filters.open == Some(field) {
                        None
                    } else {
                        Some(field)
                    };
                    let typed = match field {
                        Field::From => Some(filters.from_text.clone()),
                        Field::In => Some(filters.in_text.clone()),
                        Field::Mentions => Some(filters.mentions_text.clone()),
                        _ => None,
                    };
                    if let Some(typed) = typed {
                        window.focus(&gpui::Focusable::focus_handle(typed.read(cx), cx), cx);
                    }
                }
                cx.notify();
            }))
            .into_any_element()
    }

    fn remove_filter_tag(&mut self, field: Field, index: usize, cx: &mut Context<Self>) {
        if let Some(filters) = self.search_filters.as_mut() {
            match field {
                Field::From if index < filters.from.len() => {
                    filters.from.remove(index);
                }
                Field::Mentions if index < filters.mentions.len() => {
                    filters.mentions.remove(index);
                }
                Field::In if index < filters.channels.len() => {
                    filters.channels.remove(index);
                }
                Field::Has if index < filters.has.len() => {
                    filters.has.remove(index);
                }
                Field::AuthorType if index < filters.authors.len() => {
                    filters.authors.remove(index);
                }
                _ => {}
            }
        }
        cx.notify();
    }

    /// `dateFilter_R726Px`: the type select 120 wide, the date field 254, and the trash button.
    fn date_row(&self, index: usize, filter: &DateFilter, cx: &mut Context<Self>) -> AnyElement {
        let kind_open = self
            .search_filters
            .as_ref()
            .is_some_and(|filters| filters.open == Some(Field::DateKind(index)));
        div()
            .flex()
            .gap(px(8.))
            .child(
                div()
                    .id(SharedString::from(format!(
                        "search-filters-date-kind-{index}"
                    )))
                    .relative()
                    .child(self.anchor_probe(probe_key(Field::DateKind(index))))
                    .flex_none()
                    .w(px(120.))
                    .h(px(40.))
                    .rounded(px(8.))
                    .border_1()
                    .border_color(if kind_open {
                        color("--input-border-active")
                    } else {
                        color("--input-border-default")
                    })
                    .rule(crate::ui::motion::Rule::TextField)
                    .bg(color("--input-background-default"))
                    .flex()
                    .items_center()
                    .cursor_pointer()
                    .child(
                        div()
                            .flex_1()
                            .pl(px(12.))
                            .text_size(px(16.))
                            .line_height(px(20.))
                            .text_color(color("--text-strong"))
                            .child(filter.kind.label()),
                    )
                    .child(
                        div()
                            .w(px(36.))
                            .pr(px(4.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(icon("settings_select_chevron", 20., "--text-strong")),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if let Some(filters) = this.search_filters.as_mut() {
                            let field = Field::DateKind(index);
                            filters.open = (filters.open != Some(field)).then_some(field);
                        }
                        cx.notify();
                    })),
            )
            .child(
                div()
                    .flex_none()
                    .w(px(254.))
                    .h(px(40.))
                    .rounded(px(8.))
                    .border_1()
                    .border_color(color("--input-border-default"))
                    .bg(color("--input-background-default"))
                    .px(px(12.))
                    .flex()
                    .items_center()
                    .child(div().flex_1().child(filter.text.clone()))
                    .child(icon("search_date_calendar", 18., "--text-strong")),
            )
            .child(
                div()
                    .id(SharedString::from(format!(
                        "search-filters-date-remove-{index}"
                    )))
                    .flex_none()
                    .size(px(40.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .cursor_pointer()
                    .child(icon("search_trash", 24., "--text-muted"))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.remove_date_filter(index, cx);
                    })),
            )
            .into_any_element()
    }

    /// The open field's list under (or, when it would leave the dialog, over) the field.
    fn filters_dropdown(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let field = self.search_filters.as_ref()?.open?;
        let anchor = self.tip_anchors.borrow().get(&probe_key(field)).copied()?;
        let options = self.filter_options(field, cx);
        let anchor = anchor.dilate(px(1.));
        let rows_height = if options.is_empty() {
            56.
        } else {
            options.len() as f32 * 40.
        };
        let height = (rows_height + 8.).min(208.) + 2.;
        let window_height = self.viewport.1;
        // The dialog's own bottom edge: the list may not cross into its footer.
        let below = f32::from(anchor.bottom()) + 4. + height <= window_height - 56. - 64.;
        let top = if below {
            anchor.bottom() + px(4.)
        } else {
            anchor.top() - px(4.) - px(height)
        };
        let multiple = matches!(
            field,
            Field::From | Field::In | Field::Has | Field::Mentions | Field::AuthorType
        );
        let rows: Vec<AnyElement> = options
            .iter()
            .enumerate()
            .map(|(index, option)| {
                div()
                    .id(SharedString::from(format!("search-filters-option-{index}")))
                    .flex_none()
                    .h(px(40.))
                    .rounded(px(4.))
                    .cursor_pointer()
                    .hover(|style| style.bg(color("--background-mod-normal")))
                    .flex()
                    .items_center()
                    .pl(px(8.))
                    .pr(px(4.))
                    .gap(px(4.))
                    .when_some(option.icon, |this, symbol| {
                        this.child(
                            div()
                                .w(px(28.))
                                .ml(px(-4.))
                                .flex()
                                .items_center()
                                .justify_center()
                                .child(icon(symbol, 20., "--text-subtle")),
                        )
                    })
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .pl(px(0.))
                            .text_size(px(16.))
                            .line_height(px(20.))
                            .text_color(color("--text-strong"))
                            .child(option.label.clone()),
                    )
                    .when_some(option.trailing.clone(), |this, trailing| {
                        this.child(
                            div()
                                .text_size(px(14.))
                                .line_height(px(18.))
                                .text_color(color("--text-subtle"))
                                .child(trailing),
                        )
                    })
                    .when(multiple, |this| {
                        this.child(
                            div()
                                .p(px(4.))
                                .child(super::server_games::checkbox(option.checked)),
                        )
                    })
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.pick_filter(field, index, cx);
                    }))
                    .into_any_element()
            })
            .collect();
        Some(
            deferred(
                div()
                    .parity_with_border("search-filters-dropdown", 1.)
                    .id("search-filters-dropdown")
                    .occlude()
                    .absolute()
                    .left(anchor.left())
                    .top(top)
                    .w(anchor.size.width)
                    .h(px(height))
                    .rounded(px(8.))
                    .border_1()
                    .border_color(color("--border-subtle"))
                    .bg(color("--background-surface-higher"))
                    .shadow(vec![gpui::BoxShadow {
                        color: gpui::hsla(0., 0., 0., 0.16),
                        offset: gpui::point(px(0.), px(2.)),
                        blur_radius: px(8.),
                        spread_radius: px(0.),
                        inset: false,
                    }])
                    .on_mouse_down_out(cx.listener(
                        move |this, event: &gpui::MouseDownEvent, _, cx| {
                            let on_field = event.button == gpui::MouseButton::Left
                                && this
                                    .tip_anchors
                                    .borrow()
                                    .get(&probe_key(field))
                                    .is_some_and(|bounds| {
                                        bounds.dilate(px(1.)).contains(&event.position)
                                    });
                            if !on_field
                                && this.press_outside(super::keyboard::Layer::SearchFilters, cx)
                                && event.button != gpui::MouseButton::Right
                            {
                                cx.stop_propagation();
                            }
                        },
                    ))
                    .child(if rows.is_empty() {
                        div()
                            .size_full()
                            .p(px(4.))
                            .child(
                                div()
                                    .h(px(56.))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .text_size(px(16.))
                                    .line_height(px(20.))
                                    .font_weight(FontWeight(500.))
                                    .italic()
                                    .text_color(color("--text-muted"))
                                    .child("No items to show"),
                            )
                            .into_any_element()
                    } else {
                        div()
                            .id("search-filters-list")
                            .size_full()
                            .p(px(4.))
                            .pr(px(16.))
                            .overflow_y_scroll()
                            .flex()
                            .flex_col()
                            .children(rows)
                            .into_any_element()
                    }),
            )
            .with_priority(3)
            .into_any_element(),
        )
    }
}

struct FilterOption {
    id: Id,
    label: String,
    trailing: Option<String>,
    icon: Option<&'static str>,
    checked: bool,
}

impl FilterOption {
    fn user(id: Id, name: String, tag: String) -> Self {
        Self {
            id,
            trailing: (tag != name).then_some(tag),
            label: name,
            icon: Some("search_from"),
            checked: false,
        }
    }
}

/// Icons of the `has:` rows: the six captured ones, and the three beyond the list's first screen from the nearest
/// assets (the list was not scrolled in the capture).
fn has_icon(has: &str) -> &'static str {
    match has {
        "image" => "search_has_image",
        "video" => "search_has_video",
        "link" => "search_has_link",
        "file" => "search_has_file",
        "embed" => "search_has_embed",
        "sound" => "search_has_sound",
        "poll" => "cp_poll",
        "sticker" => "sticker",
        _ => "forwarded_icon",
    }
}

fn author_icon(author: &str) -> &'static str {
    match author {
        "bot" => "oauth_entry_robot",
        "webhook" => "integrations_webhook",
        _ => "search_from",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn day(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).unwrap()
    }

    #[test]
    fn filters_become_a_query_in_the_official_order() {
        let query = filter_query(&Selection {
            from: &["onugeccen".to_owned()],
            mentions: &["lorean".to_owned()],
            channels: &["general".to_owned()],
            has: &["link", "image"],
            dates: &[
                (DateKind::Before, day(2026, 11, 9)),
                (DateKind::After, day(2026, 1, 2)),
            ],
            authors: &["bot"],
            pinned: Some(true),
        });
        assert_eq!(
            query,
            "from:onugeccen mentions:lorean in:general has:link has:image before:2026-11-09 \
             after:2026-01-02 authorType:bot pinned:true"
        );
        let nothing = Selection {
            from: &[],
            mentions: &[],
            channels: &[],
            has: &[],
            dates: &[],
            authors: &[],
            pinned: None,
        };
        assert_eq!(filter_query(&nothing), "");
        assert_eq!(
            filter_query(&Selection {
                pinned: Some(false),
                ..nothing
            }),
            "pinned:false"
        );
    }

    #[test]
    fn date_filters_bound_by_snowflake_like_the_official_tokens() {
        let date = day(2026, 11, 9);
        let start = Utc
            .with_ymd_and_hms(2026, 11, 9, 0, 0, 0)
            .unwrap()
            .timestamp_millis();
        let next = start + 86_400_000;
        let (min, max) = date_bounds(&Utc, DateKind::Before, date);
        assert_eq!((min, max), (None, Some(snowflake(start))));
        let (min, max) = date_bounds(&Utc, DateKind::After, date);
        assert_eq!((min, max), (Some(snowflake(next)), None));
        let (min, max) = date_bounds(&Utc, DateKind::During, date);
        assert_eq!((min, max), (Some(snowflake(start)), Some(snowflake(next))));
        // The Discord epoch is the zero of the snowflake clock.
        assert_eq!(snowflake(1_420_070_400_000), 0);
        assert_eq!(snowflake(1_420_070_400_001), 1 << 22);
        assert_eq!(snowflake(0), 0);
    }

    #[test]
    fn typed_dates_read_in_the_pickers_format_and_iso() {
        assert_eq!(parse_date("Nov 9, 2026"), Some(day(2026, 11, 9)));
        assert_eq!(parse_date(" 2026-11-09 "), Some(day(2026, 11, 9)));
        assert_eq!(parse_date("2026-13-09"), None);
        assert_eq!(parse_date("tomorrow"), None);
        assert_eq!(date_text(day(2026, 11, 9)), "Nov 9, 2026");
        // The field is read back to the same day.
        assert_eq!(
            parse_date(&date_text(day(2026, 1, 31))),
            Some(day(2026, 1, 31))
        );
    }

    #[test]
    fn the_has_list_keeps_the_dropdowns_order() {
        assert_eq!(&HAS[..5], &["image", "video", "link", "file", "embed"]);
        assert_eq!(HAS.len(), 9);
        assert_eq!(has_icon("image"), "search_has_image");
    }
}
