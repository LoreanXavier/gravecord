//! Interactions in flight: a press on a component, a command, a modal's submit. The pinned build's `InteractionStore`
//! (module 436283), `InteractionModalStore` (750128) and `LocalInteractionComponentStateStore` (935548), on the events
//! `INTERACTION_CREATE`, `INTERACTION_SUCCESS`, `INTERACTION_FAILURE`, `INTERACTION_MODAL_CREATE` and
//! `INTERACTION_IFRAME_MODAL_CREATE` (module 225123).

use super::components::{self, Component};
use super::{Id, id, text};
use serde_json::Value;
use std::collections::HashMap;

/// `m` in module 337591.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    Queued,
    Created,
    Failed,
}

/// What was asked (`interactionType` of the queued data).
#[derive(Clone, Debug, PartialEq)]
pub enum Kind {
    /// An application command (type 2).
    Command,
    /// A component of a message (type 3).
    Component { component_id: String },
    /// A modal submit (type 5).
    ModalSubmit,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Interaction {
    pub state: State,
    pub kind: Kind,
    pub application_id: Option<Id>,
    pub channel_id: Id,
    pub message_id: Option<Id>,
    pub error_code: Option<u64>,
    pub error_message: Option<String>,
    pub reason_code: Option<u64>,
    /// The interaction's own id, once `INTERACTION_CREATE` named it.
    pub interaction_id: Option<Id>,
}

/// The application a modal belongs to (`application` of `INTERACTION_MODAL_CREATE`).
#[derive(Clone, Debug, PartialEq)]
pub struct Application {
    pub id: Id,
    pub name: String,
    pub icon: Option<String>,
    pub bot_name: Option<String>,
    pub bot_id: Option<Id>,
}

impl Application {
    pub fn from_value(value: &Value) -> Option<Self> {
        Some(Self {
            id: id(&value["id"])?,
            name: text(&value["name"]).unwrap_or_default(),
            icon: text(&value["icon"]),
            bot_name: text(&value["bot"]["username"]),
            bot_id: id(&value["bot"]["id"]),
        })
    }

    /// `applicationName` of module 969508: the bot's name, else the application's.
    pub fn display_name(&self) -> &str {
        self.bot_name.as_deref().unwrap_or(&self.name)
    }
}

/// A modal a command or a component asked for.
#[derive(Clone, Debug, PartialEq)]
pub struct Modal {
    pub interaction_id: Id,
    pub nonce: Option<String>,
    pub channel_id: Id,
    pub custom_id: String,
    pub application: Application,
    pub title: String,
    pub components: Vec<Component>,
}

/// Module 949548 (`InteractionIframeModal`): embedded application iframe modal payload
/// from `INTERACTION_IFRAME_MODAL_CREATE`.
#[derive(Clone, Debug, PartialEq)]
pub struct IframeModal {
    pub interaction_id: Id,
    pub channel_id: Id,
    pub custom_id: String,
    pub title: String,
    pub iframe_path: String,
    pub application: Application,
    pub bot_verified: bool,
}

/// `Z` in module 750128.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModalState {
    InFlight,
    Errored,
    Succeeded,
}

/// What the person chose or typed in a component (`getInteractionComponentState`).
#[derive(Clone, Debug, PartialEq)]
pub enum Chosen {
    /// A string select's values, or a text input's text.
    Strings(Vec<String>),
    Text(String),
    /// An entity select's `selectedOptions`: (value, label).
    Entities(Vec<(String, String)>),
}

#[derive(Debug, Default)]
pub struct InteractionStore {
    pending: HashMap<String, Interaction>,
    /// message id to the nonce of its interaction (`f` in module 436283).
    by_message: HashMap<Id, String>,
    pub modal: Option<Modal>,
    modal_state: Option<(String, ModalState)>,
    /// The iframe modal an app asked for (`INTERACTION_IFRAME_MODAL_CREATE`, module 949548).
    pub iframe_modal: Option<IframeModal>,
    /// Chosen values per container (a message id, or a modal's custom id) and component id.
    chosen: HashMap<(String, String), Chosen>,
    /// The nonce of a queued component press to the (container, component) it holds a value for.
    queued_value: HashMap<String, (String, String)>,
}

impl InteractionStore {
    /// `INTERACTION_QUEUE`.
    pub fn queue(
        &mut self,
        nonce: &str,
        kind: Kind,
        channel: Id,
        message: Option<Id>,
        application: Option<Id>,
    ) {
        if let Some(message) = message {
            self.by_message.insert(message, nonce.to_owned());
        }
        self.pending.insert(
            nonce.to_owned(),
            Interaction {
                state: State::Queued,
                kind,
                application_id: application,
                channel_id: channel,
                message_id: message,
                error_code: None,
                error_message: None,
                reason_code: None,
                interaction_id: None,
            },
        );
    }

