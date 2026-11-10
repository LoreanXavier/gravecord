//! GuildScheduledEventStore, module 698441: a guild's scheduled events, fed by `READY`, `GUILD_CREATE`, a fetched list
//! (`FETCH_GUILD_EVENTS_FOR_GUILD`) and the three `GUILD_SCHEDULED_EVENT_*` dispatches. Events are indexed the way the
//! official store indexes them: per guild, with the active ones first and then by start time.

use crate::{Id, id, text};
use chrono::{DateTime, Utc};
use serde_json::Value;
use std::collections::HashMap;

/// `GuildScheduledEventStatus` (module 988794): `COMPLETED` and `CANCELED` events are over.
pub const STATUS_SCHEDULED: u64 = 1;
pub const STATUS_ACTIVE: u64 = 2;
pub const STATUS_COMPLETED: u64 = 3;
pub const STATUS_CANCELED: u64 = 4;

/// `GuildScheduledEventEntityTypes` (module 988794).
pub const ENTITY_NONE: u64 = 0;
pub const ENTITY_STAGE_INSTANCE: u64 = 1;
pub const ENTITY_VOICE: u64 = 2;
pub const ENTITY_EXTERNAL: u64 = 3;

/// A scheduled event as the gateway and the REST API describe it.
#[derive(Clone, Debug, PartialEq)]
pub struct ScheduledEvent {
    pub id: Id,
    pub guild_id: Id,
    pub channel_id: Option<Id>,
    pub creator_id: Option<Id>,
    pub name: String,
    pub description: Option<String>,
    /// RFC 3339, as received.
    pub scheduled_start_time: String,
    pub scheduled_end_time: Option<String>,
    pub privacy_level: u64,
    pub status: u64,
    pub entity_type: u64,
    pub entity_id: Option<Id>,
    /// `entity_metadata.location` of an external event.
    pub location: Option<String>,
    /// The cover image hash.
    pub image: Option<String>,
    pub user_count: Option<u64>,
    /// `recurrence_rule`, kept as received.
    pub recurrence_rule: Value,
}

impl ScheduledEvent {
    pub fn from_value(value: &Value) -> Option<Self> {
        Some(Self {
            id: id(&value["id"])?,
            guild_id: id(&value["guild_id"])?,
            channel_id: id(&value["channel_id"]),
            creator_id: id(&value["creator_id"]),
            name: text(&value["name"]).unwrap_or_default(),
            description: text(&value["description"]),
            scheduled_start_time: text(&value["scheduled_start_time"])?,
            scheduled_end_time: text(&value["scheduled_end_time"]),
            privacy_level: value["privacy_level"].as_u64().unwrap_or(2),
            status: value["status"].as_u64().unwrap_or(STATUS_SCHEDULED),
            entity_type: value["entity_type"].as_u64().unwrap_or(ENTITY_NONE),
            entity_id: id(&value["entity_id"]),
            location: text(&value["entity_metadata"]["location"]),
            image: text(&value["image"]),
            user_count: value["user_count"].as_u64(),
            recurrence_rule: value["recurrence_rule"].clone(),
        })
    }

    /// `Fd`: the event is running now.
    pub fn is_active(&self) -> bool {
        self.status == STATUS_ACTIVE
    }

    /// `AZ`: completed or canceled.
    pub fn is_over(&self) -> bool {
        matches!(self.status, STATUS_COMPLETED | STATUS_CANCELED)
    }

    /// `d6`: the upcoming index holds everything that is not over.
    pub fn is_upcoming(&self) -> bool {
        !self.is_over()
    }

    pub fn start(&self) -> Option<DateTime<Utc>> {
        DateTime::parse_from_rfc3339(&self.scheduled_start_time)
            .ok()
            .map(|time| time.with_timezone(&Utc))
    }

    /// The index sort key (`"\0"` or `"\x01"`, the start in milliseconds, the id): active events first, then by start.
    fn sort_key(&self) -> (u8, i64, String) {
        (
            u8::from(!self.is_active()),
            self.start().map_or(0, |time| time.timestamp_millis()),
            self.id.to_string(),
        )
    }
}

