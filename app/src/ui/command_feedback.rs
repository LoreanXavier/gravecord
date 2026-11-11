//! What a command shows while an application answers it, and what its private reply looks like.
//!
//! * `594808` (the status line): "Sending command..." until the gateway says the application has it, "{app} is
//!   thinking..." after that and while a reply carries the LOADING flag, "The application did not respond" when a
//!   loading reply is older than fifteen minutes (`p` in 594808), and the interaction's error when it failed.
//! * `88484`: the line takes the place of the message text when the message is a command's and has no text, or
//!   carries the LOADING flag.
//! * `376767`: a private reply (flag 64) ends with "Only you can see this • Dismiss message"; Dismiss deletes it
//!   here only, as `dismissAutomatedMessage` does.

use super::components::dots;
use super::{Root, color, icon};
use crate::net::Request;
use dc_state::components::{FLAG_EPHEMERAL, FLAG_LOADING};
use dc_state::{Id, Message};
use gpui::{AnyElement, Context, IntoElement, SharedString, div, prelude::*, px};
use serde_json::{Value, json};

/// `p` in module 594808: how long a loading reply may wait (message id time + 15 minutes).
const REPLY_TIMEOUT_MS: i64 = 15 * 60 * 1000;
/// Discord's epoch, for the time a message id carries.
const DISCORD_EPOCH_MS: i64 = 1_420_070_400_000;

/// The message a command shows before the application has replied.
#[derive(Clone, Debug)]
pub(super) struct LocalCommand {
    pub message: Id,
    pub channel: Id,
    /// The application's name, for "{app} is thinking...".
    pub app_name: String,
    /// Set when the interaction failed: its error text, if the server gave one.
    pub failed: Option<Option<String>>,
}

/// A user message type that is a command's reply (`isCommandType`: chat input and context menu commands).
fn is_command_kind(kind: u64) -> bool {
    kind == 20 || kind == 23
}

