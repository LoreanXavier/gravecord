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

/// `GuildScheduledEventUserResponses` (module 988794).
pub const RESPONSE_UNINTERESTED: u64 = 0;
pub const RESPONSE_INTERESTED: u64 = 1;

/// The key of the whole series in the RSVP and count maps (`SERIES` in module 698441).
const SERIES: &str = "SERIES";

/// One user's answer to an event (`guild_scheduled_event_user`): who, what they answered, and their nickname there.
#[derive(Clone, Debug, PartialEq)]
pub struct EventUser {
    pub user_id: Id,
    pub response: u64,
    pub nick: Option<String>,
}

impl EventUser {
    /// A user object of the gateway or REST shape; the answer defaults to interested when the server does not say.
    fn from_value(value: &Value) -> Option<Self> {
        Some(Self {
            user_id: id(&value["user_id"]).or_else(|| id(&value["user"]["id"]))?,
            response: value["response"].as_u64().unwrap_or(RESPONSE_INTERESTED),
            nick: text(&value["member"]["nick"]),
        })
    }
}

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
    /// Event, then recurrence (or `SERIES`), then the answers in the order they arrived.
    rsvps: HashMap<Id, HashMap<String, Vec<EventUser>>>,
    /// Event, then recurrence (or `SERIES`): the interested count the server gave, moved by each answer.
    counts: HashMap<Id, HashMap<String, i64>>,
    /// The signed-in user, from `READY`: the store only counts a removed answer of someone else as the server's.
    me: Option<Id>,
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

    /// `getRsvp`: the answer of `user` to the event (`recurrence` is a recurrence id, `None` the whole series).
    pub fn get_rsvp(&self, event: Id, recurrence: Option<&str>, user: Id) -> Option<&EventUser> {
        self.rsvps
            .get(&event)?
            .get(recurrence.unwrap_or(SERIES))?
            .iter()
            .find(|known| known.user_id == user)
    }

    /// `isInterestedInEventRecurrence`: interested in the series and not out of this recurrence, or in the recurrence.
    pub fn is_interested_in_event_recurrence(
        &self,
        event: Id,
        recurrence: Option<&str>,
        me: Id,
    ) -> bool {
        let series = self.get_rsvp(event, None, me);
        let one = self.get_rsvp(event, recurrence, me);
        let in_series = series.is_some_and(|rsvp| rsvp.response == RESPONSE_INTERESTED);
        let in_one = one.is_some_and(|rsvp| rsvp.response == RESPONSE_INTERESTED);
        let out_of_one = one.is_some_and(|rsvp| rsvp.response == RESPONSE_UNINTERESTED);
        in_series && !out_of_one || in_one
    }

    /// `getUserCount`: the series count, less what a recurrence's own count says when one is asked for.
    pub fn get_user_count(&self, event: Id, recurrence: Option<&str>) -> i64 {
        let Some(counts) = self.counts.get(&event) else {
            return 0;
        };
        let series = counts.get(SERIES).copied().unwrap_or(0);
        match recurrence {
            None => series,
            Some(recurrence) => series - counts.get(recurrence).copied().unwrap_or(0),
        }
    }

    /// `hasUserCount`.
    pub fn has_user_count(&self, event: Id, recurrence: Option<&str>) -> bool {
        self.counts
            .get(&event)
            .is_some_and(|counts| counts.contains_key(recurrence.unwrap_or(SERIES)))
    }

    /// `getUsersForGuildEvent`.
    pub fn get_users_for_guild_event(&self, event: Id, recurrence: Option<&str>) -> &[EventUser] {
        self.rsvps
            .get(&event)
            .and_then(|by_recurrence| by_recurrence.get(recurrence.unwrap_or(SERIES)))
            .map_or(&[], Vec::as_slice)
    }

    /// The count moves by one for an answer that is interested (of the series) or not interested (of a recurrence).
    fn count_step(recurrence: Option<&str>, response: u64) -> i64 {
        let counted = recurrence.is_some() && response == RESPONSE_UNINTERESTED
            || recurrence.is_none() && response == RESPONSE_INTERESTED;
        if counted { 1 } else { -1 }
    }

    fn move_count(&mut self, event: Id, recurrence: Option<&str>, by: i64) {
        let count = self
            .counts
            .entry(event)
            .or_default()
            .entry(recurrence.unwrap_or(SERIES).to_owned())
            .or_insert(0);
        *count += by;
    }

    /// `GUILD_SCHEDULED_EVENT_USER_ADD`: the answer replaces the user's earlier one. The count moves only when the
    /// answer is new, so the gateway's echo of an answer already applied here does not count it twice.
    pub fn add_rsvp(&mut self, event: Id, recurrence: Option<&str>, user: EventUser) {
        let known = self.get_rsvp(event, recurrence, user.user_id).cloned();
        let response = user.response;
        self.store_rsvp(event, recurrence, user);
        if known.is_none_or(|known| known.response != response) {
            self.move_count(event, recurrence, Self::count_step(recurrence, response));
        }
    }

    fn store_rsvp(&mut self, event: Id, recurrence: Option<&str>, user: EventUser) {
        let list = self
            .rsvps
            .entry(event)
            .or_default()
            .entry(recurrence.unwrap_or(SERIES).to_owned())
            .or_default();
        list.retain(|known| known.user_id != user.user_id);
        list.push(user);
    }

    /// `GUILD_SCHEDULED_EVENT_USER_REMOVE`: the answer goes, and the count follows when it was one we knew (or was
    /// someone else's: the server counted it).
    pub fn remove_rsvp(&mut self, event: Id, recurrence: Option<&str>, user: Id, response: u64) {
        let Some(list) = self
            .rsvps
            .get_mut(&event)
            .and_then(|by_recurrence| by_recurrence.get_mut(recurrence.unwrap_or(SERIES)))
        else {
            if self.me != Some(user) {
                self.move_count(event, recurrence, -Self::count_step(recurrence, response));
            }
            return;
        };
        let before = list.len();
        list.retain(|known| known.user_id != user);
        if list.len() != before || self.me != Some(user) {
            self.move_count(event, recurrence, -Self::count_step(recurrence, response));
        }
    }

    fn forget_event(&mut self, event: Id) {
        self.events.remove(&event);
        self.rsvps.remove(&event);
        self.counts.remove(&event);
    }

    fn set(&mut self, value: &Value) {
        if let Some(event) = ScheduledEvent::from_value(value) {
            self.events.insert(event.id, event);
        }
    }

    fn remove_guild(&mut self, guild: Id) {
        let gone: Vec<Id> = self
            .events
            .values()
            .filter(|event| event.guild_id == guild)
            .map(|event| event.id)
            .collect();
        for event in gone {
            self.forget_event(event);
        }
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
                self.rsvps.clear();
                self.counts.clear();
                self.me = id(&data["user"]["id"]);
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
                let gone: Vec<Id> = self
                    .events
                    .values()
                    .filter(|known| known.guild_id == guild && !listed.contains(&known.id))
                    .map(|known| known.id)
                    .collect();
                for event in gone {
                    self.forget_event(event);
                }
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
                    self.forget_event(event);
                }
            }
            // The gateway's shape: `guild_scheduled_event_id`, `guild_scheduled_event_exception_id`, `user_id`.
            "GUILD_SCHEDULED_EVENT_USER_ADD" | "GUILD_SCHEDULED_EVENT_USER_REMOVE" => {
                let (Some(known_event), Some(user)) =
                    (id(&data["guild_scheduled_event_id"]), id(&data["user_id"]))
                else {
                    return;
                };
                let recurrence = text(&data["guild_scheduled_event_exception_id"]);
                let response = data["response"].as_u64().unwrap_or(RESPONSE_INTERESTED);
                if event == "GUILD_SCHEDULED_EVENT_USER_ADD" {
                    self.add_rsvp(
                        known_event,
                        recurrence.as_deref(),
                        EventUser {
                            user_id: user,
                            response,
                            nick: text(&data["member"]["nick"]),
                        },
                    );
                } else {
                    self.remove_rsvp(known_event, recurrence.as_deref(), user, response);
                }
            }
            // `getGuildEventsForCurrentUser` and `fetchUsersForGuildEvent`: the answers the server lists.
            "GUILD_SCHEDULED_EVENT_RSVPS_FETCH_SUCESS"
            | "GUILD_SCHEDULED_EVENT_USERS_FETCH_SUCCESS" => {
                for value in data.as_array().into_iter().flatten() {
                    let (Some(known_event), Some(user)) = (
                        id(&value["guild_scheduled_event_id"]),
                        EventUser::from_value(value),
                    ) else {
                        continue;
                    };
                    let recurrence = text(&value["guild_scheduled_event_exception_id"]);
                    self.store_rsvp(known_event, recurrence.as_deref(), user);
                }
            }
            // `fetchGuildEventUserCounts`: the series count and, per recurrence, how many of it are not going.
            "GUILD_SCHEDULED_EVENT_USER_COUNTS_FETCH_SUCCESS" => {
                let Some(known_event) = id(&data["event_id"]) else {
                    return;
                };
                let count = data["guild_scheduled_event_count"].as_i64().unwrap_or(0);
                self.counts
                    .entry(known_event)
                    .or_default()
                    .insert(SERIES.to_owned(), count);
                if let Some(per_recurrence) =
                    data["guild_scheduled_event_exception_counts"].as_object()
                {
                    for (recurrence, taken) in per_recurrence {
                        self.counts
                            .entry(known_event)
                            .or_default()
                            .insert(recurrence.clone(), count - taken.as_i64().unwrap_or(0));
                    }
                }
            }
            "LOGOUT" => {
                self.events.clear();
                self.rsvps.clear();
                self.counts.clear();
            }
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
    #[test]
    fn answers_and_counts_follow_the_official_store() {
        let mut store = GuildScheduledEventStore::default();
        store.dispatch("READY", &json!({"user": {"id": "7"}, "guilds": []}));
        store.dispatch(
            "GUILD_SCHEDULED_EVENT_CREATE",
            &event("5", "10", "2026-11-12T10:00:00+00:00", 1),
        );
        // The server's count, then the current user's own answers.
        store.dispatch(
            "GUILD_SCHEDULED_EVENT_USER_COUNTS_FETCH_SUCCESS",
            &json!({"event_id": "5", "guild_scheduled_event_count": 1, "guild_scheduled_event_exception_counts": {}}),
        );
        assert_eq!(store.get_user_count(5, None), 1);
        assert!(!store.is_interested_in_event_recurrence(5, None, 7));
        store.dispatch(
            "GUILD_SCHEDULED_EVENT_RSVPS_FETCH_SUCESS",
            &json!([{"guild_scheduled_event_id": "5", "user_id": "7", "response": 1}]),
        );
        assert!(store.is_interested_in_event_recurrence(5, None, 7));
        // Toggling off moves the count, the gateway's echo of it does not move it twice.
        store.remove_rsvp(5, None, 7, RESPONSE_INTERESTED);
        assert_eq!(store.get_user_count(5, None), 0);
        assert!(!store.is_interested_in_event_recurrence(5, None, 7));
        store.add_rsvp(
            5,
            None,
            EventUser {
                user_id: 7,
                response: RESPONSE_INTERESTED,
                nick: None,
            },
        );
        store.dispatch(
            "GUILD_SCHEDULED_EVENT_USER_ADD",
            &json!({"guild_scheduled_event_id": "5", "user_id": "7", "guild_id": "10", "response": 1}),
        );
        assert_eq!(store.get_user_count(5, None), 1);
        assert_eq!(store.get_users_for_guild_event(5, None).len(), 1);
        // Someone else's answer arrives and goes.
        store.dispatch(
            "GUILD_SCHEDULED_EVENT_USER_ADD",
            &json!({"guild_scheduled_event_id": "5", "user_id": "8", "guild_id": "10", "response": 1}),
        );
        assert_eq!(store.get_user_count(5, None), 2);
        store.dispatch(
            "GUILD_SCHEDULED_EVENT_USER_REMOVE",
            &json!({"guild_scheduled_event_id": "5", "user_id": "8", "guild_id": "10", "response": 1}),
        );
        assert_eq!(store.get_user_count(5, None), 1);
        // A deleted event takes its answers and counts along.
        store.dispatch(
            "GUILD_SCHEDULED_EVENT_DELETE",
            &json!({"id": "5", "guild_id": "10"}),
        );
        assert_eq!(store.get_user_count(5, None), 0);
        assert!(store.get_users_for_guild_event(5, None).is_empty());
    }
}
