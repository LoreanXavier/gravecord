//! A press on a component of a message, and what comes back (modules 964363 `Gv`, 436283, 911408).
//!
//! The press queues an interaction for the message, posts `POST /interactions` (type 3), and the control loads.
//! `INTERACTION_CREATE` says the application got it; `INTERACTION_SUCCESS` ends it; `INTERACTION_FAILURE`, a refused
//! post, or three seconds of silence (`Gv`'s `timeout: 3000`) fail it with the error under the row of the control.

use super::components::OpenSelect;
use super::{Root, color, icon};
use crate::editor::{Editor, EditorEvent};
use crate::net::Request;
use dc_state::components::{self as model, Component};
use dc_state::interactions::{Chosen, Kind as InteractionKind, State as InteractionState};
use dc_state::{Id, Message};
use gpui::{AnyElement, Context, Focusable as _, IntoElement, Window, div, prelude::*, px};
use serde_json::{Value, json};
use std::collections::HashSet;
use std::time::Duration;

/// `Gv`: how long an application has to answer a press before the control says it failed.
const TIMEOUT: Duration = Duration::from_millis(3000);

/// `fitPBS`: the answer to a press that was refused for coming too fast.
const TOO_FAST: &str = "Slow down! You're clicking too fast.";
/// `VCsUJu`: every other failure.
const FAILED: &str = "This interaction failed";

#[derive(Default)]
pub(super) struct State {
    /// Presses whose post this run refused (a harness run): they wait for the events a script injects instead of
    /// timing out.
    pub held: HashSet<String>,
    /// A harness run asked for presses not to be posted at all (`interaction hold`).
    pub hold_posts: bool,
    /// The messages commands show before their reply, by the nonce of the command.
    /// The form an application asked for.
    pub form: Option<super::interaction_modal::Form>,
    pub locals: std::collections::HashMap<String, super::command_feedback::LocalCommand>,
}

/// The line a failed press leaves under its row: the critical circle and the error text (module 594808).
pub(super) fn failure_line(message: Option<&str>, _reason: Option<u64>) -> AnyElement {
    div()
        .flex()
        .items_center()
        .child(
            div()
                .mr(px(8.))
                .child(icon("login_error", 16., "--icon-feedback-critical")),
        )
        .child(
            div()
                .text_size(px(16.))
                .line_height(px(20.))
                .text_color(color("--text-feedback-critical"))
                .child(message.unwrap_or(FAILED).to_owned()),
        )
        .into_any_element()
}

/// The body of `POST /interactions` for a component (type 3).
#[allow(clippy::too_many_arguments)]
pub(super) fn component_body(
    nonce: &str,
    guild: Option<Id>,
    message: &Message,
    component: &Component,
    session: &str,
    values: Option<&[String]>,
) -> Value {
    let mut data = json!({
        "component_type": component.type_number(),
        "custom_id": component.custom_id(),
    });
    if let Some(values) = values {
        data["type"] = json!(component.type_number());
        data["values"] = json!(values);
    }
    let mut body = json!({
        "type": 3,
        "nonce": nonce,
        "channel_id": message.channel_id.to_string(),
        "message_flags": message.flags,
        "message_id": message.id.to_string(),
        // `message.application_id ?? message.author.id`.
        "application_id": message.application_id.unwrap_or(message.author.id).to_string(),
        "session_id": session,
        "data": data,
    });
    if let Some(guild) = guild {
        body["guild_id"] = json!(guild.to_string());
    }
    body
}

impl Root {
    fn message_by_id(&self, id: Id) -> Option<Message> {
        self.channel
            .and_then(|channel| self.stores.messages.get(&channel))
            .and_then(|messages| messages.iter().find(|known| known.id == id))
            .cloned()
    }

    /// The press of a button that is not a link.
    pub(super) fn press_button(&mut self, message: Id, component: &str, cx: &mut Context<Self>) {
        let Some(known) = self.message_by_id(message) else {
            return;
        };
        let Some(button) = model::find(&known.extra.components, component).cloned() else {
            return;
        };
        self.send_component(&known, &button, None, cx);
    }

