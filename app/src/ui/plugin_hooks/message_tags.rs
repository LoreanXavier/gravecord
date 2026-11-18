//! MessageTags (now CustomCommands; Vencord `src/plugins/customCommands`, GPL-3.0-or-later): tags you save become
//! built-in slash commands named after them. A tag's response may hold `{{argument}}` or `{{argument = default}}`, each
//! an option of its command (required when it has no default), plus `ephemeral`, which makes Clyde say it to you alone;
//! `\n` is a line break. `/tags create` opens the tag dialog, `/tags list` has Clyde list them (72 characters of each),
//! `/tags delete` removes one. The plugin's settings list the registered tags with Edit and Delete and a Create Tag
//! button (`SettingsTagList`). The dialog (Create New Tag / Edit Tag) has Name and Response, the detected arguments,
//! a notice when `ephemeral` is used or the name exists, and the arguments guide. Tags are kept apart from the settings
//! (storage key `plugin-message-tags`), as the original's `tagsList`. Not part of the pinned client.

use super::super::Root;
use super::super::slash::{BOOLEAN, Cmd, Filled, Opt, STRING, SUB_COMMAND, built_in, string_value};
use super::super::{color, editor_style, modal};
use crate::editor::{Editor, EditorEvent};
use dc_state::Id;
use gpui::AppContext as _;
use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyElement, Context, Entity, FontWeight, InteractiveElement, IntoElement, ParentElement,
    SharedString, StatefulInteractiveElement, Styled, Subscription, div, px,
};
use regex::Regex;
use serde_json::{Value, json};
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::sync::OnceLock;

const ID: &str = "MessageTags";
const STORAGE_KEY: &str = "plugin-message-tags";
/// `EXAMPLE_RESPONSE`.
const EXAMPLE: &str = "Hello {{user}}! I am feeling {{mood = great}}.";
/// A tag command's place among the plugins' commands: this bit and the tag's place in the sorted list.
pub(in crate::ui) const TAG_BIT: u16 = 0x8000;

/// A tag.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::ui) struct Tag {
    pub(in crate::ui) name: String,
    pub(in crate::ui) message: String,
}

fn argument_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\{\{(.+?)\}\}").expect("argument regex"))
}

/// `parseTagArguments`: each argument once, by its lower-cased name, with its default.
pub(in crate::ui) fn arguments(message: &str) -> Vec<(String, Option<String>)> {
    let mut out: Vec<(String, Option<String>)> = Vec::new();
    for caps in argument_regex().captures_iter(message) {
        let mut parts = caps[1].split('=').map(str::trim);
        let name = parts.next().unwrap_or_default();
        if name.is_empty() || out.iter().any(|(known, _)| known == name) {
            continue;
        }
        out.push((name.to_lowercase(), parts.next().map(str::to_owned)));
    }
    out
}

/// A tag's response with the given arguments (`findOption(args, name) ?? default ?? {{…}}`).
pub(in crate::ui) fn response(message: &str, given: &dyn Fn(&str) -> Option<String>) -> String {
    argument_regex()
        .replace_all(message, |caps: &regex::Captures| {
            let mut parts = caps[1].split('=').map(str::trim);
            let name = parts.next().unwrap_or_default();
            given(name)
                .or_else(|| parts.next().map(str::to_owned))
                .unwrap_or_else(|| caps[0].to_owned())
        })
        .replace("\\n", "\n")
}

/// `/tags list`'s line of a tag.
fn list_line(tag: &Tag) -> String {
    let short: String = tag
        .message
        .chars()
        .take(72)
        .collect::<String>()
        .replace("\\n", " ");
    let more = if tag.message.chars().count() > 72 {
        "..."
    } else {
        ""
    };
    format!("`{}`: {short}{more}", tag.name)
}

fn on() -> bool {
    !crate::plugins::bypassed() && crate::plugins::on(ID)
}

fn load() -> BTreeMap<String, Tag> {
    if cfg!(test) {
        return BTreeMap::new();
    }
    crate::storage::get(STORAGE_KEY)
        .and_then(|value| value.as_object().cloned())
        .map(|map| {
            map.into_iter()
                .filter_map(|(name, tag)| {
                    Some((
                        name.clone(),
                        Tag {
                            name,
                            message: tag["message"].as_str()?.to_owned(),
                        },
                    ))
                })
                .collect()
        })
        .unwrap_or_default()
}

fn save(tags: &BTreeMap<String, Tag>) {
    if cfg!(test) {
        return;
    }
    let value: serde_json::Map<String, Value> = tags
        .iter()
        .map(|(name, tag)| {
            (
                name.clone(),
                json!({"name": tag.name, "message": tag.message}),
            )
        })
        .collect();
    crate::storage::set(STORAGE_KEY, Value::Object(value));
}

