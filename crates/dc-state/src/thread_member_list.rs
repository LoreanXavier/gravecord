//! `THREAD_MEMBER_LIST_UPDATE {guild_id, thread_id, members: [{user_id, member, presence, ...}], presences}`: the
//! gateway's answer when a thread's member sidebar opens (module 225123).
//!
//! The official client fans it out to: `UserStore` (module 287809 `eh`: the users of the members are cached),
//! `GuildMemberStore` (module 696451: the `member` objects are stored for the server), `PresenceStore` (module 290863:
//! each member's `presence`, and the top-level `presences` through `PRESENCE_UPDATES`, are stored for the server) and
//! `ThreadMemberListStore` (module 219065: the thread's member list is rebuilt from the `user_id`s, only for a thread
//! whose parent channel is known). A thread that is deleted or archived loses its list.

use crate::{Id, Stores, id};
use serde_json::{Value, json};
use std::collections::HashMap;

#[derive(Default)]
pub struct ThreadMemberLists {
    /// Thread to the users the last update listed, in the order sent.
    lists: HashMap<Id, Vec<Id>>,
    pub version: u64,
}

impl ThreadMemberLists {
    /// The user ids of a thread's member list, when one was received.
    pub fn members_of(&self, thread: Id) -> Option<&[Id]> {
        self.lists.get(&thread).map(Vec::as_slice)
    }
}

impl Stores {
    pub(crate) fn thread_member_list_dispatch(&mut self, name: &str, data: &Value) {
        match name {
            "THREAD_MEMBER_LIST_UPDATE" => self.thread_member_list_update(data),
            "THREAD_DELETE" => {
                if let Some(thread) = id(&data["id"])
                    && self.thread_member_lists.lists.remove(&thread).is_some()
                {
                    self.thread_member_lists.version += 1;
                }
            }
            "THREAD_UPDATE" => {
                if let Some(thread) = id(&data["id"])
                    && data["thread_metadata"]["archived"] == true
                    && self.thread_member_lists.lists.remove(&thread).is_some()
                {
                    self.thread_member_lists.version += 1;
                }
            }
            _ => {}
        }
    }

    fn thread_member_list_update(&mut self, data: &Value) {
        let (Some(guild), Some(thread)) = (id(&data["guild_id"]), id(&data["thread_id"])) else {
            return;
        };
        let members = data["members"].as_array().cloned().unwrap_or_default();
        let mut objects = Vec::new();
        let mut presences = Vec::new();
        let mut listed = Vec::new();
        for entry in &members {
            if let Some(user) = id(&entry["user_id"]) {
                listed.push(user);
            }
            if entry["member"].is_object() {
                objects.push(entry["member"].clone());
            }
            if entry["presence"].is_object()
                && let Some(user) = id(&entry["user_id"])
            {
                let mut presence = entry["presence"].clone();
                if !presence["user"].is_object() {
                    presence["user"] = json!({"id": user.to_string()});
                }
                presences.push(presence);
            }
        }
        for presence in data["presences"].as_array().into_iter().flatten() {
            presences.push(presence.clone());
        }
        // The member objects, their users and the presences travel the way a members chunk does.
        if !objects.is_empty() || !presences.is_empty() {
            self.dispatch(
                "GUILD_MEMBERS_CHUNK",
                &json!({"guild_id": guild.to_string(), "members": objects, "presences": presences}),
            );
        }
        let parent_known = self.guild(guild).is_some_and(|known| {
            known
                .threads
                .iter()
                .any(|thread_known| thread_known.id == thread)
        });
        if parent_known {
            self.thread_member_lists.lists.insert(thread, listed);
            self.thread_member_lists.version += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn with_thread(stores: &mut Stores) {
        stores.dispatch(
            "GUILD_CREATE",
            &json!({"id": "5", "name": "g", "channels": [{"id": "50", "type": 0, "name": "general"}],
                    "threads": [{"id": "70", "guild_id": "5", "parent_id": "50", "type": 11, "name": "t",
                                 "thread_metadata": {"archived": false}}]}),
        );
    }

    fn update() -> Value {
        json!({
            "guild_id": "5", "thread_id": "70",
            "members": [
                {"user_id": "11", "join_timestamp": "2026-11-11T10:00:00.000000+00:00", "flags": 0,
                 "member": {"user": {"id": "11", "username": "ann", "discriminator": "0"}, "roles": [], "nick": "A"},
                 "presence": {"status": "online", "activities": [], "client_status": {"web": "online"}}},
                {"user_id": "12", "join_timestamp": "2026-11-11T10:05:00.000000+00:00", "flags": 0,
                 "member": {"user": {"id": "12", "username": "bob", "discriminator": "0"}, "roles": []}},
            ],
            "presences": [{"user": {"id": "12"}, "status": "idle", "activities": []}],
        })
    }

    #[test]
    fn the_list_caches_users_members_and_presences_and_builds_the_thread_list() {
        let mut stores = Stores::default();
        with_thread(&mut stores);
        stores.dispatch("THREAD_MEMBER_LIST_UPDATE", &update());
        assert_eq!(
            stores.thread_member_lists.members_of(70),
            Some(&[11, 12][..])
        );
        assert_eq!(stores.users[&11].username, "ann");
        assert_eq!(stores.users[&12].username, "bob");
        let ann = stores
            .members
            .of(5)
            .iter()
            .find(|member| member.user.id == 11);
        assert!(ann.is_some_and(|member| member.nick.as_deref() == Some("A")));
        assert_eq!(stores.presence.get(11, Some(5)).unwrap().status, "online");
        assert_eq!(stores.presence.get(12, Some(5)).unwrap().status, "idle");
    }

    #[test]
    fn a_thread_the_client_does_not_know_gets_no_list_and_a_delete_or_archive_drops_it() {
        let mut stores = Stores::default();
        stores.dispatch("THREAD_MEMBER_LIST_UPDATE", &update());
        assert!(stores.thread_member_lists.members_of(70).is_none());
        // The users are cached all the same.
        assert!(stores.users.contains_key(&11));
        with_thread(&mut stores);
        stores.dispatch("THREAD_MEMBER_LIST_UPDATE", &update());
        assert!(stores.thread_member_lists.members_of(70).is_some());
        stores.dispatch(
            "THREAD_UPDATE",
            &json!({"id": "70", "guild_id": "5", "parent_id": "50", "type": 11, "name": "t",
                    "thread_metadata": {"archived": true}}),
        );
        assert!(stores.thread_member_lists.members_of(70).is_none());
        stores.dispatch("THREAD_MEMBER_LIST_UPDATE", &update());
        stores.dispatch(
            "THREAD_DELETE",
            &json!({"id": "70", "guild_id": "5", "parent_id": "50"}),
        );
        assert!(stores.thread_member_lists.members_of(70).is_none());
    }
}
