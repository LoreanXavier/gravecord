//! Gateway dispatches that change messages and the account without a store of their own.
//!
//! `MESSAGE_DELETE_BULK` (module 596344: `removeMany`, every id leaves the channel's messages), the reaction events
//! `MESSAGE_REACTION_REMOVE_ALL` (`set("reactions", [])`), `MESSAGE_REACTION_REMOVE_EMOJI`
//! (`removeReactionsForEmoji`) and `MESSAGE_REACTION_ADD_MANY` (`addReactionBatch`: each listed user adds the
//! emoji, `me` when the user is the account; module 383233), and `USER_UPDATE`, which the client dispatches as
//! `CURRENT_USER_UPDATE` with the whole user (module 225123).

use crate::{Account, Id, Stores, User, id, text};
use serde_json::{Value, json};

impl Stores {
    /// `MESSAGE_DELETE_BULK {ids, channel_id, guild_id}`.
    pub(crate) fn message_delete_bulk(&mut self, data: &Value) {
        let Some(channel) = id(&data["channel_id"]) else {
            return;
        };
        let ids: Vec<Id> = data["ids"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(id)
            .collect();
        self.delete_messages(channel, &ids);
    }

    /// `VOICE_STATE_UPDATE_BATCH {voice_states: [...]}`: each state is one `VOICE_STATE_UPDATE` (module 225123 `eH`).
    pub(crate) fn voice_state_batch(&mut self, data: &Value) {
        for state in data["voice_states"].as_array().into_iter().flatten() {
            self.dispatch("VOICE_STATE_UPDATE", state);
        }
    }

    /// `MESSAGE_REACTION_REMOVE_ALL {channel_id, message_id}` and `MESSAGE_REACTION_REMOVE_EMOJI {…, emoji}`: all
    /// reactions go, or the ones of one emoji (matched by id for a custom emoji, otherwise by name).
    pub(crate) fn reactions_removed(&mut self, data: &Value, only_emoji: bool) {
        let (Some(channel), Some(message_id)) = (id(&data["channel_id"]), id(&data["message_id"]))
        else {
            return;
        };
        let Some(message) = self
            .messages
            .get_mut(&channel)
            .and_then(|list| list.iter_mut().find(|known| known.id == message_id))
        else {
            return;
        };
        if !only_emoji {
            message.reactions.clear();
            return;
        }
        let emoji_id = id(&data["emoji"]["id"]);
        let name = text(&data["emoji"]["name"]);
        message.reactions.retain(|reaction| {
            let same = match emoji_id {
                Some(emoji_id) => reaction.emoji_id == Some(emoji_id),
                None => reaction.emoji_id.is_none() && Some(&reaction.name) == name.as_ref(),
            };
            !same
        });
    }

    /// `MESSAGE_REACTION_ADD_MANY {channel_id, message_id, reactions: [{emoji, users: [id]}]}`.
    pub(crate) fn reaction_add_many(&mut self, data: &Value) {
        for reaction in data["reactions"].as_array().into_iter().flatten() {
            for user in reaction["users"].as_array().into_iter().flatten() {
                self.react(
                    &json!({
                        "channel_id": data["channel_id"],
                        "message_id": data["message_id"],
                        "user_id": user,
                        "emoji": reaction["emoji"],
                    }),
                    1,
                );
            }
        }
    }

    /// `USER_UPDATE`: the account's own user object changed (name, avatar, banner, flags, premium). The cached copy
    /// every list reads is replaced too, and the private half the account settings read is refreshed.
    pub(crate) fn user_update(&mut self, data: &Value) {
        let Some(user) = User::from_value(data) else {
            return;
        };
        if self.current_user.as_ref().map(|me| me.id) != Some(user.id) {
            return;
        }
        let mine: Id = user.id;
        self.current_user = Some(user.clone());
        self.users.insert(mine, user);
        // The fields USER_UPDATE leaves out stay as they were (`mfa_enabled` and the like arrive only on some).
        let before = self.account.clone();
        let mut account = Account::from_value(data);
        if data.get("email").is_none() {
            account.email = before.email;
        }
        if data.get("phone").is_none() {
            account.phone = before.phone;
        }
        if data.get("mfa_enabled").is_none() {
            account.mfa_enabled = before.mfa_enabled;
        }
        if data.get("verified").is_none() {
            account.verified = before.verified;
        }
        if data.get("nsfw_allowed").is_none() {
            account.nsfw_allowed = before.nsfw_allowed;
        }
        if data.get("premium_type").is_none() {
            account.premium_type = before.premium_type;
        }
        if data.get("age_verification_status").is_none() {
            account.age_verification_status = before.age_verification_status;
        }
        account.age_gated_features = before.age_gated_features;
        self.mfa_enabled = account.mfa_enabled;
        self.account = account;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn message(channel: u64, id: u64) -> Value {
        json!({
            "id": id.to_string(), "channel_id": channel.to_string(), "content": "x", "type": 0,
            "timestamp": "2026-11-11T12:00:00.000000+00:00",
            "author": {"id": "7", "username": "a", "discriminator": "0"},
            "reactions": [],
        })
    }

    fn me(stores: &mut Stores) {
        stores.current_user = Some(User {
            id: 10,
            ..User::default()
        });
    }

    #[test]
    fn a_voice_state_batch_is_one_update_per_state() {
        let mut stores = Stores::default();
        let state = |user: &str, channel: Value| {
            json!({"guild_id": "1", "user_id": user, "channel_id": channel, "session_id": "s",
                   "deaf": false, "mute": false, "self_deaf": false, "self_mute": false})
        };
        stores.dispatch(
            "VOICE_STATE_UPDATE_BATCH",
            &json!({"voice_states": [state("5", json!("30")), state("6", json!("30")), state("5", Value::Null)]}),
        );
        let users: Vec<Id> = stores.voice_states[&1]
            .iter()
            .map(|state| state.user_id)
            .collect();
        assert_eq!(users, vec![6]);
    }

    #[test]
    fn a_bulk_delete_removes_every_listed_message() {
        let mut stores = Stores::default();
        for id in [5, 6, 7, 8] {
            stores.dispatch("MESSAGE_CREATE", &message(23, id));
        }
        stores.dispatch(
            "MESSAGE_DELETE_BULK",
            &json!({"channel_id": "23", "guild_id": "1", "ids": ["5", "7", "99"]}),
        );
        let left: Vec<Id> = stores.messages[&23].iter().map(|known| known.id).collect();
        assert_eq!(left, vec![6, 8]);
    }

    #[test]
    fn a_bulk_delete_in_an_unknown_channel_changes_nothing() {
        let mut stores = Stores::default();
        stores.dispatch(
            "MESSAGE_DELETE_BULK",
            &json!({"channel_id": "23", "ids": ["5"]}),
        );
        assert!(stores.messages.is_empty());
    }

    #[test]
    fn removing_all_reactions_and_one_emoji() {
        let mut stores = Stores::default();
        me(&mut stores);
        stores.dispatch("MESSAGE_CREATE", &message(23, 5));
        let react = |user: &str, name: &str, id: Value| json!({"channel_id": "23", "message_id": "5", "user_id": user, "emoji": {"name": name, "id": id}});
        stores.dispatch("MESSAGE_REACTION_ADD", &react("10", "a", Value::Null));
        stores.dispatch("MESSAGE_REACTION_ADD", &react("11", "b", json!("900")));
        stores.dispatch("MESSAGE_REACTION_ADD", &react("11", "b", Value::Null));
        assert_eq!(stores.messages[&23][0].reactions.len(), 3);
        // The custom emoji is matched by its id: the unicode one of the same name stays.
        stores.dispatch(
            "MESSAGE_REACTION_REMOVE_EMOJI",
            &json!({"channel_id": "23", "message_id": "5", "emoji": {"name": "b", "id": "900"}}),
        );
        let names: Vec<(&str, Option<Id>)> = stores.messages[&23][0]
            .reactions
            .iter()
            .map(|reaction| (reaction.name.as_str(), reaction.emoji_id))
            .collect();
        assert_eq!(names, vec![("a", None), ("b", None)]);
        stores.dispatch(
            "MESSAGE_REACTION_REMOVE_ALL",
            &json!({"channel_id": "23", "message_id": "5"}),
        );
        assert!(stores.messages[&23][0].reactions.is_empty());
    }

    #[test]
    fn a_reaction_batch_counts_every_user_and_marks_mine() {
        let mut stores = Stores::default();
        me(&mut stores);
        stores.dispatch("MESSAGE_CREATE", &message(23, 5));
        stores.dispatch(
            "MESSAGE_REACTION_ADD_MANY",
            &json!({"channel_id": "23", "message_id": "5", "reactions": [
                {"emoji": {"name": "a", "id": null}, "users": ["10", "11", "12"]},
                {"emoji": {"name": "b", "id": null}, "users": ["11"]},
            ]}),
        );
        let reactions = &stores.messages[&23][0].reactions;
        assert_eq!((reactions[0].count, reactions[0].me), (3, true));
        assert_eq!((reactions[1].count, reactions[1].me), (1, false));
    }

    #[test]
    fn user_update_replaces_the_current_user_and_keeps_the_private_fields() {
        let mut stores = Stores::default();
        stores.dispatch(
            "READY",
            &json!({
                "user": {"id": "10", "username": "old", "discriminator": "0", "email": "a@b.c", "mfa_enabled": true, "verified": true},
                "guilds": [], "private_channels": [],
            }),
        );
        assert_eq!(stores.account.email.as_deref(), Some("a@b.c"));
        stores.dispatch(
            "USER_UPDATE",
            &json!({"id": "10", "username": "new", "global_name": "New Name", "discriminator": "0", "avatar": "abc"}),
        );
        let user = stores.current_user.as_ref().unwrap();
        assert_eq!(user.username, "new");
        assert_eq!(user.global_name.as_deref(), Some("New Name"));
        assert_eq!(user.avatar.as_deref(), Some("abc"));
        assert_eq!(stores.users[&10].username, "new");
        assert_eq!(stores.account.email.as_deref(), Some("a@b.c"));
        assert!(stores.account.mfa_enabled && stores.mfa_enabled);
        // Another user's object is not the account.
        stores.dispatch(
            "USER_UPDATE",
            &json!({"id": "11", "username": "other", "discriminator": "0"}),
        );
        assert_eq!(stores.current_user.as_ref().unwrap().username, "new");
        assert!(!stores.users.contains_key(&11));
    }
}
