//! Unread pins: `lastPinTimestamp` and `ackPinTimestamp` of every channel read state (module 218461).
//!
//! The newest pin time comes from the channels (READY, GUILD_CREATE, CHANNEL_CREATE, a thread's own field),
//! PASSIVE_UPDATE_V2 and CHANNEL_PINS_UPDATE; the acknowledged one from READY's `read_state` entries
//! (`last_pin_timestamp`) and CHANNEL_PINS_ACK. `hasUnreadPins` is "the newest pin is later than the acknowledged
//! one". Opening the pins popout while it is true acknowledges (module 945830 calls `ackPins`): the app sends
//! `POST /channels/{id}/pins/ack` and the local acknowledged time becomes the newest.

use crate::{Id, Stores, id};
use chrono::DateTime;
use serde_json::Value;
use std::collections::HashMap;

/// `ey`: an ISO time as Unix milliseconds, 0 when absent or unreadable.
fn time_ms(value: &Value) -> i64 {
    value
        .as_str()
        .and_then(|text| DateTime::parse_from_rfc3339(text).ok())
        .map_or(0, |time| time.timestamp_millis())
}

#[derive(Debug, Default)]
pub struct PinStates {
    last: HashMap<Id, i64>,
    acked: HashMap<Id, i64>,
}

impl PinStates {
    /// Channel objects (`last_pin_timestamp`); an absent field is 0, as `ey(null)` is.
    fn note_channels(&mut self, channels: &Value) {
        for channel in channels.as_array().into_iter().flatten() {
            self.note_channel(channel);
        }
    }

    fn note_channel(&mut self, channel: &Value) {
        if let Some(channel_id) = id(&channel["id"]) {
            self.last
                .insert(channel_id, time_ms(&channel["last_pin_timestamp"]));
        }
    }

    pub fn last_pin(&self, channel: Id) -> i64 {
        self.last.get(&channel).copied().unwrap_or(0)
    }

    pub fn acked_pin(&self, channel: Id) -> i64 {
        self.acked.get(&channel).copied().unwrap_or(0)
    }

    /// `lastPinTimestamp > ackPinTimestamp`.
    pub fn newer_than_acked(&self, channel: Id) -> bool {
        self.last_pin(channel) > self.acked_pin(channel)
    }

    /// `ackPins(null)`: true when a `POST /channels/{id}/pins/ack` is due; the acknowledged time becomes the newest.
    pub fn ack_local(&mut self, channel: Id) -> bool {
        if self.last_pin(channel) == self.acked_pin(channel) {
            return false;
        }
        self.acked.insert(channel, self.last_pin(channel));
        true
    }

    pub fn dispatch(&mut self, name: &str, data: &Value) {
        match name {
            "READY" => {
                self.last.clear();
                self.acked.clear();
                for guild in data["guilds"].as_array().into_iter().flatten() {
                    self.note_channels(&guild["channels"]);
                    self.note_channels(&guild["threads"]);
                }
                self.note_channels(&data["private_channels"]);
                for entry in data["read_state"]["entries"]
                    .as_array()
                    .into_iter()
                    .flatten()
                {
                    if entry["read_state_type"].as_u64().unwrap_or(0) != 0 {
                        continue;
                    }
                    if let Some(channel) = id(&entry["id"]) {
                        self.acked
                            .insert(channel, time_ms(&entry["last_pin_timestamp"]));
                    }
                }
            }
            "GUILD_CREATE" => {
                self.note_channels(&data["channels"]);
                self.note_channels(&data["threads"]);
            }
            "CHANNEL_CREATE" | "THREAD_CREATE" => self.note_channel(data),
            "THREAD_LIST_SYNC" => self.note_channels(&data["threads"]),
            "PASSIVE_UPDATE_V2" => {
                for channel in data["updated_channels"].as_array().into_iter().flatten() {
                    self.note_channel(channel);
                }
            }
            // `CHANNEL_PINS_UPDATE`: the new newest pin time.
            "CHANNEL_PINS_UPDATE" => {
                if let Some(channel) = id(&data["channel_id"]) {
                    self.last
                        .insert(channel, time_ms(&data["last_pin_timestamp"]));
                }
            }
            // `CHANNEL_PINS_ACK`: another session acknowledged; its time, or the newest when it names none.
            "CHANNEL_PINS_ACK" => {
                if let Some(channel) = id(&data["channel_id"]) {
                    let time = time_ms(&data["timestamp"]);
                    let value = if time != 0 {
                        time
                    } else {
                        self.last_pin(channel)
                    };
                    self.acked.insert(channel, value);
                }
            }
            "CHANNEL_DELETE" | "THREAD_DELETE" => {
                if let Some(channel) = id(&data["id"]) {
                    self.last.remove(&channel);
                    self.acked.remove(&channel);
                }
            }
            _ => {}
        }
    }
}

