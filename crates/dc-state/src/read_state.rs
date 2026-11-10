//! `ReadStateStore`: where each channel was last read, how many mentions wait in it, and whether it is unread.
//! Entries come from READY's `read_state` (channel entries have no `read_state_type`, or 0) and `MESSAGE_ACK`;
//! a channel's newest message comes from READY's channels and `MESSAGE_CREATE`.

use crate::Id;
use serde_json::Value;
use std::collections::HashMap;

/// Discord's snowflake epoch, in Unix milliseconds.
const EPOCH_MS: u64 = 1_420_070_400_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Importance {
    High,
    Low,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ReadState {
    /// The last message read; 0 when nothing was.
    pub acked: Id,
    pub mention_count: u64,
    pub low_importance: bool,
    /// Set by Mark Unread (`manual: true`): this client leaves it unread until told otherwise.
    pub manual: bool,
}

#[derive(Default)]
pub struct ReadStates {
    pub states: HashMap<Id, ReadState>,
    /// Channel to its newest message.
    pub latest: HashMap<Id, Id>,
}

fn snowflake(value: &Value) -> Option<Id> {
    match value {
        Value::String(text) => text.parse().ok(),
        Value::Number(number) => number.as_u64(),
        _ => None,
    }
}

/// The first snowflake of a moment, for comparing a join date with message ids.
pub fn snowflake_at(unix_ms: u64) -> Id {
    unix_ms.saturating_sub(EPOCH_MS) << 22
}

/// The Unix milliseconds a snowflake was made at.
pub fn snowflake_ms(id: Id) -> u64 {
    (id >> 22) + EPOCH_MS
}

/// `last_viewed` in an ack: whole days since Discord's epoch (2015-01-01).
pub fn days_since_epoch(unix_ms: u64) -> u64 {
    unix_ms.saturating_sub(EPOCH_MS) / 86_400_000
}

impl ReadStates {
    pub fn load(&mut self, read_state: &Value) {
        for entry in read_state["entries"].as_array().into_iter().flatten() {
            if entry["read_state_type"].as_u64().unwrap_or(0) != 0 {
                continue;
            }
            let Some(channel) = snowflake(&entry["id"]) else {
                continue;
            };
            self.states.insert(
                channel,
                ReadState {
                    acked: snowflake(&entry["last_message_id"]).unwrap_or(0),
                    mention_count: entry["mention_count"].as_u64().unwrap_or(0),
                    low_importance: entry["flags"].as_u64().unwrap_or(0) & 4 != 0,
                    manual: false,
                },
            );
        }
    }

    pub fn ack(&mut self, data: &Value) {
        let (Some(channel), Some(message)) = (
            snowflake(&data["channel_id"]),
            snowflake(&data["message_id"]),
        ) else {
            return;
        };
        self.states.insert(
            channel,
            ReadState {
                acked: message,
                mention_count: data["mention_count"].as_u64().unwrap_or(0),
                low_importance: data["flags"].as_u64().unwrap_or(0) & 4 != 0,
                manual: data["manual"].as_bool().unwrap_or(false),
            },
        );
    }

    /// Local ack before the gateway confirms it.
    pub fn ack_local(&mut self, channel: Id, message: Id, manual: bool) {
        self.states.insert(
            channel,
            ReadState {
                acked: message,
                mention_count: 0,
                low_importance: false,
                manual,
            },
        );
    }

    pub fn message_created(
        &mut self,
        channel: Id,
        message: Id,
        mention: Option<Importance>,
        mine: bool,
    ) {
        let latest = self.latest.entry(channel).or_insert(0);
        *latest = (*latest).max(message);
        if mine {
            // The client acks its own messages as it sends them.
            self.ack_local(channel, message, false);
        } else if let Some(importance) = mention {
            let state = self.states.entry(channel).or_default();
            if importance == Importance::High || state.mention_count == 0 {
                state.low_importance = importance == Importance::Low;
            }
            state.mention_count = state.mention_count.saturating_add(1);
        }
    }

    /// The ack that counts for `channel`: its read state, or for a channel never read, the moment the guild was
    /// joined (messages before then are not unread).
    pub fn acked(&self, channel: Id, joined: Id) -> Id {
        self.states.get(&channel).map_or(joined, |state| {
            state.acked.max(if state.acked == 0 { joined } else { 0 })
        })
    }

    pub fn is_unread(&self, channel: Id, joined: Id) -> bool {
        self.latest
            .get(&channel)
            .is_some_and(|latest| *latest > self.acked(channel, joined))
    }

    pub fn mentions(&self, channel: Id) -> u64 {
        self.states
            .get(&channel)
            .map_or(0, |state| state.mention_count)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn unread_follows_acks_and_new_messages() {
        let mut states = ReadStates::default();
        states.load(&json!({"entries": [
            {"id": "10", "last_message_id": "100", "mention_count": 0},
            {"id": "1", "last_acked_id": 0, "read_state_type": 2, "badge_count": 2},
        ]}));
        states.latest.insert(10, 100);
        assert!(!states.is_unread(10, 0));
        states.message_created(10, 120, Some(Importance::High), false);
        assert!(states.is_unread(10, 0));
        assert_eq!(states.mentions(10), 1);
        states.ack(&json!({"channel_id": "10", "message_id": "120", "manual": false}));
        assert!(!states.is_unread(10, 0));
        assert_eq!(states.mentions(10), 0);
        states.message_created(10, 130, None, true);
        assert!(!states.is_unread(10, 0));
        assert!(!states.states.contains_key(&1));
    }

    #[test]
    fn never_read_channels_count_from_the_join() {
        let mut states = ReadStates::default();
        states.latest.insert(20, snowflake_at(2_000_000_000_000));
        assert!(states.is_unread(20, snowflake_at(1_900_000_000_000)));
        assert!(!states.is_unread(20, snowflake_at(2_100_000_000_000)));
        assert_eq!(days_since_epoch(1_794_098_294_600), 4329);
    }
}
