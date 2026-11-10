//! The lazy member list. Port of the operations in `ChannelMemberStore`, module 963307.

use crate::{Id, User};
use serde_json::Value;

#[derive(Clone, Debug, PartialEq)]
// Rows live in one list and are read by reference, so the member variant stays inline.
#[allow(clippy::large_enum_variant)]
pub enum Row {
    Group {
        id: String,
        count: u64,
    },
    Member {
        user: User,
        nick: Option<String>,
        /// Their role ids; the audit log's user filter reads them.
        roles: Vec<Id>,
        status: String,
    },
    /// A slot inside the list that the server has not sent yet.
    Unknown,
}

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
            user: User {
                id,
                username: user["username"].as_str().unwrap_or_default().to_owned(),
                global_name: user["global_name"].as_str().map(str::to_owned),
                avatar: user["avatar"].as_str().map(str::to_owned),
                discriminator: user["discriminator"].as_str().unwrap_or("0").to_owned(),
                tag: crate::ServerTag::from_value(&user["primary_guild"]),
                bot: user["bot"].as_bool().unwrap_or(false),
                nameplate: crate::collectibles::Nameplate::from_value(
                    &user["collectibles"]["nameplate"],
                ),
                avatar_decoration: crate::collectibles::AvatarDecoration::from_value(
                    &user["avatar_decoration_data"],
                )
                .map(Box::new),
                flags: user["flags"].as_u64().unwrap_or(0),
                public_flags: user["public_flags"].as_u64().unwrap_or(0),
            },
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
}

impl MemberList {
    pub fn apply(&mut self, data: &Value) {
        let groups = data["groups"].as_array().into_iter().flatten();
        self.groups = groups
            .map(|group| {
                (
                    group["id"].as_str().unwrap_or_default().to_owned(),
                    group["count"].as_u64().unwrap_or(0),
                )
            })
            .collect();
        for op in data["ops"].as_array().into_iter().flatten() {
            match op["op"].as_str() {
                Some("SYNC") => self.sync(op),
                Some("INVALIDATE") => self.invalidate(op),
                Some("INSERT") => {
                    let row = Row::from_value(&op["item"], &self.groups);
                    let index = op["index"].as_u64().unwrap_or(0) as usize;
                    self.rows.insert(index.min(self.rows.len()), row);
                }
                Some("UPDATE") => {
                    let row = Row::from_value(&op["item"], &self.groups);
                    if let Some(slot) = op["index"]
                        .as_u64()
                        .and_then(|index| self.rows.get_mut(index as usize))
                    {
                        *slot = row;
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
        self.refresh_group_counts();
    }

    fn range(op: &Value) -> Option<(usize, usize)> {
        let range = op["range"].as_array()?;
        Some((
            range.first()?.as_u64()? as usize,
            range.get(1)?.as_u64()? as usize,
        ))
    }

    fn sync(&mut self, op: &Value) {
        let Some((start, _)) = Self::range(op) else {
            return;
        };
        let items = op["items"].as_array().map_or(&[][..], Vec::as_slice);
        let end = start + items.len();
        if self.rows.len() < end {
            self.rows.resize(end, Row::Unknown);
        }
        for (offset, item) in items.iter().enumerate() {
            self.rows[start + offset] = Row::from_value(item, &self.groups);
        }
    }

    fn invalidate(&mut self, op: &Value) {
        if let Some((start, end)) = Self::range(op) {
            for row in self
                .rows
                .iter_mut()
                .skip(start)
                .take(end.saturating_sub(start) + 1)
            {
                *row = Row::Unknown;
            }
        }
    }

    fn refresh_group_counts(&mut self) {
        for row in &mut self.rows {
            if let Row::Group { id, count } = row
                && let Some((_, known)) = self.groups.iter().find(|(group, _)| group == id)
            {
                *count = *known;
            }
        }
    }
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
        list.apply(&json!({"groups": [], "ops": [{"op": "SYNC", "range": [2, 3], "items": [member("1", "dnd")]}]}));
        assert_eq!(names(&list), ["?", "?", "user1"]);
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
    fn bad_indexes_are_ignored() {
        let mut list = synced();
        list.apply(&json!({"groups": [], "ops": [{"op": "DELETE", "index": 50}, {"op": "UPDATE", "index": 50, "item": member("1", "dnd")}]}));
        assert_eq!(list.rows.len(), 3);
    }
}
