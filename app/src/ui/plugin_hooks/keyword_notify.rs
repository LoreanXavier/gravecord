//! KeywordNotify (Equicord `src/equicordplugins/keywordNotify`, GPL-3.0-or-later): a message that matches a keyword
//! entry (`plugins/keyword_notify.rs`) gets you added to its mentions before the client sees it (`m.mentions.push`),
//! so the client's own mention path does the rest: the mention highlight, the unread mention count and the desktop
//! notification with its sound (by the notification settings, as for any mention). That holds for messages that
//! arrive (`MESSAGE_CREATE`, `MESSAGE_UPDATE`) and for loaded pages (`LOAD_MESSAGES_SUCCESS`, highlight only: a page
//! raises no notification). A matching message by someone else is also logged, newest first, at most
//! `amountToKeep`, and kept across restarts; the Inbox gets a Keywords tab after Scheduled with the log, each
//! message under its channel with Jump to Message and Mark As Read (which takes it out of the log), the first match
//! of its text highlighted, and Clear All (the double check) in the header. The entries are edited in the card's own
//! section (`KeywordEntries`): a "Keyword Entry N" fold per entry with its expression and delete button, Ignore
//! Case, Ignore Bots, the Whitelist and Blacklist with Add ID and a box per ID, and List priority (Advanced); Add
//! Keyword Entry under them. Not part of the pinned client.
//!
//! The original's Mark As Read button drops the message from the shown log only (its `discardMessage` is never
//! called), so it came back after a restart; here it leaves the stored log too.

use super::super::{Root, color, icon};
use crate::editor::{Editor, EditorEvent};
use crate::plugins::keyword_notify::{self as rules, Entry, List, Seen};
use dc_state::{Id, Message};
use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyElement, AppContext as _, Context, Entity, FontWeight, InteractiveElement, IntoElement,
    ParentElement, SharedString, StatefulInteractiveElement, Styled, Subscription, div, px,
};
use regex::Regex;
use serde_json::{Value, json};
use std::cell::RefCell;
use std::collections::{BTreeSet, HashMap};

const ID: &str = "KeywordNotify";
/// `KEYWORD_ENTRIES_KEY` and `KEYWORD_LOG_KEY`, kept apart from the settings as the original's data store keys.
const ENTRIES_KEY: &str = "plugin-keyword-notify-entries";
const LOG_KEY: &str = "plugin-keyword-notify-log";
/// The Inbox tab's label (`keywordTabBar`).
pub(in crate::ui) const TAB_LABEL: &str = "Keywords";

/// A text box of the entries' section: an entry's expression, or one ID of its whitelist or blacklist.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Field {
    Regex(usize),
    White(usize, usize),
    Black(usize, usize),
}

#[derive(Default)]
struct Kept {
    /// The entries and their expressions, read once (`start`).
    entries: Option<Vec<(Entry, Option<Regex>)>>,
    /// The log, newest first, read once.
    log: Option<Vec<Value>>,
    /// The section's open folds (`Collapsible`, closed at first).
    open: BTreeSet<usize>,
    /// The section's boxes, made when first drawn and dropped when the entries change shape.
    fields: HashMap<Field, (Entity<Editor>, Subscription)>,
}

thread_local! {
    static KEPT: RefCell<Kept> = RefCell::new(Kept::default());
}

fn on() -> bool {
    !crate::plugins::bypassed() && crate::plugins::on(ID)
}

/// The plugin's `amountToKeep`.
fn keep() -> usize {
    crate::plugins::with(|state| state.value(ID, "amountToKeep").as_f64().unwrap_or(50.)) as usize
}

