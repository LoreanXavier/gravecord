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

    /// `I(messageId)` of 93474: the notice leaves with a new version.
    pub fn automod_notice_removed(&mut self, message: &str) {
        if self.automod.failed.remove(message).is_some() {
            self.automod.version += 1;
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