    /// Queues, posts and times one press. Nothing is sent while another interaction of the message is in flight.
    pub(super) fn send_component(
        &mut self,
        message: &Message,
        component: &Component,
        values: Option<Vec<(String, String)>>,
        cx: &mut Context<Self>,
    ) {
        let nonce = crate::net::nonce();
        if !self.stores.interactions.can_queue(message.id, &nonce) {
            return;
        }
        let application = message.application_id.unwrap_or(message.author.id);
        self.stores.interactions.queue(
            &nonce,
            InteractionKind::Component {
                component_id: component.id().to_owned(),
            },
            message.channel_id,
            Some(message.id),
            Some(application),
        );
        let plain: Option<Vec<String>> = values
            .as_ref()
            .map(|values| values.iter().map(|(value, _)| value.clone()).collect());
        if let Some(values) = values {
            let chosen = match component {
                Component::Select(select) if select.kind == model::STRING_SELECT => {
                    Chosen::Strings(values.iter().map(|(value, _)| value.clone()).collect())
                }
                _ => Chosen::Entities(values),
            };
            self.stores.interactions.queue_value(
                &nonce,
                &message.id.to_string(),
                component.id(),
                chosen,
            );
        }
        let session = self.stores.session_id.clone().unwrap_or_default();
        let body = component_body(
            &nonce,
            self.guild,
            message,
            component,
            &session,
            plain.as_deref(),
        );
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
        self.interaction_deadline(nonce, cx);
        cx.notify();
    }

