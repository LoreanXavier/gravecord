//! PresenceStore activity ordering and guild-scoped snapshots (modules 290863/342312).

use crate::Id;
use serde_json::Value;
use std::collections::HashMap;

#[derive(Clone, Debug, Default)]
pub struct Presence {
    pub status: String,
    pub activities: Vec<Value>,
    pub clients: Value,
    revision: u64,
}

#[derive(Default)]
pub struct PresenceStore {
    entries: HashMap<(Option<Id>, Id), Presence>,
    guild_order: Vec<Option<Id>>,
    revision: u64,
}

fn id(value: &Value) -> Option<Id> {
    value
        .as_str()
        .and_then(|s| s.parse().ok())
        .or_else(|| value.as_u64())
}

fn priority(activity: &Value) -> u8 {
    match activity["type"].as_u64() {
        Some(4) => 4,
        Some(5) => 3,
        Some(1) => 2,
        Some(0) => 1,
        _ => 0,
    }
}

fn rich(activity: &Value) -> bool {
    activity["type"] != 4
        && [
            &activity["details"],
            &activity["state"],
            &activity["party"],
            &activity["secrets"],
            &activity["assets"]["large_image"],
            &activity["assets"]["small_text"],
        ]
        .iter()
        .any(|value| !value.is_null())
}

impl PresenceStore {
    pub fn get(&self, user: Id, guild: Option<Id>) -> Option<&Presence> {
        if let Some(guild) = guild {
            return self.entries.get(&(Some(guild), user));
        }
        self.entries
            .iter()
            .filter(|((_, id), _)| *id == user)
            .map(|(_, presence)| presence)
            .max_by_key(|presence| (presence.revision, presence.activities.len()))
    }

    pub fn all(&self) -> impl Iterator<Item = &Presence> {
        self.entries.values()
    }

    fn update(&mut self, guild: Option<Id>, user: Option<Id>, data: &Value) {
        let Some(user) = user
            .or_else(|| id(&data["user_id"]))
            .or_else(|| id(&data["user"]["id"]))
        else {
            return;
        };
        self.revision += 1;
        let presence = self.entries.entry((guild, user)).or_default();
        presence.revision = self.revision;
        if let Some(status) = data["status"].as_str() {
            presence.status = status.to_owned();
        }
        if let Some(activities) = data.get("activities") {
            presence.activities = activities.as_array().cloned().unwrap_or_default();
            presence.activities.sort_by(|a, b| {
                priority(b)
                    .cmp(&priority(a))
                    .then_with(|| rich(b).cmp(&rich(a)))
                    .then_with(|| {
                        b["created_at"]
                            .as_u64()
                            .unwrap_or(0)
                            .cmp(&a["created_at"].as_u64().unwrap_or(0))
                    })
            });
        }
        if let Some(clients) = data.get("client_status") {
            presence.clients = clients.clone();
        }
        if presence.status == "offline" || presence.status == "invisible" {
            presence.activities.clear();
            presence.clients = Value::Null;
        }
    }