    /// `canQueueInteraction`: nothing about this message or this nonce is in flight (a failed one does not count).
    pub fn can_queue(&self, message: Id, nonce: &str) -> bool {
        let message_free = self
            .by_message
            .get(&message)
            .and_then(|nonce| self.pending.get(nonce))
            .is_none_or(|known| known.state == State::Failed);
        let nonce_free = self
            .pending
            .get(nonce)
            .is_none_or(|known| known.state == State::Failed);
        message_free && nonce_free
    }

    pub fn get(&self, nonce: &str) -> Option<&Interaction> {
        self.pending.get(nonce)
    }

    /// `getInteraction(message)`.
    pub fn for_message(&self, message: Id) -> Option<&Interaction> {
        self.by_message
            .get(&message)
            .and_then(|nonce| self.pending.get(nonce))
    }

    /// The nonce of the interaction of a message.
    pub fn nonce_for_message(&self, message: Id) -> Option<&str> {
        self.by_message.get(&message).map(String::as_str)
    }

    /// Whether anything is in flight.
    pub fn any_pending(&self) -> bool {
        self.pending
            .values()
            .any(|known| known.state != State::Failed)
    }

    /// `m`: the interaction is over and leaves the table.
    fn finish(&mut self, nonce: &str) {
        self.pending.remove(nonce);
        self.by_message.retain(|_, known| known != nonce);
    }

    /// `p`: success.
    fn succeed(&mut self, nonce: &str) -> bool {
        if !self.pending.contains_key(nonce) {
            return false;
        }
        self.finish(nonce);
        // The value stays shown: the next message update clears it.
        self.queued_value.remove(nonce);
        true
    }

    /// `C1` and `INTERACTION_FAILURE`: a command leaves at once; the rest stay as failed ones so the control can say so.
    pub fn fail(
        &mut self,
        nonce: &str,
        code: Option<u64>,
        message: Option<String>,
        reason: Option<u64>,
    ) {
        let Some(known) = self.pending.get_mut(nonce) else {
            return;
        };
        if known.kind == Kind::Command {
            self.finish(nonce);
        } else {
            known.state = State::Failed;
            known.error_code = code;
            known.error_message = message;
            known.reason_code = reason;
        }
        // The value of the failed press is forgotten.
        if let Some(key) = self.queued_value.remove(nonce) {
            self.chosen.remove(&key);
        }
        if let Some((current, state)) = self.modal_state.as_mut()
            && current == nonce
        {
            *state = ModalState::Errored;
        }
    }

    /// The state of the modal submit with this nonce.
    pub fn modal_state(&self, nonce: &str) -> Option<ModalState> {
        self.modal_state
            .as_ref()
            .filter(|(current, _)| current == nonce)
            .map(|(_, state)| *state)
    }

    /// `INTERACTION_QUEUE` of a modal submit: one at a time.
    pub fn begin_modal_submit(&mut self, nonce: &str) {
        self.modal_state = Some((nonce.to_owned(), ModalState::InFlight));
    }

    /// `SET_INTERACTION_COMPONENT_STATE` / `QUEUE_INTERACTION_COMPONENT_STATE`.
    pub fn set_value(&mut self, container: &str, component: &str, value: Chosen) {
        self.chosen
            .insert((container.to_owned(), component.to_owned()), value);
    }

    /// A press that carries a value keeps it under its nonce until the answer comes.
    pub fn queue_value(&mut self, nonce: &str, container: &str, component: &str, value: Chosen) {
        self.queued_value.insert(
            nonce.to_owned(),
            (container.to_owned(), component.to_owned()),
        );
        self.set_value(container, component, value);
    }

    pub fn value(&self, container: &str, component: &str) -> Option<&Chosen> {
        self.chosen
            .get(&(container.to_owned(), component.to_owned()))
    }

    /// `CLEAR_INTERACTION_MODAL_STATE`.
    pub fn clear_container(&mut self, container: &str) {
        self.chosen.retain(|(held, _), _| held != container);
    }

    /// The modal closes (submitted, cancelled).
    pub fn close_modal(&mut self) {
        if let Some(modal) = self.modal.take() {
            self.clear_container(&modal.custom_id);
        }
        self.modal_state = None;
    }

