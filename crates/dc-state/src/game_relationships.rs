//! GameRelationshipStore (module 800828): friend links made inside games. The desktop badge and the tray's unread
//! state add its pending-incoming count to the friend request count (912309 `dH`); the DM profile reads a person's
//! incoming requests (the Game Friend request notice of module 477534 through 922590 `fi`) and their game friendships
//! (module 361311 through 821269 `q` and 922590 `f1`).
//!
//! `CONNECTION_OPEN` (READY `game_relationships`) replaces every entry; `GAME_RELATIONSHIP_ADD` sets one and
//! `GAME_RELATIONSHIP_REMOVE` drops one (225123 passes `id` and `application_id`); `APPLICATIONS_FETCH_SUCCESS` drops
//! the pending entries of applications the lookup did not know. Entries are keyed by user and application and kept in
//! `since` order (`sortBy` of 713402). The counts are recomputed only on these events (`h`), skipping
//! pending-incoming entries whose user is spam or ignored in RelationshipStore at that moment, as the source does.

use crate::{Id, Stores, id};
use serde_json::Value;
use std::cmp::Reverse;
use std::collections::HashMap;

/// RelationshipType `FRIEND`, `PENDING_INCOMING`, `PENDING_OUTGOING`.
pub const FRIEND: u64 = 1;
pub const PENDING_INCOMING: u64 = 3;
pub const PENDING_OUTGOING: u64 = 4;

struct Entry {
    kind: u64,
    since: String,
    /// Insertion order: 713402 places a new value before others with the same `since`.
    order: u64,
}

#[derive(Default)]
pub struct GameRelationshipStore {
    /// (user, application) to the entry.
    entries: HashMap<(Id, Id), Entry>,
    next: u64,
    pending_incoming: u64,
    pending_outgoing: u64,
    friends: u64,
}

impl GameRelationshipStore {
    /// `getPendingIncomingCount`.
    pub fn pending_incoming_count(&self) -> u64 {
        self.pending_incoming
    }

    /// `getPendingOutgoingCount`.
    pub fn pending_outgoing_count(&self) -> u64 {
        self.pending_outgoing
    }

    /// `getGameFriendCount`.
    pub fn game_friend_count(&self) -> u64 {
        self.friends
    }

    /// `getGameRelationshipsForUserByType`: the applications of the person's entries of that type, in `since` order.
    pub fn applications_for_user(&self, user: Id, kind: u64) -> Vec<Id> {
        let mut found: Vec<(&str, Reverse<u64>, Id)> = self
            .entries
            .iter()
            .filter(|((who, _), entry)| *who == user && entry.kind == kind)
            .map(|((_, app), entry)| (entry.since.as_str(), Reverse(entry.order), *app))
            .collect();
        found.sort();
        found.into_iter().map(|(_, _, app)| app).collect()
    }

    /// `getGameFriendsForApplication`: the people who are game friends through this application.
    pub fn friends_for_application(&self, application: Id) -> Vec<Id> {
        let mut found: Vec<(&str, Reverse<u64>, Id)> = self
            .entries
            .iter()
            .filter(|((_, app), entry)| *app == application && entry.kind == FRIEND)
            .map(|((user, _), entry)| (entry.since.as_str(), Reverse(entry.order), *user))
            .collect();
        found.sort();
        found.into_iter().map(|(_, _, user)| user).collect()
    }

    /// The type of one entry.
    pub fn kind(&self, user: Id, application: Id) -> Option<u64> {
        self.entries
            .get(&(user, application))
            .map(|entry| entry.kind)
    }

    fn insert(&mut self, key: (Id, Id), kind: u64, since: String) {
        self.next += 1;
        self.entries.insert(
            key,
            Entry {
                kind,
                since,
                order: self.next,
            },
        );
    }

    fn recount(&mut self, skipped: impl Fn(Id) -> bool) {
        let (mut incoming, mut outgoing, mut friends) = (0, 0, 0);
        for ((user, _), entry) in &self.entries {
            match entry.kind {
                FRIEND => friends += 1,
                PENDING_OUTGOING => outgoing += 1,
                PENDING_INCOMING if !skipped(*user) => incoming += 1,
                _ => {}
            }
        }
        self.pending_incoming = incoming;
        self.pending_outgoing = outgoing;
        self.friends = friends;
    }
}