    pub fn dispatch(&mut self, event: &str, data: &Value) {
        let guild = id(&data["guild_id"]);
        match event {
            "READY" | "READY_SUPPLEMENTAL" => {
                if event == "READY" {
                    self.entries.clear();
                    self.guild_order = data["guilds"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .map(|g| id(&g["id"]))
                        .collect();
                }
                for presence in data["merged_presences"]["friends"]
                    .as_array()
                    .into_iter()
                    .flatten()
                {
                    self.update(None, None, presence);
                }
                for (index, presences) in data["merged_presences"]["guilds"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .enumerate()
                {
                    if let Some(guild) = data["guilds"]
                        .get(index)
                        .and_then(|g| id(&g["id"]))
                        .or_else(|| self.guild_order.get(index).copied().flatten())
                    {
                        for presence in presences.as_array().into_iter().flatten() {
                            self.update(Some(guild), None, presence);
                        }
                    }
                }
                for guild in data["guilds"].as_array().into_iter().flatten() {
                    for presence in guild["presences"].as_array().into_iter().flatten() {
                        self.update(id(&guild["id"]), None, presence);
                    }
                }
            }
            "PRESENCE_UPDATE" => self.update(guild, None, data),
            "GUILD_CREATE" | "GUILD_MEMBERS_CHUNK" => {
                let guild = guild.or_else(|| id(&data["id"]));
                for presence in data["presences"].as_array().into_iter().flatten() {
                    self.update(guild, None, presence);
                }
            }
            "GUILD_MEMBER_LIST_UPDATE" => {
                for op in data["ops"].as_array().into_iter().flatten() {
                    let items: &[Value] = match op["op"].as_str() {
                        Some("SYNC") => op["items"].as_array().map_or(&[], Vec::as_slice),
                        Some("INSERT" | "UPDATE") => std::slice::from_ref(&op["item"]),
                        _ => &[],
                    };
                    for item in items {
                        let member = &item["member"];
                        if let Some(presence) = member.get("presence") {
                            self.update(guild, id(&member["user"]["id"]), presence);
                        }
                    }
                }
            }
            "GUILD_DELETE" => {
                self.entries
                    .retain(|(guild, _), _| *guild != id(&data["id"]));
            }
            _ => {}
        }
    }
}

impl Presence {
    pub fn custom(&self) -> Option<&Value> {
        self.activities.iter().find(|a| {
            a["type"] == 4
                && (a["state"].as_str().is_some_and(|s| !s.trim().is_empty())
                    || a["emoji"].is_object())
        })
    }

    pub fn visible_activities(&self) -> Vec<&Value> {
        let mut activities: Vec<&Value> = Vec::new();
        for activity in &self.activities {
            if matches!(activity["type"].as_u64(), Some(4 | 6)) {
                continue;
            }
            if activity["type"] == 0 && activities.iter().any(|a| a["type"] == 0) {
                continue;
            }
            if activities.iter().any(|known| {
                (!known["application_id"].is_null()
                    && known["application_id"] == activity["application_id"])
                    || (!known["name"].is_null() && known["name"] == activity["name"])
            }) {
                continue;
            }
            activities.push(activity);
        }
        activities
    }
}

/// Compact activity text, including Spotify artists and the application's requested display field.
pub fn activity_text(activity: &Value) -> Option<String> {
    let field = |key| activity[key].as_str().filter(|s| !s.is_empty());
    let name = field("name");
    if activity["type"] == 2
        && name == Some("Spotify")
        && let Some(state) = field("state")
    {
        return Some(state.replace("; ", ", "));
    }
    let fallback = if activity["type"] == 1 {
        field("details").or(name)
    } else {
        name
    };
    let text = match activity["status_display_type"].as_u64() {
        Some(0) => name.or(fallback),
        Some(1) => field("state").or(fallback),
        Some(2) => field("details").or(fallback),
        _ => fallback,
    };
    text.map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn member_sync_update_and_offline_keep_activity_lifecycle() {
        let mut stores = crate::Stores::default();
        stores.dispatch("GUILD_MEMBER_LIST_UPDATE", &json!({"guild_id":"1", "ops":[{"op":"SYNC","range":[0,0],"items":[{"member":{"user":{"id":"2"},"presence":{"status":"online","activities":[{"type":0,"name":"Game"}]}}}]}]}));
        assert_eq!(
            activity_text(&stores.presence.get(2, Some(1)).unwrap().activities[0]),
            Some("Game".into())
        );
        stores.dispatch(
            "PRESENCE_UPDATE",
            &json!({"guild_id":"1","user":{"id":"2"},"status":"idle"}),
        );
        assert_eq!(stores.presence.get(2, Some(1)).unwrap().activities.len(), 1);
        stores.dispatch(
            "PRESENCE_UPDATE",
            &json!({"guild_id":"1","user":{"id":"2"},"activities":[]}),
        );
        assert!(
            stores
                .presence
                .get(2, Some(1))
                .unwrap()
                .activities
                .is_empty()
        );
        stores.dispatch("PRESENCE_UPDATE", &json!({"guild_id":"1","user":{"id":"2"},"status":"offline","activities":[{"type":0,"name":"stale"}]}));
        assert!(
            stores
                .presence
                .get(2, Some(1))
                .unwrap()
                .activities
                .is_empty()
        );
    }

    #[test]
    fn supplemental_guild_and_friend_presences_are_scoped_and_reset() {
        let mut store = PresenceStore::default();
        store.dispatch("READY", &json!({"guilds":[{"id":"9"},{"id":"10"}]}));
        store.dispatch("READY_SUPPLEMENTAL", &json!({"merged_presences":{"friends":[{"user_id":"2","status":"online","activities":[{"type":2,"name":"Spotify","state":"A; B"}]}],"guilds":[[],[{"user_id":"2","status":"dnd","activities":[{"type":0,"name":"Game"}]}]]}}));
        assert_eq!(store.get(2, Some(10)).unwrap().status, "dnd");
        assert_eq!(
            activity_text(&store.entries[&(None, 2)].activities[0]),
            Some("A, B".into())
        );
        store.dispatch("READY", &json!({"guilds":[]}));
        assert!(store.get(2, None).is_none());
    }

    #[test]
    fn activity_order_deduplication_and_display_fields_follow_source() {
        let mut store = PresenceStore::default();
        store.dispatch("PRESENCE_UPDATE", &json!({"user_id":"2","status":"online","activities":[
            {"type":2,"name":"Spotify"}, {"type":0,"name":"Game","application_id":"3"},
            {"type":0,"name":"Rich game","application_id":"3","state":"Level","status_display_type":1},
            {"type":4,"name":"Custom Status","state":"hello","emoji":{"name":"❤️"}}
        ]}));
        let presence = store.get(2, None).unwrap();
        assert_eq!(presence.custom().unwrap()["state"], "hello");
        let activities = presence.visible_activities();
        assert_eq!(activities.len(), 2);
        assert_eq!(activity_text(activities[0]), Some("Level".into()));
        assert_eq!(activities[1]["name"], "Spotify");
    }
}
