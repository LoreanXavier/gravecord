//! Small stores the gateway fills and no screen reads yet: `UserRequiredActionStore`, `FriendSuggestionStore`,
//! `ForumPostUnreadCountStore` and the pending-bulk-ban set of `GuildBulkBanStore`.
//!
//! - `UserRequiredActionStore` (module 870570): READY's `required_action` and `USER_REQUIRED_ACTION_UPDATE
//!   {required_action}` replace the one action the account has to take (`hasAction`, `getAction`).
//! - `FriendSuggestionStore` (module 565776): READY's `friend_suggestion_count` sets the count and empties the list;
//!   `FRIEND_SUGGESTION_CREATE` keeps a suggestion by its user (counted once), `FRIEND_SUGGESTION_DELETE
//!   {suggested_user_id}` removes one and counts it down.
//! - `ForumPostUnreadCountStore` (module 474399): `FORUM_UNREADS {channel_id, threads: [{thread_id, count, missing}],
//!   permission_denied}` sets the unread count of each post that carries one; a new message in a counted post adds
//!   one; READY forgets them.
//! - `GuildBulkBanStore` (module 879408): `GUILD_BULK_BAN_STARTED` marks a server pending, `GUILD_BULK_BAN_UPDATE` or
//!   `GUILD_BULK_BAN_FAILED` clears it; an update that arrives with nothing pending is kept as completed before it
//!   started; READY forgets both.

use crate::{Id, Stores, id, text};
use serde_json::Value;
use std::collections::{HashMap, HashSet};

/// One suggestion as the store keeps it (`E`, module 565776).
#[derive(Clone, Debug, PartialEq)]
pub struct FriendSuggestion {
    pub key: Id,
    /// The first reason's name.
    pub name: Option<String>,
    pub user: Value,
    pub mutual_friends_count: u64,
    /// At most two contact names, and only when the payload lists at least two.
    pub contact_names: Vec<String>,
}

impl FriendSuggestion {
    fn from_value(suggestion: &Value) -> Option<FriendSuggestion> {
        let key = id(&suggestion["suggested_user"]["id"])?;
        let contacts = suggestion["contact_names"].as_array();
        Some(FriendSuggestion {
            key,
            name: text(&suggestion["reasons"][0]["name"]),
            user: suggestion["suggested_user"].clone(),
            mutual_friends_count: suggestion["mutual_friends_count"].as_u64().unwrap_or(0),
            contact_names: contacts
                .filter(|names| names.len() >= 2)
                .map(|names| names.iter().take(2).filter_map(text).collect())
                .unwrap_or_default(),
        })
    }
}

#[derive(Default)]
pub struct AccountEvents {
    required_action: Option<String>,
    suggestion_count: u64,
    suggestions: Vec<FriendSuggestion>,
    forum_unreads: HashMap<Id, u64>,
    pending_bulk_bans: HashSet<Id>,
    completed_before_started: HashSet<Id>,
}

impl AccountEvents {
    /// `UserRequiredActionStore.hasAction`.
    pub fn has_required_action(&self) -> bool {
        self.required_action.is_some()
    }

    /// `UserRequiredActionStore.getAction`.
    pub fn required_action(&self) -> Option<&str> {
        self.required_action.as_deref()
    }

    /// `FriendSuggestionStore.getSuggestionCount`.
    pub fn suggestion_count(&self) -> u64 {
        self.suggestion_count
    }

    /// `FriendSuggestionStore.getSuggestions`.
    pub fn suggestions(&self) -> &[FriendSuggestion] {
        &self.suggestions
    }

    /// `LOAD_FRIEND_SUGGESTIONS_SUCCESS`: the list answers replace the store and set the count to its size.
    pub fn suggestions_loaded(&mut self, list: &[Value]) {
        self.suggestions = list
            .iter()
            .filter_map(FriendSuggestion::from_value)
            .collect();
        self.suggestion_count = self.suggestions.len() as u64;
    }

    /// `ForumPostUnreadCountStore` getter: the unread count of a post, when one is known.
    pub fn forum_unread_count(&self, thread: Id) -> Option<u64> {
        self.forum_unreads.get(&thread).copied()
    }

    /// `GuildBulkBanStore.hasPendingBulkBan`.
    pub fn has_pending_bulk_ban(&self, guild: Id) -> bool {
        self.pending_bulk_bans.contains(&guild)
    }

    /// `GuildBulkBanStore.consumeCompletedBeforeStarted`: true once for an update that came before its start.
    pub fn consume_completed_before_started(&mut self, guild: Id, user: Id) -> bool {
        let _ = user;
        self.completed_before_started.remove(&guild)
    }

    /// `GUILD_BULK_BAN_STARTED`.
    pub fn bulk_ban_started(&mut self, guild: Id) {
        self.pending_bulk_bans.insert(guild);
    }
}

