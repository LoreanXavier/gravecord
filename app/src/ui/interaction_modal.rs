//! The form an application asks for (`INTERACTION_MODAL_CREATE`; module 121338 for the dialog, 911408 and 750128 for
//! the values and the submit state).
//!
//! The dialog shows an error callout when the submit failed, the warning that the form goes to the application, the
//! fields, and Cancel and Submit. Submit validates (required, length, how many picked), posts type 5 to
//! `/interactions`, and waits: success closes the dialog, a failure or ten seconds of silence shows "Something went
//! wrong. Try again."

use super::modal::{self, Spec, Tone};
use super::{Root, color, icon};
use crate::editor::{Editor, EditorEvent};
use crate::net::Request;
use dc_state::Id;
use dc_state::components::{self as model, Component, Select};
use dc_state::interactions::{Kind as InteractionKind, ModalState};
use gpui::{
    AnyElement, Context, Entity, FontWeight, IntoElement, SharedString, Subscription, Window,
    deferred, div, prelude::*, px,
};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::time::Duration;

/// `Z.IN_FLIGHT` waits ten seconds (module 750128).
const SUBMIT_TIMEOUT: Duration = Duration::from_secs(10);

/// `Jmwzdx`, `LDvfRP`, `eJEUvD`, `ONSqYd`, `uJgdEu` and `dSTy/w`.
const REQUIRED: &str = "This field is required.";
const SOMETHING_WENT_WRONG: &str = "Something went wrong. Try again.";

/// What a field holds.
pub(super) enum Input {
    Text {
        editor: Entity<Editor>,
        min: u64,
        max: u64,
        paragraph: bool,
    },
    Choose {
        select: Select,
        chosen: Vec<(String, String)>,
        open: bool,
    },
    Display(String),
}

pub(super) struct Field {
    pub id: String,
    pub label: Option<String>,
    pub description: Option<String>,
    pub required: bool,
    pub input: Input,
}

pub(super) struct Form {
    pub fields: Vec<Field>,
    pub errors: HashMap<String, String>,
    /// The nonce of the submit in flight or last sent.
    pub nonce: Option<String>,
    /// The first text field takes the keyboard when the dialog is first drawn.
    pub focus_first: bool,
    pub _events: Vec<Subscription>,
}

fn field_of(
    component: &Component,
    label: Option<String>,
    description: Option<String>,
    fields: &mut Vec<Field>,
    make_editor: &mut dyn FnMut(&model::TextInput) -> Entity<Editor>,
) {
    match component {
        Component::TextInput(input) => fields.push(Field {
            id: input.id.clone(),
            label: label.or_else(|| input.label.clone()),
            description,
            required: input.required,
            input: Input::Text {
                editor: make_editor(input),
                min: input.min_length.unwrap_or(0),
                max: input.max_length.unwrap_or(4000),
                paragraph: input.paragraph,
            },
        }),
        Component::Select(select) => {
            let chosen = select
                .options
                .iter()
                .filter(|option| option.default)
                .map(|option| (option.value.clone(), option.label.clone()))
                .collect();
            fields.push(Field {
                id: select.id.clone(),
                label,
                description,
                required: select.required,
                input: Input::Choose {
                    select: select.clone(),
                    chosen,
                    open: false,
                },
            });
        }
        Component::TextDisplay { id, content } => fields.push(Field {
            id: id.clone(),
            label: None,
            description: None,
            required: false,
            input: Input::Display(content.clone()),
        }),
        Component::ActionRow { components, .. } => {
            for inner in components {
                field_of(inner, None, None, fields, make_editor);
            }
        }
        Component::Label {
            label,
            description,
            component,
            ..
        } => field_of(
            component,
            label.clone(),
            description.clone(),
            fields,
            make_editor,
        ),
        _ => {}
    }
}

/// Why a text of this length does not fit (`ONSqYd`), or why it is missing (`eJEUvD`).
pub(super) fn text_error(text: &str, required: bool, min: u64, max: u64) -> Option<String> {
    let length = text.chars().count() as u64;
    if length == 0 && required {
        return Some(REQUIRED.to_owned());
    }
    if length == 0 && min == 0 {
        return None;
    }
    (length < min || length > max).then(|| format!("Must be between {min} and {max} in length."))
}

/// How many picks a select needs (`Jmwzdx`, `LDvfRP`, `eJEUvD`).
pub(super) fn picks_error(count: u64, required: bool, min: u64, max: u64) -> Option<String> {
    if count == 0 && required {
        return Some(REQUIRED.to_owned());
    }
    if count < min && (count > 0 || required) {
        return Some(format!("Select at least {min}"));
    }
    (count > max).then(|| format!("Select at most {max}"))
}