/// The open tag dialog: its fields and the tag it edits.
struct Dialog {
    name: Entity<Editor>,
    message: Entity<Editor>,
    editing: Option<String>,
    guide: bool,
    _subs: Vec<Subscription>,
}

thread_local! {
    static DIALOG: RefCell<Option<Dialog>> = const { RefCell::new(None) };
}

impl Root {
    /// `/tags` and a command per tag, while the plugin is on.
    pub(in crate::ui) fn message_tags_commands(&self, tags_index: u16) -> Vec<Cmd> {
        if !on() {
            return Vec::new();
        }
        let mut create = Opt::new("create", SUB_COMMAND, "Create a new tag", false);
        create.options = Vec::new();
        let list = Opt::new("list", SUB_COMMAND, "List all your tags", false);
        let mut delete = Opt::new("delete", SUB_COMMAND, "Remove a tag by name", false);
        delete.options = vec![Opt::new("tag-name", STRING, "The name of the tag", true)];
        let mut out = vec![built_in(
            -2000,
            "tags",
            super::super::slash::Builtin::Plugin(tags_index),
            "Manage all custom commands",
            vec![create, list, delete],
        )];
        for (index, tag) in load().values().enumerate() {
            let mut options: Vec<Opt> = arguments(&tag.message)
                .into_iter()
                .map(|(name, default)| Opt::new(&name, STRING, &name, default.is_none()))
                .collect();
            options.push(Opt::new(
                "ephemeral",
                BOOLEAN,
                "Whether the response should only be visible to you",
                false,
            ));
            out.push(built_in(
                -2001 - index as i32,
                &tag.name,
                super::super::slash::Builtin::Plugin(TAG_BIT | index as u16),
                &tag.name,
                options,
            ));
        }
        out
    }

    /// `/tags` ran.
    pub(in crate::ui) fn message_tags_manage(
        &mut self,
        filled: &[Filled],
        channel: Id,
        cx: &mut Context<Self>,
    ) {
        let sub = filled
            .iter()
            .find(|step| step.opt.kind == SUB_COMMAND)
            .map(|step| step.opt.name.clone())
            .unwrap_or_default();
        match sub.as_str() {
            "create" => self.open_tag_dialog(None, cx),
            "delete" => {
                let name = string_value(filled, "tag-name");
                let mut tags = load();
                if tags.remove(&name).is_none() {
                    self.clyde_says(
                        channel,
                        &format!("A Tag with the name **{name}** does not exist!"),
                        "Clyde",
                    );
                    return;
                }
                save(&tags);
                self.clyde_says(
                    channel,
                    &format!("Successfully deleted the tag **{name}**!"),
                    "Clyde",
                );
            }
            _ => {
                let content = load()
                    .values()
                    .map(list_line)
                    .collect::<Vec<_>>()
                    .join("\n");
                let content = if content.is_empty() {
                    "Woops! There are no tags yet, use `/tags create` to create one!".to_owned()
                } else {
                    content
                };
                self.clyde_says(channel, &content, "Clyde");
            }
        }
    }

    /// A tag's command ran: its response is sent, or said by Clyde with `ephemeral`.
    pub(in crate::ui) fn message_tags_run(&mut self, index: usize, filled: &[Filled], channel: Id) {
        let Some(tag) = load().into_values().nth(index) else {
            return;
        };
        let ephemeral = filled
            .iter()
            .find(|step| step.opt.name == "ephemeral")
            .is_some_and(|step| step.value == Value::Bool(true));
        let text = response(&tag.message, &|name: &str| {
            filled
                .iter()
                .find(|step| step.opt.name == name.to_lowercase())
                .and_then(|step| step.value.as_str().map(str::to_owned))
        });
        if ephemeral {
            self.clyde_says(channel, &text, "Clyde");
        } else {
            self.last_typing_sent = None;
            self.send(text);
        }
    }

    /// Create New Tag, or Edit Tag for `tag`.
    pub(in crate::ui) fn open_tag_dialog(&mut self, tag: Option<Tag>, cx: &mut Context<Self>) {
        let (name_text, message_text) = tag
            .as_ref()
            .map(|tag| (tag.name.clone(), tag.message.replace("\\n", "\n")))
            .unwrap_or_default();
        let name = cx.new(|cx| {
            let mut editor = Editor::new(editor_style(), cx);
            editor.set_single_line(true);
            editor.set_placeholder("greet".to_owned());
            editor.set_text(&name_text);
            editor
        });
        let message = cx.new(|cx| {
            let mut editor = Editor::new(editor_style(), cx);
            editor.set_placeholder(EXAMPLE.to_owned());
            editor.set_text(&message_text);
            editor
        });
        let notify =
            |this: &mut Root, _: Entity<Editor>, event: &EditorEvent, cx: &mut Context<Root>| {
                if matches!(event, EditorEvent::Cancel) {
                    DIALOG.with(|dialog| dialog.borrow_mut().take());
                }
                let _ = this;
                cx.notify();
            };
        let subs = vec![cx.subscribe(&name, notify), cx.subscribe(&message, notify)];
        DIALOG.with(|dialog| {
            *dialog.borrow_mut() = Some(Dialog {
                name,
                message,
                editing: tag.map(|tag| tag.name),
                guide: false,
                _subs: subs,
            });
        });
        cx.notify();
    }

