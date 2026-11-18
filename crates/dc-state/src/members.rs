//! `GuildMemberStore`: the members this session has seen, per guild, in the order they arrived. Members come from
//! `GUILD_MEMBERS_CHUNK` (op 8 answers), `GUILD_CREATE`, the member list, `GUILD_MEMBER_ADD`/`UPDATE`/`REMOVE` and the
//! `member` of a `MESSAGE_CREATE`.

use crate::{Id, User, id, text};
use serde_json::Value;
use std::collections::HashMap;

#[derive(Clone, Debug, PartialEq)]
pub struct Member {
    pub user: User,
    pub nick: Option<String>,
    pub avatar: Option<String>,
    pub avatar_decoration: Option<crate::collectibles::AvatarDecoration>,
    pub roles: Vec<Id>,
    pub joined_at: Option<String>,
    /// `pending`: still in rules screening, so not mentionable (`isPending`).
    pub pending: bool,
    /// The member's own `display_name_styles`, which wins over the user's (609425).
    pub display_name_styles: Option<crate::display_name_styles::DisplayNameStyles>,
    /// `communication_disabled_until`: when the member's timeout ends (ISO 8601), if one is set.
    pub timeout_until: Option<String>,
    /// `unusual_dm_activity_until` (`unusualDMActivityUntil` of module 696451, read from the member objects by module
    /// 225123): until when the member is flagged for unusual DM activity (ISO 8601), which Mod View shows.
    pub unusual_dm_activity_until: Option<String>,
    /// Member flags (`flags`); bit 2 is `BYPASSES_VERIFICATION`, which Verify Member sets (module 340837).
    pub flags: u64,
}