impl Root {
    /// The store holds a new modal: the form for it is made.
    pub(super) fn open_interaction_modal(&mut self, cx: &mut Context<Self>) {
        let Some(modal) = self.stores.interactions.modal.clone() else {
            return;
        };
        let mut events = Vec::new();
        let mut make = |input: &model::TextInput| -> Entity<Editor> {
            let editor = cx.new(|cx| {
                let mut editor = Editor::new(super::editor_style(), cx);
                editor.set_single_line(!input.paragraph);
                editor.set_enter_inserts_newline(input.paragraph);
                editor.set_reports_all_edits(true);
                if let Some(placeholder) = &input.placeholder {
                    editor.set_placeholder(placeholder.clone());
                }
                if let Some(value) = &input.value {
                    editor.set_text(value);
                }
                if let Some(max) = input.max_length {
                    editor.set_max_utf16(max as usize * 2);
                }
                editor
            });
            let id = input.id.clone();
            events.push(
                cx.subscribe(&editor, move |this, _, event: &EditorEvent, cx| {
                    match event {
                        EditorEvent::Edited => {
                            if let Some(form) = this.interaction_ui.form.as_mut() {
                                form.errors.remove(&id);
                            }
                        }
                        EditorEvent::Submit(_) => this.submit_interaction_modal(cx),
                        EditorEvent::Cancel => this.close_interaction_modal(),
                    }
                    cx.notify();
                }),
            );
            editor
        };
        let mut fields = Vec::new();
        for component in &modal.components {
            field_of(component, None, None, &mut fields, &mut make);
        }
        self.interaction_ui.form = Some(Form {
            fields,
            errors: HashMap::new(),
            nonce: None,
            focus_first: true,
            _events: events,
        });
        cx.notify();
    }

    /// The dialog closes (Cancel, the close button, the scrim, Escape): what was typed is forgotten.
    pub(super) fn close_interaction_modal(&mut self) {
        self.interaction_ui.form = None;
        self.stores.interactions.close_modal();
    }

    /// Called at the top of every frame: the first field takes the keyboard once.
    pub(super) fn interaction_modal_frame(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(form) = self.interaction_ui.form.as_mut() else {
            return;
        };
        if !form.focus_first {
            return;
        }
        form.focus_first = false;
        use gpui::Focusable as _;
        let first = form.fields.iter().find_map(|field| match &field.input {
            Input::Text { editor, .. } => Some(editor.clone()),
            _ => None,
        });
        if let Some(editor) = first {
            window.focus(&editor.read(cx).focus_handle(cx), cx);
        }
    }

    /// Escape: an open list closes first, then the dialog (or active iframe modal). True when it closed something.
    pub(super) fn interaction_modal_escape(&mut self) -> bool {
        if self.stores.interactions.close_iframe_modal() {
            return true;
        }
        let Some(form) = self.interaction_ui.form.as_mut() else {
            return false;
        };
        for field in &mut form.fields {
            if let Input::Choose { open, .. } = &mut field.input
                && *open
            {
                *open = false;
                return true;
            }
        }
        self.close_interaction_modal();
        true
    }

