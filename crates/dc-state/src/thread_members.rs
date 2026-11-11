//! `JoinedThreadsStore` (module 152007): the threads the current user has joined, by thread id.
//!
//! The pinned client learns a membership from exactly these: `threads[].member` of READY / GUILD_CREATE (the whole
//! guild's entries are replaced), a THREAD_CREATE channel carrying `member`, `members` of THREAD_LIST_SYNC, the
//! `members` of message search, thread search and archived-thread responses, THREAD_MEMBER_UPDATE (the current user's
//! own only), THREAD_MEMBERS_UPDATE (`added_members` / `removed_member_ids`), and the local join / leave of a forum
//! post (THREAD_MEMBER_LOCAL_UPDATE). THREAD_DELETE and GUILD_DELETE drop entries. `newly_created` on THREAD_CREATE is
//! not a membership: the server follows it with a THREAD_MEMBER_UPDATE.

use crate::{Id, Thread, id};
use chrono::{DateTime, Utc};
use serde_json::Value;
use std::collections::HashMap;

/// One joined thread: a thread member object of the current user.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ThreadMember {
    pub guild: Id,
    /// `flags`: 2 all messages, 4 only mentions, 8 nothing. `None` when the object carried none.
    pub flags: Option<u64>,
    pub muted: bool,
    /// `mute_config.end_time`.
    pub mute_end: Option<DateTime<Utc>>,
    pub join_timestamp: Option<String>,
}

impl ThreadMember {
    /// A thread member object (`{flags, muted, mute_config, join_timestamp}`, with or without `id` / `user_id`).
    pub fn from_value(guild: Id, member: &Value) -> Self {
        Self {
            guild,
            flags: member["flags"].as_u64(),
            muted: member["muted"].as_bool().unwrap_or(false),
            mute_end: member["mute_config"]["end_time"]
                .as_str()
                .and_then(|time| DateTime::parse_from_rfc3339(time).ok())
                .map(|time| time.with_timezone(&Utc)),
            join_timestamp: crate::text(&member["join_timestamp"]),
        }
    }
}

#[derive(Debug, Default)]
pub struct ThreadMembers {
    joined: HashMap<Id, ThreadMember>,
}

impl ThreadMembers {
    /// Events after which the guilds' threads take their membership from this store again.
    pub const AFFECTS_THREADS: &[&str] = &[
        "READY",
        "GUILD_CREATE",
        "GUILD_DELETE",
        "THREAD_CREATE",
        "THREAD_UPDATE",
        "THREAD_DELETE",
        "THREAD_LIST_SYNC",
        "THREAD_MEMBER_UPDATE",
        "THREAD_MEMBERS_UPDATE",
    ];

    /// `hasJoined`.
    pub fn has_joined(&self, thread: Id) -> bool {
        self.joined.contains_key(&thread)
    }

    pub fn get(&self, thread: Id) -> Option<&ThreadMember> {
        self.joined.get(&thread)
    }

    pub fn len(&self) -> usize {
        self.joined.len()
    }

    pub fn is_empty(&self) -> bool {
        self.joined.is_empty()
    }

    /// `GUILD_CREATE` (`E` then `A`) and one guild of `CONNECTION_OPEN`: the guild's entries become those of the
    /// threads that carry a `member`.
    fn load_guild(&mut self, guild: Id, threads: &Value) {
        self.joined.retain(|_, member| member.guild != guild);
        for thread in threads.as_array().into_iter().flatten() {
            self.load_thread(guild, thread);
        }
    }

    /// `f`: a thread channel of the thread types with a `member`.
    fn load_thread(&mut self, guild: Id, thread: &Value) {
        if matches!(thread["type"].as_u64(), Some(10..=12))
            && thread["member"].is_object()
            && let Some(thread_id) = id(&thread["id"])
        {
            let guild = id(&thread["guild_id"]).unwrap_or(guild);
            self.joined.insert(
                thread_id,
                ThreadMember::from_value(guild, &thread["member"]),
            );
        }
    }

    /// `I` / `p`: `members` of THREAD_LIST_SYNC, of a thread search and of a message search. Each object's `id` is the
    /// thread; the server only sends the current user's, so nothing is checked against the user.
    pub fn add_members(&mut self, guild: Id, members: &Value) {
        for member in members.as_array().into_iter().flatten() {
            if let Some(thread) = id(&member["id"]) {
                self.joined
                    .insert(thread, ThreadMember::from_value(guild, member));
            }
        }
    }