impl Stores {
    /// The pin button's dot (`hasUnreadPins`): the newest pin is newer than the acknowledged one, and the channel can
    /// be unread (a thread only when the account joined it).
    pub fn has_unread_pins(&self, channel: Id) -> bool {
        if !self.pin_states.newer_than_acked(channel) {
            return false;
        }
        self.guilds
            .iter()
            .flat_map(|guild| guild.threads.iter())
            .find(|thread| thread.id == channel)
            .is_none_or(|thread| thread.joined)
    }

    /// Opening the pins popout (module 945830): acknowledges when there is something unread. True when
    /// `POST /channels/{id}/pins/ack` is to be sent.
    pub fn ack_pins(&mut self, channel: Id) -> bool {
        self.has_unread_pins(channel) && self.pin_states.ack_local(channel)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn ready_and_events_decide_the_dot() {
        let mut stores = Stores::default();
        stores.dispatch(
            "READY",
            &json!({"user": {"id": "3", "username": "me"}, "users": [],
                "guilds": [{"id": "1", "name": "g", "channels": [
                    {"id": "5", "type": 0, "last_pin_timestamp": "2026-11-10T10:00:00+00:00"},
                    {"id": "6", "type": 0}]}],
                "read_state": {"entries": [
                    {"id": "5", "last_pin_timestamp": "2026-11-09T10:00:00+00:00"},
                    {"id": "6"}]}}),
        );
        assert!(stores.has_unread_pins(5));
        assert!(!stores.has_unread_pins(6));
        // Another session acknowledges: the dot goes without a request.
        stores.dispatch(
            "CHANNEL_PINS_ACK",
            &json!({"channel_id": "5", "timestamp": "2026-11-10T10:00:00+00:00"}),
        );
        assert!(!stores.has_unread_pins(5));
        // A new pin: CHANNEL_PINS_UPDATE raises the newest time.
        stores.dispatch(
            "CHANNEL_PINS_UPDATE",
            &json!({"channel_id": "6", "guild_id": "1", "last_pin_timestamp": "2026-11-11T08:00:00+00:00"}),
        );
        assert!(stores.has_unread_pins(6));
        // Opening the popout acknowledges once and asks for the POST once.
        assert!(stores.ack_pins(6));
        assert!(!stores.has_unread_pins(6));
        assert!(!stores.ack_pins(6));
        // An ack without a time takes the newest.
        stores.dispatch(
            "CHANNEL_PINS_UPDATE",
            &json!({"channel_id": "6", "last_pin_timestamp": "2026-11-12T08:00:00+00:00"}),
        );
        stores.dispatch("CHANNEL_PINS_ACK", &json!({"channel_id": "6"}));
        assert!(!stores.has_unread_pins(6));
    }

    #[test]
    fn a_thread_the_account_has_not_joined_never_shows_the_dot() {
        let mut stores = Stores::default();
        stores.dispatch(
            "READY",
            &json!({"user": {"id": "3", "username": "me"}, "users": [], "guilds": [
                {"id": "1", "name": "g", "channels": [], "threads": [
                    {"id": "9", "parent_id": "5", "type": 11, "name": "t",
                     "last_pin_timestamp": "2026-11-10T10:00:00+00:00",
                     "thread_metadata": {"archived": false}}]}]}),
        );
        assert!(!stores.has_unread_pins(9));
    }
}
