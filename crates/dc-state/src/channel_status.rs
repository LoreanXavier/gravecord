//! ChannelStatusStore (module 309698) and VoiceChannelStartTimeStore (module 725613): the status line of a voice
//! channel and the moment its call began, both per guild and channel.
//!
//! The statuses are not part of GUILD_CREATE. A client asks for them once per guild and connection (op 43
//! `REQUEST_CHANNEL_INFO` with `fields: ["status", "voice_start_time"]`, module 52074) and the answer arrives as
//! `CHANNEL_INFO {guild_id, channels: [{id, status, voice_start_time}]}`; later changes come as
//! `VOICE_CHANNEL_STATUS_UPDATE {id, guild_id, status}` and `VOICE_CHANNEL_START_TIME_UPDATE {id, guild_id,
//! voice_start_time}`.

use crate::Id;
use serde_json::Value;
use std::collections::{HashMap, HashSet};

fn id(value: &Value) -> Option<Id> {
    value
        .as_str()
        .and_then(|s| s.parse().ok())
        .or_else(|| value.as_u64())
}

/// A status of `""` or null is no status (`getChannelStatus` hands the raw value back, and the callers test its
/// length, module 882840 and 616740).
fn status_of(value: &Value) -> Option<String> {
    value
        .as_str()
        .filter(|text| !text.is_empty())
        .map(str::to_owned)
}

/// `voice_start_time` is in seconds; the store keeps milliseconds (`o` in module 725613).
fn start_ms(value: &Value) -> Option<u64> {
    value
        .as_u64()
        .or_else(|| value.as_f64().map(|seconds| seconds as u64))
        .map(|seconds| seconds * 1000)
}

#[derive(Default)]
pub struct ChannelStatusStore {
    statuses: HashMap<Id, HashMap<Id, Option<String>>>,
    start_times: HashMap<Id, HashMap<Id, Option<u64>>>,
    requested: HashSet<Id>,
}

impl ChannelStatusStore {
    /// `getChannelStatus`: the status of a voice channel of a guild, when it has one.
    pub fn get_channel_status(&self, guild: Id, channel: Id) -> Option<&str> {
        self.statuses
            .get(&guild)?
            .get(&channel)?
            .as_deref()
            .filter(|text| !text.is_empty())
    }

    /// `getStartTime` of the start-time store: when the call of the channel began, in milliseconds since the epoch.
    pub fn get_start_time(&self, guild: Id, channel: Id) -> Option<u64> {
        self.start_times.get(&guild)?.get(&channel).copied()?
    }

    /// `hasRequestedStatuses`: op 43 went out for the guild on this connection.
    pub fn has_requested(&self, guild: Id) -> bool {
        self.requested.contains(&guild)
    }

    pub fn dispatch(&mut self, event: &str, data: &Value) {
        match event {
            // `CONNECTION_OPEN` and `CONNECTION_RESUMED`: every guild is asked again.
            "READY" | "RESUMED" => self.requested.clear(),
            "GUILD_CREATE" | "GUILD_DELETE" => {
                if let Some(guild) = id(&data["id"]).or_else(|| id(&data["guild_id"])) {
                    self.requested.remove(&guild);
                }
            }
            // The action the request itself dispatches (module 52074).
            "FETCH_CHANNEL_INFO" => {
                if let Some(guild) = id(&data["guild_id"]) {
                    self.requested.insert(guild);
                }
            }
            "VOICE_CHANNEL_STATUS_UPDATE" => {
                if let (Some(guild), Some(channel)) = (id(&data["guild_id"]), id(&data["id"])) {
                    self.statuses
                        .entry(guild)
                        .or_default()
                        .insert(channel, status_of(&data["status"]));
                }
            }
            "VOICE_CHANNEL_START_TIME_UPDATE" => {
                if let (Some(guild), Some(channel)) = (id(&data["guild_id"]), id(&data["id"])) {
                    self.start_times
                        .entry(guild)
                        .or_default()
                        .insert(channel, start_ms(&data["voice_start_time"]));
                }
            }
            // The answer of op 43 replaces both maps of the guild.
            "CHANNEL_INFO" => {
                let Some(guild) = id(&data["guild_id"]) else {
                    return;
                };
                let statuses = self.statuses.entry(guild).or_default();
                let starts = self.start_times.entry(guild).or_default();
                statuses.clear();
                starts.clear();
                for channel in data["channels"].as_array().into_iter().flatten() {
                    let Some(channel_id) = id(&channel["id"]) else {
                        continue;
                    };
                    statuses.insert(channel_id, status_of(&channel["status"]));
                    starts.insert(channel_id, start_ms(&channel["voice_start_time"]));
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

    #[test]
    fn channel_info_fills_and_replaces_a_guilds_statuses() {
        let mut store = ChannelStatusStore::default();
        store.dispatch(
            "CHANNEL_INFO",
            &json!({"guild_id": "1", "channels": [
                {"id": "10", "status": "Playing", "voice_start_time": 1000},
                {"id": "11", "status": null},
            ]}),
        );
        assert_eq!(store.get_channel_status(1, 10), Some("Playing"));
        assert_eq!(store.get_channel_status(1, 11), None);
        assert_eq!(store.get_start_time(1, 10), Some(1_000_000));
        assert_eq!(store.get_start_time(1, 11), None);
        // A second answer replaces the first one.
        store.dispatch(
            "CHANNEL_INFO",
            &json!({"guild_id": "1", "channels": [{"id": "11", "status": "Now"}]}),
        );
        assert_eq!(store.get_channel_status(1, 10), None);
        assert_eq!(store.get_channel_status(1, 11), Some("Now"));
    }

    #[test]
    fn status_updates_set_and_clear() {
        let mut store = ChannelStatusStore::default();
        store.dispatch(
            "VOICE_CHANNEL_STATUS_UPDATE",
            &json!({"id": "10", "guild_id": "1", "status": "Hello"}),
        );
        assert_eq!(store.get_channel_status(1, 10), Some("Hello"));
        store.dispatch(
            "VOICE_CHANNEL_STATUS_UPDATE",
            &json!({"id": "10", "guild_id": "1", "status": null}),
        );
        assert_eq!(store.get_channel_status(1, 10), None);
        store.dispatch(
            "VOICE_CHANNEL_STATUS_UPDATE",
            &json!({"id": "10", "guild_id": "1", "status": ""}),
        );
        assert_eq!(store.get_channel_status(1, 10), None);
    }

    #[test]
    fn start_time_updates_follow_the_event() {
        let mut store = ChannelStatusStore::default();
        store.dispatch(
            "VOICE_CHANNEL_START_TIME_UPDATE",
            &json!({"id": "10", "guild_id": "1", "voice_start_time": 5}),
        );
        assert_eq!(store.get_start_time(1, 10), Some(5000));
        store.dispatch(
            "VOICE_CHANNEL_START_TIME_UPDATE",
            &json!({"id": "10", "guild_id": "1"}),
        );
        assert_eq!(store.get_start_time(1, 10), None);
    }

    #[test]
    fn requests_are_remembered_until_a_new_connection_or_guild_change() {
        let mut store = ChannelStatusStore::default();
        store.dispatch("FETCH_CHANNEL_INFO", &json!({"guild_id": "1"}));
        assert!(store.has_requested(1));
        store.dispatch("GUILD_CREATE", &json!({"id": "1"}));
        assert!(!store.has_requested(1));
        store.dispatch("FETCH_CHANNEL_INFO", &json!({"guild_id": "1"}));
        store.dispatch("RESUMED", &json!({}));
        assert!(!store.has_requested(1));
    }
}