fn key(value: &Value) -> Option<(Id, Id)> {
    Some((id(&value["id"])?, id(&value["application_id"])?))
}

fn since(value: &Value) -> String {
    value["since"].as_str().unwrap_or_default().to_owned()
}

impl Stores {
    pub(crate) fn game_relationships_dispatch(&mut self, name: &str, data: &Value) {
        let store = &mut self.game_relationships;
        match name {
            "READY" => {
                store.entries.clear();
                for entry in data["game_relationships"].as_array().into_iter().flatten() {
                    if let (Some(key), Some(kind)) = (key(entry), entry["type"].as_u64()) {
                        store.insert(key, kind, since(entry));
                    }
                }
            }
            "GAME_RELATIONSHIP_ADD" => {
                let (Some(key), Some(kind)) = (key(data), data["type"].as_u64()) else {
                    return;
                };
                store.insert(key, kind, since(data));
            }
            "GAME_RELATIONSHIP_REMOVE" => {
                let Some(key) = key(data) else {
                    return;
                };
                store.entries.remove(&key);
            }
            _ => return,
        }
        self.recount_game_relationships();
    }

    /// `APPLICATIONS_FETCH_SUCCESS` with `unknownApplicationIds`: pending requests from games the lookup did not know
    /// are dropped.
    pub fn drop_unknown_game_applications(&mut self, unknown: &[Id]) {
        if unknown.is_empty() {
            return;
        }
        self.game_relationships.entries.retain(|(_, app), entry| {
            !(unknown.contains(app) && matches!(entry.kind, PENDING_INCOMING | PENDING_OUTGOING))
        });
        self.recount_game_relationships();
    }

    /// This client's own accept (`Some(FRIEND)`), ignore or removal (`None`), ahead of the gateway's event.
    pub fn set_game_relationship(&mut self, user: Id, application: Id, kind: Option<u64>) {
        let store = &mut self.game_relationships;
        match kind {
            Some(kind) => {
                let since = store
                    .entries
                    .get(&(user, application))
                    .map(|entry| entry.since.clone())
                    .unwrap_or_default();
                store.insert((user, application), kind, since);
            }
            None => {
                store.entries.remove(&(user, application));
            }
        }
        self.recount_game_relationships();
    }

    fn recount_game_relationships(&mut self) {
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

    #[test]
    fn a_persons_game_links_come_in_since_order_with_friend_and_outgoing_counts() {
        let mut stores = Stores::default();
        stores.dispatch(
            "READY",
            &json!({"user":{"id":"1","username":"me"},
            "game_relationships":[
                {"id":"20","application_id":"102","type":1,"since":"2026-11-03T00:00:00+00:00"},
                {"id":"20","application_id":"101","type":1,"since":"2026-11-01T00:00:00+00:00"},
                {"id":"20","application_id":"103","type":3,"since":"2026-11-02T00:00:00+00:00"},
                {"id":"21","application_id":"101","type":4,"since":"2026-11-02T00:00:00+00:00"}
            ]}),
        );
        let store = &stores.game_relationships;
        assert_eq!(store.applications_for_user(20, FRIEND), [101, 102]);
        assert_eq!(store.applications_for_user(20, PENDING_INCOMING), [103]);
        assert_eq!(store.friends_for_application(101), [20]);
        assert_eq!(store.game_friend_count(), 2);
        assert_eq!(store.pending_outgoing_count(), 1);
        // A lookup that did not know 101 and 103 drops their pending entries, not the friendship.
        stores.drop_unknown_game_applications(&[101, 103]);
        let store = &stores.game_relationships;
        assert_eq!(store.pending_outgoing_count(), 0);
        assert_eq!(store.pending_incoming_count(), 0);
        assert_eq!(store.applications_for_user(20, FRIEND), [101, 102]);
        // Accepting here turns the request into a friendship at once.
        stores.dispatch(
            "GAME_RELATIONSHIP_ADD",
            &json!({"id":"22","application_id":"104","type":3}),
        );
        stores.set_game_relationship(22, 104, Some(FRIEND));
        assert_eq!(stores.game_relationships.kind(22, 104), Some(FRIEND));
        assert_eq!(stores.game_relationships.game_friend_count(), 3);
        stores.set_game_relationship(22, 104, None);
        assert_eq!(stores.game_relationships.kind(22, 104), None);
    }
}