    fn close_tag_dialog(&mut self, cx: &mut Context<Self>) {
        DIALOG.with(|dialog| dialog.borrow_mut().take());
        cx.notify();
    }

    /// Create / Save: the tag is stored (renamed: the old one goes).
    fn finish_tag_dialog(&mut self, cx: &mut Context<Self>) {
        let Some((name, message, editing)) = DIALOG.with(|dialog| {
            dialog.borrow().as_ref().map(|open| {
                (
                    open.name.read(cx).text().trim().to_owned(),
                    open.message.read(cx).text().to_owned(),
                    open.editing.clone(),
                )
            })
        }) else {
            return;
        };
        if name.is_empty()
            || message.is_empty()
            || arguments(&message)
                .iter()
                .any(|(arg, _)| arg == "ephemeral")
        {
            return;
        }
        let mut tags = load();
        if let Some(old) = editing.filter(|old| *old != name) {
            tags.remove(&old);
        }
        tags.insert(name.clone(), Tag { name, message });
        save(&tags);
        self.close_tag_dialog(cx);
    }

    /// The tag dialog over everything, while it is open.
    pub(in crate::ui) fn message_tags_dialog(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let (name, message, editing, guide) = DIALOG.with(|dialog| {
            dialog.borrow().as_ref().map(|open| {
                (
                    open.name.clone(),
                    open.message.clone(),
                    open.editing.clone(),
                    open.guide,
                )
            })
        })?;
        let name_text = name.read(cx).text().trim().to_owned();
        let message_text = message.read(cx).text().to_owned();
        let detected = arguments(&message_text);
        let reserved = detected.iter().any(|(arg, _)| arg == "ephemeral");
        let exists = Some(&name_text) != editing.as_ref() && load().contains_key(&name_text);
        let notice = if reserved {
            Some("The argument name \"ephemeral\" is reserved and cannot be used.".to_owned())
        } else if exists {
            Some(format!(
                "A tag with the name \"{name_text}\" already exists and will be overwritten."
            ))
        } else {
            None
        };
        let field = |editor: Entity<Editor>, tall: bool| {
            div()
                .min_h(px(if tall { 80. } else { 40. }))
                .rounded(px(8.))
                .border_1()
                .border_color(color("--input-border-default"))
                .bg(gpui::rgba(0x0000001f))
                .px(px(12.))
                .py(px(10.))
                .child(editor)
        };
        let heading = |text: &'static str| {
            div()
                .mb(px(8.))
                .text_size(px(16.))
                .line_height(px(20.))
                .font_weight(FontWeight(600.))
                .text_color(color("--text-strong"))
                .child(text)
        };
        let paragraph = |text: String| {
            div()
                .text_size(px(14.))
                .line_height(px(18.))
                .text_color(color("--text-default"))
                .child(text)
        };
        let body = div()
            .flex()
            .flex_col()
            .gap(px(12.))
            .child(div().child(heading("Name")).child(field(name, false)))
            .child(div().child(heading("Response")).child(field(message, true)))
            .when(!detected.is_empty(), |this| {
                this.child(
                    div().child(heading("Detected Arguments")).children(detected.iter().map(|(arg, default)| {
                        paragraph(match default {
                            Some(default) => format!("— {arg} (default: {default})"),
                            None => format!("— {arg}"),
                        })
                    })),
                )
            })
            .child(
                div()
                    .id("message-tags-guide")
                    .cursor_pointer()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .text_size(px(14.))
                    .text_color(color("--text-default"))
                    .child(super::super::icon("settings_info", 16., "--text-muted"))
                    .child("View Arguments guide")
                    .on_click(cx.listener(|_, _, _, cx| {
                        DIALOG.with(|dialog| {
                            if let Some(open) = dialog.borrow_mut().as_mut() {
                                open.guide = !open.guide;
                            }
                        });
                        cx.notify();
                    })),
            )
            .when(guide, |this| {
                this.child(paragraph(
                    "Your response can include variables wrapped in double curly braces which will become command \
                     arguments, for example Hello {{user}}."
                        .to_owned(),
                ))
                .child(paragraph(
                    "You can specify arguments with default values by using an equals sign, for example Hello {{user = pal}}."
                        .to_owned(),
                ))
                .child(paragraph(format!("Example Command response: {EXAMPLE}")))
                .child(paragraph("Example usage: /greet user:@Clyde".to_owned()))
                .child(paragraph("Example output: Hello @Clyde! I am feeling great.".to_owned()))
            })
            .children(notice.map(|text| {
                div()
                    .p(px(8.))
                    .rounded(px(8.))
                    .bg(color("--background-feedback-critical"))
                    .text_size(px(14.))
                    .text_color(color("--text-feedback-critical"))
                    .child(text)
            }))
            .into_any_element();
        let ready = !name_text.is_empty() && !message_text.is_empty() && !reserved;
        let spec = modal::Spec {
            id: "message-tags-dialog",
            title: SharedString::from(if editing.is_some() {
                "Edit Tag"
            } else {
                "Create New Tag"
            }),
            subtitle: Some(
                div()
                    .text_size(px(14.))
                    .text_color(color("--text-muted"))
                    .child(if editing.is_some() {
                        "Edit your custom command."
                    } else {
                        "Create a new tag which will be registered as a slash command."
                    })
                    .into_any_element(),
            ),
            body: Some(body),
            buttons: vec![
                modal::button(
                    "message-tags-cancel",
                    "Cancel",
                    modal::Tone::Secondary,
                    |this, _, cx| this.close_tag_dialog(cx),
                ),
                modal::button(
                    "message-tags-save",
                    if editing.is_some() { "Save" } else { "Create" },
                    if ready {
                        modal::Tone::Primary
                    } else {
                        modal::Tone::PrimaryDisabled
                    },
                    |this, _, cx| this.finish_tag_dialog(cx),
                ),
            ],
            close: Box::new(|this, _, cx| this.close_tag_dialog(cx)),
        };
        Some(modal::render(spec, cx))
    }

    /// `SettingsTagList`: the plugin's Registered Tags, each with Edit and Delete, and Create Tag.
    pub(in crate::ui) fn message_tags_settings(&self, cx: &mut Context<Self>) -> AnyElement {
        let tags: Vec<Tag> = load().into_values().collect();
        let icon_button = |id: String, icon: &'static str, tint: &'static str| {
            div()
                .id(SharedString::from(id))
                .size(px(32.))
                .rounded(px(8.))
                .flex()
                .items_center()
                .justify_center()
                .cursor_pointer()
                .bg(color("--control-secondary-background-default"))
                .hover(|style| style.bg(color("--control-secondary-background-hover")))
                .child(super::super::icon(icon, 20., tint))
        };
        div()
            .mt(px(8.))
            .flex()
            .flex_col()
            .gap(px(8.))
            .child(
                div()
                    .text_size(px(16.))
                    .font_weight(FontWeight(600.))
                    .text_color(color("--text-strong"))
                    .child("Registered Tags"),
            )
            .children(tags.into_iter().map(|tag| {
                let (edit, delete) = (tag.clone(), tag.name.clone());
                div()
                    .p(px(8.))
                    .pl(px(16.))
                    .rounded(px(8.))
                    .border_1()
                    .border_color(color("--border-subtle"))
                    .flex()
                    .items_center()
                    .gap(px(4.))
                    .child(
                        div()
                            .flex_1()
                            .text_size(px(16.))
                            .font_weight(FontWeight(500.))
                            .text_color(color("--text-default"))
                            .child(tag.name.clone()),
                    )
                    .child(
                        icon_button(
                            format!("tag-edit-{}", tag.name),
                            "menu_edit_message",
                            "--interactive-text-default",
                        )
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.open_tag_dialog(Some(edit.clone()), cx);
                        })),
                    )
                    .child(
                        icon_button(
                            format!("tag-delete-{}", tag.name),
                            "menu_delete_message",
                            "--text-feedback-critical",
                        )
                        .on_click(cx.listener(move |_, _, _, cx| {
                            let mut tags = load();
                            tags.remove(&delete);
                            save(&tags);
                            cx.notify();
                        })),
                    )
            }))
            .child(
                div()
                    .id("tag-create")
                    .h(px(38.))
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
                    .child("Create Tag")
                    .on_click(cx.listener(|this, _, _, cx| this.open_tag_dialog(None, cx))),
            )
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arguments_and_responses_follow_the_original() {
        assert_eq!(
            arguments(EXAMPLE),
            vec![
                ("user".to_owned(), None),
                ("mood".to_owned(), Some("great".to_owned()))
            ]
        );
        let given = |name: &str| (name == "user").then(|| "@Clyde".to_owned());
        assert_eq!(
            response(EXAMPLE, &given),
            "Hello @Clyde! I am feeling great."
        );
        assert_eq!(response("a\\nb {{x}}", &|_: &str| None), "a\nb {{x}}");
        let long = Tag {
            name: "t".to_owned(),
            message: "x".repeat(80),
        };
        assert_eq!(list_line(&long), format!("`t`: {}...", "x".repeat(72)));
    }
}
