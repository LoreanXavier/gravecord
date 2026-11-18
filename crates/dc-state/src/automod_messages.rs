//! GuildAutomodMessageStore (module 93474): the messages AutoMod blocked or deleted, with the notice shown in their
//! place, and the mention-raid alert of each server.
//!
//! `AUTO_MODERATION_CONTENT_DELETED {message, notice}` keeps `{id, messageData: undefined, isBlockedEdit: false,
//! errorMessage: notice}` under the message id (only when the event carries a message) and counts a new version;
//! `READY` (`CONNECTION_OPEN`) forgets them all; `REMOVE_AUTOMOD_MESSAGE_NOTICE` drops one;
//! `AUTO_MODERATION_MENTION_RAID_DETECTION {guild_id, decision_id, suspicious_mention_activity_until}` sets the
//! server's alert and `AUTO_MODERATION_MENTION_RAID_NOTICE_DISMISS` removes it.
use crate::{Id, Stores, id, text};
use serde_json::Value;
use std::collections::HashMap;

/// `MessageFlags.EPHEMERAL` (module 652215 `pr7`).
const EPHEMERAL: u64 = 1 << 6;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct FailedMessage {
    pub id: String,
    pub is_blocked_edit: bool,
    pub error_message: Value,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct MentionRaid {
    pub guild: Id,
    pub decision_id: Option<String>,
    pub suspicious_mention_activity_until: Option<String>,
}

#[derive(Default)]
pub struct AutomodMessages {
    failed: HashMap<String, FailedMessage>,
    /// MessageStore's `k` (module 596344): the notice of each message AutoMod deleted after it was posted, kept when
    /// the message's channel was loaded.
    removal_notices: HashMap<Id, Value>,
    raids: HashMap<Id, MentionRaid>,
    version: u64,
}

impl Stores {
    pub(crate) fn automod_dispatch(&mut self, name: &str, data: &Value) {
        let store = &mut self.automod;
        match name {
            "READY" => {
                if !store.failed.is_empty() {
                    store.failed.clear();
                    store.version += 1;
                }
            }
            "AUTO_MODERATION_CONTENT_DELETED" => {
                self.automod_content_deleted(data);
                let store = &mut self.automod;
                if let Some(message) = text(&data["message"]["id"]) {
                    store.failed.insert(
                        message.clone(),
                        FailedMessage {
                            id: message,
                            is_blocked_edit: false,
                            error_message: data["notice"].clone(),
                        },
                    );
                    store.version += 1;
                }
            }
            "AUTO_MODERATION_MENTION_RAID_DETECTION" => {
                if let Some(guild) = id(&data["guild_id"]) {
                    store.raids.insert(
                        guild,
                        MentionRaid {
                            guild,
                            decision_id: text(&data["decision_id"]),
                            suspicious_mention_activity_until: text(
                                &data["suspicious_mention_activity_until"],
                            ),
                        },
                    );
                }
            }
            "AUTO_MODERATION_MENTION_RAID_NOTICE_DISMISS" => {
                if let Some(guild) = id(&data["guild_id"]) {
                    store.raids.remove(&guild);
                }
            }
            "REMOVE_AUTOMOD_MESSAGE_NOTICE" => {
                if let Some(message) = text(&data["message_id"]) {
                    self.automod_notice_removed(&message);
                }
            }
            _ => {}
        }
    }

    /// MessageStore (module 596344) on `AUTO_MODERATION_CONTENT_DELETED {message, thread, notice}`: a message (not a
    /// thread) of a loaded channel is received if it is not there yet, becomes EPHEMERAL (`1 << 6`) and keeps its
    /// notice (`getAutomodRemovalNotice`).
    fn automod_content_deleted(&mut self, data: &Value) {
        if !data["thread"].is_null() || !data["message"].is_object() {
            return;
        }
        let Some(message) = crate::Message::from_value(&data["message"]) else {
            return;
        };
        let channel = message.channel_id;
        if !self
            .windows
            .get(&channel)
            .is_some_and(|window| window.ready)
        {
            return;
        }
        let id = message.id;
        if !self
            .messages
            .get(&channel)
            .is_some_and(|list| list.iter().any(|known| known.id == id))
        {
            self.receive_message(message);
        }
        let Some(known) = self
            .messages
            .get_mut(&channel)
            .and_then(|list| list.iter_mut().find(|known| known.id == id))
        else {
            return;
        };
        known.flags |= EPHEMERAL;
        self.automod
            .removal_notices
            .insert(id, data["notice"].clone());
    }

    /// MessageStore `getAutomodRemovalNotice(messageId)`.
    pub fn automod_removal_notice(&self, message: Id) -> Option<&Value> {
        self.automod.removal_notices.get(&message)
    }

    /// `I(messageId)` of 93474: the notice leaves with a new version.
    pub fn automod_notice_removed(&mut self, message: &str) {
        if self.automod.failed.remove(message).is_some() {
            self.automod.version += 1;
        }
    }

    /// `MESSAGE_SEND_FAILED_AUTOMOD` and `MESSAGE_EDIT_FAILED_AUTOMOD` (`S` of 93474): the message keeps its reason
    /// under `id` (the send's message, or the edited one) with a new version.
    pub fn automod_failed(&mut self, id: String, is_blocked_edit: bool, reason: String) {
        self.automod.failed.insert(
            id.clone(),
            FailedMessage {
                id,
                is_blocked_edit,
                error_message: Value::String(reason),
            },
        );
        self.automod.version += 1;
    }

    /// `MESSAGE_EDIT_FAILED_AUTOMOD` (module 469759's blocked edit): GuildAutomodMessageStore keeps the reason under the
    /// edited message with `isBlockedEdit`, and MessageStore's `q` (module 596344) empties the message's embeds when
    /// one of them is not AutoMod's own (`auto_moderation_message` / `auto_moderation_notification`). The message keeps
    /// its old text: the server never took the edit.
    pub fn automod_edit_failed(&mut self, channel: Id, message: Id, reason: String) {
        if let Some(known) = self
            .messages
            .get_mut(&channel)
            .and_then(|list| list.iter_mut().find(|known| known.id == message))
            && known.embeds.iter().any(|embed| {
                embed.kind != "auto_moderation_message"
                    && embed.kind != "auto_moderation_notification"
            })
        {
            known.embeds.clear();
        }
        self.automod_failed(message.to_string(), true, reason);
    }

    /// `MESSAGE_END_EDIT {response}` of 93474: an edit the server took (its answer carries the message id and no
    /// `AUTOMOD_MESSAGE_BLOCKED` code) drops the message's notice.
    pub fn automod_edit_ended(&mut self, response: &Value) {
        if response["code"].as_u64() == Some(200_000) {
            return;
        }
        if let Some(id) = text(&response["id"]) {
            self.automod_notice_removed(&id);
        }
    }

    /// `getMessage(id)`.
    pub fn automod_message(&self, message: &str) -> Option<&FailedMessage> {
        self.automod.failed.get(message)
    }

    /// `getMessagesVersion()`.
    pub fn automod_messages_version(&self) -> u64 {
        self.automod.version
    }

    /// `getMentionRaidDetected(guild)`.
    pub fn automod_mention_raid(&self, guild: Id) -> Option<&MentionRaid> {
        self.automod.raids.get(&guild)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_deleted_message_keeps_its_notice_until_removed_or_ready() {
        let mut stores = Stores::default();
        let notice = "Your message was blocked by AutoMod";
        stores.dispatch(
            "AUTO_MODERATION_CONTENT_DELETED",
            &json!({"guild_id": "1", "channel_id": "2", "message": {"id": "50"}, "notice": notice}),
        );
        assert_eq!(
            stores
                .automod_message("50")
                .map(|m| m.error_message.clone()),
            Some(json!(notice))
        );
        assert_eq!(stores.automod_messages_version(), 1);
        // An event without a message changes nothing.
        stores.dispatch(
            "AUTO_MODERATION_CONTENT_DELETED",
            &json!({"guild_id": "1", "notice": notice}),
        );
        assert_eq!(stores.automod_messages_version(), 1);
        stores.dispatch(
            "REMOVE_AUTOMOD_MESSAGE_NOTICE",
            &json!({"message_id": "50"}),
        );
        assert!(stores.automod_message("50").is_none());
        assert_eq!(stores.automod_messages_version(), 2);
        stores.dispatch(
            "AUTO_MODERATION_CONTENT_DELETED",
            &json!({"message": {"id": "51"}, "notice": notice}),
        );
        stores.dispatch("READY", &json!({}));
        assert!(stores.automod_message("51").is_none());
    }

    #[test]
    fn a_message_deleted_in_a_loaded_channel_turns_private_and_keeps_its_notice() {
        let mut stores = Stores::default();
        let message = |id: &str, channel: &str| {
            json!({"id": id, "channel_id": channel, "content": "x", "type": 0, "flags": 0,
                "timestamp": "2026-11-17T21:00:00+00:00",
                "author": {"id": "9", "username": "a", "discriminator": "0"}})
        };
        stores.load_messages(2, &json!([]));
        stores.dispatch(
            "AUTO_MODERATION_CONTENT_DELETED",
            &json!({"guild_id": "1", "channel_id": "2", "message": message("60", "2"), "notice": "n"}),
        );
        let known = stores.messages[&2].iter().find(|m| m.id == 60).unwrap();
        assert_eq!(known.flags & EPHEMERAL, EPHEMERAL);
        assert_eq!(stores.automod_removal_notice(60), Some(&json!("n")));
        // A channel that was never loaded, or a thread, keeps no notice: the dialog shows instead.
        stores.dispatch(
            "AUTO_MODERATION_CONTENT_DELETED",
            &json!({"channel_id": "3", "message": message("61", "3"), "notice": "n"}),
        );
        assert!(stores.automod_removal_notice(61).is_none());
        stores.dispatch(
            "AUTO_MODERATION_CONTENT_DELETED",
            &json!({"channel_id": "2", "message": message("62", "2"), "thread": {"id": "5"}, "notice": "n"}),
        );
        assert!(stores.automod_removal_notice(62).is_none());
    }

    #[test]
    fn a_blocked_edit_keeps_the_old_text_and_its_notice_until_an_edit_goes_through() {
        let mut stores = Stores::default();
        stores.load_messages(
            2,
            &json!([{"id": "70", "channel_id": "2", "content": "old", "type": 0, "flags": 0,
                "timestamp": "2026-11-17T21:00:00+00:00",
                "author": {"id": "9", "username": "a", "discriminator": "0"},
                "embeds": [{"type": "rich", "title": "t"}]}]),
        );
        stores.automod_edit_failed(2, 70, "blocked".to_owned());
        let failed = stores.automod_message("70").unwrap();
        assert!(failed.is_blocked_edit);
        assert_eq!(failed.error_message, json!("blocked"));
        let known = &stores.messages[&2][0];
        assert_eq!(known.content, "old");
        assert!(known.embeds.is_empty());
        assert_eq!(known.flags & EPHEMERAL, 0);
        // A second refusal's answer keeps the notice; an edit the server took drops it.
        stores.automod_edit_ended(&json!({"code": 200000, "message": "no"}));
        assert!(stores.automod_message("70").is_some());
        stores.automod_edit_ended(&json!({"id": "70", "content": "new"}));
        assert!(stores.automod_message("70").is_none());
    }

    #[test]
    fn the_mention_raid_alert_is_set_and_dismissed_per_server() {
        let mut stores = Stores::default();
        stores.dispatch(
            "AUTO_MODERATION_MENTION_RAID_DETECTION",
            &json!({"guild_id": "7", "decision_id": "d", "suspicious_mention_activity_until": "2026-11-12T00:00:00Z"}),
        );
        assert_eq!(
            stores
                .automod_mention_raid(7)
                .and_then(|r| r.decision_id.clone())
                .as_deref(),
            Some("d")
        );
        assert!(stores.automod_mention_raid(8).is_none());
        stores.dispatch(
            "AUTO_MODERATION_MENTION_RAID_NOTICE_DISMISS",
            &json!({"guild_id": "7"}),
        );
        assert!(stores.automod_mention_raid(7).is_none());
    }
}