    /// Module 949548 (`InteractionIframeModal`, CSS `589478`/`319060`):
    /// renders the embedded application iframe modal from `INTERACTION_IFRAME_MODAL_CREATE`.
    pub(super) fn interaction_iframe_modal(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let modal = self.stores.interactions.iframe_modal.as_ref()?;
        let vp = window.viewport_size();
        let modal_w = iframe_modal_width(f32::from(vp.width), f32::from(vp.height));
        let frame_h = (modal_w * 9.0 / 16.0).round();
        let guild_id = self.guild_of_channel(modal.channel_id);
        let url = iframe_modal_url(modal, guild_id);
        let app_name = modal.application.display_name().to_owned();
        let title = modal.title.clone();
        let verified = modal.bot_verified;
        let open_url = url.clone();
        let initial = app_name
            .chars()
            .next()
            .unwrap_or('A')
            .to_ascii_uppercase()
            .to_string();

        Some(
            deferred(
                div()
                    .id("interaction-iframe-modal-backdrop")
                    .occlude()
                    .absolute()
                    .inset_0()
                    .bg(gpui::rgba(0x000000b8))
                    .flex()
                    .items_center()
                    .justify_center()
                    .on_mouse_down(
                        gpui::MouseButton::Left,
                        cx.listener(|this, _, _, cx| {
                            this.stores.interactions.close_iframe_modal();
                            cx.notify();
                        }),
                    )
                    .child(
                        div()
                            .id("interaction-iframe-modal")
                            .w(px(modal_w))
                            .rounded(px(12.))
                            .border_1()
                            .border_color(color("--border-subtle"))
                            .bg(color("--background-surface-high"))
                            .overflow_hidden()
                            .flex()
                            .flex_col()
                            .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| {
                                cx.stop_propagation();
                            })
                            // `.modalHeader_sKUDgo`: 52px header (`padding: 12px 16px`, bottom subtle border)
                            .child(
                                div()
                                    .min_h(px(52.))
                                    .px(px(16.))
                                    .py(px(10.))
                                    .border_b_1()
                                    .border_color(color("--border-subtle"))
                                    .flex()
                                    .items_center()
                                    .justify_between()
                                    .gap(px(10.))
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w_0()
                                            .flex()
                                            .items_center()
                                            .gap(px(8.))
                                            .child(
                                                div()
                                                    .size(px(32.))
                                                    .flex_none()
                                                    .rounded(px(8.))
                                                    .bg(color("--control-primary-background-default"))
                                                    .flex()
                                                    .items_center()
                                                    .justify_center()
                                                    .text_size(px(14.))
                                                    .font_weight(FontWeight(700.))
                                                    .text_color(color("--white"))
                                                    .child(initial),
                                            )
                                            .child(
                                                div()
                                                    .flex_1()
                                                    .min_w_0()
                                                    .flex()
                                                    .flex_col()
                                                    .child(
                                                        div()
                                                            .flex()
                                                            .items_center()
                                                            .gap(px(4.))
                                                            .child(
                                                                div()
                                                                    .text_size(px(14.))
                                                                    .line_height(px(18.))
                                                                    .font_weight(FontWeight(600.))
                                                                    .text_color(color("--text-strong"))
                                                                    .child(app_name),
                                                            )
                                                            .child(
                                                                div()
                                                                    .px(px(5.))
                                                                    .h(px(16.))
                                                                    .rounded(px(4.))
                                                                    .bg(color("--control-primary-background-default"))
                                                                    .flex()
                                                                    .items_center()
                                                                    .gap(px(2.))
                                                                    .when(verified, |b| {
                                                                        b.child(icon("select_check", 10., "--white"))
                                                                    })
                                                                    .child(
                                                                        div()
                                                                            .text_size(px(10.))
                                                                            .font_weight(FontWeight(700.))
                                                                            .text_color(color("--white"))
                                                                            .child(dc_intl::tr("APP")),
                                                                    ),
                                                            ),
                                                    )
                                                    .when(!title.is_empty(), |col| {
                                                        col.child(
                                                            div()
                                                                .text_size(px(12.))
                                                                .line_height(px(16.))
                                                                .font_weight(FontWeight(500.))
                                                                .text_color(color("--text-muted"))
                                                                .child(title),
                                                        )
                                                    }),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .id("interaction-iframe-modal-close")
                                            .size(px(28.))
                                            .rounded(px(6.))
                                            .cursor_pointer()
                                            .hover(|s| s.bg(color("--background-mod-subtle")))
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .aria_label(SharedString::from(dc_intl::t("WAI6xu")))
                                            .child(icon("modal_close", 18., "--interactive-text-default"))
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.stores.interactions.close_iframe_modal();
                                                cx.notify();
                                            })),
                                    ),
                            )
                            // 16:9 embedded app iframe surface (`.outer_spl_hF` / `.iframe_sKUDgo`)
                            .child(
                                div()
                                    .w_full()
                                    .h(px(frame_h))
                                    .bg(color("--background-base-lowest"))
                                    .flex()
                                    .flex_col()
                                    .items_center()
                                    .justify_center()
                                    .gap(px(12.))
                                    .p(px(24.))
                                    .child(
                                        div()
                                            .text_size(px(13.))
                                            .line_height(px(18.))
                                            .text_color(color("--text-muted"))
                                            .child(url),
                                    )
                                    .child(
                                        div()
                                            .id("interaction-iframe-open-external")
                                            .h(px(34.))
                                            .px(px(14.))
                                            .rounded(px(8.))
                                            .bg(color("--control-primary-background-default"))
                                            .cursor_pointer()
                                            .flex()
                                            .items_center()
                                            .gap(px(6.))
                                            .child(icon("link_external_small", 14., "--white"))
                                            .child(
                                                div()
                                                    .text_size(px(13.))
                                                    .font_weight(FontWeight(600.))
                                                    .text_color(color("--white"))
                                                    .child(dc_intl::t("wuRE8M")),
                                            )
                                            .on_click(cx.listener(move |_, _, _, cx| {
                                                cx.open_url(&open_url);
                                            })),
                                    ),
                            ),
                    ),
            )
            .with_priority(3)
            .into_any_element(),
        )
    }

    #[cfg(feature = "capture")]
    pub(crate) fn script_iframe_modal(&mut self, arg: &str, cx: &mut Context<Self>) -> String {
        if arg.trim() == "close" {
            self.stores.interactions.close_iframe_modal();
            cx.notify();
            return "iframe modal closed".to_owned();
        }
        self.stores.interactions.iframe_modal = Some(dc_state::interactions::IframeModal {
            interaction_id: 949548,
            channel_id: self.channel.unwrap_or(1),
            custom_id: "board_picker".to_owned(),
            title: "Configure Match Settings".to_owned(),
            iframe_path: "/embed/configure".to_owned(),
            application: dc_state::interactions::Application {
                id: 99123456,
                name: "Arena Activity".to_owned(),
                icon: None,
                bot_name: Some("ArenaBot".to_owned()),
                bot_id: Some(99123457),
            },
            bot_verified: true,
        });
        cx.notify();
        "iframe modal opened".to_owned()
    }

    /// The submit state moved: a success closes the dialog.
    pub(super) fn interaction_modal_synced(&mut self) {
        let done = self
            .interaction_ui
            .form
            .as_ref()
            .and_then(|form| form.nonce.as_deref())
            .and_then(|nonce| self.stores.interactions.modal_state(nonce))
            == Some(ModalState::Succeeded);
        if done {
            self.close_interaction_modal();
        }
    }

    fn submit_state(&self) -> Option<ModalState> {
        let nonce = self.interaction_ui.form.as_ref()?.nonce.as_deref()?;
        self.stores.interactions.modal_state(nonce)
    }

    /// Validates, then posts the form.
    pub(super) fn submit_interaction_modal(&mut self, cx: &mut Context<Self>) {
        if self.submit_state() == Some(ModalState::InFlight) {
            return;
        }
        let Some(modal) = self.stores.interactions.modal.clone() else {
            return;
        };
        let Some(form) = self.interaction_ui.form.as_mut() else {
            return;
        };
        form.errors.clear();
        let mut values: HashMap<String, Value> = HashMap::new();
        for field in &form.fields {
            match &field.input {
                Input::Text {
                    editor, min, max, ..
                } => {
                    let text = editor.read(cx).text().to_owned();
                    if let Some(error) = text_error(&text, field.required, *min, *max) {
                        form.errors.insert(field.id.clone(), error);
                    }
                    values.insert(field.id.clone(), json!(text));
                }
                Input::Choose { select, chosen, .. } => {
                    if let Some(error) = picks_error(
                        chosen.len() as u64,
                        field.required,
                        select.min_values,
                        select.max_values,
                    ) {
                        form.errors.insert(field.id.clone(), error);
                    }
                    let picked: Vec<&String> = chosen.iter().map(|(value, _)| value).collect();
                    values.insert(field.id.clone(), json!(picked));
                }
                Input::Display(_) => {}
            }
        }
        if !form.errors.is_empty() {
            cx.notify();
            return;
        }
        let nonce = crate::net::nonce();
        form.nonce = Some(nonce.clone());
        self.stores.interactions.queue(
            &nonce,
            InteractionKind::ModalSubmit,
            modal.channel_id,
            None,
            Some(modal.application.id),
        );
        self.stores.interactions.begin_modal_submit(&nonce);
        let session = self.stores.session_id.clone().unwrap_or_default();
        let body = modal_body(&nonce, self.guild, &modal, &session, &values);
        if self.interaction_ui.hold_posts {
            self.interaction_ui.held.insert(nonce.clone());
        } else {
            let _ = self.requests.send(Request::Rest {
                key: Some(format!("interaction:{nonce}")),
                method: reqwest::Method::POST,
                route: "/interactions".to_owned(),
                body: Some(body),
            });
        }
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(SUBMIT_TIMEOUT).await;
            let _ = this.update(cx, |this, cx| {
                if this.interaction_ui.held.contains(&nonce) {
                    return;
                }
                if this.stores.interactions.modal_state(&nonce) == Some(ModalState::InFlight) {
                    this.stores.interactions.fail(&nonce, None, None, None);
                    cx.notify();
                }
            });
        })
        .detach();
        cx.notify();
    }

    /// A picked option of a select field.
    fn modal_pick(&mut self, field: &str, value: &str, label: &str, cx: &mut Context<Self>) {
        if let Some(form) = self.interaction_ui.form.as_mut() {
            form.errors.remove(field);
            for known in &mut form.fields {
                if known.id != field {
                    continue;
                }
                if let Input::Choose {
                    select,
                    chosen,
                    open,
                } = &mut known.input
                {
                    match chosen.iter().position(|(held, _)| held == value) {
                        Some(at) => {
                            chosen.remove(at);
                        }
                        None if select.max_values <= 1 => {
                            chosen.clear();
                            chosen.push((value.to_owned(), label.to_owned()));
                            *open = false;
                        }
                        None => {
                            if (chosen.len() as u64) < select.max_values {
                                chosen.push((value.to_owned(), label.to_owned()));
                            }
                        }
                    }
                }
            }
        }
        cx.notify();
    }

    fn modal_field(&self, field: &Field, cx: &mut Context<Self>) -> AnyElement {
        let error = self
            .interaction_ui
            .form
            .as_ref()
            .and_then(|form| form.errors.get(&field.id))
            .cloned();
        let id = field.id.clone();
        let heading = field.label.as_ref().map(|label| {
            div()
                .mb(px(8.))
                .flex()
                .flex_col()
                .child(
                    div()
                        .h(px(20.))
                        .flex()
                        .gap(px(4.))
                        .text_size(px(16.))
                        .line_height(px(20.))
                        .child(
                            div()
                                .font_weight(FontWeight(500.))
                                .text_color(color("--text-strong"))
                                .child(label.clone()),
                        )
                        .when(field.required, |this| {
                            this.child(
                                div()
                                    .text_color(color("--text-feedback-critical"))
                                    .child("*"),
                            )
                        }),
                )
                .children(field.description.clone().map(|text| {
                    div()
                        .text_size(px(14.))
                        .line_height(px(18.))
                        .text_color(color("--text-muted"))
                        .child(text)
                }))
        });
        let input: AnyElement = match &field.input {
            Input::Display(content) => div()
                .text_size(px(14.))
                .line_height(px(18.))
                .text_color(color("--text-default"))
                .child(content.clone())
                .into_any_element(),
            Input::Text {
                editor, paragraph, ..
            } => {
                let paragraph = *paragraph;
                div()
                    .w_full()
                    .when(paragraph, |this| this.min_h(px(108.)))
                    .when(!paragraph, |this| this.h(px(40.)))
                    .px(px(12.))
                    .py(px(10.))
                    .rounded(px(8.))
                    .border_1()
                    .border_color(color(if error.is_some() {
                        "--text-feedback-critical"
                    } else {
                        "--input-border-default"
                    }))
                    .bg(color("--input-background-default"))
                    .child(editor.clone())
                    .into_any_element()
            }
            Input::Choose {
                select,
                chosen,
                open,
            } => {
                let toggled = id.clone();
                let shown = chosen
                    .iter()
                    .map(|(_, label)| label.clone())
                    .collect::<Vec<_>>()
                    .join(", ");
                let placeholder = select
                    .placeholder
                    .clone()
                    .unwrap_or_else(|| "Make a selection".to_owned());
                let options: Vec<(String, String)> = if select.kind == model::STRING_SELECT {
                    select
                        .options
                        .iter()
                        .map(|option| (option.value.clone(), option.label.clone()))
                        .collect()
                } else {
                    let channel = self
                        .stores
                        .interactions
                        .modal
                        .as_ref()
                        .map_or(0, |modal| modal.channel_id);
                    self.entity_options(channel, select, "")
                        .into_iter()
                        .map(|entity| (entity.value, entity.label))
                        .collect()
                };
                div()
                    .relative()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .id(SharedString::from(format!("modal-select-{id}")))
                            .relative()
                            .child(self.anchor_probe(&format!("modal-select-{id}")))
                            .flex()
                            .items_center()
                            .min_h(px(40.))
                            .w_full()
                            .py(px(8.))
                            .pl(px(12.))
                            .pr(px(8.))
                            .gap(px(8.))
                            .rounded(px(8.))
                            .border_1()
                            .border_color(color(if *open {
                                "--text-brand"
                            } else {
                                "--input-border-default"
                            }))
                            .bg(color("--input-background-default"))
                            .text_size(px(16.))
                            .line_height(px(20.))
                            .font_weight(FontWeight(500.))
                            .cursor_pointer()
                            .child(
                                div()
                                    .flex_1()
                                    .min_w(px(0.))
                                    .overflow_hidden()
                                    .text_ellipsis()
                                    .whitespace_nowrap()
                                    .text_color(color(if shown.is_empty() {
                                        "--text-subtle"
                                    } else {
                                        "--text-default"
                                    }))
                                    .child(if shown.is_empty() { placeholder } else { shown }),
                            )
                            .child(icon("settings_select_chevron", 20., "--text-default"))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if let Some(form) = this.interaction_ui.form.as_mut() {
                                    for field in &mut form.fields {
                                        if field.id == toggled
                                            && let Input::Choose { open, .. } = &mut field.input
                                        {
                                            *open = !*open;
                                        }
                                    }
                                }
                                cx.notify();
                            })),
                    )
                    .when(*open, |this| {
                        // A popout layer (deferred, absolute under the trigger like the select of a bot message): drawn
                        // above the dialog and outside the fields' scroll area, so the scroll area does not clip it and
                        // the dialog does not change height.
                        let anchor_key = format!("modal-select-{id}");
                        let flip_up = self
                            .tip_anchors
                            .borrow()
                            .get(anchor_key.as_str())
                            .copied()
                            .is_some_and(|b| {
                                let est_h = (options.len().min(5) as f32 * 38. + 8.).max(80.);
                                let below = self.viewport.1 - f32::from(b.bottom()) - 12.;
                                below < est_h && f32::from(b.top()) - 12. > below
                            });
                        let list = super::keyboard::outside(
                            div()
                                .id(SharedString::from(format!("modal-list-{id}")))
                                .occlude()
                                .absolute()
                                .when(!flip_up, |d| d.top(px(48.)).rounded_b(px(4.)))
                                .when(flip_up, |d| d.bottom(px(48.)).rounded_t(px(4.)))
                                .left_0()
                                .right_0()
                                .border_1()
                                .border_color(color("--border-subtle"))
                                .bg(color("--background-surface-higher"))
                                .shadow(vec![super::shadow_high()])
                                .max_h(px(5. * 38.))
                                .overflow_y_scroll()
                                .flex()
                                .flex_col(),
                            super::keyboard::Layer::InteractionSelect,
                            cx,
                        );
                        // Priority 2: above the dialog's own deferred layer (priority 1).
                        this.child(
                            deferred(list.children(options.into_iter().enumerate().map(
                                |(index, (value, label))| {
                                    let picked = chosen.iter().any(|(held, _)| *held == value);
                                    let (field, pick_value, pick_label) =
                                        (id.clone(), value.clone(), label.clone());
                                    div()
                                        .id(SharedString::from(format!(
                                            "modal-option-{id}-{index}"
                                        )))
                                        .p(px(8.))
                                        .text_size(px(14.))
                                        .line_height(px(18.))
                                        .cursor_pointer()
                                        .text_color(color(if picked {
                                            "--interactive-text-active"
                                        } else {
                                            "--text-strong"
                                        }))
                                        .when(picked, |this| {
                                            this.bg(color("--interactive-background-selected"))
                                        })
                                        .hover(|style| {
                                            style.bg(color("--interactive-background-hover"))
                                        })
                                        .child(label)
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            this.modal_pick(&field, &pick_value, &pick_label, cx);
                                        }))
                                },
                            )))
                            .with_priority(2),
                        )
                    })
                    .into_any_element()
            }
        };
        div()
            .flex()
            .flex_col()
            .children(heading)
            .child(input)
            .children(error.map(|text| {
                div()
                    .mt(px(8.))
                    .flex()
                    .items_center()
                    .gap(px(4.))
                    .child(icon("login_error", 16., "--text-feedback-critical"))
                    .child(
                        div()
                            .text_size(px(12.))
                            .line_height(px(16.))
                            .text_color(color("--text-feedback-critical"))
                            .child(text),
                    )
            }))
            .into_any_element()
    }

    /// An inline notice (`container_0v8Ae1`): radius 8, padding 12, the icon and 14/18 text 8 apart.
    fn modal_callout(&self, text: String, critical: bool) -> AnyElement {
        let (background, border, glyph, tint) = if critical {
            (
                "--background-feedback-mod-critical",
                "--inlinenotice-border-critical",
                "login_error",
                "--icon-feedback-critical",
            )
        } else {
            (
                "--background-feedback-mod-warning",
                "--inlinenotice-border-warning",
                "warning_triangle",
                "--icon-feedback-warning",
            )
        };
        div()
            .mb(px(16.))
            .p(px(12.))
            .rounded(px(8.))
            .border_1()
            .border_color(color(border))
            .bg(color(background))
            .flex()
            .gap(px(8.))
            .child(
                div()
                    .flex_none()
                    .h(px(18.))
                    .flex()
                    .items_center()
                    .child(icon(glyph, 16., tint)),
            )
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .text_size(px(14.))
                    .line_height(px(18.))
                    .text_color(color("--text-strong"))
                    .child(text),
            )
            .into_any_element()
    }

    /// Capture only (`interaction fill`): puts "Ada" into every text field of the open form.
    #[cfg(feature = "capture")]
    pub(super) fn fill_interaction_modal(&mut self, cx: &mut Context<Self>) -> String {
        let Some(form) = self.interaction_ui.form.as_ref() else {
            return "interaction: no form open".to_owned();
        };
        let mut filled = 0;
        for field in &form.fields {
            if let Input::Text { editor, .. } = &field.input {
                editor.update(cx, |editor, _| editor.set_text("Ada"));
                filled += 1;
            }
        }
        cx.notify();
        format!("interaction: filled {filled} text fields")
    }

    /// A select field's list is open (the layer a press outside or Escape closes).
    pub(super) fn interaction_select_is_open(&self) -> bool {
        self.interaction_ui.form.as_ref().is_some_and(|form| {
            form.fields
                .iter()
                .any(|field| matches!(field.input, Input::Choose { open: true, .. }))
        })
    }

    pub(super) fn close_interaction_selects(&mut self) {
        if let Some(form) = self.interaction_ui.form.as_mut() {
            for field in &mut form.fields {
                if let Input::Choose { open, .. } = &mut field.input {
                    *open = false;
                }
            }
        }
    }

    /// The dialog, or nothing when no form is open.
    pub(super) fn interaction_modal(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let form = self.interaction_ui.form.as_ref()?;
        let modal = self.stores.interactions.modal.as_ref()?;
        let state = form
            .nonce
            .as_deref()
            .and_then(|nonce| self.stores.interactions.modal_state(nonce));
        let application = modal.application.display_name().to_owned();
        let mut body = div().flex().flex_col();
        if state == Some(ModalState::Errored) {
            body = body.child(self.modal_callout(SOMETHING_WENT_WRONG.to_owned(), true));
        }
        body = body.child(self.modal_callout(
            format!(
                "This form will be submitted to {application}. Do not share passwords or other sensitive information."
            ),
            false,
        ));
        let fields: Vec<AnyElement> = form
            .fields
            .iter()
            .map(|field| self.modal_field(field, cx))
            .collect();
        body = body.child(
            div()
                .id("interaction-modal-fields")
                .max_h(px(380.))
                .overflow_y_scroll()
                .flex()
                .flex_col()
                .gap(px(24.))
                .children(fields),
        );
        let submit_tone = if state == Some(ModalState::InFlight) {
            Tone::PrimaryLoading
        } else {
            Tone::Primary
        };
        Some(modal::render(
            Spec {
                id: "interaction-modal",
                title: modal.title.clone().into(),
                subtitle: None,
                body: Some(body.into_any_element()),
                buttons: vec![
                    modal::button(
                        "interaction-modal-cancel",
                        "Cancel",
                        Tone::Secondary,
                        |this, _, _| {
                            this.close_interaction_modal();
                        },
                    ),
                    modal::button(
                        "interaction-modal-submit",
                        "Submit",
                        submit_tone,
                        |this, _, cx| {
                            this.submit_interaction_modal(cx);
                        },
                    ),
                ],
                close: Box::new(|this, _, _| this.close_interaction_modal()),
            },
            cx,
        ))
    }
}

