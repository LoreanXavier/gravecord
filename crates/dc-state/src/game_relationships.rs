//! GameRelationshipStore (module 800828): friend links made inside games. Only its pending-incoming count is read
//! here: the desktop badge and the tray's unread state add it to the friend request count (912309 `dH`).
//!
//! `CONNECTION_OPEN` (READY `game_relationships`) replaces every entry; `GAME_RELATIONSHIP_ADD` sets one and
//! `GAME_RELATIONSHIP_REMOVE` drops one (225123 passes `id` and `application_id`). Entries are keyed by user and
//! application. The counts are recomputed only on these events (`h`), skipping pending-incoming entries whose user is
//! spam or ignored in RelationshipStore at that moment, as the source does.

use crate::{Id, Stores, id};
use serde_json::Value;
use std::collections::HashMap;

/// RelationshipType `PENDING_INCOMING`.
const PENDING_INCOMING: u64 = 3;

#[derive(Default)]
pub struct GameRelationshipStore {
    /// (user, application) to relationship type.
    entries: HashMap<(Id, Id), u64>,
    pending_incoming: u64,
}

impl GameRelationshipStore {
    /// `getPendingIncomingCount`.
    pub fn pending_incoming_count(&self) -> u64 {
        self.pending_incoming
    }

    fn recount(&mut self, skipped: impl Fn(Id) -> bool) {
        self.pending_incoming = self
            .entries
            .iter()
            .filter(|((user, _), kind)| **kind == PENDING_INCOMING && !skipped(*user))
            .count() as u64;
    }
}

fn key(value: &Value) -> Option<(Id, Id)> {
    Some((id(&value["id"])?, id(&value["application_id"])?))
}

impl Stores {
    pub(crate) fn game_relationships_dispatch(&mut self, name: &str, data: &Value) {
        let store = &mut self.game_relationships;
        match name {
            "READY" => {
                store.entries.clear();
                for entry in data["game_relationships"].as_array().into_iter().flatten() {
                    if let (Some(key), Some(kind)) = (key(entry), entry["type"].as_u64()) {
                        store.entries.insert(key, kind);
                    }
                }
            }
            "GAME_RELATIONSHIP_ADD" => {
                let (Some(key), Some(kind)) = (key(data), data["type"].as_u64()) else {
                    return;
                };
                store.entries.insert(key, kind);
            }
            "GAME_RELATIONSHIP_REMOVE" => {
                let Some(key) = key(data) else {
                    return;
                };
                store.entries.remove(&key);
            }
            _ => return,
        }
        let spam = &self.spam_relationships;
        let ignored = &self.ignored;
        let relationships = &self.relationships;
        // RelationshipStore `isSpam` / `isIgnored` (ignored and not blocked).
        self.game_relationships.recount(|user| {
            spam.contains(&user)
                || (ignored.contains(&user) && relationships.get(&user) != Some(&2))
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn pending_incoming_game_requests_follow_ready_add_remove_and_skip_spam_or_ignored() {
        let mut stores = Stores::default();
        stores.dispatch(
            "READY",
            &json!({"user":{"id":"1","username":"me"},
            "relationships":[{"id":"21","type":3,"is_spam_request":true}],
            "game_relationships":[
                {"id":"20","application_id":"100","type":3,"since":"2026-11-01T00:00:00+00:00"},
                {"id":"20","application_id":"101","type":3},
                {"id":"21","application_id":"100","type":3},
                {"id":"22","application_id":"100","type":1},
                {"id":"23","application_id":"100","type":4}
            ]}),
        );
        // One user with two games counts twice; the spam user and friend/outgoing entries do not count.
        assert_eq!(stores.game_relationships.pending_incoming_count(), 2);
        stores.dispatch(
            "GAME_RELATIONSHIP_ADD",
            &json!({"id":"24","application_id":"100","type":3}),
        );
        assert_eq!(stores.game_relationships.pending_incoming_count(), 3);
        stores.dispatch(
            "GAME_RELATIONSHIP_REMOVE",
            &json!({"id":"20","application_id":"101"}),
        );
        assert_eq!(stores.game_relationships.pending_incoming_count(), 2);
        // Accepting turns the entry into a friend.
        stores.dispatch(
            "GAME_RELATIONSHIP_ADD",
            &json!({"id":"24","application_id":"100","type":1}),
        );
        assert_eq!(stores.game_relationships.pending_incoming_count(), 1);
        stores.dispatch("READY", &json!({"user":{"id":"1","username":"me"}}));
        assert_eq!(stores.game_relationships.pending_incoming_count(), 0);
    }
}
