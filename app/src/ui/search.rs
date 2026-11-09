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

fn encode(text: &str) -> String {
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
    fn ensure_search(&mut self, cx: &mut Context<Self>) -> Entity<Editor> {
        if let Some(search) = self.search.as_ref() {
            return search.field.clone();
        }
        let field = cx.new(|cx| {
            let mut style = editor_style();
            style.size = 14.;
            style.line_height = 22.;
            Editor::new(style, cx)
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
        }
        cx.notify();
    }

    pub(super) fn run_search(&mut self, query: String, cx: &mut Context<Self>) {
        if query.trim().is_empty() {
            return;
        }
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
        params.push("sort_by=timestamp".to_owned());
        params.push(format!(
            "sort_order={}",
            if oldest_first { "asc" } else { "desc" }
        ));
        params.push("offset=0".to_owned());
        let route = match (self.guild, self.channel) {
            (Some(guild), _) => format!("/guilds/{guild}/messages/search?{}", params.join("&")),
            (None, Some(channel)) => {
                format!("/channels/{channel}/messages/search?{}", params.join("&"))
            }
            _ => return,
        };
        self.fetched.remove(RESULTS_KEY);
        let _ = self.requests.send(Request::Fetch {
            key: RESULTS_KEY.to_owned(),
            route,
        });
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
                    .left(anchor.right() - px(360.))
                    .top(anchor.bottom() + px(8.))
                    .child(
                        div()
                            .id("search-popout")
                            .occlude()
                            .w(px(356.))
                            .pt(px(12.))
                            .pb(px(12.))
                            .px(px(8.))
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
        let lower = text.to_lowercase();
        let mut marks = vec![false; text.len()];
        for word in words {
            let mut from = 0;
            while let Some(at) = lower[from..].find(word.as_str()) {
                let start = from + at;
                for mark in marks.iter_mut().skip(start).take(word.len()) {
                    *mark = true;
                }
                from = start + word.len().max(1);
            }
        }
        let mut runs: Vec<TextRun> = Vec::new();
        let mut index = 0;
        let mut base = gpui::font(super::SANS);
        base.weight = FontWeight(weight);
        while index < text.len() {
            let marked = marks[index];
            let mut end = index;
            while end < text.len() && marks[end] == marked {
                end += 1;
            }
            while !text.is_char_boundary(end) {
                end += 1;
            }
            runs.push(TextRun {
                len: end - index,
                font: base.clone(),
                color: color("--text-default").into(),
                background_color: marked.then(|| gpui::rgba(HIGHLIGHT).into()),
                underline: None,
                strikethrough: None,
            });
            index = end;
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
                    .child(self.highlighted(&message.content, words, 16., 400.)),
            )
            .into_any_element()
    }

    /// `searchResultsWrap_QHVbp3`: the count, Filters and Sort, then results grouped by channel.
    pub(super) fn search_panel(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let search = self.search.as_ref()?;
        let query = search.shown.clone()?;
        let words = plain_words(&query);
        let body = self.fetched.get(RESULTS_KEY);
        let total = body.and_then(|body| body["total_results"].as_u64());
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
                        .h(px(49.))
                        .px(px(16.))
                        .flex()
                        .items_center()
                        .child(
                            div()
                                .flex_1()
                                .line_height(px(20.))
                                .font_weight(FontWeight(500.))
                                .text_color(color("--text-strong"))
                                .child(match total {
                                    Some(1) => "1 Result".to_owned(),
                                    Some(count) => format!("{count} Results"),
                                    None => "Searching\u{2026}".to_owned(),
                                }),
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
                                        }
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
                        .px(px(16.))
                        .pt(px(16.))
                        .children(groups),
                )
                .into_any_element(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::{operators, plain_words};

    #[test]
    fn operators_leave_the_content() {
        let (found, content) = operators("from:onugeccen hello in:#general has:link world");
        assert_eq!(content, "hello world");
        assert_eq!(found.len(), 3);
        assert_eq!(found[1], ("in", "general".to_owned()));
        assert_eq!(plain_words("from:x Fixture"), vec!["fixture".to_owned()]);
    }
}