    /// After three seconds an interaction nobody answered has failed.
    pub(super) fn interaction_deadline(&mut self, nonce: String, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(TIMEOUT).await;
            let _ = this.update(cx, |this, cx| {
                if this.interaction_ui.held.contains(&nonce) {
                    return;
                }
                let waiting = this
                    .stores
                    .interactions
                    .get(&nonce)
                    .is_some_and(|known| known.state == InteractionState::Queued);
                if waiting {
                    this.stores.interactions.fail(&nonce, None, None, None);
                    this.interaction_ended(&nonce, cx);
                }
            });
        })
        .detach();
    }

    /// An interaction left the table or failed: the rows redraw, and a command's pseudo-message follows it.
    pub(super) fn interaction_ended(&mut self, nonce: &str, cx: &mut Context<Self>) {
        self.command_interaction_ended(nonce);
        cx.notify();
    }

    /// The answer to the post (`Update::Fetched` of an `interaction:` key). A refusal of this run keeps the press
    /// waiting; any failure of the server fails it.
    pub(super) fn interaction_answered(
        &mut self,
        nonce: &str,
        body: &Value,
        cx: &mut Context<Self>,
    ) {
        if body["refused"] == true {
            self.interaction_ui.held.insert(nonce.to_owned());
            return;
        }
        if body["ok"] == true {
            return;
        }
        let status = body["status"].as_u64().unwrap_or(0);
        // The official text for anything but a rate limit is the generic one.
        let shown = (status == 429).then(|| TOO_FAST.to_owned());
        let code = body["body"]["code"].as_u64();
        self.stores.interactions.fail(nonce, code, shown, None);
        self.interaction_ended(nonce, cx);
    }

    /// The gateway told of an interaction: modals open, commands settle, rows redraw.
    pub(super) fn interaction_dispatched(
        &mut self,
        name: &str,
        data: &Value,
        cx: &mut Context<Self>,
    ) {
        match name {
            "INTERACTION_CREATE"
            | "INTERACTION_SUCCESS"
            | "INTERACTION_FAILURE"
            | "MESSAGE_CREATE" => {
                self.command_event(name, data);
                self.interaction_modal_synced();
                cx.notify();
            }
            "INTERACTION_MODAL_CREATE" => {
                self.open_interaction_modal(cx);
                cx.notify();
            }
            _ => {}
        }
    }

    /// A select's list opens under its control, or closes (and applies a multi select).
    pub(super) fn toggle_select(
        &mut self,
        message: Id,
        component: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let was_open = self
            .components
            .open
            .as_ref()
            .is_some_and(|open| open.message == message && open.component == component);
        if self.components.open.is_some() {
            self.close_component_select(cx);
        }
        if was_open {
            return;
        }
        let Some(known) = self.message_by_id(message) else {
            return;
        };
        let Some(Component::Select(select)) =
            model::find(&known.extra.components, component).cloned()
        else {
            return;
        };
        let draft = self.select_values(&message.to_string(), &select);
        let initial = draft.iter().map(|(value, _)| value.clone()).collect();
        let (search, event) = if select.kind == model::STRING_SELECT {
            (None, None)
        } else {
            let editor = cx.new(|cx| {
                let mut editor = Editor::new(super::editor_style(), cx);
                editor.set_single_line(true);
                editor.set_reports_all_edits(true);
                editor
            });
            let event = cx.subscribe_in(&editor, window, |_, _, event: &EditorEvent, _, cx| {
                if matches!(event, EditorEvent::Edited) {
                    cx.notify();
                }
            });
            // The pictures of the people the list may show.
            for entity in self.entity_options(known.channel_id, &select, "") {
                if let Some((key, url)) = entity.avatar {
                    self.request_image(key, url);
                }
            }
            window.focus(&editor.read(cx).focus_handle(cx), cx);
            (Some(editor), Some(event))
        };
        self.components.open = Some(OpenSelect {
            message,
            component: component.to_owned(),
            draft,
            initial,
            search,
            _event: event,
        });
        cx.notify();
    }

    /// An option was picked: a single select answers at once, a multi select keeps it in its draft.
    pub(super) fn pick_option(
        &mut self,
        message: Id,
        component: &str,
        value: &str,
        label: &str,
        cx: &mut Context<Self>,
    ) {
        let Some(known) = self.message_by_id(message) else {
            return;
        };
        let Some(select) =
            model::find(&known.extra.components, component).and_then(|found| match found {
                Component::Select(select) => Some(select.clone()),
                _ => None,
            })
        else {
            return;
        };
        if select.max_values > 1 {
            if let Some(open) = self.components.open.as_mut() {
                match open.draft.iter().position(|(held, _)| held == value) {
                    Some(at) => {
                        open.draft.remove(at);
                    }
                    None if (open.draft.len() as u64) < select.max_values => {
                        open.draft.push((value.to_owned(), label.to_owned()));
                    }
                    None => {}
                }
            }
            cx.notify();
            return;
        }
        self.components.open = None;
        self.send_component(
            &known,
            &Component::Select(select),
            Some(vec![(value.to_owned(), label.to_owned())]),
            cx,
        );
    }

    /// The list closes; a multi select whose picks changed (and fit its limits) sends them now.
    pub(super) fn close_component_select(&mut self, cx: &mut Context<Self>) {
        let Some(open) = self.components.open.take() else {
            return;
        };
        cx.notify();
        let Some(known) = self.message_by_id(open.message) else {
            return;
        };
        let Some(Component::Select(select)) =
            model::find(&known.extra.components, &open.component).cloned()
        else {
            return;
        };
        if select.max_values <= 1 {
            return;
        }
        let picked: Vec<String> = open.draft.iter().map(|(value, _)| value.clone()).collect();
        let count = picked.len() as u64;
        if picked != open.initial && count >= select.min_values && count <= select.max_values {
            self.send_component(&known, &Component::Select(select), Some(open.draft), cx);
        }
    }
}