fn with_entries<R>(run: impl FnOnce(&mut Vec<(Entry, Option<Regex>)>) -> R) -> R {
    KEPT.with(|kept| {
        let mut kept = kept.borrow_mut();
        let entries = kept.entries.get_or_insert_with(|| {
            // `entry.ignoreBots ?? settings.store.ignoreBots`.
            let bots = crate::plugins::with(|state| state.value(ID, "ignoreBots"))
                .as_bool()
                .unwrap_or(true);
            let stored = if cfg!(test) {
                None
            } else {
                crate::storage::get(ENTRIES_KEY)
            };
            stored
                .and_then(|value| value.as_array().cloned())
                .unwrap_or_default()
                .iter()
                .map(|value| {
                    let entry = Entry::from_value(value, bots);
                    let regex = entry.compile();
                    (entry, regex)
                })
                .collect()
        });
        run(entries)
    })
}

/// Changes the entries, compiles the changed ones and stores them (`DataStore.set(KEYWORD_ENTRIES_KEY)`).
fn edit_entries(change: impl FnOnce(&mut Vec<(Entry, Option<Regex>)>)) {
    let stored = with_entries(|entries| {
        change(entries);
        for (entry, regex) in entries.iter_mut() {
            *regex = entry.compile();
        }
        Value::Array(entries.iter().map(|(entry, _)| entry.to_value()).collect())
    });
    if !cfg!(test) {
        crate::storage::set(ENTRIES_KEY, stored);
    }
}

fn with_log<R>(run: impl FnOnce(&mut Vec<Value>) -> R) -> R {
    KEPT.with(|kept| {
        let mut kept = kept.borrow_mut();
        let log = kept.log.get_or_insert_with(|| {
            let stored = if cfg!(test) {
                None
            } else {
                crate::storage::get(LOG_KEY)
            };
            stored
                .and_then(|value| value.as_array().cloned())
                .unwrap_or_default()
        });
        run(log)
    })
}

fn store_log() {
    if cfg!(test) {
        return;
    }
    let log = with_log(|log| Value::Array(log.clone()));
    crate::storage::set(LOG_KEY, log);
}

impl Root {
    /// `applyKeywordEntries` on one message: you join its mentions when an entry matches; true when it did.
    fn keyword_notify_apply(&self, message: &mut Value, page_channel: Option<Id>) -> bool {
        let channel = message["channel_id"]
            .as_str()
            .and_then(|id| id.parse::<Id>().ok())
            .or(page_channel);
        let guild = message["guild_id"]
            .as_str()
            .and_then(|id| id.parse::<Id>().ok())
            .or_else(|| channel.and_then(|channel| self.guild_of_channel(channel)))
            .map(|guild| guild.to_string());
        let channel = channel
            .map(|channel| channel.to_string())
            .unwrap_or_default();
        let author = message["author"]["id"]
            .as_str()
            .unwrap_or_default()
            .to_owned();
        let matched = with_entries(|entries| {
            rules::matches(
                entries,
                &Seen {
                    author: &author,
                    bot: message["author"]["bot"] == Value::Bool(true),
                    channel: &channel,
                    guild: guild.as_deref(),
                    message,
                },
            )
        });
        if !matched {
            return false;
        }
        let me = self.stores.current_user.as_ref().map(|user| user.id);
        if let Some(me) = me
            && let Some(mentions) = message["mentions"].as_array_mut()
            && !mentions
                .iter()
                .any(|known| known["id"].as_str() == Some(me.to_string().as_str()))
        {
            mentions.push(json!({ "id": me.to_string() }));
        }
        // `m.author.id !== id`: your own matching messages are not logged.
        if me.map(|me| me.to_string()) != Some(author) {
            let mut logged = message.clone();
            if logged["channel_id"].is_null() {
                logged["channel_id"] = Value::from(channel);
            }
            let keep = keep();
            if with_log(|log| rules::add_to_log(log, logged, keep)) {
                store_log();
            }
        }
        true
    }