    /// The `members` of a message-search response body (`tabs[].members` or the flat form) and of a thread-search /
    /// archived-threads response body. Message search needs a guild (`p`: `null != guildId`).
    pub fn add_response(&mut self, guild: Option<Id>, body: &Value) {
        let Some(guild) = guild else { return };
        self.add_members(guild, &body["members"]);
        if let Some(tabs) = body["tabs"].as_object() {
            for tab in tabs.values() {
                self.add_members(guild, &tab["members"]);
            }
        }
    }

    /// `THREAD_MEMBER_LOCAL_UPDATE`: this client joined or left a forum post and has not heard back yet.
    pub fn local_update(&mut self, thread: Id, guild: Id, joining: bool) {
        if joining {
            self.joined.insert(
                thread,
                ThreadMember {
                    guild,
                    flags: Some(0),
                    muted: true,
                    mute_end: None,
                    join_timestamp: Some(Utc::now().to_rfc3339()),
                },
            );
        } else {
            self.joined.remove(&thread);
        }
    }

    /// One gateway event. `me` is the current user.
    pub fn dispatch(&mut self, name: &str, data: &Value, me: Option<Id>) {
        match name {
            "READY" => {
                self.joined.clear();
                for guild in data["guilds"].as_array().into_iter().flatten() {
                    if let Some(guild_id) = id(&guild["id"]) {
                        self.load_guild(guild_id, &guild["threads"]);
                    }
                }
            }
            "GUILD_CREATE" => {
                if let Some(guild) = id(&data["id"]) {
                    self.load_guild(guild, &data["threads"]);
                }
            }
            "GUILD_DELETE" => {
                if let Some(guild) = id(&data["id"]) {
                    self.joined.retain(|_, member| member.guild != guild);
                }
            }
            "THREAD_CREATE" => {
                if let Some(guild) = id(&data["guild_id"]) {
                    self.load_thread(guild, data);
                }
            }
            "THREAD_DELETE" => {
                if let Some(thread) = id(&data["id"]) {
                    self.joined.remove(&thread);
                }
            }
            "THREAD_LIST_SYNC" => {
                if let Some(guild) = id(&data["guild_id"]) {
                    self.add_members(guild, &data["members"]);
                }
            }
            "THREAD_MEMBER_UPDATE" => {
                // The event is one member's fields beside the thread id; only the current user's counts.
                if let (Some(thread), Some(guild), Some(user)) =
                    (id(&data["id"]), id(&data["guild_id"]), id(&data["user_id"]))
                    && Some(user) == me
                {
                    self.joined
                        .insert(thread, ThreadMember::from_value(guild, data));
                }
            }
            "THREAD_MEMBERS_UPDATE" => {
                let (Some(thread), Some(guild), Some(me)) =
                    (id(&data["id"]), id(&data["guild_id"]), me)
                else {
                    return;
                };
                if data["removed_member_ids"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .any(|user| id(user) == Some(me))
                {
                    self.joined.remove(&thread);
                }
                for member in data["added_members"].as_array().into_iter().flatten() {
                    if id(&member["user_id"]) == Some(me) {
                        self.joined
                            .insert(thread, ThreadMember::from_value(guild, member));
                    }
                }
            }
            _ => {}
        }
    }
}

impl Thread {
    /// Takes the membership fields from the store's entry, or clears them when the account has not joined.
    pub fn set_member(&mut self, member: Option<&ThreadMember>) {
        match member {
            Some(member) => {
                self.joined = true;
                self.member_flags = member.flags;
                self.muted = member.muted;
                self.mute_end = member.mute_end;
                self.join_timestamp = member.join_timestamp.clone();
            }
            None => self.clear_member(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const ME: Id = 7;

    fn store() -> ThreadMembers {
        ThreadMembers::default()
    }

    #[test]
    fn ready_and_guild_create_replace_the_guilds_entries() {
        let mut members = store();
        members.dispatch(
            "READY",
            &json!({"guilds": [
                {"id": "1", "threads": [
                    {"id": "10", "type": 11, "member": {"flags": 2, "join_timestamp": "2026-11-08T02:57:03+00:00"}},
                    {"id": "11", "type": 11},
                ]},
                {"id": "2", "threads": [{"id": "20", "type": 12, "member": {"flags": 4}}]},
            ]}),
            None,
        );
        assert!(members.has_joined(10) && !members.has_joined(11) && members.has_joined(20));
        assert_eq!(members.get(10).unwrap().flags, Some(2));
        assert_eq!(members.get(10).unwrap().guild, 1);
        // GUILD_CREATE of guild 1 swaps its entries and leaves guild 2's.
        members.dispatch(
            "GUILD_CREATE",
            &json!({"id": "1", "threads": [{"id": "12", "type": 11, "member": {"flags": 8}}]}),
            None,
        );
        assert!(!members.has_joined(10) && members.has_joined(12) && members.has_joined(20));
        members.dispatch("GUILD_DELETE", &json!({"id": "2"}), None);
        assert!(!members.has_joined(20));
        // A new READY forgets everything.
        members.dispatch("READY", &json!({"guilds": []}), None);
        assert!(members.is_empty());
    }

    #[test]
    fn thread_create_joins_only_with_a_member_and_not_by_newly_created() {
        let mut members = store();
        members.dispatch(
            "THREAD_CREATE",
            &json!({"id": "30", "guild_id": "1", "type": 11, "newly_created": true}),
            Some(ME),
        );
        assert!(!members.has_joined(30));
        members.dispatch(
            "THREAD_MEMBER_UPDATE",
            &json!({"id": "30", "guild_id": "1", "user_id": "7", "flags": 1, "muted": false,
                    "mute_config": null, "join_timestamp": "2026-11-08T02:57:03.036078+00:00"}),
            Some(ME),
        );
        assert!(members.has_joined(30));
        assert_eq!(members.get(30).unwrap().flags, Some(1));
        members.dispatch(
            "THREAD_CREATE",
            &json!({"id": "31", "guild_id": "1", "type": 11, "member": {"flags": 1}}),
            Some(ME),
        );
        assert!(members.has_joined(31));
        members.dispatch(
            "THREAD_DELETE",
            &json!({"id": "31", "guild_id": "1"}),
            Some(ME),
        );
        assert!(!members.has_joined(31));
    }

    #[test]
    fn another_users_member_update_is_ignored() {
        let mut members = store();
        members.dispatch(
            "THREAD_MEMBER_UPDATE",
            &json!({"id": "30", "guild_id": "1", "user_id": "8", "flags": 1}),
            Some(ME),
        );
        members.dispatch(
            "THREAD_MEMBER_UPDATE",
            &json!({"id": "30", "guild_id": "1", "flags": 1}),
            Some(ME),
        );
        assert!(members.is_empty());
    }

    #[test]
    fn thread_members_update_adds_and_removes_only_the_current_user() {
        let mut members = store();
        members.dispatch(
            "THREAD_MEMBERS_UPDATE",
            &json!({"id": "40", "guild_id": "1", "member_count": 3, "added_members": [
                {"id": "40", "user_id": "8", "flags": 1}, {"id": "40", "user_id": "7", "flags": 2, "muted": true,
                 "mute_config": {"end_time": "2999-01-01T00:00:00+00:00"}},
            ]}),
            Some(ME),
        );
        let member = members.get(40).unwrap();
        assert_eq!(member.flags, Some(2));
        assert!(member.muted && member.mute_end.is_some());
        members.dispatch(
            "THREAD_MEMBERS_UPDATE",
            &json!({"id": "40", "guild_id": "1", "removed_member_ids": ["8"]}),
            Some(ME),
        );
        assert!(members.has_joined(40));
        members.dispatch(
            "THREAD_MEMBERS_UPDATE",
            &json!({"id": "40", "guild_id": "1", "removed_member_ids": ["7"]}),
            Some(ME),
        );
        assert!(!members.has_joined(40));
    }

    #[test]
    fn list_sync_and_rest_responses_carry_members() {
        let mut members = store();
        members.dispatch(
            "THREAD_LIST_SYNC",
            &json!({"guild_id": "1", "threads": [], "members": [
                {"id": "50", "user_id": "7", "join_timestamp": "2026-11-08T02:57:03.036000+00:00", "flags": 1,
                 "muted": false, "mute_config": null},
            ]}),
            Some(ME),
        );
        assert!(members.has_joined(50));
        // Message search: the flat body and the tabbed one; a search without a guild changes nothing.
        members.add_response(
            Some(1),
            &json!({"members": [{"id": "51", "user_id": "7", "flags": 1}]}),
        );
        members.add_response(
            Some(1),
            &json!({"tabs": {"messages": {"members": [{"id": "52", "flags": 1}]}}}),
        );
        members.add_response(None, &json!({"members": [{"id": "53", "flags": 1}]}));
        assert!(members.has_joined(51) && members.has_joined(52) && !members.has_joined(53));
        // Thread search and archived threads: `members` beside `threads`.
        members.add_members(1, &json!([{"id": "54", "flags": 2}]));
        assert!(members.has_joined(54));
    }

    #[test]
    fn local_update_joins_and_leaves() {
        let mut members = store();
        members.local_update(60, 1, true);
        let member = members.get(60).unwrap();
        assert_eq!(member.flags, Some(0));
        assert!(member.muted && member.join_timestamp.is_some());
        members.local_update(60, 1, false);
        assert!(!members.has_joined(60));
    }
}