/// The body of `POST /interactions` for a submitted form (type 5): the form's `custom_id` and each field's value,
/// laid out as the form was (rows and labels around the inputs).
pub(super) fn modal_body(
    nonce: &str,
    guild: Option<Id>,
    modal: &dc_state::interactions::Modal,
    session: &str,
    values: &HashMap<String, Value>,
) -> Value {
    fn answer(component: &Component, values: &HashMap<String, Value>) -> Option<Value> {
        match component {
            Component::TextInput(input) => Some(json!({
                "type": 4,
                "custom_id": input.custom_id,
                "value": values.get(&input.id).cloned().unwrap_or(json!("")),
            })),
            Component::Select(select) => Some(json!({
                "type": select.kind,
                "custom_id": select.custom_id,
                "values": values.get(&select.id).cloned().unwrap_or(json!([])),
            })),
            Component::ActionRow { components, .. } => Some(json!({
                "type": 1,
                "components": components.iter().filter_map(|inner| answer(inner, values)).collect::<Vec<_>>(),
            })),
            Component::Label { component, .. } => Some(json!({
                "type": 18,
                "component": answer(component, values),
            })),
            _ => None,
        }
    }
    let mut body = json!({
        "type": 5,
        "nonce": nonce,
        "channel_id": modal.channel_id.to_string(),
        "application_id": modal.application.id.to_string(),
        "session_id": session,
        "data": {
            "id": modal.interaction_id.to_string(),
            "custom_id": modal.custom_id,
            "components": modal.components.iter().filter_map(|component| answer(component, values)).collect::<Vec<_>>(),
            "attachments": [],
        },
    });
    if let Some(guild) = guild {
        body["guild_id"] = json!(guild.to_string());
    }
    body
}