impl Root {
    /// Sends a command's interaction and shows the message that waits for its answer.
    pub(super) fn start_command_interaction(
        &mut self,
        name: &str,
        app_id: &str,
        app_name: &str,
        body: Value,
        cx: &mut Context<Self>,
    ) {
        let (Some(channel), Some(nonce)) =
            (self.channel, body["nonce"].as_str().map(str::to_owned))
        else {
            return;
        };
        let application = app_id.parse::<Id>().ok();
        self.stores.interactions.queue(
            &nonce,
            dc_state::interactions::Kind::Command,
            channel,
            None,
            application,
        );
        let now = chrono::Utc::now();
        let id = ((now.timestamp_millis() - DISCORD_EPOCH_MS) as u64) << 22;
        let me = self.stores.current_user.clone();
        let user = me.map_or(json!({"id": "0", "username": ""}), |me| {
            json!({"id": me.id.to_string(), "username": me.username, "avatar": me.avatar, "global_name": me.global_name})
        });
        let value = json!({
            "id": id.to_string(),
            "channel_id": channel.to_string(),
            "type": 20,
            "content": "",
            "timestamp": now.to_rfc3339_opts(chrono::SecondsFormat::Micros, false),
            "author": {"id": app_id, "username": app_name, "bot": true},
            "application_id": app_id,
            "interaction": {"id": id.to_string(), "type": 2, "name": name, "user": user},
        });
        if let Some(message) = Message::from_value(&value) {
            self.stores
                .messages
                .entry(channel)
                .or_default()
                .push(message);
            self.interaction_ui.locals.insert(
                nonce.clone(),
                LocalCommand {
                    message: id,
                    channel,
                    app_name: app_name.to_owned(),
                    failed: None,
                },
            );
            self.rebuild_rows();
        }
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

    /// An interaction failed on this side (a refused post, the answer's error, three silent seconds).
    pub(super) fn command_interaction_ended(&mut self, nonce: &str) {
        self.command_failed(nonce, None);
    }

    /// A gateway event that concerns a command: the reply, the success and the failure.
    pub(super) fn command_event(&mut self, name: &str, data: &Value) {
        let Some(nonce) = data["nonce"].as_str() else {
            return;
        };
        match name {
            "INTERACTION_FAILURE" => {
                let text = data["error_message"].as_str().map(str::to_owned);
                self.command_failed(nonce, text);
            }
            // The application took it, or its reply is here: the waiting message goes.
            "INTERACTION_SUCCESS" | "MESSAGE_CREATE" => self.command_done(nonce),
            _ => {}
        }
    }

    fn command_failed(&mut self, nonce: &str, text: Option<String>) {
        let Some(local) = self.interaction_ui.locals.get_mut(nonce) else {
            return;
        };
        local.failed = Some(text);
        let (channel, message) = (local.channel, local.message);
        // `SEND_FAILED` and the EPHEMERAL flag: the failure is for this account only.
        if let Some(known) = self
            .stores
            .messages
            .get_mut(&channel)
            .and_then(|list| list.iter_mut().find(|known| known.id == message))
        {
            known.flags |= FLAG_EPHEMERAL;
        }
        self.rebuild_rows();
    }

    fn command_done(&mut self, nonce: &str) {
        let Some(local) = self.interaction_ui.locals.remove(nonce) else {
            return;
        };
        if let Some(list) = self.stores.messages.get_mut(&local.channel) {
            list.retain(|known| known.id != local.message);
        }
        self.rebuild_rows();
    }

    /// Whether the status line takes the place of the message text (module 88484).
    pub(super) fn command_replaces_content(&self, message: &Message) -> bool {
        is_command_kind(message.kind) && message.content.is_empty()
            || message.flags & FLAG_LOADING != 0
    }

    fn local_of(&self, message: Id) -> Option<&LocalCommand> {
        self.interaction_ui
            .locals
            .values()
            .find(|local| local.message == message)
    }

    /// The status line of a command's message, or nothing.
    pub(super) fn command_status(&self, message: &Message) -> Option<AnyElement> {
        if !self.command_replaces_content(message) {
            return None;
        }
        let local = self.local_of(message.id);
        let (text, waiting, failed) = match local {
            Some(local) => match &local.failed {
                Some(error) => (
                    error
                        .clone()
                        .unwrap_or_else(|| "This interaction failed".to_owned()),
                    false,
                    true,
                ),
                None => {
                    let created = self
                        .interaction_ui
                        .locals
                        .iter()
                        .find(|(_, held)| held.message == message.id)
                        .and_then(|(nonce, _)| self.stores.interactions.get(nonce))
                        .is_some_and(|known| known.state == dc_state::interactions::State::Created);
                    if created {
                        (format!("{} is thinking...", local.app_name), true, false)
                    } else {
                        ("Sending command...".to_owned(), true, false)
                    }
                }
            },
            None => {
                let sent_at = (message.id >> 22) as i64 + DISCORD_EPOCH_MS;
                if chrono::Utc::now().timestamp_millis() > sent_at + REPLY_TIMEOUT_MS {
                    ("The application did not respond".to_owned(), false, true)
                } else {
                    (
                        format!("{} is thinking...", message.author.display_name()),
                        true,
                        false,
                    )
                }
            }
        };
        Some(
            div()
                .mt(px(4.))
                .flex()
                .items_center()
                .child(div().mr(px(8.)).child(if waiting {
                    dots(&format!("status-{}", message.id), "--text-default")
                } else {
                    icon("login_error", 16., "--icon-feedback-critical").into_any_element()
                }))
                .child(
                    div()
                        .text_size(px(16.))
                        .line_height(px(20.))
                        .text_color(color(if failed {
                            "--text-feedback-critical"
                        } else {
                            "--text-default"
                        }))
                        .child(text),
                )
                .into_any_element(),
        )
    }

    /// "Only you can see this • Dismiss message" under a private reply.
    pub(super) fn ephemeral_notice(
        &self,
        message: &Message,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if message.flags & FLAG_EPHEMERAL == 0 {
            return None;
        }
        let (channel, id) = (message.channel_id, message.id);
        Some(
            div()
                .mt(px(4.))
                .flex()
                .items_center()
                .text_size(px(12.))
                .line_height(px(16.))
                .text_color(color("--text-muted"))
                .child(
                    div()
                        .mr(px(4.))
                        .child(icon("clips_eye", 16., "--text-muted")),
                )
                .child("Only you can see this \u{2022} ")
                .child(
                    div()
                        .id(SharedString::from(format!("dismiss-{id}")))
                        .cursor_pointer()
                        .text_color(color("--text-link"))
                        .hover(|style| style.underline())
                        .child("Dismiss message")
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.dismiss_message(channel, id, cx);
                        })),
                )
                .into_any_element(),
        )
    }

    /// `dismissAutomatedMessage`: the message leaves this client only.
    fn dismiss_message(&mut self, channel: Id, id: Id, cx: &mut Context<Self>) {
        self.interaction_ui
            .locals
            .retain(|_, local| local.message != id);
        if let Some(list) = self.stores.messages.get_mut(&channel) {
            list.retain(|known| known.id != id);
        }
        self.stores.interactions.message_changed(id);
        self.rebuild_rows();
        cx.notify();
    }
}
