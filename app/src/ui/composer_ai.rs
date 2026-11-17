//! The AI rows of the composer's right-click menu (module 194400 `F`): with text selected and a staff account
//! (`b` of module 147087: `getCurrentUser().isStaff()`), the spelling group ends with Fix Grammar
//! (`POST /ai/fix-grammar`), Translate (`POST /ai/translate`, the interface language, or a language of the submenu) and
//! Transforms (`POST /ai/text-transform` with one of the account's own prompts, then Manage). Each answer replaces the
//! selection (`deleteFragment` then `insertText`) and says so in a toast; while a request is out its row reads "Fixing
//! Grammar" / "Translating..." / "Applying {name}" and is disabled.
//!
//! The prompts live in `CustomTextTransformStore` (module 869388, persisted on this machine), managed in "Manage Text
//! Transforms" (module 665906: the list with Edit and Delete, Close and Create) and "Create / Edit Text Transform"
//! (module 183036: Name up to 50 characters, Prompt up to 500, Save once both have text).
//! Not here yet: the flags beside the languages of the Translate submenu.
use super::{Root, color};
use crate::editor::{Editor, EditorEvent};
use crate::net::Request;
use gpui::{
    AnyElement, Context, Entity, FontWeight, IntoElement, Subscription, Window, div, prelude::*, px,
};
use serde_json::{Value, json};

const STORE_KEY: &str = "CustomTextTransformStore";
/// Capture only: the run acts as a staff account, so the rows can be drawn.
#[cfg(feature = "capture")]
pub(crate) static AS_STAFF: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);
pub(super) const KEY_PREFIX: &str = "composer-ai:";

#[derive(Clone, Debug, PartialEq)]
pub(super) struct Transform {
    pub id: String,
    pub name: String,
    pub prompt: String,
}

/// What a request is doing, for the row's label.
#[derive(Clone, Debug, PartialEq)]
pub(super) enum Busy {
    Grammar,
    Translate(String),
    Transform(String),
}

pub(super) struct TransformEditor {
    /// `None` creates a transform.
    editing: Option<String>,
    name: Entity<Editor>,
    prompt: Entity<Editor>,
    _events: Vec<Subscription>,
}

#[derive(Default)]
pub(super) struct ComposerAi {
    pub busy: Option<Busy>,
    pub manage_open: bool,
    pub editor: Option<TransformEditor>,
}

pub(super) fn transforms() -> Vec<Transform> {
    crate::storage::get(STORE_KEY)
        .and_then(|value| value["transforms"].as_array().cloned())
        .unwrap_or_default()
        .iter()
        .filter_map(|entry| {
            Some(Transform {
                id: entry["id"].as_str()?.to_owned(),
                name: entry["name"].as_str()?.to_owned(),
                prompt: entry["prompt"].as_str()?.to_owned(),
            })
        })
        .collect()
}

fn save(list: &[Transform]) {
    let transforms: Vec<Value> = list
        .iter()
        .map(|t| json!({"id": t.id, "name": t.name, "prompt": t.prompt}))
        .collect();
    crate::storage::set(STORE_KEY, json!({ "transforms": transforms }));
}

/// `CUSTOM_TEXT_TRANSFORM_ADD` / `UPDATE` / `DELETE`.
fn add(list: &mut Vec<Transform>, id: String, name: &str, prompt: &str) {
    list.push(Transform {
        id,
        name: name.to_owned(),
        prompt: prompt.to_owned(),
    });
}

fn update(list: &mut [Transform], id: &str, name: &str, prompt: &str) {
    if let Some(found) = list.iter_mut().find(|t| t.id == id) {
        found.name = name.to_owned();
        found.prompt = prompt.to_owned();
    }
}

fn delete(list: &mut Vec<Transform>, id: &str) {
    list.retain(|t| t.id != id);
}

/// The languages of the Translate submenu, by their names.
pub(super) fn languages() -> Vec<(&'static str, String)> {
    dc_intl::locales()
        .map(|code| {
            let name = dc_intl::localized_locale_name(code);
            (
                code,
                if name.is_empty() {
                    code.to_owned()
                } else {
                    name.to_owned()
                },
            )
        })
        .collect()
}

