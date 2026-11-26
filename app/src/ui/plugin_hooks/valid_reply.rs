//! ValidReply (Vencord `src/plugins/validReply`, GPL-3.0-or-later): the pointer entering a reply's "Message could not
//! be loaded" bar fetches the message it answers (`GET /channels/<c>/messages?limit=1&around=<id>`, a read, two
//! retries in the original as here the HTTP client's own). The answer's first message is that message: the bar then
//! shows it (`ReferencedMessageStore.set(..., Loaded)` and `MESSAGE_UPDATE`); another message means it is gone, and the
//! bar reads "Original message was deleted" (`Deleted` and `MESSAGE_DELETE`). A failed fetch changes nothing, and a
//! message being fetched is not asked for again until its answer is in (`fetching`). The fetched message is kept for
//! the bar only, not put in the channel's messages. Not part of the pinned client.

use super::super::{Reference, Root};
use dc_state::{Id, Message};
use gpui::Context;
use serde_json::Value;
use std::cell::RefCell;
use std::collections::HashMap;

const ID: &str = "ValidReply";
/// The fetch's key: `<prefix><channel>:<message>`.
const KEY: &str = "plugin-valid-reply:";

/// What a fetch found for the message.
enum Found {
    Loaded(Box<Message>),
    Deleted,
}

thread_local! {
    /// By message id: what its fetch found. Empty until the plugin fetched something.
    static FOUND: RefCell<HashMap<Id, Found>> = RefCell::new(HashMap::new());
}

fn on() -> bool {
    !crate::plugins::bypassed() && crate::plugins::on(ID)
}

/// What the answer to the fetch of `id` says: the message, or that it is gone; `None` for no message (the original
/// returns and changes nothing).
fn read_answer(id: Id, body: &Value) -> Option<Found> {
    let first = body.as_array()?.first()?;
    if first["id"].as_str() != Some(id.to_string().as_str()) {
        return Some(Found::Deleted);
    }
    Message::from_value(first).map(|message| Found::Loaded(Box::new(message)))
}

impl Root {
    /// The bar of a reply whose message is not loaded: what the plugin fetched for it, if it did.
    pub(in crate::ui) fn valid_reply_reference(id: Id) -> Option<Reference> {
        // Nothing was fetched: the plugin state is not even read.
        if FOUND.with(|found| found.borrow().is_empty()) || !on() {
            return None;
        }
        FOUND.with(|found| {
            found.borrow().get(&id).map(|found| match found {
                Found::Loaded(message) => Reference::Loaded(message.clone()),
                Found::Deleted => Reference::Deleted,
            })
        })
    }

    /// The pointer entered the bar of a reply to `id` in `channel`: fetches it unless a fetch is out.
    pub(in crate::ui) fn valid_reply_hovered(&mut self, channel: Id, id: Id) {
        let key = format!("{KEY}{channel}:{id}");
        if !on() || self.requested.contains(&key) {
            return;
        }
        tracing::info!("ValidReply: fetching the replied message {id}");
        self.requested.insert(key.clone());
        let _ = self.requests.send(crate::net::Request::Fetch {
            key,
            route: format!("/channels/{channel}/messages?limit=1&around={id}"),
        });
    }

    /// The fetch's answer. True when the key was ours.
    pub(in crate::ui) fn valid_reply_fetched(
        &mut self,
        key: &str,
        body: &Value,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(rest) = key.strip_prefix(KEY) else {
            return false;
        };
        // `finally`: the message may be fetched again.
        self.requested.remove(key);
        let Some(id) = rest.split(':').nth(1).and_then(|id| id.parse::<Id>().ok()) else {
            return true;
        };
        match read_answer(id, body) {
            Some(found) => {
                tracing::info!(
                    "ValidReply: the replied message {id} is {}",
                    match found {
                        Found::Loaded(_) => "loaded",
                        Found::Deleted => "deleted",
                    }
                );
                FOUND.with(|known| known.borrow_mut().insert(id, found));
                cx.notify();
            }
            None => tracing::info!("ValidReply: no message in the answer for {id}"),
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn message(id: &str) -> Value {
        json!({"id": id, "channel_id": "2", "content": "hi", "type": 0,
            "author": {"id": "3", "username": "a"}, "timestamp": "2026-11-18T12:27:04.698000+00:00"})
    }

    #[test]
    fn the_first_message_of_the_answer_decides() {
        assert!(
            matches!(read_answer(7, &json!([message("7")])), Some(Found::Loaded(m)) if m.id == 7)
        );
        assert!(matches!(
            read_answer(7, &json!([message("8")])),
            Some(Found::Deleted)
        ));
        assert!(read_answer(7, &json!([])).is_none());
        // A failed fetch answers an error object, not a list: nothing changes.
        assert!(read_answer(7, &json!({"message": "404: Not Found"})).is_none());
    }
}