impl Stores {
    pub(crate) fn account_events_dispatch(&mut self, name: &str, data: &Value) {
        let me = self.current_user.as_ref().map(|user| user.id);
        // Module 225123 + 218461 `e2`: another session acked a non-channel read state (notification center and
        // message requests belong to the account, guild home, events and onboarding to the guild).
        if matches!(name, "USER_NON_CHANNEL_ACK" | "GUILD_FEATURE_ACK") {
            let resource = if name == "USER_NON_CHANNEL_ACK" {
                me
            } else {
                id(&data["resource_id"])
            };
            if let (Some(resource), Some(kind)) = (resource, data["ack_type"].as_u64()) {
                let now = crate::read_state::snowflake_at(
                    chrono::Utc::now().timestamp_millis().max(0) as u64,
                );
                self.read_states
                    .non_channel_ack(resource, kind, id(&data["entity_id"]), now);
            }
            return;
        }
        let store = &mut self.account_events;
        match name {
            "READY" => {
                store.required_action = text(&data["required_action"]);
                store.suggestions.clear();
                store.suggestion_count = data["friend_suggestion_count"].as_u64().unwrap_or(0);
                store.forum_unreads.clear();
                store.pending_bulk_bans.clear();
                store.completed_before_started.clear();
            }
            "USER_REQUIRED_ACTION_UPDATE" => store.required_action = text(&data["required_action"]),
            "FRIEND_SUGGESTION_CREATE" => {
                if let Some(suggestion) = FriendSuggestion::from_value(&data["suggestion"])
                    .or_else(|| FriendSuggestion::from_value(data))
                    && !store
                        .suggestions
                        .iter()
                        .any(|known| known.key == suggestion.key)
                {
                    store.suggestion_count += 1;
                    store.suggestions.push(suggestion);
                }
            }
            "FRIEND_SUGGESTION_DELETE" => {
                store.suggestion_count = store.suggestion_count.saturating_sub(1);
                if let Some(user) = id(&data["suggested_user_id"]) {
                    store.suggestions.retain(|known| known.key != user);
                }
            }
            "FORUM_UNREADS" => {
                if data["permission_denied"] == true {
                    return;
                }
                for thread in data["threads"].as_array().into_iter().flatten() {
                    if let (Some(thread_id), Some(count)) =
                        (id(&thread["thread_id"]), thread["count"].as_u64())
                    {
                        store.forum_unreads.insert(thread_id, count);
                    }
                }
            }
            "MESSAGE_CREATE" => {
                // The account's own and optimistic messages are not unread ones.
                let mine = id(&data["author"]["id"]) == me && me.is_some();
                if !mine
                    && let Some(channel) = id(&data["channel_id"])
                    && let Some(count) = store.forum_unreads.get_mut(&channel)
                {
                    *count += 1;
                }
            }
            "MESSAGE_ACK" => {
                if let Some(channel) = id(&data["channel_id"])
                    && let Some(count) = store.forum_unreads.get_mut(&channel)
                {
                    *count = 0;
                }
            }
            "GUILD_BULK_BAN_STARTED" => {
                if let Some(guild) = id(&data["guild_id"]) {
                    store.pending_bulk_bans.insert(guild);
                }
            }
            "GUILD_BULK_BAN_FAILED" => {
                if let Some(guild) = id(&data["guild_id"]) {
                    store.pending_bulk_bans.remove(&guild);
                }
            }
            "GUILD_BULK_BAN_UPDATE" => {
                if let Some(guild) = id(&data["guild_id"])
                    && !store.pending_bulk_bans.remove(&guild)
                {
                    store.completed_before_started.insert(guild);
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

    fn ready(extra: Value) -> Value {
        let mut ready = json!({
            "user": {"id": "10", "username": "me", "discriminator": "0"},
            "guilds": [], "private_channels": [],
        });
        for (key, value) in extra.as_object().into_iter().flatten() {
            ready[key] = value.clone();
        }
        ready
    }

    fn suggestion(user: &str) -> Value {
        json!({
            "suggested_user": {"id": user, "username": "maybe", "discriminator": "0"},
            "reasons": [{"type": 1, "platform_type": "contacts", "name": "Sam"}],
            "mutual_friends_count": 3,
            "contact_names": ["A", "B", "C"],
        })
    }

    #[test]
    fn the_required_action_follows_ready_and_its_update() {
        let mut stores = Stores::default();
        stores.dispatch(
            "READY",
            &ready(json!({"required_action": "REQUIRE_VERIFIED_EMAIL"})),
        );
        assert_eq!(
            stores.account_events.required_action(),
            Some("REQUIRE_VERIFIED_EMAIL")
        );
        stores.dispatch(
            "USER_REQUIRED_ACTION_UPDATE",
            &json!({"required_action": null}),
        );
        assert!(!stores.account_events.has_required_action());
        stores.dispatch(
            "USER_REQUIRED_ACTION_UPDATE",
            &json!({"required_action": "REQUIRE_VERIFIED_PHONE"}),
        );
        assert_eq!(
            stores.account_events.required_action(),
            Some("REQUIRE_VERIFIED_PHONE")
        );
    }

    #[test]
    fn friend_suggestions_count_up_once_per_user_and_down_on_delete() {
        let mut stores = Stores::default();
        stores.dispatch("READY", &ready(json!({"friend_suggestion_count": 4})));
        assert_eq!(stores.account_events.suggestion_count(), 4);
        stores.dispatch("FRIEND_SUGGESTION_CREATE", &suggestion("21"));
        stores.dispatch("FRIEND_SUGGESTION_CREATE", &suggestion("21"));
        stores.dispatch("FRIEND_SUGGESTION_CREATE", &suggestion("22"));
        assert_eq!(stores.account_events.suggestion_count(), 6);
        let first = &stores.account_events.suggestions()[0];
        assert_eq!(
            (first.key, first.name.as_deref(), first.mutual_friends_count),
            (21, Some("Sam"), 3)
        );
        assert_eq!(first.contact_names, vec!["A", "B"]);
        stores.dispatch(
            "FRIEND_SUGGESTION_DELETE",
            &json!({"suggested_user_id": "21"}),
        );
        assert_eq!(stores.account_events.suggestion_count(), 5);
        assert_eq!(stores.account_events.suggestions().len(), 1);
        // The list answer sets the count to its size.
        stores
            .account_events
            .suggestions_loaded(&[suggestion("30"), suggestion("31")]);
        assert_eq!(stores.account_events.suggestion_count(), 2);
    }

    #[test]
    fn forum_unreads_set_counts_and_new_messages_add_to_counted_posts() {
        let mut stores = Stores::default();
        stores.dispatch("READY", &ready(json!({})));
        stores.dispatch(
            "FORUM_UNREADS",
            &json!({"channel_id": "50", "threads": [
                {"thread_id": "70", "count": 3, "missing": false},
                {"thread_id": "71", "count": null, "missing": true},
            ]}),
        );
        assert_eq!(stores.account_events.forum_unread_count(70), Some(3));
        assert_eq!(stores.account_events.forum_unread_count(71), None);
        let message = |author: &str| {
            json!({"id": "900", "channel_id": "70", "content": "x", "type": 0,
                   "timestamp": "2026-11-11T12:00:00.000000+00:00",
                   "author": {"id": author, "username": "a", "discriminator": "0"}})
        };
        stores.dispatch("MESSAGE_CREATE", &message("77"));
        assert_eq!(stores.account_events.forum_unread_count(70), Some(4));
        // The account's own message does not count.
        stores.dispatch("MESSAGE_CREATE", &message("10"));
        assert_eq!(stores.account_events.forum_unread_count(70), Some(4));
        stores.dispatch(
            "FORUM_UNREADS",
            &json!({"channel_id": "50", "permission_denied": true, "threads": [{"thread_id": "70", "count": 9}]}),
        );
        assert_eq!(stores.account_events.forum_unread_count(70), Some(4));
        stores.dispatch(
            "MESSAGE_ACK",
            &json!({"channel_id": "70", "message_id": "900"}),
        );
        assert_eq!(stores.account_events.forum_unread_count(70), Some(0));
    }

    #[test]
    fn a_bulk_ban_is_pending_until_its_update_and_an_early_update_is_remembered() {
        let mut stores = Stores::default();
        stores.dispatch("GUILD_BULK_BAN_STARTED", &json!({"guild_id": "5"}));
        assert!(stores.account_events.has_pending_bulk_ban(5));
        stores.dispatch(
            "GUILD_BULK_BAN_UPDATE",
            &json!({"guild_id": "5", "bulk_ban": {"banned_users": ["1"], "failed_users": []}}),
        );
        assert!(!stores.account_events.has_pending_bulk_ban(5));
        assert!(
            !stores
                .account_events
                .consume_completed_before_started(5, 10)
        );
        // An update with nothing pending came first.
        stores.dispatch("GUILD_BULK_BAN_UPDATE", &json!({"guild_id": "6"}));
        assert!(
            stores
                .account_events
                .consume_completed_before_started(6, 10)
        );
        assert!(
            !stores
                .account_events
                .consume_completed_before_started(6, 10)
        );
        stores.dispatch("GUILD_BULK_BAN_STARTED", &json!({"guild_id": "7"}));
        stores.dispatch("GUILD_BULK_BAN_FAILED", &json!({"guild_id": "7"}));
        assert!(!stores.account_events.has_pending_bulk_ban(7));
    }
}