/// Capture only: the sample form `interaction modal` opens.
#[cfg(feature = "capture")]
pub(super) fn sample_modal(channel: Id) -> Value {
    json!({
        "id": "5550001",
        "nonce": "sample-modal-nonce",
        "channel_id": channel.to_string(),
        "custom_id": "feedback_form",
        "title": "Send feedback",
        "application": {"id": "902", "name": "Helper", "bot": {"id": "902", "username": "Helper"}},
        "components": [
            {"type": 18, "label": "Your name", "component": {"type": 4, "custom_id": "name", "style": 1, "required": true, "min_length": 2, "max_length": 40, "placeholder": "Ada"}},
            {"type": 18, "label": "What happened", "description": "A few words are enough.", "component": {"type": 4, "custom_id": "what", "style": 2, "required": false, "max_length": 400}},
            {"type": 18, "label": "How bad", "component": {"type": 3, "custom_id": "how", "required": true, "placeholder": "Choose one", "options": [
                {"label": "Small", "value": "s"}, {"label": "Large", "value": "l"}
            ]}}
        ],
    })
}

/// Module 949548 (`319060` constants): responsive 16:9 width clamp
/// `max(320, min(viewportW - 160, min((viewportH - 52 - 160) * 16 / 9, 1280)))`.
pub(super) fn iframe_modal_width(viewport_w: f32, viewport_h: f32) -> f32 {
    const MIN_W: f32 = 320.0;
    const MAX_W: f32 = 1280.0;
    const MARGIN: f32 = 80.0;
    const HEADER_H: f32 = 52.0;
    let by_w = (viewport_w - MARGIN * 2.0).max(MIN_W);
    let by_h = ((viewport_h - HEADER_H - MARGIN * 2.0).max(180.0) * 16.0 / 9.0).min(MAX_W);
    by_w.min(by_h).clamp(MIN_W, MAX_W).round()
}

