//! `ThreadMembersStore` (module 802958): per thread, how many members it has and the first few of them, for the
//! facepile at the end of a thread row. Fed by the threads' `member_count` and `member_ids_preview` (READY,
//! GUILD_CREATE, THREAD_CREATE / UPDATE, THREAD_LIST_SYNC, the thread-loading responses) and by THREAD_MEMBERS_UPDATE.

use crate::{Id, id};
use serde_json::Value;
use std::collections::HashMap;

/// `l.A_`: the thread channel types (announcement, public, private).
fn is_thread_type(kind: u64) -> bool {
    (10..=12).contains(&kind)
}

#[derive(Debug, Default, Clone)]
struct Entry {
    guild: Id,
    parent: Id,
    count: u64,
    preview: Vec<Id>,
}

#[derive(Debug, Default)]
pub struct ThreadMemberPreviews {
    threads: HashMap<Id, Entry>,
}

impl ThreadMemberPreviews {
    /// `getMemberCount`: `None` for a thread the store has not seen.
    pub fn member_count(&self, thread: Id) -> Option<u64> {
        self.threads.get(&thread).map(|entry| entry.count)
    }

    /// `getMemberIdsPreview`.
    pub fn member_ids_preview(&self, thread: Id) -> Option<&[Id]> {
        self.threads
            .get(&thread)
            .map(|entry| entry.preview.as_slice())
    }

    /// `u`: a thread channel adds its entry when new and overwrites the count and the preview when it carries them.
    fn note(&mut self, guild: Option<Id>, channel: &Value) {
        if !channel["type"].as_u64().is_some_and(is_thread_type) {
            return;
        }
        let Some(thread) = id(&channel["id"]) else {
            return;
        };
        let entry = self.threads.entry(thread).or_insert_with(|| Entry {
            guild: guild.or_else(|| id(&channel["guild_id"])).unwrap_or(0),
            parent: id(&channel["parent_id"]).unwrap_or(0),
            count: channel["member_count"].as_u64().unwrap_or(0),
            preview: preview_of(&channel["member_ids_preview"]),
        });
        if let Some(count) = channel["member_count"].as_u64() {
            entry.count = count;
        }
        if channel["member_ids_preview"].is_array() {
            entry.preview = preview_of(&channel["member_ids_preview"]);
        }
    }

    fn note_all(&mut self, guild: Option<Id>, threads: &Value) {
        for thread in threads.as_array().into_iter().flatten() {
            self.note(guild, thread);
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
            "THREAD_CREATE" | "THREAD_UPDATE" => self.note(None, data),
            "THREAD_LIST_SYNC" => self.note_all(id(&data["guild_id"]), &data["threads"]),
            "THREAD_DELETE" => {
                if let Some(thread) = id(&data["id"]) {
                    self.threads.remove(&thread);
                }
            }
            // The count always follows the update; the preview only when the update carries one.
            "THREAD_MEMBERS_UPDATE" => {
                if let Some(entry) =
                    id(&data["id"]).and_then(|thread| self.threads.get_mut(&thread))
                {
                    if data["member_ids_preview"].is_array() {
                        entry.preview = preview_of(&data["member_ids_preview"]);
                    }
                    entry.count = data["member_count"].as_u64().unwrap_or(0);
                }
            }
            _ => {}
        }
    }

    /// `LOAD_THREADS_SUCCESS` / `LOAD_ARCHIVED_THREADS_SUCCESS` (`E`): the threads of a response.
    pub fn add_response(&mut self, guild: Option<Id>, body: &Value) {
        self.note_all(guild, &body["threads"]);
    }
}

fn preview_of(value: &Value) -> Vec<Id> {
    value
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(id)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn counts_and_previews_follow_the_events() {
        let mut store = ThreadMemberPreviews::default();
        store.dispatch(
            "READY",
            &json!({"guilds": [{"id": "1", "threads": [
                {"id": "9", "parent_id": "5", "type": 11, "member_count": 2, "member_ids_preview": ["3", "4"]},
                {"id": "10", "parent_id": "5", "type": 11}]}]}),
        );
        assert_eq!(store.member_count(9), Some(2));
        assert_eq!(store.member_ids_preview(9), Some(&[3, 4][..]));
        // A thread without the fields has an empty entry, an unknown one none.
        assert_eq!(store.member_count(10), Some(0));
        assert_eq!(store.member_ids_preview(10), Some(&[][..]));
        assert_eq!(store.member_count(77), None);
        store.dispatch(
            "THREAD_MEMBERS_UPDATE",
            &json!({"id": "9", "guild_id": "1", "member_count": 3, "member_ids_preview": ["3", "4", "6"]}),
        );
        assert_eq!(store.member_count(9), Some(3));
        assert_eq!(store.member_ids_preview(9), Some(&[3, 4, 6][..]));
        // An update without a preview keeps it, and an unknown thread is ignored.
        store.dispatch(
            "THREAD_MEMBERS_UPDATE",
            &json!({"id": "9", "member_count": 4}),
        );
        assert_eq!(store.member_count(9), Some(4));
        assert_eq!(store.member_ids_preview(9), Some(&[3, 4, 6][..]));
        store.dispatch(
            "THREAD_MEMBERS_UPDATE",
            &json!({"id": "77", "member_count": 4}),
        );
        assert_eq!(store.member_count(77), None);
        // A non-thread channel is not noted; delete and parent delete drop entries.
        store.dispatch(
            "THREAD_CREATE",
            &json!({"id": "11", "type": 0, "member_count": 1}),
        );
        assert_eq!(store.member_count(11), None);
        store.dispatch("THREAD_DELETE", &json!({"id": "10"}));
        assert_eq!(store.member_count(10), None);
        store.dispatch("CHANNEL_DELETE", &json!({"id": "5"}));
        assert_eq!(store.member_count(9), None);
    }
}
