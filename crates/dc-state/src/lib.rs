//! Flux stores. M0 slice: the parts of UserStore, GuildStore, ChannelStore and MessageStore the first screen reads.
//! Names and getters follow `STORES`; the rest of each store is tracked as open rows in `FEATURES.md`.

pub mod grouping;
pub mod member_list;

use chrono::{DateTime, Local, Utc};
use dc_proto::settings;
use serde_json::Value;
use std::collections::HashMap;

pub type Id = u64;

fn id(value: &Value) -> Option<Id> {
    value.as_str()?.parse().ok()
}

fn text(value: &Value) -> Option<String> {
    value.as_str().map(str::to_owned)
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct User {
    pub id: Id,
    pub username: String,
    pub global_name: Option<String>,
    pub avatar: Option<String>,
    pub discriminator: String,
}

impl User {
    fn from_value(value: &Value) -> Option<Self> {
        Some(Self {
            id: id(&value["id"])?,
            username: text(&value["username"])?,
            global_name: text(&value["global_name"]),
            avatar: text(&value["avatar"]),
            discriminator: text(&value["discriminator"]).unwrap_or_else(|| "0".to_owned()),
        })
    }

    pub fn display_name(&self) -> &str {
        self.global_name.as_deref().unwrap_or(&self.username)
    }

    /// `(id >> 22) % 6` for pomelo users, `discriminator % 5` for legacy ones.
    pub fn default_avatar_index(&self) -> u64 {
        match self.discriminator.parse::<u64>() {
            Ok(discriminator) if discriminator != 0 => discriminator % 5,
            _ => (self.id >> 22) % 6,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChannelKind {
    Text,
    Voice,
    Category,
    Other(u64),
}

impl ChannelKind {
    fn from_u64(value: u64) -> Self {
        match value {
            0 => Self::Text,
            2 => Self::Voice,
            4 => Self::Category,
            other => Self::Other(other),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Channel {
    pub id: Id,
    pub name: String,
    pub kind: ChannelKind,
    pub position: i64,
    pub parent_id: Option<Id>,
}

impl Channel {
    fn from_value(value: &Value) -> Option<Self> {
        Some(Self {
            id: id(&value["id"])?,
            name: text(&value["name"]).unwrap_or_default(),
            kind: ChannelKind::from_u64(value["type"].as_u64()?),
            position: value["position"].as_i64().unwrap_or(0),
            parent_id: id(&value["parent_id"]),
        })
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Guild {
    pub id: Id,
    pub name: String,
    pub icon: Option<String>,
    pub owner_id: Option<Id>,
    pub joined_at: String,
    pub channels: Vec<Channel>,
}

impl Guild {
    /// READY nests name and icon under `properties`; GUILD_CREATE may not.
    fn from_value(value: &Value) -> Option<Self> {
        let properties = if value["properties"].is_object() {
            &value["properties"]
        } else {
            value
        };
        let channels = value["channels"].as_array()?;
        Some(Self {
            id: id(&value["id"])?,
            name: text(&properties["name"])?,
            icon: text(&properties["icon"]),
            owner_id: id(&properties["owner_id"]),
            joined_at: text(&value["joined_at"]).unwrap_or_default(),
            channels: channels.iter().filter_map(Channel::from_value).collect(),
        })
    }

    /// The official server acronym: the first letter of each word.
    pub fn acronym(&self) -> String {
        self.name
            .split_whitespace()
            .filter_map(|word| word.chars().next())
            .collect()
    }

    /// Sidebar order: uncategorized channels, then each category with its children; text before voice inside a group.
    pub fn sidebar_rows(&self) -> Vec<&Channel> {
        let sort_key = |channel: &&Channel| {
            (
                channel.kind == ChannelKind::Voice,
                channel.position,
                channel.id,
            )
        };
        let children = |parent: Option<Id>| {
            let mut rows: Vec<&Channel> = self
                .channels
                .iter()
                .filter(|channel| {
                    channel.parent_id == parent && channel.kind != ChannelKind::Category
                })
                .collect();
            rows.sort_by_key(sort_key);
            rows
        };
        let mut categories: Vec<&Channel> = self
            .channels
            .iter()
            .filter(|channel| channel.kind == ChannelKind::Category)
            .collect();
        categories.sort_by_key(|channel| (channel.position, channel.id));
        let mut rows = children(None);
        for category in categories {
            rows.push(category);
            rows.extend(children(Some(category.id)));
        }
        rows
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Message {
    pub id: Id,
    pub channel_id: Id,
    pub author: User,
    pub content: String,
    pub edited: bool,
    /// The message this one replies to, as the server embedded it.
    pub referenced: Option<Box<Message>>,
    pub timestamp: DateTime<Utc>,
    pub kind: u64,
    pub flags: u64,
    pub webhook_id: Option<Id>,
    pub application_id: Option<Id>,
    pub mentions: usize,
    pub mention_roles: usize,
    pub mention_everyone: bool,
    pub blocked: bool,
    pub ignored: bool,
}

impl Message {
    pub fn from_value(value: &Value) -> Option<Self> {
        Some(Self {
            id: id(&value["id"])?,
            channel_id: id(&value["channel_id"])?,
            author: User::from_value(&value["author"])?,
            content: text(&value["content"]).unwrap_or_default(),
            edited: value["edited_timestamp"].is_string(),
            referenced: Self::from_value(&value["referenced_message"]).map(Box::new),
            timestamp: DateTime::parse_from_rfc3339(value["timestamp"].as_str()?)
                .ok()?
                .with_timezone(&Utc),
            kind: value["type"].as_u64().unwrap_or(0),
            flags: value["flags"].as_u64().unwrap_or(0),
            webhook_id: id(&value["webhook_id"]),
            application_id: id(&value["application_id"]),
            mentions: value["mentions"].as_array().map_or(0, Vec::len),
            mention_roles: value["mention_roles"].as_array().map_or(0, Vec::len),
            mention_everyone: value["mention_everyone"].as_bool().unwrap_or(false),
            blocked: false,
            ignored: false,
        })
    }

    pub fn local_time(&self) -> DateTime<Local> {
        self.timestamp.with_timezone(&Local)
    }
}

/// `RelationshipTypes` from the bundle.
pub const RELATIONSHIP_FRIEND: u64 = 1;

#[derive(Clone, Debug, PartialEq)]
pub struct PrivateChannel {
    pub id: Id,
    pub kind: u64,
    pub recipients: Vec<Id>,
    pub last_message_id: Id,
}

impl PrivateChannel {
    fn from_value(value: &Value) -> Option<Self> {
        Some(Self {
            id: id(&value["id"])?,
            kind: value["type"].as_u64()?,
            recipients: value["recipient_ids"]
                .as_array()
                .map(|ids| ids.iter().filter_map(id).collect())
                .unwrap_or_default(),
            last_message_id: id(&value["last_message_id"]).unwrap_or(0),
        })
    }
}

#[derive(Default)]
pub struct Stores {
    pub users: HashMap<Id, User>,
    /// User id to relationship type.
    pub relationships: HashMap<Id, u64>,
    pub private_channels: Vec<PrivateChannel>,
    pub member_lists: HashMap<Id, member_list::MemberList>,
    /// User id to status, for people this session has presence for.
    pub presences: HashMap<Id, String>,
    pub current_user: Option<User>,
    /// This session's status as READY reports it.
    pub status: String,
    pub guild_order: Vec<Id>,
    pub guilds: Vec<Guild>,
    pub messages: HashMap<Id, Vec<Message>>,
}

impl Stores {
    pub fn dispatch(&mut self, name: &str, data: &Value) {
        match name {
            "READY" => self.on_ready(data),
            "READY_SUPPLEMENTAL" => {
                let friends = data["merged_presences"]["friends"].as_array();
                for presence in friends.into_iter().flatten() {
                    self.set_presence(presence);
                }
            }
            "PRESENCE_UPDATE" => self.set_presence(data),
            "GUILD_MEMBER_LIST_UPDATE" => {
                if let Some(guild) = id(&data["guild_id"]) {
                    self.member_lists.entry(guild).or_default().apply(data);
                }
            }
            "GUILD_CREATE" => {
                if let Some(guild) = Guild::from_value(data) {
                    self.guilds.retain(|known| known.id != guild.id);
                    self.guilds.push(guild);
                    self.sort_guilds();
                }
            }
            "MESSAGE_CREATE" => {
                if let Some(message) = Message::from_value(data) {
                    self.insert_message(message);
                }
            }
            "MESSAGE_UPDATE" => self.update_message(data),
            "MESSAGE_DELETE" => {
                if let (Some(channel), Some(message)) = (id(&data["channel_id"]), id(&data["id"]))
                    && let Some(list) = self.messages.get_mut(&channel)
                {
                    list.retain(|known| known.id != message);
                }
            }
            _ => {}
        }
    }

    fn on_ready(&mut self, data: &Value) {
        self.current_user = User::from_value(&data["user"]);
        self.guild_order = data["user_settings_proto"]
            .as_str()
            .and_then(settings::guild_order)
            .unwrap_or_default();
        self.status = text(&data["sessions"][0]["status"]).unwrap_or_else(|| "online".to_owned());
        for user in data["users"].as_array().into_iter().flatten() {
            if let Some(user) = User::from_value(user) {
                self.users.insert(user.id, user);
            }
        }
        self.relationships = data["relationships"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|entry| Some((id(&entry["id"])?, entry["type"].as_u64()?)))
            .collect();
        self.private_channels = data["private_channels"]
            .as_array()
            .map(|channels| {
                channels
                    .iter()
                    .filter_map(PrivateChannel::from_value)
                    .collect()
            })
            .unwrap_or_default();
        // Most recent conversation first, as the official list is.
        self.private_channels
            .sort_by_key(|channel| std::cmp::Reverse(channel.last_message_id));
        self.guilds = data["guilds"]
            .as_array()
            .map(|guilds| guilds.iter().filter_map(Guild::from_value).collect())
            .unwrap_or_default();
        self.sort_guilds();
    }

    /// The order from the settings proto. Guilds it does not list yet go on top, newest first.
    fn sort_guilds(&mut self) {
        let mut ranked: Vec<(Option<usize>, Guild)> =
            self.guilds.drain(..).map(|guild| (None, guild)).collect();
        for entry in &mut ranked {
            entry.0 = self.guild_order.iter().position(|id| *id == entry.1.id);
        }
        ranked.sort_by(|(a, first), (b, second)| match (a, b) {
            (Some(a), Some(b)) => a.cmp(b),
            (None, Some(_)) => std::cmp::Ordering::Less,
            (Some(_), None) => std::cmp::Ordering::Greater,
            (None, None) => second.joined_at.cmp(&first.joined_at),
        });
        self.guilds = ranked.into_iter().map(|(_, guild)| guild).collect();
    }

    /// MESSAGE_UPDATE may be partial, so only what it carries changes.
    fn update_message(&mut self, data: &Value) {
        let (Some(channel), Some(message)) = (id(&data["channel_id"]), id(&data["id"])) else {
            return;
        };
        let known = self
            .messages
            .get_mut(&channel)
            .and_then(|list| list.iter_mut().find(|known| known.id == message));
        if let (Some(known), Some(content)) = (known, text(&data["content"])) {
            known.content = content;
            known.edited = data["edited_timestamp"].is_string();
        }
    }

    fn set_presence(&mut self, data: &Value) {
        let user = id(&data["user_id"]).or_else(|| id(&data["user"]["id"]));
        if let (Some(user), Some(status)) = (user, text(&data["status"])) {
            self.presences.insert(user, status);
        }
    }

    /// `offline` when nothing says otherwise, as the official presence store answers.
    pub fn status(&self, user: Id) -> &str {
        self.presences.get(&user).map_or("offline", String::as_str)
    }

    pub fn friends(&self) -> Vec<&User> {
        let mut friends: Vec<&User> = self
            .relationships
            .iter()
            .filter(|(_, kind)| **kind == RELATIONSHIP_FRIEND)
            .filter_map(|(user, _)| self.users.get(user))
            .collect();
        friends.sort_by_key(|user| user.display_name().to_lowercase());
        friends
    }

    pub fn online_friends(&self) -> Vec<&User> {
        let mut friends = self.friends();
        friends.retain(|user| self.status(user.id) != "offline");
        friends
    }

    /// The other person in a DM, or nothing for a group.
    pub fn dm_recipient(&self, channel: &PrivateChannel) -> Option<&User> {
        match channel.recipients.as_slice() {
            [only] => self.users.get(only),
            _ => None,
        }
    }

    pub fn insert_message(&mut self, message: Message) {
        let list = self.messages.entry(message.channel_id).or_default();
        match list.binary_search_by_key(&message.id, |known| known.id) {
            Ok(index) => list[index] = message,
            Err(index) => list.insert(index, message),
        }
    }

    /// `GET /channels/:id/messages` returns newest first.
    pub fn load_messages(&mut self, channel: Id, page: &Value) {
        for message in page
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Message::from_value)
        {
            let mut message = message;
            message.channel_id = channel;
            self.insert_message(message);
        }
    }

    pub fn guild(&self, guild: Id) -> Option<&Guild> {
        self.guilds.iter().find(|known| known.id == guild)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn ready() -> Value {
        json!({
            "user": {"id": "10", "username": "me", "global_name": null, "avatar": null, "discriminator": "0"},
            "guilds": [
                {"id": "1", "joined_at": "2026-01-01T00:00:00+00:00", "properties": {"name": "Old Guild", "icon": "abc"}, "channels": []},
                {"id": "2", "joined_at": "2026-06-01T00:00:00+00:00", "properties": {"name": "gravecord fixture", "icon": null}, "channels": [
                    {"id": "20", "type": 4, "name": "Text Channels", "position": 0, "parent_id": null},
                    {"id": "21", "type": 4, "name": "Voice Channels", "position": 1, "parent_id": null},
                    {"id": "22", "type": 2, "name": "General", "position": 0, "parent_id": "21"},
                    {"id": "23", "type": 0, "name": "general", "position": 0, "parent_id": "20"},
                ]},
            ],
        })
    }

    fn message(id: &str, content: &str) -> Value {
        json!({"id": id, "channel_id": "23", "content": content, "timestamp": "2026-11-07T18:58:00.000000+00:00",
               "type": 0, "flags": 0, "author": {"id": "10", "username": "me", "discriminator": "0"}})
    }

    #[test]
    fn ready_fills_user_and_guilds_newest_first() {
        let mut stores = Stores::default();
        stores.dispatch("READY", &ready());
        assert_eq!(stores.current_user.as_ref().unwrap().username, "me");
        assert_eq!(stores.guilds[0].name, "gravecord fixture");
        assert_eq!(stores.guilds[1].icon.as_deref(), Some("abc"));
    }

    fn ready_with_people() -> Value {
        json!({
            "user": {"id": "10", "username": "me", "discriminator": "0"},
            "guilds": [],
            "users": [
                {"id": "20", "username": "zed", "global_name": "Zed", "discriminator": "0"},
                {"id": "21", "username": "amy", "global_name": null, "discriminator": "0"},
                {"id": "22", "username": "blocked", "discriminator": "0"},
            ],
            "relationships": [{"id": "20", "type": 1}, {"id": "21", "type": 1}, {"id": "22", "type": 2}],
            "private_channels": [
                {"id": "30", "type": 1, "recipient_ids": ["20"], "last_message_id": "100"},
                {"id": "31", "type": 1, "recipient_ids": ["21"], "last_message_id": "200"},
                {"id": "32", "type": 3, "recipient_ids": ["20", "21"], "last_message_id": null},
            ],
        })
    }

    #[test]
    fn friends_exclude_other_relationships_and_sort_by_name() {
        let mut stores = Stores::default();
        stores.dispatch("READY", &ready_with_people());
        let names: Vec<&str> = stores
            .friends()
            .iter()
            .map(|user| user.username.as_str())
            .collect();
        assert_eq!(names, ["amy", "zed"]);
    }

    #[test]
    fn private_channels_are_most_recent_first() {
        let mut stores = Stores::default();
        stores.dispatch("READY", &ready_with_people());
        let ids: Vec<Id> = stores
            .private_channels
            .iter()
            .map(|channel| channel.id)
            .collect();
        assert_eq!(ids, [31, 30, 32]);
    }

    #[test]
    fn dm_recipient_is_only_for_one_to_one_channels() {
        let mut stores = Stores::default();
        stores.dispatch("READY", &ready_with_people());
        assert_eq!(
            stores
                .dm_recipient(&stores.private_channels[0])
                .unwrap()
                .username,
            "amy"
        );
        assert_eq!(stores.dm_recipient(&stores.private_channels[2]), None);
    }

    #[test]
    fn presence_comes_from_supplemental_and_updates() {
        let mut stores = Stores::default();
        stores.dispatch("READY", &ready_with_people());
        assert_eq!(stores.status(20), "offline");
        assert!(stores.online_friends().is_empty());
        stores.dispatch(
            "READY_SUPPLEMENTAL",
            &json!({"merged_presences": {"friends": [{"user_id": "20", "status": "dnd"}]}}),
        );
        assert_eq!(stores.status(20), "dnd");
        stores.dispatch(
            "PRESENCE_UPDATE",
            &json!({"user": {"id": "20"}, "status": "idle"}),
        );
        assert_eq!(stores.status(20), "idle");
        assert_eq!(stores.online_friends().len(), 1);
    }

    #[test]
    fn settings_order_wins_and_unlisted_guilds_go_on_top() {
        let mut stores = Stores::default();
        stores.dispatch("READY", &ready());
        stores.guild_order = vec![1, 2];
        stores.sort_guilds();
        assert_eq!(stores.guilds[0].id, 1);
        stores.guild_order = vec![1];
        stores.sort_guilds();
        assert_eq!(stores.guilds[0].id, 2);
    }

    #[test]
    fn sidebar_groups_channels_under_their_category() {
        let mut stores = Stores::default();
        stores.dispatch("READY", &ready());
        let names: Vec<&str> = stores.guilds[0]
            .sidebar_rows()
            .iter()
            .map(|channel| channel.name.as_str())
            .collect();
        assert_eq!(
            names,
            ["Text Channels", "general", "Voice Channels", "General"]
        );
    }

    #[test]
    fn acronym_takes_each_first_letter() {
        let mut stores = Stores::default();
        stores.dispatch("READY", &ready());
        assert_eq!(stores.guilds[0].acronym(), "gf");
    }

    #[test]
    fn messages_stay_sorted_and_unique() {
        let mut stores = Stores::default();
        stores.dispatch("MESSAGE_CREATE", &message("5", "b"));
        stores.dispatch("MESSAGE_CREATE", &message("3", "a"));
        stores.dispatch("MESSAGE_CREATE", &message("5", "edited"));
        let list = &stores.messages[&23];
        assert_eq!(list.iter().map(|m| m.id).collect::<Vec<_>>(), [3, 5]);
        assert_eq!(list[1].content, "edited");
    }

    #[test]
    fn message_update_changes_content_and_marks_it_edited() {
        let mut stores = Stores::default();
        stores.dispatch("MESSAGE_CREATE", &message("5", "before"));
        stores.dispatch(
            "MESSAGE_UPDATE",
            &json!({"id": "5", "channel_id": "23", "content": "after", "edited_timestamp": "2026-11-07T19:00:00+00:00"}),
        );
        assert_eq!(stores.messages[&23][0].content, "after");
        assert!(stores.messages[&23][0].edited);
    }

    #[test]
    fn partial_message_update_changes_nothing() {
        let mut stores = Stores::default();
        stores.dispatch("MESSAGE_CREATE", &message("5", "before"));
        stores.dispatch(
            "MESSAGE_UPDATE",
            &json!({"id": "5", "channel_id": "23", "embeds": []}),
        );
        assert_eq!(stores.messages[&23][0].content, "before");
    }

    #[test]
    fn a_reply_carries_the_message_it_answers() {
        let mut reply = message("6", "the reply");
        reply["type"] = json!(19);
        reply["referenced_message"] = message("5", "the original");
        let parsed = Message::from_value(&reply).unwrap();
        assert_eq!(parsed.referenced.unwrap().content, "the original");
    }

    #[test]
    fn message_delete_removes_it() {
        let mut stores = Stores::default();
        stores.dispatch("MESSAGE_CREATE", &message("5", "b"));
        stores.dispatch("MESSAGE_DELETE", &json!({"id": "5", "channel_id": "23"}));
        assert!(stores.messages[&23].is_empty());
    }

    #[test]
    fn default_avatar_index_follows_the_id_for_pomelo_users() {
        let user = User {
            id: 1 << 22,
            discriminator: "0".into(),
            ..User::default()
        };
        assert_eq!(user.default_avatar_index(), 1);
        let legacy = User {
            id: 0,
            discriminator: "1337".into(),
            ..User::default()
        };
        assert_eq!(legacy.default_avatar_index(), 2);
    }
}