impl Root {
    /// `isStaff()`.
    pub(super) fn composer_ai_allowed(&self) -> bool {
        #[cfg(feature = "capture")]
        if AS_STAFF.load(std::sync::atomic::Ordering::Relaxed) {
            return true;
        }
        self.stores
            .current_user
            .as_ref()
            .is_some_and(|user| (user.flags | user.public_flags) & dc_state::USER_FLAG_STAFF != 0)
    }

    fn composer_ai_send(&mut self, busy: Busy, route: &str, body: Value, cx: &mut Context<Self>) {
        if self.composer_ai.busy.is_some() {
            return;
        }
        let (toast, key) = match &busy {
            Busy::Grammar => ("Fixing Grammar".to_owned(), "grammar".to_owned()),
            Busy::Translate(name) => (
                format!("Translating to {name}"),
                format!("translate:{name}"),
            ),
            Busy::Transform(name) => (format!("Applying {name}"), format!("transform:{name}")),
        };
        self.show_app_toast_owned(toast, cx);
        self.composer_ai.busy = Some(busy);
        let _ = self.requests.send(Request::Rest {
            key: Some(format!("{KEY_PREFIX}{key}")),
            method: reqwest::Method::POST,
            route: route.to_owned(),
            body: Some(body),
        });
        cx.notify();
    }

    fn composer_selection(&self, cx: &Context<Self>) -> String {
        self.composer.read(cx).selected_text().to_owned()
    }

    pub(super) fn composer_fix_grammar(&mut self, cx: &mut Context<Self>) {
        let content = self.composer_selection(cx);
        self.composer_ai_send(
            Busy::Grammar,
            "/ai/fix-grammar",
            json!({ "content": content }),
            cx,
        );
    }

    /// `u(locale, name)`: the interface language when none is picked.
    pub(super) fn composer_translate(
        &mut self,
        locale: Option<(&str, String)>,
        cx: &mut Context<Self>,
    ) {
        let content = self.composer_selection(cx);
        let current = dc_intl::locale().to_owned();
        let (code, name) = match locale {
            Some((code, name)) => (code.to_owned(), name),
            None => (current.clone(), current),
        };
        self.composer_ai_send(
            Busy::Translate(name),
            "/ai/translate",
            json!({ "content": content, "locale": code }),
            cx,
        );
    }

    pub(super) fn composer_transform(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(transform) = transforms().into_iter().nth(index) else {
            return;
        };
        let content = self.composer_selection(cx);
        self.composer_ai_send(
            Busy::Transform(transform.name.clone()),
            "/ai/text-transform",
            json!({ "content": content, "prompt": transform.prompt }),
            cx,
        );
    }

    /// The answer: the selection becomes the new text, and the toast says it is done.
    pub(super) fn composer_ai_answer(&mut self, body: &Value, cx: &mut Context<Self>) {
        let busy = self.composer_ai.busy.take();
        if body["ok"] == true
            && let Some(content) = body["body"]["content"].as_str()
        {
            let content = content.to_owned();
            self.composer
                .update(cx, |composer, cx| composer.insert(&content, cx));
            let done = match busy {
                Some(Busy::Grammar) => "Grammar Fixed".to_owned(),
                Some(Busy::Translate(name)) => format!("Translated to {name}"),
                Some(Busy::Transform(name)) => name,
                None => return,
            };
            self.show_success_toast(done, cx);
        }
        cx.notify();
    }

    fn show_app_toast_owned(&mut self, message: String, cx: &mut Context<Self>) {
        self.put_app_toast(message, false, cx);
    }