    /// The interceptor for a gateway event: `MESSAGE_CREATE` and `MESSAGE_UPDATE` carry the message.
    pub(in crate::ui) fn keyword_notify_dispatch(&self, name: &str, mut data: Value) -> Value {
        if !matches!(name, "MESSAGE_CREATE" | "MESSAGE_UPDATE") || !on() {
            return data;
        }
        self.keyword_notify_apply(&mut data, None);
        data
    }

    /// The interceptor for `LOAD_MESSAGES_SUCCESS`: each message of a loaded page.
    pub(in crate::ui) fn keyword_notify_page(&self, channel: Id, mut page: Value) -> Value {
        if !on() {
            return page;
        }
        if let Some(messages) = page.as_array_mut() {
            for message in messages {
                self.keyword_notify_apply(message, Some(channel));
            }
        }
        page
    }

    /// Whether the Inbox has the Keywords tab.
    pub(in crate::ui) fn keyword_notify_tab(&self) -> bool {
        on()
    }

    /// `keywordClearButton`: Clear All empties the log.
    pub(in crate::ui) fn keyword_notify_clear_all(&self, cx: &mut Context<Self>) -> AnyElement {
        div()
            .mr(px(8.))
            .child(self.inbox_button(
                "keyword-notify-clear".to_owned(),
                "plugin_double_check",
                cx,
                |_, cx| {
                    with_log(Vec::clear);
                    store_log();
                    cx.notify();
                },
            ))
            .into_any_element()
    }

    /// `tryKeywordMenu`: the log's messages, each under its channel with Jump to Message and Mark As Read
    /// (`deleteKeyword`), its text with the first match highlighted (`highlightKeywords`).
    pub(in crate::ui) fn keyword_notify_page_cards(
        &self,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        if !on() {
            return Vec::new();
        }
        let log = with_log(|log| log.clone());
        log.iter()
            .filter_map(Message::from_value)
            .map(|message| {
                let jump = message.clone();
                let id = message.id;
                let buttons = vec![
                    self.inbox_button(
                        format!("keyword-jump-{id}"),
                        "inbox_jump",
                        cx,
                        move |this, cx| {
                            this.inbox = None;
                            this.jump_to_message_checked(&jump, cx);
                        },
                    ),
                    self.inbox_button(
                        format!("keyword-read-{id}"),
                        "inbox_mark_read",
                        cx,
                        move |_, cx| {
                            let key = id.to_string();
                            with_log(|log| log.retain(|known| known["id"].as_str() != Some(&key)));
                            store_log();
                            cx.notify();
                        },
                    ),
                ];
                let content = keyword_content(&message.content);
                div()
                    .mb(px(16.))
                    .rounded(px(12.))
                    .border_1()
                    .border_color(color("--border-subtle"))
                    .bg(color("--background-surface-higher"))
                    .overflow_hidden()
                    .child(self.inbox_channel_header(&message, buttons))
                    .child(self.inbox_message_with(&message, content, cx))
                    .into_any_element()
            })
            .collect()
    }

    /// The box of one field of the entries' section, made on its first drawing; typing stores the entries.
    fn keyword_field(&self, field: Field, text: &str, cx: &mut Context<Self>) -> Entity<Editor> {
        if let Some(editor) = KEPT.with(|kept| {
            kept.borrow()
                .fields
                .get(&field)
                .map(|(editor, _)| editor.clone())
        }) {
            return editor;
        }
        let editor = cx.new(|cx| {
            let mut editor = Editor::new(super::super::editor_style(), cx);
            editor.set_single_line(true);
            editor.set_text(text);
            editor
        });
        let typing = cx.subscribe(&editor, move |_, editor, _: &EditorEvent, cx| {
            let text = editor.read(cx).text().to_owned();
            edit_entries(|entries| match field {
                Field::Regex(at) => {
                    if let Some((entry, _)) = entries.get_mut(at) {
                        entry.regex = text;
                    }
                }
                // `ListedIds`: `values[index] = e.trim()`.
                Field::White(at, index) => {
                    if let Some(id) = entries
                        .get_mut(at)
                        .and_then(|(entry, _)| entry.whitelist.get_mut(index))
                    {
                        *id = text.trim().to_owned();
                    }
                }
                Field::Black(at, index) => {
                    if let Some(id) = entries
                        .get_mut(at)
                        .and_then(|(entry, _)| entry.blacklist.get_mut(index))
                    {
                        *id = text.trim().to_owned();
                    }
                }
            });
            cx.notify();
        });
        KEPT.with(|kept| {
            kept.borrow_mut()
                .fields
                .insert(field, (editor.clone(), typing));
        });
        editor
    }