#[derive(Default)]
pub struct GuildScheduledEventStore {
    events: HashMap<Id, ScheduledEvent>,
}

impl GuildScheduledEventStore {
    /// `getGuildScheduledEvent`.
    pub fn get_guild_scheduled_event(&self, event: Id) -> Option<&ScheduledEvent> {
        self.events.get(&event)
    }

    /// `getGuildScheduledEventsForGuild`, in index order.
    pub fn get_guild_scheduled_events_for_guild(&self, guild: Id) -> Vec<&ScheduledEvent> {
        let mut events: Vec<&ScheduledEvent> = self
            .events
            .values()
            .filter(|event| event.guild_id == guild)
            .collect();
        events.sort_by_key(|event| event.sort_key());
        events
    }

    /// `getGuildScheduledEventsByIndex(GUILD_EVENT_UPCOMING(guild))`: not completed and not canceled.
    pub fn upcoming(&self, guild: Id) -> Vec<&ScheduledEvent> {
        self.get_guild_scheduled_events_for_guild(guild)
            .into_iter()
            .filter(|event| event.is_upcoming())
            .collect()
    }

    /// `getGuildScheduledEventsByIndex(GUILD_EVENT_ACTIVE(guild))`.
    pub fn active(&self, guild: Id) -> Vec<&ScheduledEvent> {
        self.get_guild_scheduled_events_for_guild(guild)
            .into_iter()
            .filter(|event| event.is_active())
            .collect()
    }

    /// `isActive`.
    pub fn is_active(&self, event: Id) -> bool {
        self.events
            .get(&event)
            .is_some_and(ScheduledEvent::is_active)
    }

    fn set(&mut self, value: &Value) {
        if let Some(event) = ScheduledEvent::from_value(value) {
            self.events.insert(event.id, event);
        }
    }

    fn remove_guild(&mut self, guild: Id) {
        self.events.retain(|_, event| event.guild_id != guild);
    }

    fn add_guild_events(&mut self, guild: &Value) {
        for event in guild["guild_scheduled_events"]
            .as_array()
            .into_iter()
            .flatten()
        {
            self.set(event);
        }
    }

    /// The guild id of a `GUILD_CREATE` or `GUILD_DELETE` payload: top level, or inside `properties`.
    fn guild_id(guild: &Value) -> Option<Id> {
        id(&guild["id"]).or_else(|| id(&guild["properties"]["id"]))
    }