impl Member {
    /// A gateway member object (`{user, nick, roles, pending, ...}`); `user` may be given apart, as a message does.
    pub fn from_value(member: &Value, user: Option<&Value>) -> Option<Self> {
        let user = User::from_value(user.unwrap_or(&member["user"]))?;
        Some(Self {
            user,
            nick: text(&member["nick"]),
            avatar: text(&member["avatar"]),
            avatar_decoration: crate::collectibles::AvatarDecoration::from_value(
                &member["avatar_decoration_data"],
            ),
            roles: member["roles"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(id)
                .collect(),
            joined_at: text(&member["joined_at"]),
            pending: member["pending"].as_bool().unwrap_or(false),
            display_name_styles: crate::display_name_styles::DisplayNameStyles::from_value(
                &member["display_name_styles"],
            ),
            timeout_until: text(&member["communication_disabled_until"]),
            unusual_dm_activity_until: text(&member["unusual_dm_activity_until"]),
            flags: member["flags"].as_u64().unwrap_or(0),
        })
    }

    /// The member as a gateway member object that `from_value` reads back: what the start cache keeps of the open
    /// channel's authors. The decoration and the name styles are left out; the live member brings them.
    pub fn to_cache_value(&self) -> Value {
        let roles: Vec<String> = self.roles.iter().map(Id::to_string).collect();
        serde_json::json!({
            "user": self.user.to_cache_value(),
            "nick": self.nick,
            "avatar": self.avatar,
            "roles": roles,
            "joined_at": self.joined_at,
            "pending": self.pending,
            "communication_disabled_until": self.timeout_until,
            "flags": self.flags,
        })
    }

    /// `GUILD_MEMBER_AVATAR`, module 486020. No override means use the user's global avatar.
    pub fn avatar_url(&self, guild: Id) -> Option<String> {
        let hash = self.avatar.as_ref()?;
        Some(format!(
            "https://cdn.discordapp.com/guilds/{guild}/users/{}/avatars/{hash}.webp?size=128",
            self.user.id
        ))
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct GuildMembers {
    guilds: HashMap<Id, Vec<Member>>,
    /// Guild to user id to the member's position in `guilds`, so an upsert or lookup is not a scan.
    index: HashMap<Id, HashMap<Id, usize>>,
}

impl GuildMembers {
    /// The guild's members in arrival order.
    pub fn of(&self, guild: Id) -> &[Member] {
        self.guilds.get(&guild).map_or(&[], Vec::as_slice)
    }

    pub fn get(&self, guild: Id, user: Id) -> Option<&Member> {
        let at = *self.index.get(&guild)?.get(&user)?;
        self.guilds.get(&guild)?.get(at)
    }

    /// Message member objects may omit the avatar; only an explicit null removes it.
    pub fn apply_member(&mut self, guild: Id, value: &Value, user: Option<&Value>) {
        if let Some(mut member) = Member::from_value(value, user) {
            if value.get("joined_at").is_none() {
                member.joined_at = self
                    .get(guild, member.user.id)
                    .and_then(|known| known.joined_at.clone());
            }
            // An object without a `roles` key says nothing about roles (it is not "no roles"): keep the known ones.
            if value.get("roles").is_none()
                && let Some(known) = self.get(guild, member.user.id)
            {
                member.roles = known.roles.clone();
            }
            if value.get("avatar").is_none() {
                member.avatar = self
                    .get(guild, member.user.id)
                    .and_then(|known| known.avatar.clone());
            }
            if value.get("avatar_decoration_data").is_none() {
                member.avatar_decoration = self
                    .get(guild, member.user.id)
                    .and_then(|known| known.avatar_decoration.clone());
            }
            self.upsert(guild, member);
        }
    }

    /// Adds the member, or replaces the one already known (keeping its place).
    pub fn upsert(&mut self, guild: Id, member: Member) {
        let list = self.guilds.entry(guild).or_default();
        let index = self.index.entry(guild).or_default();
        match index.get(&member.user.id) {
            Some(at) => list[*at] = member,
            None => {
                index.insert(member.user.id, list.len());
                list.push(member);
            }
        }
    }

    pub fn remove(&mut self, guild: Id, user: Id) {
        if let (Some(list), Some(index)) = (self.guilds.get_mut(&guild), self.index.get_mut(&guild))
            && let Some(at) = index.remove(&user)
        {
            // Later members shift down one place, keeping arrival order.
            list.remove(at);
            for position in index.values_mut().filter(|position| **position > at) {
                *position -= 1;
            }
        }
    }

    pub fn drop_guild(&mut self, guild: Id) {
        self.guilds.remove(&guild);
        self.index.remove(&guild);
    }

    /// `GUILD_MEMBERS_CHUNK`, `GUILD_CREATE` and anything else that carries a `members` array for one guild.
    pub fn apply_list(&mut self, guild: Id, members: &Value) {
        for member in members.as_array().into_iter().flatten() {
            self.apply_member(guild, member, None);
        }
    }

    /// The members the lazy member list carries (`SYNC` items, `INSERT` and `UPDATE` items).
    pub fn apply_list_update(&mut self, guild: Id, data: &Value) {
        for op in data["ops"].as_array().into_iter().flatten() {
            let items = op["items"]
                .as_array()
                .into_iter()
                .flatten()
                .chain(std::iter::once(&op["item"]));
            for item in items {
                if let Some(member) = item.get("member") {
                    self.apply_member(guild, member, None);
                }
            }
        }
    }

    pub fn dispatch(&mut self, name: &str, data: &Value) {
        let Some(guild) = id(&data["guild_id"]).or_else(|| id(&data["id"])) else {
            return;
        };
        match name {
            "GUILD_CREATE" => self.apply_list(guild, &data["members"]),
            "GUILD_MEMBERS_CHUNK" => {
                self.apply_list(guild, &data["members"]);
                // Asked for by id and not a member (any more): what was known of them is gone.
                for user in data["not_found"].as_array().into_iter().flatten() {
                    if let Some(user) = id(user) {
                        self.remove(guild, user);
                    }
                }
            }
            "GUILD_MEMBER_ADD" | "GUILD_MEMBER_UPDATE" => {
                self.apply_member(guild, data, None);
            }
            "GUILD_MEMBER_REMOVE" => {
                if let Some(user) = id(&data["user"]["id"]) {
                    self.remove(guild, user);
                }
            }
            "GUILD_DELETE" => self.drop_guild(guild),
            "MESSAGE_CREATE" if data["member"].is_object() => {
                self.apply_member(guild, &data["member"], Some(&data["author"]));
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
    fn a_member_object_without_roles_keeps_the_known_roles() {
        let mut store = GuildMembers::default();
        store.dispatch(
            "GUILD_MEMBERS_CHUNK",
            &json!({"guild_id": "9", "members": [{"user": {"id": "1", "username": "ann"}, "roles": ["5"]}]}),
        );
        store.dispatch(
            "GUILD_MEMBER_LIST_UPDATE",
            &json!({"guild_id": "9", "ops": [{"op": "SYNC", "items": [{"member": {"user": {"id": "1", "username": "ann"}}}]}]}),
        );
        assert_eq!(store.of(9)[0].roles, vec![5]);
        store.dispatch(
            "GUILD_MEMBER_UPDATE",
            &json!({"guild_id": "9", "user": {"id": "1", "username": "ann"}, "roles": []}),
        );
        assert!(store.of(9)[0].roles.is_empty());
    }

    #[test]
    fn a_kept_member_reads_back_and_a_not_found_one_is_dropped() {
        let mut store = GuildMembers::default();
        store.dispatch(
            "GUILD_MEMBERS_CHUNK",
            &json!({"guild_id": "9", "members": [
                {"user": {"id": "1", "username": "ann"}, "roles": ["5", "6"], "nick": "A", "avatar": "h", "pending": true},
            ]}),
        );
        let kept = store.get(9, 1).unwrap().clone();
        let mut fresh = GuildMembers::default();
        fresh.apply_member(9, &kept.to_cache_value(), None);
        let back = fresh.get(9, 1).unwrap();
        assert_eq!(
            (&back.nick, &back.avatar, &back.roles, back.pending),
            (&kept.nick, &kept.avatar, &kept.roles, kept.pending)
        );
        assert_eq!(back.user.id, 1);
        // Asked for again by id: the server no longer has them as a member.
        fresh.dispatch(
            "GUILD_MEMBERS_CHUNK",
            &json!({"guild_id": "9", "members": [], "not_found": ["1"]}),
        );
        assert!(fresh.get(9, 1).is_none());
    }

    #[test]
    fn a_chunk_adds_members_in_order_and_an_update_keeps_the_place() {
        let mut store = GuildMembers::default();
        store.dispatch(
            "GUILD_MEMBERS_CHUNK",
            &json!({"guild_id": "9", "members": [
                {"user": {"id": "1", "username": "ann"}, "roles": ["5"], "nick": "A"},
                {"user": {"id": "2", "username": "bob"}, "roles": []},
            ]}),
        );
        assert_eq!(store.of(9).len(), 2);
        assert_eq!(store.of(9)[0].roles, vec![5]);
        store.dispatch(
            "GUILD_MEMBER_UPDATE",
            &json!({"guild_id": "9", "user": {"id": "1", "username": "ann"}, "nick": "Z", "roles": []}),
        );
        assert_eq!(store.of(9)[0].nick.as_deref(), Some("Z"));
        store.dispatch(
            "GUILD_MEMBER_REMOVE",
            &json!({"guild_id": "9", "user": {"id": "1"}}),
        );
        assert_eq!(store.of(9).len(), 1);
    }

    #[test]
    fn the_join_date_survives_members_that_do_not_carry_it() {
        let mut store = GuildMembers::default();
        let user = json!({"id": "1", "username": "ann"});
        store.apply_member(
            9,
            &json!({"roles": [], "joined_at": "2026-11-05T10:00:00+00:00"}),
            Some(&user),
        );
        // A message's member object has no `joined_at`.
        store.apply_member(9, &json!({"roles": ["5"], "nick": "A"}), Some(&user));
        let member = store.get(9, 1).expect("the member is known");
        assert_eq!(
            member.joined_at.as_deref(),
            Some("2026-11-05T10:00:00+00:00")
        );
        assert_eq!(member.roles, vec![5]);
        // A later full object replaces it.
        store.apply_member(
            9,
            &json!({"roles": [], "joined_at": "2026-11-06T10:00:00+00:00"}),
            Some(&user),
        );
        assert_eq!(
            store
                .get(9, 1)
                .and_then(|member| member.joined_at.as_deref()),
            Some("2026-11-06T10:00:00+00:00")
        );
    }

    #[test]
    fn a_message_member_has_the_author_as_its_user() {
        let mut store = GuildMembers::default();
        store.dispatch(
            "MESSAGE_CREATE",
            &json!({"guild_id": "9", "author": {"id": "3", "username": "cy"}, "member": {"roles": ["7"]}}),
        );
        assert_eq!(store.get(9, 3).unwrap().roles, vec![7]);
    }

    #[test]
    fn guild_avatars_survive_partial_members_and_explicit_removal_restores_global() {
        let mut store = GuildMembers::default();
        let user = json!({"id":"3", "username":"cy", "avatar":"global"});
        store.apply_list_update(
            9,
            &json!({"ops":[{"op":"SYNC", "items":[
                {"member":{"user":user, "avatar":"server", "roles":[]}}
            ]}]}),
        );
        let member = store.get(9, 3).unwrap();
        assert_eq!(member.user.avatar.as_deref(), Some("global"));
        assert_eq!(
            member.avatar_url(9).as_deref(),
            Some("https://cdn.discordapp.com/guilds/9/users/3/avatars/server.webp?size=128")
        );
        store.dispatch(
            "MESSAGE_CREATE",
            &json!({
                "guild_id":"9", "author":user, "member":{"roles":[]}
            }),
        );
        assert_eq!(store.get(9, 3).unwrap().avatar.as_deref(), Some("server"));
        store.apply_member(10, &json!({"user":user, "avatar":"other"}), None);
        assert!(
            store
                .get(10, 3)
                .unwrap()
                .avatar_url(10)
                .unwrap()
                .contains("/10/users/3/avatars/other.")
        );
        store.dispatch(
            "GUILD_MEMBER_UPDATE",
            &json!({
                "guild_id":"9", "user":user, "avatar":null, "roles":[]
            }),
        );
        assert_eq!(store.get(9, 3).unwrap().avatar_url(9), None);
        assert_eq!(
            store.get(9, 3).unwrap().user.avatar.as_deref(),
            Some("global")
        );
        assert_eq!(store.get(10, 3).unwrap().avatar.as_deref(), Some("other"));
    }

    #[test]
    fn lazy_members_keep_global_and_guild_decorations_separate() {
        let mut store = GuildMembers::default();
        let user = json!({"id":"3", "username":"cy", "avatar_decoration_data":{"sku_id":"42"}});
        store.apply_list_update(
            9,
            &json!({"ops":[{"op":"INSERT", "item":{"member":{
                "user":user, "avatar_decoration_data":{"sku_id":"43"}
            }}}]}),
        );
        let member = store.get(9, 3).unwrap();
        assert_eq!(member.user.avatar_decoration.as_ref().unwrap().sku_id, 42);
        assert_eq!(member.avatar_decoration.as_ref().unwrap().sku_id, 43);
        store.apply_member(9, &json!({"roles":[]}), Some(&user));
        assert_eq!(
            store
                .get(9, 3)
                .unwrap()
                .avatar_decoration
                .as_ref()
                .unwrap()
                .sku_id,
            43
        );
        store.apply_list_update(
            9,
            &json!({"ops":[{"op":"UPDATE", "item":{"member":{
                "user":user, "avatar_decoration_data":null
            }}}]}),
        );
        let member = store.get(9, 3).unwrap();
        assert!(member.avatar_decoration.is_none());
        assert_eq!(member.user.avatar_decoration.as_ref().unwrap().sku_id, 42);
    }

    #[test]
    fn fifty_thousand_members_upsert_and_look_up_quickly_and_keep_order() {
        let mut store = GuildMembers::default();
        let start = std::time::Instant::now();
        for n in 1..=50_000u64 {
            let user = json!({"id": n.to_string(), "username": format!("u{n}")});
            store.apply_member(9, &json!({"roles": [], "nick": "a"}), Some(&user));
        }
        // A second pass updates in place; the order does not change.
        for n in (1..=50_000u64).rev() {
            let user = json!({"id": n.to_string(), "username": format!("u{n}")});
            store.apply_member(9, &json!({"roles": ["5"], "nick": "b"}), Some(&user));
            assert_eq!(store.get(9, n).unwrap().nick.as_deref(), Some("b"));
        }
        assert!(
            start.elapsed() < std::time::Duration::from_secs(10),
            "{:?}",
            start.elapsed()
        );
        assert_eq!(store.of(9).len(), 50_000);
        assert_eq!(store.of(9)[0].user.id, 1);
        assert_eq!(store.of(9)[49_999].user.id, 50_000);
    }

    #[test]
    fn removal_keeps_the_index_and_order_consistent() {
        let mut store = GuildMembers::default();
        for n in 1..=5u64 {
            let user = json!({"id": n.to_string(), "username": "u"});
            store.apply_member(9, &json!({"roles": []}), Some(&user));
        }
        store.remove(9, 2);
        store.remove(9, 99);
        let order: Vec<Id> = store.of(9).iter().map(|member| member.user.id).collect();
        assert_eq!(order, vec![1, 3, 4, 5]);
        for (at, user) in order.iter().enumerate() {
            assert_eq!(store.get(9, *user).unwrap(), &store.of(9)[at]);
        }
        assert!(store.get(9, 2).is_none());
        let again = json!({"id": "2", "username": "u"});
        store.apply_member(9, &json!({"roles": []}), Some(&again));
        assert_eq!(store.of(9).last().unwrap().user.id, 2);
        assert_eq!(store.get(9, 2).unwrap().user.id, 2);
        store.drop_guild(9);
        assert!(store.get(9, 1).is_none());
    }
}