#[cfg(feature = "capture")]
impl Root {
    /// Capture only: `interaction press [<component id>]` presses a control of the newest components message (the
    /// post is refused in a harness run, so the control stays loading); `interaction create|success|failure
    /// [<error text>]` injects the gateway event for the newest pending press; `interaction state` logs the table.
    pub(crate) fn script_interaction(&mut self, words: &[&str], cx: &mut Context<Self>) -> String {
        if words == ["scan"] {
            let mut lines = Vec::new();
            for (channel, messages) in &self.stores.messages {
                for known in messages {
                    if !known.extra.components.is_empty() || known.application_id.is_some() {
                        lines.push(format!(
                            "{channel}/{} bot={} app={:?} components={} flags={}",
                            known.id,
                            known.author.bot,
                            known.application_id,
                            known.extra.components.len(),
                            known.flags
                        ));
                    }
                }
            }
            return format!(
                "interaction: scan {} messages: {}",
                lines.len(),
                lines.join("; ")
            );
        }
        if words == ["command"] {
            self.interaction_ui.hold_posts = true;
            let nonce = crate::net::nonce();
            self.start_command_interaction("ping", "902", "Helper", json!({"nonce": nonce}), cx);
            return "interaction: command waiting".to_owned();
        }
        if words == ["command-fail"] {
            let nonce = self.interaction_ui.locals.keys().next().cloned();
            return match nonce {
                Some(nonce) => {
                    self.command_event("INTERACTION_FAILURE", &json!({"nonce": nonce}));
                    cx.notify();
                    "interaction: command failed".to_owned()
                }
                None => "interaction: no command waiting".to_owned(),
            };
        }
        if words == ["modal"] {
            let channel = self.channel.unwrap_or(0);
            let data = super::interaction_modal::sample_modal(channel);
            self.stores.dispatch("INTERACTION_MODAL_CREATE", &data);
            self.interaction_dispatched("INTERACTION_MODAL_CREATE", &data, cx);
            return "interaction: sample form opened".to_owned();
        }
        if words == ["modal-fail"] {
            let nonce = self
                .interaction_ui
                .form
                .as_ref()
                .and_then(|form| form.nonce.clone());
            return match nonce {
                Some(nonce) => {
                    let data = json!({"nonce": nonce});
                    self.stores.dispatch("INTERACTION_FAILURE", &data);
                    self.interaction_dispatched("INTERACTION_FAILURE", &data, cx);
                    "interaction: submit failed".to_owned()
                }
                None => "interaction: nothing submitted".to_owned(),
            };
        }
        if words == ["hold"] {
            self.interaction_ui.hold_posts = true;
            return "interaction: presses are held, not posted".to_owned();
        }
        let newest = self
            .channel
            .and_then(|channel| self.stores.messages.get(&channel))
            .and_then(|messages| {
                let wanted = match words {
                    ["press", rest @ ..] => rest.first().copied().unwrap_or("0,0"),
                    _ => "",
                };
                messages.iter().rev().find(|known| {
                    !known.extra.components.is_empty()
                        && (wanted.is_empty()
                            && self.stores.interactions.for_message(known.id).is_some()
                            || !wanted.is_empty()
                                && model::find(&known.extra.components, wanted).is_some())
                })
            })
            .cloned();
        let Some(message) = newest else {
            return "interaction: no message with components in view".to_owned();
        };
        match words {
            ["press", rest @ ..] => {
                let wanted = rest.first().copied().unwrap_or("0,0");
                let Some(component) = model::find(&message.extra.components, wanted).cloned()
                else {
                    return format!("interaction: no component {wanted}");
                };
                self.send_component(&message, &component, None, cx);
                format!("interaction: pressed {wanted} of {}", message.id)
            }
            ["create" | "success" | "failure", ..] => {
                let Some(nonce) = self
                    .stores
                    .interactions
                    .nonce_for_message(message.id)
                    .map(str::to_owned)
                else {
                    return "interaction: nothing pending".to_owned();
                };
                let name = format!("INTERACTION_{}", words[0].to_uppercase());
                let mut data = json!({"id": "123", "nonce": nonce});
                if words.len() > 1 {
                    data["error_message"] = json!(words[1..].join(" ").replace('_', " "));
                }
                self.stores.dispatch(&name, &data);
                self.interaction_dispatched(&name, &data, cx);
                format!("interaction: {name}")
            }
            ["state"] => format!(
                "interaction: {:?}",
                self.stores.interactions.for_message(message.id)
            ),
            _ => "interaction: usage `interaction press|create|success|failure|state`".to_owned(),
        }
    }
}

impl Root {
    /// Capture only (`webhook demo`): asks for the webhook messages in the fixture channel.
    #[cfg(feature = "capture")]
    pub(crate) fn send_webhook_demo(&mut self) {
        let _ = self.requests.send(Request::WebhookDemo {
            channel: crate::webhook_demo::FIXTURE_CHANNEL,
        });
    }

    /// The webhook step finished: the message ids go to `design/webhook-demo-ids.json` (ids only).
    #[cfg(feature = "capture")]
    pub(super) fn webhook_demo_done(&mut self, body: &Value) {
        let text = serde_json::to_string_pretty(body).unwrap_or_default();
        let _ = std::fs::write("design/webhook-demo-ids.json", &text);
        tracing::info!("webhook demo: {} ids {}", body["ok"], body["ids"]);
    }
}
