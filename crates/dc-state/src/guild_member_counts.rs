//! GuildMemberCountStore, module 498642.

use crate::Id;
use serde_json::Value;
use std::collections::HashMap;

#[derive(Default)]
pub struct GuildMemberCountStore {
    members: HashMap<Id, u64>,
    online: HashMap<Id, u64>,
}

fn id(value: &Value) -> Option<Id> {
    value
        .as_str()
        .and_then(|s| s.parse().ok())
        .or_else(|| value.as_u64())
}

fn set(counts: &mut HashMap<Id, u64>, guild: Id, value: &Value) {
    if let Some(count) = value.as_u64() {
        counts.insert(guild, count);
    } else {
        counts.remove(&guild);
    }
}

impl GuildMemberCountStore {
    pub fn get_member_count(&self, guild: Id) -> Option<u64> {
        self.members.get(&guild).copied()
    }

    pub fn get_online_count(&self, guild: Id) -> Option<u64> {
        self.online.get(&guild).copied()
    }

    pub fn dispatch(&mut self, event: &str, data: &Value) {
        match event {
            "READY" => {
                self.members.clear();
                for guild in data["guilds"].as_array().into_iter().flatten() {
                    if let Some(guild_id) = id(&guild["id"]) {
                        set(&mut self.members, guild_id, &guild["member_count"]);
                    }
                }
            }
            "GUILD_CREATE" => {
                if let Some(guild) = id(&data["id"]) {
                    set(&mut self.members, guild, &data["member_count"]);
                }
            }
            "GUILD_DELETE" => {
                if let Some(guild) = id(&data["id"]) {
                    self.members.remove(&guild);
                    self.online.remove(&guild);
                }
            }
            "GUILD_MEMBER_LIST_UPDATE" => {
                if let Some(guild) = id(&data["guild_id"]) {
                    set(&mut self.members, guild, &data["member_count"]);
                    set(&mut self.online, guild, &data["online_count"]);
                }
            }
            // 498642 `l`: an invite's guild carries the online count it was resolved or accepted with.
            "INVITE_RESOLVE_SUCCESS" | "INVITE_ACCEPT_SUCCESS" => {
                let invite = &data["invite"];
                if let (Some(guild), Some(count)) = (
                    id(&invite["guild"]["id"]),
                    invite["approximate_presence_count"].as_u64(),
                ) {
                    self.online.insert(guild, count);
                }
            }
            "ONLINE_GUILD_MEMBER_COUNT_UPDATE" => {
                if let (Some(guild), Some(count)) = (
                    id(&data["guild_id"]).or_else(|| id(&data["guildId"])),
                    data["count"].as_u64(),
                ) {
                    self.online.insert(guild, count);
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    #[test]
    fn ready_and_lazy_counts_preserve_unknown_zero_and_guild_lifecycle() {
        let mut stores = crate::Stores::default();
        stores.dispatch("READY", &json!({"guilds":[{"id":"1","member_count":12000},{"id":"2","member_count":0},{"id":"3"}]}));
        assert_eq!(stores.member_counts.get_member_count(1), Some(12000));
        assert_eq!(stores.member_counts.get_member_count(2), Some(0));
        assert_eq!(stores.member_counts.get_member_count(3), None);
        stores.dispatch(
            "GUILD_MEMBER_LIST_UPDATE",
            &json!({"guild_id":"1","member_count":9500,"online_count":400,"ops":[]}),
        );
        assert_eq!(stores.member_counts.get_member_count(1), Some(9500));
        assert_eq!(stores.member_counts.get_online_count(1), Some(400));
        stores.dispatch("GUILD_CREATE", &json!({"id":"4","member_count":8}));
        assert_eq!(stores.member_counts.get_member_count(4), Some(8));
        stores.dispatch("READY", &json!({"guilds":[{"id":"1","member_count":9501}]}));
        assert_eq!(stores.member_counts.get_member_count(4), None);
        // CONNECTION_OPEN replaces member counts; 498642 retains the separate online-count map.
        assert_eq!(stores.member_counts.get_online_count(1), Some(400));
        stores.dispatch("GUILD_DELETE", &json!({"id":"1"}));
        assert_eq!(stores.member_counts.get_member_count(1), None);
        assert_eq!(stores.member_counts.get_online_count(1), None);
    }

    #[test]
    fn online_counts_follow_invites_and_the_online_count_update() {
        let mut stores = crate::Stores::default();
        stores.dispatch(
            "INVITE_RESOLVE_SUCCESS",
            &json!({"invite":{"code":"abc","guild":{"id":"7","name":"g"},
                "approximate_member_count":900,"approximate_presence_count":120}}),
        );
        assert_eq!(stores.member_counts.get_online_count(7), Some(120));
        // Without a presence count the invite changes nothing.
        stores.dispatch(
            "INVITE_ACCEPT_SUCCESS",
            &json!({"invite":{"guild":{"id":"7"},"approximate_presence_count":null}}),
        );
        assert_eq!(stores.member_counts.get_online_count(7), Some(120));
        stores.dispatch(
            "ONLINE_GUILD_MEMBER_COUNT_UPDATE",
            &json!({"guild_id":"7","count":133}),
        );
        assert_eq!(stores.member_counts.get_online_count(7), Some(133));
        stores.dispatch("ONLINE_GUILD_MEMBER_COUNT_UPDATE", &json!({"guild_id":"7"}));
        assert_eq!(stores.member_counts.get_online_count(7), Some(133));
    }
}
