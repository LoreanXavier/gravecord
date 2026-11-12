//! `ThreadMessageStore` (module 456874): the most recent message of each thread, for the subtext of a thread row in
//! the thread browser and the "All Active Threads" modal. Fed by `most_recent_message` on the threads of READY and
//! GUILD_CREATE (`ev` in module 225123), `most_recent_messages` of THREAD_LIST_SYNC, and by MESSAGE_CREATE in a thread
//! it knows. Thread starter messages (type 21) are never kept.

use crate::{Id, Message, id};
use serde_json::Value;
use std::collections::HashMap;

/// `A.lAJ.THREAD_STARTER_MESSAGE`.
const THREAD_STARTER_MESSAGE: u64 = 21;

/// `l.A_`: the thread channel types (announcement, public, private).
fn is_thread_type(kind: u64) -> bool {
    (10..=12).contains(&kind)
}

#[derive(Debug, Default)]
struct Entry {
    guild: Id,
    parent: Id,
    latest: Option<Message>,
}

#[derive(Debug, Default)]
pub struct ThreadMessages {
    threads: HashMap<Id, Entry>,
}

impl ThreadMessages {
    /// `getMostRecentMessage`.
    pub fn most_recent_message(&self, thread: Id) -> Option<&Message> {
        self.threads
            .get(&thread)
            .and_then(|entry| entry.latest.as_ref())
    }

    /// `p`: a thread channel adds its entry when new.
    fn note(&mut self, guild: Option<Id>, channel: &Value) -> Option<Id> {
        if !channel["type"].as_u64().is_some_and(is_thread_type) {
            return None;
        }
        let thread = id(&channel["id"])?;
        let entry = self.threads.entry(thread).or_insert_with(|| Entry {
            guild: guild.or_else(|| id(&channel["guild_id"])).unwrap_or(0),
            parent: id(&channel["parent_id"]).unwrap_or(0),
            latest: None,
        });
        // `S`: a thread whose `last_message_id` differs from the stored message forgets it.
        if let (Some(last), Some(known)) = (id(&channel["last_message_id"]), entry.latest.as_ref())
            && known.id != last
        {
            entry.latest = None;
        }
        Some(thread)
    }

    /// `g`: the message of a thread, unless it is the starter message.
    fn set_latest(&mut self, thread: Id, message: &Value) {
        if message["type"].as_u64() == Some(THREAD_STARTER_MESSAGE) {
            return;
        }
        let Some(message) = Message::from_value(message) else {
            return;
        };
        if let Some(entry) = self.threads.get_mut(&thread) {
            entry.latest = Some(message);
        }
    }

    fn note_all(&mut self, guild: Option<Id>, threads: &Value) {
        for channel in threads.as_array().into_iter().flatten() {
            if let Some(thread) = self.note(guild, channel)
                && channel["most_recent_message"].is_object()
            {
                self.set_latest(thread, &channel["most_recent_message"]);
            }
        }
    }

    pub fn dispatch(&mut self, name: &str, data: &Value) {
        match name {
            "READY" => {
                self.threads.clear();
                for guild in data["guilds"].as_array().into_iter().flatten() {
                    self.note_all(id(&guild["id"]), &guild["threads"]);
                }
            }
            "GUILD_CREATE" => self.note_all(id(&data["id"]), &data["threads"]),
            "GUILD_DELETE" => {
                if let Some(guild) = id(&data["id"]) {
                    self.threads.retain(|_, entry| entry.guild != guild);
                }
            }
            "CHANNEL_DELETE" => {
                if let Some(parent) = id(&data["id"]) {
                    self.threads.retain(|_, entry| entry.parent != parent);
                }
            }
            "THREAD_CREATE" | "THREAD_UPDATE" => {
                self.note(None, data);
            }
            "THREAD_LIST_SYNC" => {
                let guild = id(&data["guild_id"]);
                for channel in data["threads"].as_array().into_iter().flatten() {
                    self.note(guild, channel);
                }
                for message in data["most_recent_messages"]
                    .as_array()
                    .into_iter()
                    .flatten()
                {
                    if let Some(thread) = id(&message["channel_id"]) {
                        self.set_latest(thread, message);
                    }
                }
            }
            "THREAD_DELETE" => {
                if let Some(thread) = id(&data["id"]) {
                    self.threads.remove(&thread);
                }
            }
            "MESSAGE_CREATE" => {
                if let Some(thread) = id(&data["channel_id"]) {
                    self.set_latest(thread, data);
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn message(channel: &str, id: &str, kind: u64, content: &str) -> Value {
        json!({"id": id, "channel_id": channel, "content": content, "timestamp": "2026-11-07T18:58:00.000000+00:00",
               "type": kind, "flags": 0, "author": {"id": "10", "username": "me", "discriminator": "0"}})
    }

    #[test]
    fn the_latest_message_follows_the_gateway() {
        let mut store = ThreadMessages::default();
        store.dispatch(
            "GUILD_CREATE",
            &json!({"id": "1", "threads": [{"id": "50", "type": 11, "parent_id": "2", "last_message_id": "60",
                "most_recent_message": message("50", "60", 0, "hi")}]}),
        );
        assert_eq!(
            store.most_recent_message(50).map(|m| m.content.as_str()),
            Some("hi")
        );
        // The starter message never replaces it.
        store.dispatch(
            "MESSAGE_CREATE",
            &message("50", "61", THREAD_STARTER_MESSAGE, "start"),
        );
        assert_eq!(store.most_recent_message(50).map(|m| m.id), Some(60));
        store.dispatch("MESSAGE_CREATE", &message("50", "62", 0, "next"));
        assert_eq!(store.most_recent_message(50).map(|m| m.id), Some(62));
        // A channel that is not a known thread is ignored.
        store.dispatch("MESSAGE_CREATE", &message("9", "63", 0, "x"));
        assert!(store.most_recent_message(9).is_none());
        // THREAD_UPDATE with a newer `last_message_id` forgets the stored message.
        store.dispatch(
            "THREAD_UPDATE",
            &json!({"id": "50", "type": 11, "parent_id": "2", "guild_id": "1", "last_message_id": "70"}),
        );
        assert!(store.most_recent_message(50).is_none());
        store.dispatch(
            "THREAD_LIST_SYNC",
            &json!({"guild_id": "1", "threads": [{"id": "50", "type": 11, "parent_id": "2"}],
                "most_recent_messages": [message("50", "71", 0, "sync")]}),
        );
        assert_eq!(
            store.most_recent_message(50).map(|m| m.content.as_str()),
            Some("sync")
        );
        store.dispatch("THREAD_DELETE", &json!({"id": "50"}));
        assert!(store.most_recent_message(50).is_none());
    }
}
