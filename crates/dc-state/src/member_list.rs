//! The lazy member list. Port of `ChannelMemberStore`, module 963307: one list per guild and member list id (the
//! `id` of `GUILD_MEMBER_LIST_UPDATE`), its operations, and the list id a channel reads (`D`).

use crate::{Channel, Guild, Id, User, permissions};
use serde_json::Value;
use std::collections::HashMap;

#[derive(Clone, Debug, PartialEq)]
pub enum Row {
    Group {
        id: String,
        count: u64,
    },
    Member {
        /// Boxed so that the unsent slots of a long list (`Unknown`) stay small.
        user: Box<User>,
        nick: Option<String>,
        /// Their role ids; the audit log's user filter reads them.
        roles: Vec<Id>,
        status: String,
    },
    /// A slot inside the list that the server has not sent yet.
    Unknown,
}

/// The farthest row a list may reach; the list is dense, so a start past this would allocate a row per slot.
const MAX_LIST_ROWS: usize = 1_000_000;

/// 963307 `O`: the list of a channel @everyone can see.
pub const EVERYONE: &str = "everyone";

fn id_of(value: &Value) -> Option<Id> {
    value.as_str()?.parse().ok()
}

impl Row {
    fn from_value(item: &Value, groups: &[(String, u64)]) -> Self {
        if let Some(group) = item.get("group") {
            let id = group["id"].as_str().unwrap_or_default().to_owned();
            let count = groups
                .iter()
                .find(|(known, _)| *known == id)
                .map_or(0, |(_, count)| *count);
            return Self::Group { id, count };
        }
        let member = &item["member"];
        let user = &member["user"];
        let Some(id) = id_of(&user["id"]) else {
            return Self::Unknown;
        };
        Self::Member {
            user: Box::new(User {
                id,
                username: user["username"].as_str().unwrap_or_default().to_owned(),
                global_name: user["global_name"].as_str().map(str::to_owned),
                avatar: user["avatar"].as_str().map(str::to_owned),
                discriminator: user["discriminator"].as_str().unwrap_or("0").to_owned(),
                tag: crate::ServerTag::from_value(&user["primary_guild"]),
                bot: user["bot"].as_bool().unwrap_or(false),
                system: user["system"].as_bool().unwrap_or(false),
                nameplate: crate::collectibles::Nameplate::from_value(
                    &user["collectibles"]["nameplate"],
                ),
                avatar_decoration: crate::collectibles::AvatarDecoration::from_value(
                    &user["avatar_decoration_data"],
                )
                .map(Box::new),
                flags: user["flags"].as_u64().unwrap_or(0),
                public_flags: user["public_flags"].as_u64().unwrap_or(0),
                // 609425: the member's own style wins over the user's.
                display_name_styles: crate::display_name_styles::DisplayNameStyles::from_value(
                    &member["display_name_styles"],
                )
                .or_else(|| {
                    crate::display_name_styles::DisplayNameStyles::from_value(
                        &user["display_name_styles"],
                    )
                })
                .map(Box::new),
            }),
            nick: member["nick"].as_str().map(str::to_owned),
            roles: member["roles"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(id_of)
                .collect(),
            status: member["presence"]["status"]
                .as_str()
                .unwrap_or("offline")
                .to_owned(),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct MemberList {
    pub rows: Vec<Row>,
    pub groups: Vec<(String, u64)>,
    /// The row each group's header sits at (`setGroups`: each group takes its header and `count` rows).
    group_starts: Vec<usize>,
}

impl MemberList {
    /// `GUILD_MEMBER_LIST_UPDATE`: the operations in order, then `setGroups`, which sizes the list to its groups (a
    /// header and `count` rows each): rows past the end are dropped and the slots not sent yet are `Unknown`.
    pub fn apply(&mut self, data: &Value) {
        for op in data["ops"].as_array().into_iter().flatten() {
            match op["op"].as_str() {
                Some("SYNC") => self.sync(op),
                Some("INVALIDATE") => self.invalidate(op),
                Some("INSERT") => {
                    let row = Row::from_value(&op["item"], &self.groups);
                    let index = op["index"].as_u64().unwrap_or(0) as usize;
                    // `insert` leaves the list alone for a member without a user.
                    if row != Row::Unknown && self.rows.len() < MAX_LIST_ROWS {
                        self.rows.insert(index.min(self.rows.len()), row);
                    }
                }
                Some("UPDATE") => {
                    let row = Row::from_value(&op["item"], &self.groups);
                    if let Some(index) = op["index"].as_u64().map(|index| index as usize)
                        && row != Row::Unknown
                        && index < MAX_LIST_ROWS
                    {
                        // `this.rows[e] = ...` past the end grows the array; `setGroups` sizes it after.
                        if self.rows.len() <= index {
                            self.rows.resize(index + 1, Row::Unknown);
                        }
                        self.rows[index] = row;
                    }
                }
                Some("DELETE") => {
                    let index = op["index"].as_u64().unwrap_or(u64::MAX) as usize;
                    if index < self.rows.len() {
                        self.rows.remove(index);
                    }
                }
                _ => {}
            }
        }
        if let Some(groups) = data["groups"].as_array() {
            self.set_groups(groups);
        }
    }

    /// `setGroups`: the groups in order, each with its header row and `count` (at least 0) rows after it.
    fn set_groups(&mut self, groups: &[Value]) {
        self.groups.clear();
        self.group_starts.clear();
        let mut total = 0usize;
        for group in groups {
            let count = group["count"].as_i64().unwrap_or(0).max(0) as u64;
            self.groups
                .push((group["id"].as_str().unwrap_or_default().to_owned(), count));
            self.group_starts.push(total);
            total = total.saturating_add(count as usize).saturating_add(1);
        }
        self.rows.resize(total.min(MAX_LIST_ROWS), Row::Unknown);
        for row in &mut self.rows {
            if let Row::Group { id, count } = row
                && let Some((_, known)) = self.groups.iter().find(|(group, _)| group == id)
            {
                *count = *known;
            }
        }
    }

    /// The group whose header sits at `index`. The official list draws its section headers from the groups
    /// (289287 `renderSection`), so a header shows before its row is sent.
    pub fn group_at(&self, index: usize) -> Option<(&str, u64)> {
        let position = self.group_starts.binary_search(&index).ok()?;
        self.groups
            .get(position)
            .map(|(id, count)| (id.as_str(), *count))
    }

    /// `GUILD_MEMBER_UPDATE` (module 963307 `rebuildMember`): the listed row of that user takes the new nickname and
    /// roles; rows of other users, and the group sizes, only change through list operations.
    pub fn member_updated(&mut self, data: &Value) {
        let Some(user) = id_of(&data["user"]["id"]) else {
            return;
        };
        for row in &mut self.rows {
            if let Row::Member {
                user: listed,
                nick,
                roles,
                ..
            } = row
                && listed.id == user
            {
                if data.get("nick").is_some() {
                    *nick = data["nick"].as_str().map(str::to_owned);
                }
                if let Some(new) = data["roles"].as_array() {
                    *roles = new.iter().filter_map(id_of).collect();
                }
            }
        }
    }

    fn range(op: &Value) -> Option<(usize, usize)> {
        let range = op["range"].as_array()?;
        Some((
            range.first()?.as_u64()? as usize,
            range.get(1)?.as_u64()? as usize,
        ))
    }

    /// `sync`: the items fill the rows from the range's start.
    fn sync(&mut self, op: &Value) {
        let Some((start, _)) = Self::range(op) else {
            return;
        };
        let items = op["items"].as_array().map_or(&[][..], Vec::as_slice);
        // A range start beyond any real list (a guild has at most a few million members) is malformed: skip it
        // instead of allocating that many rows.
        if start > MAX_LIST_ROWS {
            return;
        }
        let end = (start + items.len()).min(MAX_LIST_ROWS);
        if self.rows.len() < end {
            self.rows.resize(end, Row::Unknown);
        }
        for (offset, item) in items.iter().enumerate().take(end - start) {
            self.rows[start + offset] = Row::from_value(item, &self.groups);
        }
    }

    /// `invalidate`: the range's rows are unsent again, up to the first that already was.
    fn invalidate(&mut self, op: &Value) {
        let Some((start, end)) = Self::range(op) else {
            return;
        };
        for index in start..=end {
            match self.rows.get_mut(index) {
                None | Some(Row::Unknown) => break,
                Some(row) => *row = Row::Unknown,
            }
        }
    }
}

/// The lists of every guild (963307 `v`), and which list of each guild its open channel shows.
#[derive(Clone, Debug, Default)]
pub struct MemberLists {
    lists: HashMap<Id, HashMap<String, MemberList>>,
    shown: HashMap<Id, String>,
}

impl MemberLists {
    /// The list the guild's open channel shows (`show`; the @everyone list until one is set).
    pub fn get(&self, guild: &Id) -> Option<&MemberList> {
        self.list(*guild, self.shown_id(*guild))
    }

    pub fn get_mut(&mut self, guild: &Id) -> Option<&mut MemberList> {
        let shown = self
            .shown
            .get(guild)
            .map_or(EVERYONE, String::as_str)
            .to_owned();
        self.lists.get_mut(guild)?.get_mut(&shown)
    }

    /// One list of a guild by its id.
    pub fn list(&self, guild: Id, list: &str) -> Option<&MemberList> {
        self.lists.get(&guild)?.get(list)
    }

    /// The id of the list the guild's open channel shows.
    pub fn shown_id(&self, guild: Id) -> &str {
        self.shown.get(&guild).map_or(EVERYONE, String::as_str)
    }

    /// The guild's open channel reads the list `list` (963307 `getProps(guildId, channelId)`).
    pub fn show(&mut self, guild: Id, list: &str) {
        if self.shown_id(guild) != list {
            self.shown.insert(guild, list.to_owned());
        }
    }

    /// `GUILD_MEMBER_LIST_UPDATE`: applied to the list its `id` names.
    pub fn apply(&mut self, guild: Id, data: &Value) {
        let list = data["id"].as_str().unwrap_or(EVERYONE).to_owned();
        self.lists
            .entry(guild)
            .or_default()
            .entry(list)
            .or_default()
            .apply(data);
    }

    /// `GUILD_MEMBER_UPDATE`: every list of the guild rebuilds that member.
    pub fn member_updated(&mut self, guild: Id, data: &Value) {
        for list in self
            .lists
            .get_mut(&guild)
            .into_iter()
            .flat_map(HashMap::values_mut)
        {
            list.member_updated(data);
        }
    }

    /// `GUILD_DELETE`: the guild's lists go.
    pub fn remove(&mut self, guild: &Id) -> Option<HashMap<String, MemberList>> {
        self.lists.remove(guild)
    }

    /// `CONNECTION_OPEN`: a new session starts with no list.
    pub fn reset(&mut self) {
        self.lists.clear();
    }

    pub fn values(&self) -> impl Iterator<Item = &MemberList> {
        self.lists.values().flat_map(HashMap::values)
    }
}

/// 488926 `Ib`: whether @everyone holds `permission` in the channel and no overwrite denies it. A private thread
/// never does; another thread is judged by its parent's overwrites.
fn everyone_can_list(
    guild: &Guild,
    channel: &Channel,
    parent: Option<&Channel>,
    permission: u64,
) -> bool {
    let overwrites = match channel.kind.as_u64() {
        PRIVATE_THREAD => return false,
        ANNOUNCEMENT_THREAD | PUBLIC_THREAD => match parent {
            Some(parent) => &parent.overwrites,
            None => return false,
        },
        _ => &channel.overwrites,
    };
    let everyone = guild.role_permissions.get(&guild.id).copied().unwrap_or(0);
    everyone & permission == permission
        && !overwrites
            .iter()
            .any(|overwrite| overwrite.deny & permission == permission)
}

const ANNOUNCEMENT_THREAD: u64 = 10;
const PUBLIC_THREAD: u64 = 11;
const PRIVATE_THREAD: u64 = 12;

/// 963307 `D`: the member list a channel reads. The channel's own `member_list_id` when it has one; "everyone" when
/// @everyone can see it; else the decimal murmurhash3 (seed 0) of its sorted `allow:<id>`/`deny:<id>` VIEW_CHANNEL
/// overwrites joined with commas. `parent` is a thread's parent channel.
pub fn list_id(guild: &Guild, channel: &Channel, parent: Option<&Channel>) -> String {
    if let Some(id) = &channel.member_list_id {
        return id.clone();
    }
    if everyone_can_list(guild, channel, parent, permissions::VIEW_CHANNEL) {
        return EVERYONE.to_owned();
    }
    let mut entries: Vec<String> = channel
        .overwrites
        .iter()
        .filter_map(|overwrite| {
            if overwrite.allow & permissions::VIEW_CHANNEL == permissions::VIEW_CHANNEL {
                Some(format!("allow:{}", overwrite.id))
            } else if overwrite.deny & permissions::VIEW_CHANNEL == permissions::VIEW_CHANNEL {
                Some(format!("deny:{}", overwrite.id))
            } else {
                None
            }
        })
        .collect();
    entries.sort();
    murmur3(entries.join(",").as_bytes()).to_string()
}

/// Module 181370 (`murmurhash` `v3`): MurmurHash3 x86 32-bit of the UTF-8 bytes, seed 0.
fn murmur3(bytes: &[u8]) -> u32 {
    const C1: u32 = 0xcc9e_2d51;
    const C2: u32 = 0x1b87_3593;
    let mix = |block: u32| block.wrapping_mul(C1).rotate_left(15).wrapping_mul(C2);
    let mut hash = 0u32;
    let (blocks, tail) = bytes.as_chunks::<4>();
    for block in blocks {
        hash ^= mix(u32::from_le_bytes(*block));
        hash = hash
            .rotate_left(13)
            .wrapping_mul(5)
            .wrapping_add(0xe654_6b64);
    }
    if !tail.is_empty() {
        let mut block = 0u32;
        for (shift, byte) in tail.iter().enumerate() {
            block |= u32::from(*byte) << (8 * shift);
        }
        hash ^= mix(block);
    }
    hash ^= bytes.len() as u32;
    hash ^= hash >> 16;
    hash = hash.wrapping_mul(0x85eb_ca6b);
    hash ^= hash >> 13;
    hash = hash.wrapping_mul(0xc2b2_ae35);
    hash ^ (hash >> 16)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn member(id: &str, status: &str) -> Value {
        json!({"member": {"nick": null, "user": {"id": id, "username": format!("user{id}"), "discriminator": "0"}, "presence": {"status": status}}})
    }

    #[test]
    fn member_updates_retain_visible_tags_and_bot_identity() {
        let mut item = member("1", "online");
        item["member"]["user"]["bot"] = json!(true);
        item["member"]["user"]["primary_guild"] = json!({
            "identity_enabled": true, "identity_guild_id": "42", "tag": "TEST", "badge": "badge-hash"
        });
        let mut list = MemberList::default();
        list.apply(&json!({"ops":[{"op":"SYNC", "range":[0,0], "items":[item.clone()]}]}));
        let Row::Member { user, .. } = &list.rows[0] else {
            panic!("member expected")
        };
        assert!(user.bot);
        assert_eq!(
            user.tag
                .as_ref()
                .map(|tag| (tag.guild_id, tag.tag.as_str())),
            Some((42, "TEST"))
        );
        item["member"]["user"]["primary_guild"]["identity_enabled"] = json!(false);
        list.apply(&json!({"ops":[{"op":"UPDATE", "index":0, "item":item}]}));
        let Row::Member { user, .. } = &list.rows[0] else {
            panic!("member expected")
        };
        assert!(user.tag.is_none());
    }

    fn names(list: &MemberList) -> Vec<String> {
        list.rows
            .iter()
            .map(|row| match row {
                Row::Group { id, count } => format!("{id}:{count}"),
                Row::Member { user, .. } => user.username.clone(),
                Row::Unknown => "?".to_owned(),
            })
            .collect()
    }

    fn synced() -> MemberList {
        let mut list = MemberList::default();
        list.apply(&json!({
            "groups": [{"id": "online", "count": 2}],
            "ops": [{"op": "SYNC", "range": [0, 99], "items": [{"group": {"id": "online"}}, member("1", "dnd"), member("2", "idle")]}],
        }));
        list
    }

    #[test]
    fn a_member_update_changes_the_listed_nick_and_roles_of_that_user_only() {
        let mut list = synced();
        list.member_updated(
            &json!({"guild_id": "9", "user": {"id": "1"}, "nick": "Nick", "roles": ["7"]}),
        );
        let rows: Vec<(Option<String>, Vec<Id>)> = list
            .rows
            .iter()
            .filter_map(|row| match row {
                Row::Member { nick, roles, .. } => Some((nick.clone(), roles.clone())),
                _ => None,
            })
            .collect();
        assert_eq!(rows[0], (Some("Nick".to_owned()), vec![7]));
        assert_eq!(rows[1], (None, vec![]));
        // A later update without `nick` keeps it; a null one clears it.
        list.member_updated(&json!({"user": {"id": "1"}, "roles": []}));
        list.member_updated(&json!({"user": {"id": "2"}, "nick": null}));
        let Row::Member { nick, roles, .. } = &list.rows[1] else {
            panic!("member expected")
        };
        assert_eq!((nick.as_deref(), roles.len()), (Some("Nick"), 0));
    }

    #[test]
    fn sync_fills_groups_and_members() {
        assert_eq!(names(&synced()), ["online:2", "user1", "user2"]);
    }

    #[test]
    fn sync_keeps_presence() {
        assert!(matches!(&synced().rows[1], Row::Member { status, .. } if status == "dnd"));
    }

    #[test]
    fn sync_past_the_end_leaves_unknown_slots() {
        let mut list = MemberList::default();
        list.apply(&json!({"groups": [{"id": "online", "count": 3}], "ops": [{"op": "SYNC", "range": [2, 3], "items": [member("1", "dnd")]}]}));
        assert_eq!(names(&list), ["?", "?", "user1", "?"]);
    }

    #[test]
    fn the_groups_size_the_list_and_place_their_headers() {
        let mut list = MemberList::default();
        list.apply(&json!({
            "groups": [{"id": "1", "count": 2}, {"id": "online", "count": 250}, {"id": "offline", "count": 0}],
            "ops": [{"op": "SYNC", "range": [0, 99], "items": [{"group": {"id": "1"}}, member("1", "online")]}],
        }));
        // 3 headers and 252 rows.
        assert_eq!(list.rows.len(), 255);
        assert_eq!(list.group_at(0), Some(("1", 2)));
        assert_eq!(list.group_at(3), Some(("online", 250)));
        assert_eq!(list.group_at(254), Some(("offline", 0)));
        assert_eq!(list.group_at(1), None);
        // A smaller list drops its rows past the new end; the rows it keeps change only through operations.
        list.apply(&json!({"groups": [{"id": "online", "count": 1}], "ops": []}));
        assert_eq!(names(&list), ["1:2", "user1"]);
        assert_eq!(list.group_at(0), Some(("online", 1)));
    }

    #[test]
    fn insert_update_and_delete_follow_their_indexes() {
        let mut list = synced();
        list.apply(&json!({"groups": [{"id": "online", "count": 3}], "ops": [{"op": "INSERT", "index": 1, "item": member("9", "online")}]}));
        assert_eq!(names(&list), ["online:3", "user9", "user1", "user2"]);
        list.apply(&json!({"groups": [{"id": "online", "count": 3}], "ops": [{"op": "UPDATE", "index": 2, "item": member("7", "idle")}]}));
        assert_eq!(names(&list), ["online:3", "user9", "user7", "user2"]);
        list.apply(&json!({"groups": [{"id": "online", "count": 2}], "ops": [{"op": "DELETE", "index": 1}]}));
        assert_eq!(names(&list), ["online:2", "user7", "user2"]);
    }

    #[test]
    fn invalidate_clears_a_range() {
        let mut list = synced();
        list.apply(&json!({"groups": [{"id": "online", "count": 2}], "ops": [{"op": "INVALIDATE", "range": [1, 2]}]}));
        assert_eq!(names(&list), ["online:2", "?", "?"]);
    }

    #[test]
    fn invalidate_stops_at_the_first_unsent_row() {
        let mut list = MemberList::default();
        list.apply(&json!({"groups": [{"id": "online", "count": 4}], "ops": [
            {"op": "SYNC", "range": [0, 1], "items": [{"group": {"id": "online"}}, member("1", "online")]},
            {"op": "SYNC", "range": [3, 4], "items": [member("3", "online"), member("4", "online")]},
        ]}));
        list.apply(&json!({"groups": [{"id": "online", "count": 4}], "ops": [{"op": "INVALIDATE", "range": [1, 4]}]}));
        assert_eq!(names(&list), ["online:4", "?", "?", "user3", "user4"]);
    }

    #[test]
    fn bad_indexes_are_ignored() {
        let mut list = synced();
        list.apply(&json!({"groups": [{"id": "online", "count": 2}], "ops": [{"op": "DELETE", "index": 50}, {"op": "UPDATE", "index": 50, "item": member("1", "dnd")}]}));
        assert_eq!(list.rows.len(), 3);
    }

    #[test]
    fn a_range_start_far_past_any_list_is_skipped() {
        let mut list = synced();
        list.apply(&json!({"ops": [{"op": "SYNC", "range": [u64::MAX / 2, u64::MAX / 2 + 99], "items": [member("9", "online")]}]}));
        list.apply(&json!({"ops": [{"op": "SYNC", "range": [4_000_000_000u64, 4_000_000_099u64], "items": [member("9", "online")]}]}));
        assert_eq!(list.rows.len(), 3);
    }

    #[test]
    fn an_unsent_slot_stays_small() {
        // A guild's list is as long as its groups; the unsent slots must not each carry a user.
        assert!(
            std::mem::size_of::<Row>() <= 96,
            "{}",
            std::mem::size_of::<Row>()
        );
    }

    #[test]
    fn each_list_id_keeps_its_own_rows() {
        let mut lists = MemberLists::default();
        lists.apply(7, &json!({"id": "everyone", "groups": [{"id": "online", "count": 1}], "ops": [{"op": "SYNC", "range": [0, 99], "items": [{"group": {"id": "online"}}, member("1", "online")]}]}));
        lists.apply(7, &json!({"id": "123", "groups": [{"id": "online", "count": 1}], "ops": [{"op": "SYNC", "range": [0, 99], "items": [{"group": {"id": "online"}}, member("2", "online")]}]}));
        // The other list going away (its channel left the subscription) leaves the shown one whole.
        lists.apply(7, &json!({"id": "123", "groups": [{"id": "online", "count": 1}], "ops": [{"op": "INVALIDATE", "range": [0, 99]}]}));
        assert_eq!(names(lists.get(&7).unwrap()), ["online:1", "user1"]);
        lists.show(7, "123");
        assert_eq!(names(lists.get(&7).unwrap()), ["?", "?"]);
        lists.member_updated(7, &json!({"user": {"id": "1"}, "nick": "N"}));
        assert!(
            matches!(&lists.list(7, "everyone").unwrap().rows[1], Row::Member { nick: Some(nick), .. } if nick == "N")
        );
        lists.reset();
        assert!(lists.get(&7).is_none());
    }

    #[test]
    fn murmur3_matches_the_official_module() {
        // Values of module 181370 for these inputs.
        assert_eq!(murmur3(b""), 0);
        assert_eq!(murmur3(b"foo"), 0xf6a5_c420);
        assert_eq!(murmur3(b"hello"), 0x248b_fa47);
        assert_eq!(murmur3(b"allow:30"), 3_939_944_384);
    }

    fn guild_with(everyone: u64) -> Guild {
        let mut guild = Guild {
            id: 100,
            ..Guild::default()
        };
        guild.role_permissions.insert(100, everyone);
        guild
    }

    fn channel_with(kind: u64, overwrites: Vec<permissions::Overwrite>) -> Channel {
        let mut channel = Channel::from_value(&json!({"id": "5", "type": kind})).unwrap();
        channel.overwrites = overwrites;
        channel
    }

    fn overwrite(id: Id, allow: u64, deny: u64) -> permissions::Overwrite {
        permissions::Overwrite {
            id,
            kind: 0,
            allow,
            deny,
        }
    }

    #[test]
    fn a_channel_reads_the_list_its_view_overwrites_name() {
        let view = permissions::VIEW_CHANNEL;
        let guild = guild_with(view);
        assert_eq!(list_id(&guild, &channel_with(0, vec![]), None), "everyone");
        // An overwrite that does not touch viewing keeps the @everyone list.
        let other = overwrite(9, 1 << 11, 0);
        assert_eq!(
            list_id(&guild, &channel_with(0, vec![other.clone()]), None),
            "everyone"
        );
        // Denied to @everyone and allowed to a role: the hash of the sorted entries.
        let private = channel_with(
            0,
            vec![overwrite(300, view, 0), overwrite(100, 0, view), other],
        );
        assert_eq!(list_id(&guild, &private, None), "248909577");
        // @everyone without the permission: the hash of no entry.
        assert_eq!(list_id(&guild_with(0), &channel_with(0, vec![]), None), "0");
        // A public thread follows its parent; a private thread never reads the @everyone list.
        let parent = channel_with(0, vec![]);
        assert_eq!(
            list_id(&guild, &channel_with(11, vec![]), Some(&parent)),
            "everyone"
        );
        assert_eq!(
            list_id(&guild, &channel_with(12, vec![]), Some(&parent)),
            "0"
        );
        let mut named = channel_with(0, vec![]);
        named.member_list_id = Some("42".into());
        assert_eq!(list_id(&guild, &named, None), "42");
    }
}