    /// The card's own section (`keywords`, `KeywordEntries`).
    pub(in crate::ui) fn keyword_notify_settings(&self, cx: &mut Context<Self>) -> AnyElement {
        let entries: Vec<Entry> =
            with_entries(|entries| entries.iter().map(|(entry, _)| entry.clone()).collect());
        let open = KEPT.with(|kept| kept.borrow().open.clone());
        let mut section = div().flex().flex_col().gap(px(8.));
        for (at, entry) in entries.iter().enumerate() {
            let folded = !open.contains(&at);
            let mut block = div().flex().flex_col().gap(px(8.)).child(
                div()
                    .id(SharedString::from(format!("keyword-entry-{at}")))
                    .flex()
                    .items_center()
                    .gap(px(4.))
                    .cursor_pointer()
                    .on_click(cx.listener(move |_, _, _, cx| {
                        KEPT.with(|kept| {
                            let mut kept = kept.borrow_mut();
                            if !kept.open.remove(&at) {
                                kept.open.insert(at);
                            }
                        });
                        cx.notify();
                    }))
                    .child(
                        div()
                            .pr(px(5.))
                            .text_color(color("--text-muted"))
                            .child(if folded { "\u{25b6}" } else { "\u{25bc}" }),
                    )
                    .child(
                        div()
                            .text_size(px(16.))
                            .font_weight(FontWeight(600.))
                            .text_color(color("--text-strong"))
                            .child(format!("Keyword Entry {}", at + 1)),
                    ),
            );
            if !folded {
                block = block
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .child(text_box(
                                self.keyword_field(Field::Regex(at), &entry.regex, cx),
                                entry.regex.is_empty().then_some("example|regex"),
                            ))
                            .child(delete_button(
                                format!("keyword-delete-{at}"),
                                cx,
                                move || {
                                    edit_entries(|entries| {
                                        if at < entries.len() {
                                            entries.remove(at);
                                        }
                                    });
                                    reshaped();
                                },
                            )),
                    )
                    .child(switch_row(
                        format!("keyword-case-{at}"),
                        "Ignore Case",
                        "",
                        entry.ignore_case,
                        cx,
                        move |on| {
                            edit_entries(|entries| {
                                if let Some((entry, _)) = entries.get_mut(at) {
                                    entry.ignore_case = on;
                                }
                            })
                        },
                    ))
                    .child(switch_row(
                        format!("keyword-bots-{at}"),
                        "Ignore Bots",
                        "Ignore messages from bots",
                        entry.ignore_bots,
                        cx,
                        move |on| {
                            edit_entries(|entries| {
                                if let Some((entry, _)) = entries.get_mut(at) {
                                    entry.ignore_bots = on;
                                }
                            })
                        },
                    ))
                    .child(self.keyword_list(at, true, &entry.whitelist, cx))
                    .child(self.keyword_list(at, false, &entry.blacklist, cx))
                    .child(priority_select(at, entry.priority, cx));
            }
            section = section.child(block);
        }
        section
            .child(div().child(action_button(
                "keyword-add-entry",
                "Add Keyword Entry",
                cx,
                || {
                    edit_entries(|entries| {
                        let entry = Entry::new();
                        let regex = entry.compile();
                        entries.push((entry, regex));
                    });
                    reshaped();
                },
            )))
            .into_any_element()
    }

    /// A Whitelist or Blacklist: its heading with Add ID, then a box with a delete button per ID.
    fn keyword_list(
        &self,
        at: usize,
        white: bool,
        ids: &[String],
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let name = if white { "Whitelist" } else { "Blacklist" };
        let mut list = div().flex().flex_col().gap(px(5.)).child(
            div()
                .flex()
                .items_center()
                .child(
                    div()
                        .flex_1()
                        .text_size(px(12.))
                        .font_weight(FontWeight(600.))
                        .text_color(color("--text-default"))
                        .child(name),
                )
                .child(action_button(
                    if white {
                        "keyword-add-white"
                    } else {
                        "keyword-add-black"
                    },
                    "Add ID",
                    cx,
                    move || {
                        edit_entries(|entries| {
                            if let Some((entry, _)) = entries.get_mut(at) {
                                let list = if white {
                                    &mut entry.whitelist
                                } else {
                                    &mut entry.blacklist
                                };
                                list.push(String::new());
                            }
                        });
                        reshaped();
                    },
                )),
        );
        for (index, id) in ids.iter().enumerate() {
            let field = if white {
                Field::White(at, index)
            } else {
                Field::Black(at, index)
            };
            list = list.child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(text_box(
                        self.keyword_field(field, id, cx),
                        id.is_empty().then_some("ID"),
                    ))
                    .child(delete_button(
                        format!("keyword-delete-{name}-{at}-{index}"),
                        cx,
                        move || {
                            edit_entries(|entries| {
                                if let Some((entry, _)) = entries.get_mut(at) {
                                    let list = if white {
                                        &mut entry.whitelist
                                    } else {
                                        &mut entry.blacklist
                                    };
                                    if index < list.len() {
                                        list.remove(index);
                                    }
                                }
                            });
                            reshaped();
                        },
                    )),
            );
        }
        list.into_any_element()
    }
}