    /// Module 949548 (`INTERACTION_IFRAME_MODAL_CLOSE`): close the active iframe modal.
    pub fn close_iframe_modal(&mut self) -> bool {
        self.iframe_modal.take().is_some()
    }

    /// A message was edited or deleted: what was chosen on it is forgotten (`MESSAGE_UPDATE`, `MESSAGE_DELETE`).
    pub fn message_changed(&mut self, message: Id) {
        self.clear_container(&message.to_string());
    }

    pub fn dispatch(&mut self, name: &str, data: &Value) {
        match name {
            "INTERACTION_CREATE" => {
                if let Some(nonce) = text(&data["nonce"])
                    && let Some(known) = self.pending.get_mut(&nonce)
                    && known.state == State::Queued
                {
                    known.state = State::Created;
                    known.interaction_id = id(&data["id"]);
                }
            }
            "INTERACTION_SUCCESS" => {
                if let Some(nonce) = text(&data["nonce"]) {
                    self.succeed(&nonce);
                    if let Some((current, state)) = self.modal_state.as_mut()
                        && *current == nonce
                    {
                        *state = ModalState::Succeeded;
                    }
                }
            }
            "INTERACTION_FAILURE" => {
                if let Some(nonce) = text(&data["nonce"]) {
                    self.fail(
                        &nonce,
                        data["error_code"].as_u64(),
                        text(&data["error_message"]),
                        data["reason_code"].as_u64(),
                    );
                }
            }
            "INTERACTION_MODAL_CREATE" => {
                if let Some(nonce) = text(&data["nonce"]) {
                    self.succeed(&nonce);
                }
                if let (Some(interaction_id), Some(channel_id), Some(application)) = (
                    id(&data["id"]),
                    id(&data["channel_id"]),
                    Application::from_value(&data["application"]),
                ) {
                    self.modal = Some(Modal {
                        interaction_id,
                        nonce: text(&data["nonce"]),
                        channel_id,
                        custom_id: text(&data["custom_id"]).unwrap_or_default(),
                        application,
                        title: text(&data["title"]).unwrap_or_default(),
                        components: components::parse(&data["components"]),
                    });
                    self.modal_state = None;
                }
            }
            "INTERACTION_IFRAME_MODAL_CREATE" => {
                if let Some(nonce) = text(&data["nonce"]) {
                    self.succeed(&nonce);
                }
                if let (Some(application), Some(interaction_id), Some(channel_id)) = (
                    Application::from_value(&data["application"]),
                    id(&data["id"]),
                    id(&data["channel_id"]),
                ) {
                    let bot_verified = data["application"]["bot"]["verified"]
                        .as_bool()
                        .unwrap_or(false)
                        || (data["application"]["bot"]["public_flags"]
                            .as_u64()
                            .unwrap_or(0)
                            & (1 << 16)
                            != 0);
                    self.iframe_modal = Some(IframeModal {
                        interaction_id,
                        channel_id,
                        custom_id: text(&data["custom_id"]).unwrap_or_default(),
                        title: text(&data["title"]).unwrap_or_default(),
                        iframe_path: text(&data["iframe_path"]).unwrap_or_else(|| "/".to_owned()),
                        application,
                        bot_verified,
                    });
                }
            }
            "INTERACTION_IFRAME_MODAL_CLOSE" => self.iframe_modal = None,
            // The reply to a command carries the nonce of the press that asked for it.
            "MESSAGE_CREATE" => {
                if let Some(nonce) = text(&data["nonce"]) {
                    self.succeed(&nonce);
                }
            }
            "MESSAGE_DELETE" => {
                if let Some(message) = id(&data["id"]) {
                    self.message_changed(message);
                }
            }
            "LOGOUT" => *self = Self::default(),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn component(message: Id, component_id: &str) -> Kind {
        let _ = message;
        Kind::Component {
            component_id: component_id.to_owned(),
        }
    }

    #[test]
    fn a_press_is_queued_created_and_gone_on_success() {
        let mut store = InteractionStore::default();
        store.queue("n1", component(5, "0,0"), 1, Some(5), Some(9));
        assert_eq!(
            store.for_message(5).map(|known| known.state),
            Some(State::Queued)
        );
        // One press at a time per message.
        assert!(!store.can_queue(5, "n2"));
        store.dispatch("INTERACTION_CREATE", &json!({"id": "77", "nonce": "n1"}));
        let known = store.for_message(5).unwrap();
        assert_eq!(
            (known.state, known.interaction_id),
            (State::Created, Some(77))
        );
        store.dispatch("INTERACTION_SUCCESS", &json!({"id": "77", "nonce": "n1"}));
        assert!(store.for_message(5).is_none());
        assert!(store.can_queue(5, "n2"));
    }

    #[test]
    fn a_failed_press_stays_with_its_error_and_may_be_tried_again() {
        let mut store = InteractionStore::default();
        store.queue("n1", component(5, "0,0"), 1, Some(5), Some(9));
        store.queue_value("n1", "5", "0,0", Chosen::Strings(vec!["a".into()]));
        store.dispatch(
            "INTERACTION_FAILURE",
            &json!({"nonce": "n1", "error_code": 429, "reason_code": 3}),
        );
        let known = store.for_message(5).unwrap();
        assert_eq!(known.state, State::Failed);
        assert_eq!((known.error_code, known.reason_code), (Some(429), Some(3)));
        // The value the failed press held is forgotten.
        assert!(store.value("5", "0,0").is_none());
        assert!(store.can_queue(5, "n2"));
    }

    #[test]
    fn a_failed_command_leaves_at_once_and_its_reply_message_ends_a_queued_one() {
        let mut store = InteractionStore::default();
        store.queue("c1", Kind::Command, 1, None, Some(9));
        store.dispatch("INTERACTION_FAILURE", &json!({"nonce": "c1"}));
        assert!(store.get("c1").is_none());
        store.queue("c2", Kind::Command, 1, None, Some(9));
        assert!(store.any_pending());
        store.dispatch("MESSAGE_CREATE", &json!({"id": "1", "nonce": "c2"}));
        assert!(!store.any_pending());
    }

    #[test]
    fn a_modal_arrives_with_its_form_and_ends_the_press_that_asked_for_it() {
        let mut store = InteractionStore::default();
        store.queue("n1", component(5, "0,0"), 1, Some(5), Some(9));
        store.dispatch(
            "INTERACTION_MODAL_CREATE",
            &json!({
                "id": "88", "nonce": "n1", "channel_id": "1", "custom_id": "form", "title": "Report",
                "application": {"id": "9", "name": "Helper", "bot": {"id": "10", "username": "HelperBot"}},
                "components": [{"type": 1, "components": [{"type": 4, "custom_id": "why", "style": 2, "label": "Why"}]}],
            }),
        );
        assert!(store.for_message(5).is_none());
        let modal = store.modal.as_ref().unwrap();
        assert_eq!(
            (modal.title.as_str(), modal.custom_id.as_str()),
            ("Report", "form")
        );
        assert_eq!(modal.application.display_name(), "HelperBot");
        assert_eq!(modal.components.len(), 1);
        store.begin_modal_submit("s1");
        assert_eq!(store.modal_state("s1"), Some(ModalState::InFlight));
        store.queue("s1", Kind::ModalSubmit, 1, None, Some(9));
        store.dispatch("INTERACTION_FAILURE", &json!({"nonce": "s1"}));
        assert_eq!(store.modal_state("s1"), Some(ModalState::Errored));
        store.begin_modal_submit("s2");
        store.dispatch("INTERACTION_SUCCESS", &json!({"nonce": "s2"}));
        assert_eq!(store.modal_state("s2"), Some(ModalState::Succeeded));
        store.close_modal();
        assert!(store.modal.is_none());
    }

    #[test]
    fn what_was_chosen_on_a_message_is_forgotten_when_it_is_deleted() {
        let mut store = InteractionStore::default();
        store.set_value("5", "0,0", Chosen::Text("x".into()));
        assert!(store.value("5", "0,0").is_some());
        store.dispatch("MESSAGE_DELETE", &json!({"id": "5", "channel_id": "1"}));
        assert!(store.value("5", "0,0").is_none());
    }

    #[test]
    fn iframe_modal_949548_preserves_metadata_and_closes() {
        let mut store = InteractionStore::default();
        store.dispatch(
            "INTERACTION_IFRAME_MODAL_CREATE",
            &json!({
                "id": "501",
                "channel_id": "22",
                "custom_id": "board",
                "title": "Pick a map",
                "iframe_path": "/embed/map",
                "application": {
                    "id": "99",
                    "name": "Chess",
                    "icon": "abc",
                    "bot": {"id": "100", "username": "ChessBot", "verified": true}
                }
            }),
        );
        let m = store.iframe_modal.as_ref().expect("iframe modal");
        assert_eq!(m.title, "Pick a map");
        assert_eq!(m.iframe_path, "/embed/map");
        assert_eq!(m.application.display_name(), "ChessBot");
        assert!(m.bot_verified);
        assert!(store.close_iframe_modal());
        assert!(store.iframe_modal.is_none());
    }
}