/// Module 949548 (`358415.oI`): constructs the embedded app iframe URL with `instance_id`,
/// `custom_id`, `channel_id`, and optional `guild_id` (omitted in DMs).
pub(super) fn iframe_modal_url(
    modal: &dc_state::interactions::IframeModal,
    guild_id: Option<Id>,
) -> String {
    let app_id = modal.application.id;
    let path = if modal.iframe_path.starts_with('/') {
        modal.iframe_path.as_str()
    } else {
        "/"
    };
    let mut url = format!(
        "https://{app_id}.discordsays.com{path}?instance_id={}:{}:{}&custom_id={}&channel_id={}",
        modal.channel_id, app_id, modal.custom_id, modal.custom_id, modal.channel_id
    );
    if let Some(gid) = guild_id {
        url.push_str(&format!("&guild_id={gid}"));
    }
    url
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_required_text_and_one_of_the_wrong_length_are_told_apart() {
        assert_eq!(text_error("", true, 0, 10).as_deref(), Some(REQUIRED));
        assert_eq!(text_error("", false, 0, 10), None);
        assert_eq!(
            text_error("a", true, 2, 40).as_deref(),
            Some("Must be between 2 and 40 in length.")
        );
        assert_eq!(text_error("ab", true, 2, 40), None);
        assert!(text_error(&"x".repeat(41), false, 0, 40).is_some());
    }

    #[test]
    fn a_select_needs_its_minimum_and_stops_at_its_maximum() {
        assert_eq!(picks_error(0, true, 1, 1).as_deref(), Some(REQUIRED));
        assert_eq!(picks_error(0, false, 1, 3), None);
        assert_eq!(
            picks_error(1, false, 2, 3).as_deref(),
            Some("Select at least 2")
        );
        assert_eq!(
            picks_error(4, false, 1, 3).as_deref(),
            Some("Select at most 3")
        );
        assert_eq!(picks_error(2, true, 1, 3), None);
    }

    #[test]
    fn the_submit_body_keeps_the_form_layout_and_names_the_application() {
        let modal = dc_state::interactions::Modal {
            interaction_id: 7,
            nonce: None,
            channel_id: 3,
            custom_id: "form".to_owned(),
            application: dc_state::interactions::Application {
                id: 9,
                name: "App".to_owned(),
                icon: None,
                bot_name: None,
                bot_id: None,
            },
            title: "T".to_owned(),
            components: dc_state::components::parse(&json!([
                {"type": 18, "component": {"type": 4, "custom_id": "name", "style": 1}},
                {"type": 1, "components": [{"type": 4, "custom_id": "more", "style": 2}]}
            ])),
        };
        let mut values = HashMap::new();
        values.insert("0".to_owned(), json!("Ada"));
        values.insert("1,0".to_owned(), json!("hi"));
        let body = modal_body("n1", Some(5), &modal, "sess", &values);
        assert_eq!(body["type"], 5);
        assert_eq!(body["application_id"], "9");
        assert_eq!(body["guild_id"], "5");
        assert_eq!(body["data"]["id"], "7");
        assert_eq!(body["data"]["custom_id"], "form");
        assert_eq!(body["data"]["components"][0]["type"], 18);
        assert_eq!(
            body["data"]["components"][0]["component"]["custom_id"],
            "name"
        );
        assert_eq!(
            body["data"]["components"][1]["components"][0]["value"],
            "hi"
        );
    }

    #[test]
    fn a_component_body_for_a_message_names_the_press() {
        let message = dc_state::Message::from_value(&json!({
            "id": "10", "channel_id": "3", "type": 0, "content": "", "flags": 0,
            "timestamp": "2026-11-11T10:00:00.000000+00:00",
            "author": {"id": "9", "username": "bot", "bot": true},
            "application_id": "8",
            "components": [{"type": 1, "components": [
                {"type": 2, "style": 1, "label": "Go", "custom_id": "go"},
                {"type": 3, "custom_id": "pick", "options": [{"label": "A", "value": "a"}]}
            ]}]
        }))
        .unwrap();
        let button = &message.extra.components[0];
        let Component::ActionRow { components, .. } = button else {
            panic!("row");
        };
        let body = super::super::interaction_flow::component_body(
            "n",
            Some(1),
            &message,
            &components[0],
            "s",
            None,
        );
        assert_eq!(body["type"], 3);
        assert_eq!(body["application_id"], "8");
        assert_eq!(body["message_id"], "10");
        assert_eq!(body["data"]["component_type"], 2);
        assert_eq!(body["data"]["custom_id"], "go");
        let values = ["a".to_owned()];
        let select = super::super::interaction_flow::component_body(
            "n",
            None,
            &message,
            &components[1],
            "s",
            Some(&values),
        );
        assert_eq!(select["data"]["values"][0], "a");
        assert_eq!(select["data"]["type"], 3);
        assert!(select["guild_id"].is_null());
    }

    #[test]
    fn iframe_modal_949548_width_clamp_and_url() {
        assert!(!dc_intl::t("WAI6xu").is_empty());
        assert_eq!(iframe_modal_width(1280., 720.), 903.);
        assert_eq!(iframe_modal_width(400., 300.), 320.);
        assert_eq!(iframe_modal_width(3000., 2000.), 1280.);
        let modal = dc_state::interactions::IframeModal {
            interaction_id: 1,
            channel_id: 22,
            custom_id: "pick".into(),
            title: "Pick".into(),
            iframe_path: "/embed/a".into(),
            application: dc_state::interactions::Application {
                id: 99,
                name: "Game".into(),
                icon: None,
                bot_name: None,
                bot_id: None,
            },
            bot_verified: false,
        };
        let dm_url = iframe_modal_url(&modal, None);
        assert!(dm_url.contains("instance_id=22:99:pick"));
        assert!(!dm_url.contains("guild_id="));
        let guild_url = iframe_modal_url(&modal, Some(777));
        assert!(guild_url.ends_with("&guild_id=777"));
    }
}
