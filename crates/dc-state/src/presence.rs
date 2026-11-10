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
    /// User to the scopes (a guild, or `None` for the friend list) holding an entry for them, so a lookup by user
    /// does not scan every entry.
    by_user: HashMap<Id, Vec<Option<Id>>>,
    guild_order: Vec<Option<Id>>,
    revision: u64,
    /// The signed-in account: 290863 ignores presence updates for it (its own status comes from the settings).
    me: Option<Id>,
    /// The account's own presence from `SelfPresenceStore` (module 894768): 290863 ignores the server's updates
    /// for it and takes this one through `setCurrentUserOnConnectionOpen`.
    own: Option<Presence>,
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
    /// Tells the store whose presence to ignore (`_.default.getId()` in 290863's `v` and `P`).
    pub fn set_current_user(&mut self, me: Option<Id>) {
        self.me = me;
        self.own = None;
    }

    /// Sets the account's own status and activities (what its presence update carries).
    pub fn set_own(&mut self, status: &str, activities: Vec<Value>) {
        self.revision += 1;
        self.own = Some(Presence {
            status: status.to_owned(),
            activities,
            clients: Value::Null,
            revision: self.revision,
        });
    }

    pub fn get(&self, user: Id, guild: Option<Id>) -> Option<&Presence> {
        if self.me == Some(user)
            && let Some(own) = &self.own
        {
            return Some(own);
        }
        if let Some(guild) = guild {
            return self.entries.get(&(Some(guild), user));
        }
        self.by_user
            .get(&user)?
            .iter()
            .filter_map(|scope| self.entries.get(&(*scope, user)))
            .max_by_key(|presence| (presence.revision, presence.activities.len()))
    }

    fn remove_entry(&mut self, scope: Option<Id>, user: Id) {
        if self.entries.remove(&(scope, user)).is_some()
            && let Some(scopes) = self.by_user.get_mut(&user)
        {
            scopes.retain(|known| *known != scope);
            if scopes.is_empty() {
                self.by_user.remove(&user);
            }
        }
    }

    fn rebuild_index(&mut self) {
        self.by_user.clear();
        for (scope, user) in self.entries.keys() {
            self.by_user.entry(*user).or_default().push(*scope);
        }
    }

    fn offline(status: &str) -> bool {
        status == "offline" || status == "invisible"
    }

    /// 290863 `y`: once every entry of a user is offline (no hidden activities are kept here), the user is dropped.
    fn prune(&mut self, user: Id) {
        let all_offline = self.by_user.get(&user).is_some_and(|scopes| {
            scopes.iter().all(|scope| {
                self.entries
                    .get(&(*scope, user))
                    .is_none_or(|presence| Self::offline(&presence.status))
            })
        });
        if all_offline && let Some(scopes) = self.by_user.remove(&user) {
            for scope in scopes {
                self.entries.remove(&(scope, user));
            }
        }
    }

    pub fn all(&self) -> impl Iterator<Item = &Presence> {
        self.entries.values()
    }

    /// 290863: web sessions do not suppress the mobile indicator; online desktop or VR sessions do.
    pub fn is_mobile_online(&self, user: Id) -> bool {
        self.get(user, None).is_some_and(|presence| {
            presence.clients["mobile"] == "online"
                && presence.clients["desktop"] != "online"
                && presence.clients["vr"] != "online"
        })
    }

    fn update(&mut self, guild: Option<Id>, user: Option<Id>, data: &Value) {
        let Some(user) = user
            .or_else(|| id(&data["user_id"]))
            .or_else(|| id(&data["user"]["id"]))
        else {
            return;
        };
        // 290863 drops the current user's own updates because its getters answer for that user from
        // SelfPresenceStore. There is no such store here, so the server's word is what the account's own row shows.
        let known = self.entries.contains_key(&(guild, user));
        // 290863 `v`: an offline update for someone with no entry creates nothing.
        if !known && data["status"].as_str().is_some_and(Self::offline) {
            return;
        }
        self.revision += 1;
        if !known {
            self.by_user.entry(user).or_default().push(guild);
        }
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
        if Self::offline(&presence.status) {
            presence.activities.clear();
            presence.clients = Value::Null;
            self.prune(user);
        }
    }

    pub fn dispatch(&mut self, event: &str, data: &Value) {
        let guild = id(&data["guild_id"]);
        match event {
            "READY" | "READY_SUPPLEMENTAL" => {
                if event == "READY" {
                    self.entries.clear();
                    self.by_user.clear();
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
                let deleted = id(&data["id"]);
                self.entries.retain(|(guild, _), _| *guild != deleted);
                self.rebuild_index();
            }
            "GUILD_MEMBER_REMOVE" => {
                if let (Some(guild), Some(user)) = (guild, id(&data["user"]["id"]))
                    && self.me != Some(user)
                {
                    self.remove_entry(Some(guild), user);
                }
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
    fn mobile_indicator_uses_latest_clients_and_excludes_online_desktop_or_vr() {
        let mut store = PresenceStore::default();
        assert!(!store.is_mobile_online(2));
        for (clients, expected) in [
            (json!({"mobile":"online"}), true),
            (json!({"mobile":"online","web":"online"}), true),
            (json!({"mobile":"online","desktop":"idle"}), true),
            (json!({"mobile":"online","desktop":"online"}), false),
            (json!({"mobile":"online","vr":"online"}), false),
            (json!({"mobile":"idle"}), false),
            (json!({}), false),
        ] {
            store.dispatch(
                "PRESENCE_UPDATE",
                &json!({"guild_id":"1","user_id":"2","status":"online","client_status":clients}),
            );
            assert_eq!(store.is_mobile_online(2), expected, "{clients}");
        }
        store.dispatch("PRESENCE_UPDATE", &json!({"guild_id":"3","user_id":"2","status":"online","client_status":{"mobile":"online"}}));
        assert!(store.is_mobile_online(2));
        store.dispatch(
            "PRESENCE_UPDATE",
            &json!({"guild_id":"3","user_id":"2","status":"offline"}),
        );
        assert!(!store.is_mobile_online(2));
        store.dispatch("READY", &json!({"guilds":[]}));
        assert!(!store.is_mobile_online(2));
    }

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
        // 290863 `y`: a user whose entries are all offline is dropped, so nothing stale remains.
        assert!(
            stores
                .presence
                .get(2, Some(1))
                .is_none_or(|presence| presence.activities.is_empty())
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

    fn real(user: &str, guild: &str, status: &str, clients: Value, activities: Value) -> Value {
        json!({"user":{"id":user,"username":"u","discriminator":"0","avatar":null},
            "guild_id":guild,"status":status,"client_status":clients,"activities":activities})
    }

    #[test]
    fn realistic_updates_prune_offline_users_and_keep_the_current_user() {
        let mut store = PresenceStore::default();
        store.set_current_user(Some(1));
        store.dispatch(
            "PRESENCE_UPDATE",
            &real("1", "10", "dnd", json!({"desktop":"dnd"}), json!([])),
        );
        assert_eq!(store.get(1, None).unwrap().status, "dnd");
        // An offline update for someone never seen creates nothing.
        store.dispatch(
            "PRESENCE_UPDATE",
            &real("2", "10", "offline", json!({}), json!([])),
        );
        assert!(store.get(2, None).is_none());
        for guild in ["10", "11"] {
            store.dispatch("PRESENCE_UPDATE", &real("2", guild, "online", json!({"desktop":"online"}),
                json!([{"type":0,"name":"Game","application_id":"3","created_at":1762650000000u64}])));
        }
        assert_eq!(store.get(2, None).unwrap().status, "online");
        store.dispatch(
            "PRESENCE_UPDATE",
            &real("2", "10", "offline", json!({}), json!([])),
        );
        assert_eq!(store.get(2, Some(11)).unwrap().status, "online");
        store.dispatch(
            "PRESENCE_UPDATE",
            &real("2", "11", "offline", json!({}), json!([])),
        );
        assert!(store.get(2, None).is_none());
        assert!(store.get(2, Some(10)).is_none());
        assert_eq!(store.all().count(), 1);
    }

    #[test]
    fn member_removal_and_guild_delete_drop_entries_and_keep_the_user_index_correct() {
        let mut store = PresenceStore::default();
        for guild in ["10", "11"] {
            store.dispatch(
                "PRESENCE_UPDATE",
                &real("2", guild, "idle", json!({"web":"idle"}), json!([])),
            );
        }
        store.dispatch(
            "GUILD_MEMBER_REMOVE",
            &json!({"guild_id":"10","user":{"id":"2","username":"u"}}),
        );
        assert!(store.get(2, Some(10)).is_none());
        assert_eq!(store.get(2, None).unwrap().status, "idle");
        store.dispatch("GUILD_DELETE", &json!({"id":"11"}));
        assert!(store.get(2, None).is_none());
    }

    #[test]
    fn lookups_by_user_do_not_scan_every_entry() {
        let mut store = PresenceStore::default();
        for user in 100..20_100u64 {
            store.dispatch(
                "PRESENCE_UPDATE",
                &json!({"guild_id":"10","user":{"id":user.to_string()},
                "status":"online","client_status":{"desktop":"online"},"activities":[]}),
            );
        }
        let start = std::time::Instant::now();
        for user in 100..20_100u64 {
            assert!(store.get(user, None).is_some());
        }
        assert!(start.elapsed() < std::time::Duration::from_secs(2));
    }
}