/// The boxes follow the entries by place, so a removed or added row makes them again from the entries.
fn reshaped() {
    KEPT.with(|kept| kept.borrow_mut().fields.clear());
}

/// The log's text with its first match highlighted (`.highlight`: yellow 300 at 30%).
fn keyword_content(text: &str) -> AnyElement {
    let cut = with_entries(|entries| {
        rules::highlight(text, entries).map(|(a, b, c)| (a.to_owned(), b.to_owned(), c.to_owned()))
    });
    let body = div()
        .text_size(px(16.))
        .line_height(px(22.))
        .text_color(color("--text-default"));
    match cut {
        Some((before, found, after)) => {
            let mut highlight = color("--yellow-300");
            highlight.a = 0.3;
            body.flex()
                .flex_wrap()
                .child(before)
                .child(div().bg(highlight).child(found))
                .child(after)
                .into_any_element()
        }
        None => body.child(text.to_owned()).into_any_element(),
    }
}

fn text_box(editor: Entity<Editor>, placeholder: Option<&'static str>) -> AnyElement {
    div()
        .relative()
        .flex_1()
        .min_w(px(0.))
        .h(px(40.))
        .px(px(12.))
        .rounded(px(8.))
        .border_1()
        .border_color(color("--input-border-default"))
        .bg(color("--input-background-default"))
        .flex()
        .items_center()
        .text_size(px(16.))
        .text_color(color("--text-default"))
        .child(div().flex_1().child(editor))
        .children(placeholder.map(|text| {
            div()
                .absolute()
                .left(px(12.))
                .text_color(color("--input-placeholder-text-default"))
                .child(text)
        }))
        .into_any_element()
}

fn delete_button(id: String, cx: &mut Context<Root>, run: impl Fn() + 'static) -> AnyElement {
    div()
        .id(SharedString::from(id))
        .flex_none()
        .size(px(24.))
        .cursor_pointer()
        .flex()
        .items_center()
        .justify_center()
        .child(icon(
            "menu_delete_message",
            20.,
            "--interactive-icon-default",
        ))
        .on_click(cx.listener(move |_, _, _, cx| {
            run();
            cx.notify();
        }))
        .into_any_element()
}

