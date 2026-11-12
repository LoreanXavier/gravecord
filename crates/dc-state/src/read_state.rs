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
    /// The read states that are not a channel's (module 218461, `read_state_type` other than 0: guild events,
    /// notification center, guild home, onboarding question, message requests), by (resource id, type).
    pub typed: HashMap<(Id, u64), TypedReadState>,
}

/// One non-channel read state: what was acked, the newest thing there is, and the unread badge.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TypedReadState {
    pub acked: Id,
    /// `lastMessageId` of the official entry; 0 when nothing newer is known.
    pub last: Id,
    pub mention_count: u64,
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
            let kind = entry["read_state_type"].as_u64().unwrap_or(0);
            if kind != 0 {
                if let Some(resource) = snowflake(&entry["id"]) {
                    self.typed.insert(
                        (resource, kind),
                        TypedReadState {
                            acked: snowflake(&entry["last_acked_id"]).unwrap_or(0),
                            last: 0,
                            mention_count: entry["badge_count"].as_u64().unwrap_or(0),
                        },
                    );
                }
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

    /// `e2` of module 218461, run by `USER_NON_CHANNEL_ACK` (the resource is the account) and `GUILD_FEATURE_ACK`
    /// (the resource is the guild): a state that has something unacked takes the acked id (else its newest, else
    /// now) and loses its badge. True when the state changed.
    pub fn non_channel_ack(&mut self, resource: Id, kind: u64, acked: Option<Id>, now: Id) -> bool {
        let state = self.typed.entry((resource, kind)).or_default();
        if acked == Some(state.acked)
            || state.last == state.acked
            || (state.last == 0 && state.mention_count == 0)
        {
            return false;
        }
        state.acked = acked
            .or(Some(state.last).filter(|last| *last != 0))
            .unwrap_or(now);
        state.mention_count = 0;
        true
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
    fn a_non_channel_ack_moves_the_typed_state_and_clears_its_badge() {
        let mut states = ReadStates::default();
        states.load(&json!({"entries": [
            {"id": "1", "last_acked_id": "50", "read_state_type": 2, "badge_count": 2},
        ]}));
        assert_eq!(states.typed[&(1, 2)].mention_count, 2);
        // The ack names an id: the state takes it and loses its badge.
        assert!(states.non_channel_ack(1, 2, Some(80), 999));
        assert_eq!(
            states.typed[&(1, 2)],
            TypedReadState {
                acked: 80,
                last: 0,
                mention_count: 0
            }
        );
        // The same ack again, or one for a state with nothing waiting, changes nothing.
        assert!(!states.non_channel_ack(1, 2, Some(80), 999));
        assert!(!states.non_channel_ack(7, 3, None, 999));
        // Without an id the newest known one is taken, else now.
        states.typed.insert(
            (9, 5),
            TypedReadState {
                acked: 1,
                last: 40,
                mention_count: 1,
            },
        );
        assert!(states.non_channel_ack(9, 5, None, 999));
        assert_eq!(states.typed[&(9, 5)].acked, 40);
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