    pub(super) fn open_transform_editor(
        &mut self,
        existing: Option<Transform>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let field = |placeholder: &str, text: &str, cx: &mut Context<Self>| {
            let placeholder = placeholder.to_owned();
            let text = text.to_owned();
            cx.new(move |cx| {
                let mut editor = Editor::new(super::editor_style(), cx);
                editor.set_placeholder(placeholder);
                editor.set_text(&text);
                editor
            })
        };
        let name = field(
            dc_intl::tr("e.g., Speak like a pirate"),
            existing.as_ref().map_or("", |t| t.name.as_str()),
            cx,
        );
        name.update(cx, |editor, _| editor.set_single_line(true));
        let prompt = field(
            dc_intl::tr("e.g., Rewrite this text as if spoken by a pirate"),
            existing.as_ref().map_or("", |t| t.prompt.as_str()),
            cx,
        );
        let events = vec![
            cx.subscribe(&name, |_, editor, _: &EditorEvent, cx| {
                // `maxLength: 50`.
                let text = editor.read(cx).text().to_owned();
                if text.chars().count() > 50 {
                    let cut: String = text.chars().take(50).collect();
                    editor.update(cx, |editor, _| editor.set_text(&cut));
                }
                cx.notify();
            }),
            cx.subscribe(&prompt, |_, editor, _: &EditorEvent, cx| {
                // `maxLength: 500`.
                let text = editor.read(cx).text().to_owned();
                if text.chars().count() > 500 {
                    let cut: String = text.chars().take(500).collect();
                    editor.update(cx, |editor, _| editor.set_text(&cut));
                }
                cx.notify();
            }),
        ];
        use gpui::Focusable;
        window.focus(&name.read(cx).focus_handle(cx), cx);
        self.composer_ai.editor = Some(TransformEditor {
            editing: existing.map(|t| t.id),
            name,
            prompt,
            _events: events,
        });
        cx.notify();
    }

    fn save_transform_editor(&mut self, cx: &mut Context<Self>) {
        let Some(open) = self.composer_ai.editor.take() else {
            return;
        };
        let name = open.name.read(cx).text().trim().to_owned();
        let prompt = open.prompt.read(cx).text().trim().to_owned();
        if name.is_empty() || prompt.is_empty() {
            self.composer_ai.editor = Some(open);
            return;
        }
        let mut list = transforms();
        match &open.editing {
            Some(id) => update(&mut list, id, &name, &prompt),
            None => add(
                &mut list,
                chrono::Utc::now().timestamp_millis().to_string(),
                &name,
                &prompt,
            ),
        }
        save(&list);
        cx.notify();
    }

    fn transform_field(label: &'static str, editor: Entity<Editor>, height: f32) -> AnyElement {
        div()
            .flex()
            .flex_col()
            .gap(px(8.))
            .child(
                div()
                    .text_size(px(16.))
                    .line_height(px(20.))
                    .font_weight(FontWeight(500.))
                    .text_color(color("--text-strong"))
                    .child(dc_intl::tr(label)),
            )
            .child(
                div()
                    .h(px(height))
                    .px(px(12.))
                    .py(px(10.))
                    .rounded(px(8.))
                    .border_1()
                    .border_color(color("--input-border-default"))
                    .bg(color("--input-background-default"))
                    .text_size(px(16.))
                    .line_height(px(20.))
                    .child(editor),
            )
            .into_any_element()
    }