fn action_button(
    id: &'static str,
    text: &'static str,
    cx: &mut Context<Root>,
    run: impl Fn() + 'static,
) -> AnyElement {
    div()
        .id(id)
        .flex_none()
        .h(px(32.))
        .px(px(16.))
        .rounded(px(8.))
        .flex()
        .items_center()
        .justify_center()
        .cursor_pointer()
        .bg(color("--control-primary-background-default"))
        .hover(|style| style.bg(color("--control-primary-background-hover")))
        .text_size(px(14.))
        .font_weight(FontWeight(500.))
        .text_color(color("--white"))
        .child(text)
        .on_click(cx.listener(move |_, _, _, cx| {
            run();
            cx.notify();
        }))
        .into_any_element()
}

fn switch_row(
    id: String,
    title: &'static str,
    description: &'static str,
    on: bool,
    cx: &mut Context<Root>,
    changed: impl Fn(bool) + 'static,
) -> AnyElement {
    div()
        .id(SharedString::from(id))
        .flex()
        .items_center()
        .gap(px(16.))
        .cursor_pointer()
        .on_click(cx.listener(move |_, _, _, cx| {
            changed(!on);
            cx.notify();
        }))
        .child(
            div()
                .flex_1()
                .flex()
                .flex_col()
                .gap(px(4.))
                .child(
                    div()
                        .text_size(px(16.))
                        .font_weight(FontWeight(500.))
                        .text_color(color("--text-strong"))
                        .child(title),
                )
                .when(!description.is_empty(), |this| {
                    this.child(
                        div()
                            .text_size(px(14.))
                            .text_color(color("--text-subtle"))
                            .child(description),
                    )
                }),
        )
        .child(super::super::create_channel::switch(on))
        .into_any_element()
}

/// `ListPrioritySelector` under "List priority (Advanced)".
fn priority_select(at: usize, priority: List, cx: &mut Context<Root>) -> AnyElement {
    let option = |list: List, cx: &mut Context<Root>| {
        let selected = list == priority;
        div()
            .id(SharedString::from(format!(
                "keyword-priority-{at}-{}",
                list.as_str()
            )))
            .flex()
            .items_center()
            .gap(px(8.))
            .p(px(8.))
            .rounded(px(8.))
            .cursor_pointer()
            .when(selected, |this| this.bg(color("--background-mod-normal")))
            .hover(|style| style.bg(color("--background-mod-muted")))
            .on_click(cx.listener(move |_, _, _, cx| {
                edit_entries(|entries| {
                    if let Some((entry, _)) = entries.get_mut(at) {
                        entry.priority = list;
                    }
                });
                cx.notify();
            }))
            .child(super::super::settings::radio_dot(selected))
            .child(
                div()
                    .text_size(px(16.))
                    .text_color(color("--text-default"))
                    .child(if list == List::Whitelist {
                        "Whitelist"
                    } else {
                        "Blacklist"
                    }),
            )
    };
    div()
        .flex()
        .flex_col()
        .gap(px(4.))
        .child(
            div()
                .text_size(px(16.))
                .font_weight(FontWeight(500.))
                .text_color(color("--text-strong"))
                .child("List priority (Advanced)"),
        )
        .child(
            div()
                .text_size(px(14.))
                .text_color(color("--text-subtle"))
                .child("Which list to prioritize in case a message triggers both lists"),
        )
        .child(
            div()
                .text_size(px(14.))
                .text_color(color("--text-subtle"))
                .child(
                    "If you are not sure, in most common scenarios it is usually set to Blacklist",
                ),
        )
        .child(option(List::Whitelist, cx))
        .child(option(List::BlackList, cx))
        .into_any_element()
}
