//! StageInstanceStore (module 446600): the Stage instances that are live, by guild and by channel.
//!
//! READY's guilds and GUILD_CREATE carry `stage_instances`; `STAGE_INSTANCE_CREATE` and `STAGE_INSTANCE_UPDATE` carry
//! the instance itself (`{id, guild_id, channel_id, topic, privacy_level, discoverable_disabled,
//! guild_scheduled_event_id}`), `STAGE_INSTANCE_DELETE` the instance that ended. `privacy_level` is 1 public, 2 guild
//! only (module 988794). Starting, editing and ending one are `POST /stage-instances`, `PATCH
//! /stage-instances/<channel>` and `DELETE /stage-instances/<channel>` (module 602146).

use crate::Id;
use serde_json::Value;
use std::collections::HashMap;

/// `privacy_level` values of an instance.
pub const PUBLIC: u64 = 1;
pub const GUILD_ONLY: u64 = 2;

fn id(value: &Value) -> Option<Id> {
    value
        .as_str()
        .and_then(|s| s.parse().ok())
        .or_else(|| value.as_u64())
}

#[derive(Clone, Debug, PartialEq)]
pub struct StageInstance {
    pub id: Id,
    pub guild_id: Id,
    pub channel_id: Id,
    pub topic: String,
    pub privacy_level: u64,
    pub discoverable_disabled: bool,
    pub guild_scheduled_event_id: Option<Id>,
}

impl StageInstance {
    fn from_value(value: &Value, guild: Option<Id>) -> Option<Self> {
        Some(Self {
            id: id(&value["id"])?,
            guild_id: id(&value["guild_id"]).or(guild)?,
            channel_id: id(&value["channel_id"])?,
            topic: value["topic"].as_str().unwrap_or_default().to_owned(),
            privacy_level: value["privacy_level"].as_u64().unwrap_or(GUILD_ONLY),
            discoverable_disabled: value["discoverable_disabled"].as_bool().unwrap_or(false),
            guild_scheduled_event_id: id(&value["guild_scheduled_event_id"]),
        })
    }
}

#[derive(Default)]
pub struct StageInstanceStore {
    by_channel: HashMap<Id, StageInstance>,
}

impl StageInstanceStore {
    /// `getStageInstanceByChannel`.
    pub fn get_by_channel(&self, channel: Id) -> Option<&StageInstance> {
        self.by_channel.get(&channel)
    }

    /// `isLive`.
    pub fn is_live(&self, channel: Id) -> bool {
        self.by_channel.contains_key(&channel)
    }

    /// `isPublic`.
    pub fn is_public(&self, channel: Id) -> bool {
        self.get_by_channel(channel)
            .is_some_and(|instance| instance.privacy_level == PUBLIC)
    }

    /// `getStageInstancesByGuild`.
    pub fn by_guild(&self, guild: Id) -> Vec<&StageInstance> {
        let mut list: Vec<_> = self
            .by_channel
            .values()
            .filter(|instance| instance.guild_id == guild)
            .collect();
        list.sort_by_key(|instance| instance.id);
        list
    }

    /// `getAllStageInstances`.
    pub fn all(&self) -> Vec<&StageInstance> {
        let mut list: Vec<_> = self.by_channel.values().collect();
        list.sort_by_key(|instance| instance.id);
        list
    }

    fn load_guild(&mut self, guild: &Value) {
        let guild_id = id(&guild["id"]);
        // The guild's list replaces what was held for it.
        if let Some(guild_id) = guild_id {
            self.by_channel
                .retain(|_, instance| instance.guild_id != guild_id);
        }
        for instance in guild["stage_instances"].as_array().into_iter().flatten() {
            if let Some(instance) = StageInstance::from_value(instance, guild_id) {
                self.by_channel.insert(instance.channel_id, instance);
            }
        }
    }

    pub fn dispatch(&mut self, event: &str, data: &Value) {
        match event {
            "READY" => {
                self.by_channel.clear();
                for guild in data["guilds"].as_array().into_iter().flatten() {
                    self.load_guild(guild);
                }
            }
            "GUILD_CREATE" => self.load_guild(data),
            "GUILD_DELETE" => {
                if let Some(guild) = id(&data["id"]) {
                    self.by_channel
                        .retain(|_, instance| instance.guild_id != guild);
                }
            }
            "STAGE_INSTANCE_CREATE" | "STAGE_INSTANCE_UPDATE" => {
                if let Some(instance) = StageInstance::from_value(data, None) {
                    self.by_channel.insert(instance.channel_id, instance);
                }
            }
            "STAGE_INSTANCE_DELETE" => {
                if let Some(channel) = id(&data["channel_id"]) {
                    self.by_channel.remove(&channel);
                }
            }
            "CHANNEL_DELETE" => {
                if let Some(channel) = id(&data["id"]) {
                    self.by_channel.remove(&channel);
                }
            }
            "LOGOUT" => self.by_channel.clear(),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn instance(id: u64, channel: u64, topic: &str) -> Value {
        json!({"id": id.to_string(), "guild_id": "1", "channel_id": channel.to_string(),
               "topic": topic, "privacy_level": 2, "discoverable_disabled": false})
    }

    #[test]
    fn guild_create_loads_and_events_change_instances() {
        let mut store = StageInstanceStore::default();
        store.dispatch(
            "GUILD_CREATE",
            &json!({"id": "1", "stage_instances": [instance(7, 20, "Hello")]}),
        );
        assert!(store.is_live(20));
        assert!(!store.is_public(20));
        assert_eq!(
            store.get_by_channel(20).map(|i| i.topic.as_str()),
            Some("Hello")
        );
        store.dispatch("STAGE_INSTANCE_UPDATE", &instance(7, 20, "Changed"));
        assert_eq!(
            store.get_by_channel(20).map(|i| i.topic.as_str()),
            Some("Changed")
        );
        store.dispatch(
            "STAGE_INSTANCE_UPDATE",
            &json!({"id": "7", "guild_id": "1", "channel_id": "20", "topic": "x", "privacy_level": 1}),
        );
        assert!(store.is_public(20));
        assert_eq!(store.by_guild(1).len(), 1);
        store.dispatch("STAGE_INSTANCE_DELETE", &instance(7, 20, "Changed"));
        assert!(!store.is_live(20));
    }

    #[test]
    fn ready_and_channel_delete_clear_instances() {
        let mut store = StageInstanceStore::default();
        store.dispatch("STAGE_INSTANCE_CREATE", &instance(7, 20, "A"));
        store.dispatch("STAGE_INSTANCE_CREATE", &instance(8, 21, "B"));
        store.dispatch("CHANNEL_DELETE", &json!({"id": "21", "guild_id": "1"}));
        assert_eq!(store.all().len(), 1);
        store.dispatch(
            "READY",
            &json!({"guilds": [{"id": "1", "stage_instances": [instance(9, 22, "C")]}]}),
        );
        assert!(!store.is_live(20));
        assert!(store.is_live(22));
        store.dispatch("GUILD_DELETE", &json!({"id": "1"}));
        assert!(store.all().is_empty());
    }
}
