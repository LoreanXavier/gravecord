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
}

impl GuildMembers {
    /// The guild's members in arrival order.
    pub fn of(&self, guild: Id) -> &[Member] {
        self.guilds.get(&guild).map_or(&[], Vec::as_slice)
    }

    pub fn get(&self, guild: Id, user: Id) -> Option<&Member> {
        self.of(guild).iter().find(|member| member.user.id == user)
    }

    /// Message member objects may omit the avatar; only an explicit null removes it.
    pub fn apply_member(&mut self, guild: Id, value: &Value, user: Option<&Value>) {
        if let Some(mut member) = Member::from_value(value, user) {
            if value.get("joined_at").is_none() {
                member.joined_at = self
                    .get(guild, member.user.id)
                    .and_then(|known| known.joined_at.clone());
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
        match list
            .iter_mut()
            .find(|known| known.user.id == member.user.id)
        {
            Some(known) => *known = member,
            None => list.push(member),
        }
    }

    pub fn remove(&mut self, guild: Id, user: Id) {
        if let Some(list) = self.guilds.get_mut(&guild) {
            list.retain(|member| member.user.id != user);
        }
    }

    pub fn drop_guild(&mut self, guild: Id) {
        self.guilds.remove(&guild);
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
            "GUILD_MEMBERS_CHUNK" => self.apply_list(guild, &data["members"]),
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
}