    /// Module 183036 and module 665906, the one that is up.
    pub(super) fn composer_ai_modal(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        use super::modal::{Spec, Tone, button, render_small};
        if let Some(open) = &self.composer_ai.editor {
            let ready = !open.name.read(cx).text().trim().is_empty()
                && !open.prompt.read(cx).text().trim().is_empty();
            let close = |this: &mut Root, _: &mut Window, _: &mut Context<Root>| {
                this.composer_ai.editor = None
            };
            let spec = Spec {
                id: "text-transform-editor",
                title: dc_intl::tr(if open.editing.is_some() {
                    "Edit Text Transform"
                } else {
                    "Create Text Transform"
                })
                .into(),
                subtitle: None,
                body: Some(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(16.))
                        .child(Self::transform_field("Name", open.name.clone(), 40.))
                        .child(Self::transform_field("Prompt", open.prompt.clone(), 80.))
                        .into_any_element(),
                ),
                buttons: vec![
                    button(
                        "text-transform-cancel",
                        dc_intl::tr("Cancel"),
                        Tone::Secondary,
                        close,
                    ),
                    button(
                        "text-transform-save",
                        dc_intl::tr("Save"),
                        if ready {
                            Tone::Primary
                        } else {
                            Tone::PrimaryDisabled
                        },
                        |this, _, cx| this.save_transform_editor(cx),
                    ),
                ],
                close: Box::new(close),
            };
            return Some(render_small(spec, cx));
        }
        if !self.composer_ai.manage_open {
            return None;
        }
        let list = transforms();
        let rows: AnyElement = if list.is_empty() {
            div()
                .text_size(px(16.))
                .line_height(px(20.))
                .text_color(color("--text-muted"))
                .child(dc_intl::tr(
                    "No custom transforms yet. Create one to get started!",
                ))
                .into_any_element()
        } else {
            div()
                .flex()
                .flex_col()
                .gap(px(8.))
                .children(list.into_iter().map(|transform| {
                    let edit = transform.clone();
                    let id = transform.id.clone();
                    let icon_button = |key: String, glyph: &'static str, critical: bool| {
                        div()
                            .id(gpui::SharedString::from(key))
                            .size(px(32.))
                            .rounded(px(8.))
                            .border_1()
                            .border_color(color(if critical {
                                "--control-critical-secondary-border-default"
                            } else {
                                "--control-secondary-border-default"
                            }))
                            .bg(color(if critical {
                                "--control-critical-secondary-background-default"
                            } else {
                                "--control-secondary-background-default"
                            }))
                            .flex()
                            .items_center()
                            .justify_center()
                            .cursor_pointer()
                            .child(super::icon(
                                glyph,
                                16.,
                                if critical {
                                    "--control-critical-secondary-text-default"
                                } else {
                                    "--control-secondary-text-default"
                                },
                            ))
                    };
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .child(
                            div()
                                .flex_1()
                                .min_w(px(0.))
                                .flex()
                                .flex_col()
                                .child(
                                    div()
                                        .text_size(px(16.))
                                        .line_height(px(20.))
                                        .font_weight(FontWeight(600.))
                                        .text_color(color("--text-strong"))
                                        .child(transform.name.clone()),
                                )
                                .child(
                                    div()
                                        .text_size(px(14.))
                                        .line_height(px(18.))
                                        .text_color(color("--text-muted"))
                                        .child(transform.prompt.clone()),
                                ),
                        )
                        .child(
                            icon_button(
                                format!("transform-edit-{}", transform.id),
                                "pencil_ai",
                                false,
                            )
                            .on_click(cx.listener(
                                move |this, _, window, cx| {
                                    this.open_transform_editor(Some(edit.clone()), window, cx)
                                },
                            )),
                        )
                        .child(
                            icon_button(
                                format!("transform-delete-{}", transform.id),
                                "trash_ai",
                                true,
                            )
                            .on_click(cx.listener(
                                move |_, _, _, cx| {
                                    let mut list = transforms();
                                    delete(&mut list, &id);
                                    save(&list);
                                    cx.notify();
                                },
                            )),
                        )
                }))
                .into_any_element()
        };
        let close = |this: &mut Root, _: &mut Window, _: &mut Context<Root>| {
            this.composer_ai.manage_open = false
        };
        let spec = Spec {
            id: "manage-text-transforms",
            title: dc_intl::tr("Manage Text Transforms").into(),
            subtitle: Some(
                div()
                    .child(dc_intl::tr(
                        "Create custom prompts to transform your text. Select text in the chat input and pick a transform from the right-click menu.",
                    ))
                    .into_any_element(),
            ),
            body: Some(rows),
            buttons: vec![
                button("manage-transforms-close", dc_intl::tr("Close"), Tone::Secondary, close),
                button("manage-transforms-create", dc_intl::tr("Create"), Tone::Primary, |this, window, cx| {
                    this.open_transform_editor(None, window, cx)
                }),
            ],
            close: Box::new(close),
        };
        Some(render_small(spec, cx))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transforms_are_added_updated_and_deleted_by_id() {
        let mut list = Vec::new();
        add(&mut list, "1".into(), "Pirate", "Speak like a pirate");
        add(&mut list, "2".into(), "Short", "Make it short");
        update(&mut list, "1", "Pirate!", "Arr");
        assert_eq!(list[0].name, "Pirate!");
        delete(&mut list, "2");
        assert_eq!(list.len(), 1);
    }
}