    pub fn dispatch(&mut self, event: &str, data: &Value) {
        match event {
            "READY" => {
                self.events.clear();
                for guild in data["guilds"].as_array().into_iter().flatten() {
                    self.add_guild_events(guild);
                }
            }
            "GUILD_CREATE" => {
                if let Some(guild) = Self::guild_id(data) {
                    self.remove_guild(guild);
                }
                self.add_guild_events(data);
            }
            "GUILD_DELETE" => {
                if let Some(guild) = Self::guild_id(data) {
                    self.remove_guild(guild);
                }
            }
            // `FETCH_GUILD_EVENTS_FOR_GUILD`: what the server no longer lists is dropped, the rest is stored.
            "FETCH_GUILD_EVENTS_FOR_GUILD" => {
                let Some(guild) = id(&data["guild_id"]) else {
                    return;
                };
                let listed: Vec<Id> = data["guild_scheduled_events"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|event| id(&event["id"]))
                    .collect();
                self.events
                    .retain(|key, known| known.guild_id != guild || listed.contains(key));
                for event in data["guild_scheduled_events"]
                    .as_array()
                    .into_iter()
                    .flatten()
                {
                    self.set(event);
                }
            }
            "GUILD_SCHEDULED_EVENT_CREATE" | "GUILD_SCHEDULED_EVENT_UPDATE" => self.set(data),
            "GUILD_SCHEDULED_EVENT_DELETE" => {
                if let Some(event) = id(&data["id"]) {
                    self.events.remove(&event);
                }
            }
            "LOGOUT" => self.events.clear(),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn event(id: &str, guild: &str, start: &str, status: u64) -> Value {
        json!({
            "id": id, "guild_id": guild, "name": format!("event {id}"),
            "scheduled_start_time": start, "status": status, "entity_type": 3,
            "entity_metadata": {"location": "somewhere"}, "privacy_level": 2,
        })
    }

    #[test]
    fn ready_and_guild_create_fill_the_store() {
        let mut store = GuildScheduledEventStore::default();
        store.dispatch(
            "READY",
            &json!({"guilds": [{"id": "10", "guild_scheduled_events": [
                event("1", "10", "2026-11-12T10:00:00+00:00", 1),
            ]}]}),
        );
        assert_eq!(store.upcoming(10).len(), 1);
        assert_eq!(
            store
                .get_guild_scheduled_event(1)
                .unwrap()
                .location
                .as_deref(),
            Some("somewhere")
        );
        // A guild's own create replaces what was known about it.
        store.dispatch(
            "GUILD_CREATE",
            &json!({"id": "10", "guild_scheduled_events": [
                event("2", "10", "2026-11-13T10:00:00+00:00", 1),
            ]}),
        );
        assert!(store.get_guild_scheduled_event(1).is_none());
        assert!(store.get_guild_scheduled_event(2).is_some());
        // READY starts over.
        store.dispatch("READY", &json!({"guilds": [{"id": "11"}]}));
        assert!(store.upcoming(10).is_empty());
    }

    #[test]
    fn gateway_events_create_update_and_delete() {
        let mut store = GuildScheduledEventStore::default();
        store.dispatch(
            "GUILD_SCHEDULED_EVENT_CREATE",
            &event("5", "10", "2026-11-12T10:00:00+00:00", 1),
        );
        assert_eq!(store.upcoming(10).len(), 1);
        store.dispatch(
            "GUILD_SCHEDULED_EVENT_UPDATE",
            &event("5", "10", "2026-11-12T10:00:00+00:00", 3),
        );
        // Completed events leave the upcoming index but stay in the guild's list.
        assert!(store.upcoming(10).is_empty());
        assert_eq!(store.get_guild_scheduled_events_for_guild(10).len(), 1);
        store.dispatch(
            "GUILD_SCHEDULED_EVENT_DELETE",
            &json!({"id": "5", "guild_id": "10"}),
        );
        assert!(store.get_guild_scheduled_events_for_guild(10).is_empty());
    }

    #[test]
    fn events_are_ordered_active_first_then_by_start() {
        let mut store = GuildScheduledEventStore::default();
        for (id, start, status) in [
            ("1", "2026-11-14T10:00:00+00:00", 1),
            ("2", "2026-11-12T10:00:00+00:00", 1),
            ("3", "2026-11-15T10:00:00+00:00", 2),
        ] {
            store.dispatch(
                "GUILD_SCHEDULED_EVENT_CREATE",
                &event(id, "10", start, status),
            );
        }
        let order: Vec<Id> = store.upcoming(10).iter().map(|event| event.id).collect();
        assert_eq!(order, vec![3, 2, 1]);
        assert!(store.is_active(3));
        assert_eq!(store.active(10).len(), 1);
    }

    #[test]
    fn a_fetched_list_drops_what_the_server_no_longer_has() {
        let mut store = GuildScheduledEventStore::default();
        store.dispatch(
            "GUILD_SCHEDULED_EVENT_CREATE",
            &event("1", "10", "2026-11-12T10:00:00+00:00", 1),
        );
        store.dispatch(
            "GUILD_SCHEDULED_EVENT_CREATE",
            &event("9", "20", "2026-11-12T10:00:00+00:00", 1),
        );
        store.dispatch(
            "FETCH_GUILD_EVENTS_FOR_GUILD",
            &json!({"guild_id": "10", "guild_scheduled_events": [
                event("2", "10", "2026-11-13T10:00:00+00:00", 1),
            ]}),
        );
        assert!(store.get_guild_scheduled_event(1).is_none());
        assert!(store.get_guild_scheduled_event(2).is_some());
        // Another guild is untouched.
        assert!(store.get_guild_scheduled_event(9).is_some());
        store.dispatch("GUILD_DELETE", &json!({"id": "20"}));
        assert!(store.get_guild_scheduled_event(9).is_none());
        store.dispatch("LOGOUT", &json!({}));
        assert!(store.upcoming(10).is_empty());
    }
}
