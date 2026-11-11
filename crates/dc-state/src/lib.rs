//! Flux stores. M0 slice: the parts of UserStore, GuildStore, ChannelStore and MessageStore the first screen reads.
//! Names and getters follow `STORES`; the rest of each store is tracked as open rows in `FEATURES.md`.

pub mod autocomplete;
pub mod calls;
pub mod collectibles;
pub mod components;
pub mod confusables;
pub mod content_inventory;
pub mod content_inventory_manager;
pub mod display_name_styles;
pub mod experiments;
pub mod frecency;
pub mod grouping;
pub mod guild_member_counts;
pub mod guild_settings;
pub mod interactions;
pub mod local_activity;
pub mod member_list;
pub mod members;
pub mod message_kinds;
pub mod message_window;
pub mod notifications;
pub mod permissions;
pub mod polls;
pub mod premium;
pub mod presence;
pub mod private_channels;
pub mod profile_effects;
pub mod quests;
pub mod read_state;
pub mod running_games;
pub mod scheduled_events;
pub mod scheduled_message;
pub mod slowmode;
pub mod stickers;
pub mod unread;

use chrono::{DateTime, Local, Utc};
use dc_proto::settings;
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

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
    /// `primary_guild`: the server tag shown after the name, when the user turned it on.
    pub tag: Option<ServerTag>,
    pub bot: bool,
    pub nameplate: Option<collectibles::Nameplate>,
    pub avatar_decoration: Option<Box<collectibles::AvatarDecoration>>,
    /// `flags` and `public_flags` of the user object; 619019's `hasFlag` reads both.
    pub flags: u64,
    pub public_flags: u64,
    /// `display_name_styles`: the Nitro font, colours and effect of the name (see `display_name_styles`).
    pub display_name_styles: Option<Box<display_name_styles::DisplayNameStyles>>,
}

/// `UserFlags.STAFF` and `UserFlags.SPAMMER` (652215).
pub const USER_FLAG_STAFF: u64 = 1;
pub const USER_FLAG_SPAMMER: u64 = 1 << 20;

/// A user's server tag (`primary_guild` with `identity_enabled`): the guild, its badge hash and the tag text.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ServerTag {
    pub guild_id: Id,
    pub badge: Option<String>,
    pub tag: String,
}

impl ServerTag {
    fn from_value(value: &Value) -> Option<Self> {
        if !value["identity_enabled"].as_bool().unwrap_or(false) {
            return None;
        }
        Some(Self {
            guild_id: id(&value["identity_guild_id"])?,
            badge: text(&value["badge"]),
            tag: text(&value["tag"])?,
        })
    }

    /// `clan-badges/{guild}/{badge}.png?size=16`, as the chip's 14px image loads it.
    pub fn badge_url(&self) -> Option<String> {
        let badge = self.badge.as_ref()?;
        Some(format!(
            "https://cdn.discordapp.com/clan-badges/{}/{badge}.png?size=16",
            self.guild_id
        ))
    }
}

impl User {
    pub fn from_value(value: &Value) -> Option<Self> {
        Some(Self {
            id: id(&value["id"])?,
            username: text(&value["username"])?,
            global_name: text(&value["global_name"]),
            avatar: text(&value["avatar"]),
            discriminator: text(&value["discriminator"]).unwrap_or_else(|| "0".to_owned()),
            tag: ServerTag::from_value(&value["primary_guild"]),
            bot: value["bot"].as_bool().unwrap_or(false),
            nameplate: collectibles::Nameplate::from_value(&value["collectibles"]["nameplate"]),
            avatar_decoration: collectibles::AvatarDecoration::from_value(
                &value["avatar_decoration_data"],
            )
            .map(Box::new),
            flags: value["flags"].as_u64().unwrap_or(0),
            public_flags: value["public_flags"].as_u64().unwrap_or(0),
            display_name_styles: display_name_styles::DisplayNameStyles::from_value(
                &value["display_name_styles"],
            )
            .map(Box::new),
        })
    }

    /// 619019 `hasFlag`: `(flags | publicFlags) & flag === flag`.
    pub fn has_flag(&self, flag: u64) -> bool {
        (self.flags | self.public_flags) & flag == flag
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
    /// The newest message when the channel arrived; `read_states.latest` follows it after that.
    pub last_message_id: Option<Id>,
    /// `permission_overwrites`.
    pub overwrites: Vec<permissions::Overwrite>,
    /// `user_limit`: 0 is no limit.
    pub user_limit: u64,
    pub topic: Option<String>,
    /// Slowmode in seconds (`rate_limit_per_user`).
    pub rate_limit_per_user: u64,
    /// Channel flags (`IS_SPOILER_CHANNEL` 1 << 21 among them).
    pub flags: u64,
    pub nsfw: bool,
    /// Minutes a new thread stays listed without activity (`default_auto_archive_duration`).
    pub default_auto_archive_duration: Option<u64>,
    /// Forum and media channels: the tags a post can carry (`available_tags`).
    pub available_tags: Vec<ForumTag>,
    /// `default_sort_order`: 0 latest activity, 1 creation date.
    pub default_sort_order: Option<u64>,
    /// `default_forum_layout`: 0 not set, 1 list, 2 gallery.
    pub default_forum_layout: u64,
}

/// One entry of a forum's `available_tags`.
#[derive(Clone, Debug, PartialEq)]
pub struct ForumTag {
    pub id: Id,
    pub name: String,
    pub moderated: bool,
    /// A unicode emoji, or the custom emoji's id.
    pub emoji_name: Option<String>,
    pub emoji_id: Option<Id>,
}

impl ForumTag {
    pub fn list(value: &Value) -> Vec<Self> {
        value
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|tag| {
                Some(Self {
                    id: id(&tag["id"])?,
                    name: text(&tag["name"]).unwrap_or_default(),
                    moderated: tag["moderated"].as_bool().unwrap_or(false),
                    emoji_name: text(&tag["emoji_name"]),
                    emoji_id: id(&tag["emoji_id"]),
                })
            })
            .collect()
    }
}

impl Channel {
    /// GUILD_FORUM (15) or GUILD_MEDIA (16).
    pub fn is_forum(&self) -> bool {
        matches!(self.kind, ChannelKind::Other(15 | 16))
    }

    pub fn is_media(&self) -> bool {
        self.kind == ChannelKind::Other(16)
    }

    fn from_value(value: &Value) -> Option<Self> {
        Some(Self {
            id: id(&value["id"])?,
            name: text(&value["name"]).unwrap_or_default(),
            kind: ChannelKind::from_u64(value["type"].as_u64()?),
            position: value["position"].as_i64().unwrap_or(0),
            parent_id: id(&value["parent_id"]),
            last_message_id: id(&value["last_message_id"]),
            overwrites: permissions::Overwrite::list(&value["permission_overwrites"]),
            user_limit: value["user_limit"].as_u64().unwrap_or(0),
            topic: text(&value["topic"]),
            rate_limit_per_user: value["rate_limit_per_user"].as_u64().unwrap_or(0),
            flags: value["flags"].as_u64().unwrap_or(0),
            default_auto_archive_duration: value["default_auto_archive_duration"].as_u64(),
            nsfw: value["nsfw"].as_bool().unwrap_or(false),
            available_tags: ForumTag::list(&value["available_tags"]),
            default_sort_order: value["default_sort_order"].as_u64(),
            default_forum_layout: value["default_forum_layout"].as_u64().unwrap_or(0),
        })
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Guild {
    pub id: Id,
    pub name: String,
    pub icon: Option<String>,
    pub owner_id: Option<Id>,
    pub home_header: Option<String>,
    pub rules_channel_id: Option<Id>,
    pub joined_at: String,
    pub default_message_notifications: u64,
    pub nsfw_level: u64,
    pub channels: Vec<Channel>,
    pub roles: HashMap<Id, String>,
    /// Role id to its permission bits (the @everyone role has the guild's id).
    pub role_permissions: HashMap<Id, u64>,
    /// What a role's mention and list order read: position, colour and whether anyone may mention it.
    pub role_info: HashMap<Id, RoleInfo>,
    /// Roles that carry an icon or an emoji (`icon`, `unicode_emoji`), which the message header draws beside the name.
    pub role_icons: HashMap<Id, RoleIcon>,
    /// The current user's roles here (READY `merged_members`, then GUILD_MEMBER_UPDATE).
    pub my_roles: Vec<Id>,
    /// The current user has not passed the rules screening yet (`pending` on their member).
    pub my_pending: bool,
    pub my_flags: u64,
    /// Their timeout's end in epoch milliseconds (`communication_disabled_until`).
    pub my_timeout_until: Option<i64>,
    /// 1 when moderation needs 2FA (`mfa_level` ELEVATED).
    pub mfa_level: u64,
    pub vanity_url_code: Option<String>,
    pub features: Vec<String>,
    /// `incidents_data`: invites and DMs paused until (ISO times) and the last lockdown length in hours.
    pub invites_disabled_until: Option<String>,
    pub dms_disabled_until: Option<String>,
    pub lockdown_duration_hours: Option<u64>,
    pub premium_tier: u64,
    pub premium_subscription_count: u64,
    pub emojis: Vec<GuildEmoji>,
    /// Active threads this session knows (READY's `threads`, then THREAD_* events).
    pub threads: Vec<Thread>,
}

/// A thread channel (types 10, 11, 12): `ThreadsStore` keeps the guild's active ones by parent.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Thread {
    pub id: Id,
    pub parent_id: Id,
    pub name: String,
    pub kind: u64,
    pub owner_id: Option<Id>,
    pub message_count: u64,
    pub last_message_id: Option<Id>,
    pub archived: bool,
    /// The current user is a member (`member` on READY threads, or THREAD_MEMBER_UPDATE).
    pub joined: bool,
    /// Forum posts: the tags the post carries (`applied_tags`), its flags (PINNED is 1 << 1) and the messages ever
    /// sent (`total_message_sent`).
    pub applied_tags: Vec<Id>,
    pub flags: u64,
    pub total_message_sent: u64,
    pub locked: bool,
    /// The current user's thread member flags (`member.flags`: 2 all messages, 4 only mentions, 8 nothing). `None`
    /// when no member data arrived: JoinedThreadsStore `flags()` is then null.
    pub member_flags: Option<u64>,
    /// `member.muted` and `member.mute_config.end_time`.
    pub muted: bool,
    pub mute_end: Option<DateTime<Utc>>,
    pub join_timestamp: Option<String>,
}

/// `ThreadMemberFlags` (37411).
pub const THREAD_ALL_MESSAGES: u64 = 2;
pub const THREAD_ONLY_MENTIONS: u64 = 4;
pub const THREAD_NO_MESSAGES: u64 = 8;

impl Thread {
    /// A thread member object (READY's `member`, THREAD_MEMBER_UPDATE, one of THREAD_LIST_SYNC's `members`).
    pub fn apply_member(&mut self, member: &Value) {
        self.joined = true;
        self.member_flags = member["flags"].as_u64();
        self.muted = member["muted"].as_bool().unwrap_or(false);
        self.mute_end = member["mute_config"]["end_time"]
            .as_str()
            .and_then(|time| DateTime::parse_from_rfc3339(time).ok())
            .map(|time| time.with_timezone(&Utc));
        self.join_timestamp = text(&member["join_timestamp"]);
    }

    /// The account left the thread (THREAD_MEMBERS_UPDATE `removed_member_ids`).
    pub fn clear_member(&mut self) {
        self.joined = false;
        self.member_flags = None;
        self.muted = false;
        self.mute_end = None;
        self.join_timestamp = None;
    }

    /// 152007 `isMuted`: muted, and the mute's end (when there is one) still ahead (544743 `setTimer`).
    pub fn is_muted(&self, now: DateTime<Utc>) -> bool {
        self.joined && self.muted && self.mute_end.is_none_or(|end| end > now)
    }

    /// A pinned forum post (`ChannelFlags.PINNED`).
    pub fn pinned(&self) -> bool {
        self.flags & 2 != 0
    }

    pub fn from_value(value: &Value) -> Option<Self> {
        let kind = value["type"].as_u64()?;
        if !matches!(kind, 10..=12) {
            return None;
        }
        let mut thread = Self {
            id: id(&value["id"])?,
            parent_id: id(&value["parent_id"])?,
            name: text(&value["name"]).unwrap_or_default(),
            kind,
            owner_id: id(&value["owner_id"]),
            message_count: value["message_count"].as_u64().unwrap_or(0),
            last_message_id: id(&value["last_message_id"]),
            archived: value["thread_metadata"]["archived"]
                .as_bool()
                .unwrap_or(false),
            joined: value["member"].is_object()
                || value["newly_created"].as_bool().unwrap_or(false),
            applied_tags: value["applied_tags"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(id)
                .collect(),
            flags: value["flags"].as_u64().unwrap_or(0),
            total_message_sent: value["total_message_sent"].as_u64().unwrap_or(0),
            locked: value["thread_metadata"]["locked"]
                .as_bool()
                .unwrap_or(false),
            ..Self::default()
        };
        if value["member"].is_object() {
            thread.apply_member(&value["member"]);
        }
        Some(thread)
    }
}

/// A guild's custom emoji (`emojis` in GUILD_CREATE and `GUILD_EMOJIS_UPDATE`).
#[derive(Clone, Debug, PartialEq)]
pub struct GuildEmoji {
    pub id: Id,
    pub name: String,
    pub animated: bool,
    /// False when the guild lost the boost level that allowed it.
    pub available: bool,
}

impl GuildEmoji {
    fn list(value: &Value) -> Vec<Self> {
        value
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|emoji| {
                Some(Self {
                    id: id(&emoji["id"])?,
                    name: text(&emoji["name"])?,
                    animated: emoji["animated"].as_bool().unwrap_or(false),
                    available: emoji["available"].as_bool().unwrap_or(true),
                })
            })
            .collect()
    }
}

/// A role's picture: the uploaded icon's hash or a unicode emoji (module 549587's `roleIconProps`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RoleIcon {
    pub icon: Option<String>,
    pub unicode_emoji: Option<String>,
    /// The role belongs to a server subscription listing (`tags.subscription_listing_id`): its icon shows even
    /// without the server feature `ROLE_ICONS` (module 376304 `fm`).
    pub subscription: bool,
}

impl RoleIcon {
    fn from_value(role: &Value) -> Option<Self> {
        let icon = text(&role["icon"]).filter(|hash| !hash.is_empty());
        let unicode_emoji = text(&role["unicode_emoji"]).filter(|emoji| !emoji.is_empty());
        (icon.is_some() || unicode_emoji.is_some()).then_some(Self {
            icon,
            unicode_emoji,
            subscription: role["tags"].get("subscription_listing_id").is_some(),
        })
    }

    fn list(roles: &Value) -> HashMap<Id, Self> {
        roles
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|role| Some((id(&role["id"])?, Self::from_value(role)?)))
            .collect()
    }

    /// `roles/{id}/{icon}.webp?size=` on the CDN (`getRoleIconData`).
    pub fn icon_url(&self, role: Id, size: u32) -> Option<String> {
        self.icon.as_ref().map(|hash| {
            format!("https://cdn.discordapp.com/role-icons/{role}/{hash}.webp?size={size}&quality=lossless")
        })
    }
}

/// The parts of a role the composer's mention list and the member list read.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RoleInfo {
    pub position: i64,
    /// The role's colour as `0xRRGGBB`; 0 is none.
    pub color: u32,
    pub mentionable: bool,
    pub guild_connection: bool,
    /// An integration's or bot's role (`managed`): never given or taken from a person by hand (module 180798).
    pub managed: bool,
}

impl RoleInfo {
    fn from_value(role: &Value) -> Self {
        Self {
            position: role["position"].as_i64().unwrap_or(0),
            color: role["color"].as_u64().unwrap_or(0) as u32,
            mentionable: role["mentionable"].as_bool().unwrap_or(false),
            guild_connection: role["tags"].get("guild_connections").is_some(),
            managed: role["managed"].as_bool().unwrap_or(false),
        }
    }

    fn list(roles: &Value) -> HashMap<Id, Self> {
        roles
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|role| Some((id(&role["id"])?, Self::from_value(role))))
            .collect()
    }
}

/// `_` in module 426983.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Badge {
    Partnered,
    Verified,
    VerifiedAndPartnered,
    Community,
    Discoverable,
    Staff,
    None,
}

impl Guild {
    /// GuildMemberStore's color role (696451, `Y`), ranked by 7864's role comparator.
    pub fn role_color(&self, roles: &[Id]) -> Option<u32> {
        roles
            .iter()
            .filter_map(|id| self.role_info.get(id).map(|role| (*id, role)))
            .filter(|(_, role)| role.color != 0)
            .min_by_key(|(id, role)| (*id == self.id, std::cmp::Reverse(role.position), *id))
            .map(|(_, role)| role.color)
    }

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
            home_header: text(&properties["home_header"]),
            rules_channel_id: id(&properties["rules_channel_id"]),
            default_message_notifications: properties["default_message_notifications"]
                .as_u64()
                .unwrap_or(0),
            nsfw_level: properties["nsfw_level"].as_u64().unwrap_or(0),
            joined_at: text(&value["joined_at"]).unwrap_or_default(),
            channels: channels.iter().filter_map(Channel::from_value).collect(),
            roles: value["roles"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|role| Some((id(&role["id"])?, text(&role["name"])?)))
                .collect(),
            role_permissions: value["roles"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|role| {
                    Some((id(&role["id"])?, permissions::bits(&role["permissions"])))
                })
                .collect(),
            role_info: RoleInfo::list(&value["roles"]),
            role_icons: RoleIcon::list(&value["roles"]),
            my_roles: Vec::new(),
            my_pending: false,
            my_flags: 0,
            my_timeout_until: None,
            mfa_level: properties["mfa_level"].as_u64().unwrap_or(0),
            vanity_url_code: text(&properties["vanity_url_code"]),
            features: properties["features"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(text)
                .collect(),
            threads: value["threads"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Thread::from_value)
                .collect(),
            invites_disabled_until: text(&properties["incidents_data"]["invites_disabled_until"]),
            dms_disabled_until: text(&properties["incidents_data"]["dms_disabled_until"]),
            lockdown_duration_hours: properties["incidents_data"]["lockdown_duration_hours"]
                .as_u64(),
            premium_tier: properties["premium_tier"].as_u64().unwrap_or(0),
            premium_subscription_count: value["premium_subscription_count"]
                .as_u64()
                .or(properties["premium_subscription_count"].as_u64())
                .unwrap_or(0),
            emojis: GuildEmoji::list(&value["emojis"]),
        })
    }

    /// `GUILD_UPDATE`: the guild's own properties, without its channels or the current user's member.
    fn apply_update(&mut self, value: &Value) {
        if let Some(name) = text(&value["name"]) {
            self.name = name;
        }
        self.icon = text(&value["icon"]);
        self.owner_id = id(&value["owner_id"]).or(self.owner_id);
        if value.get("home_header").is_some() {
            self.home_header = text(&value["home_header"]);
        }
        if value.get("rules_channel_id").is_some() {
            self.rules_channel_id = id(&value["rules_channel_id"]);
        }
        self.mfa_level = value["mfa_level"].as_u64().unwrap_or(self.mfa_level);
        self.default_message_notifications = value["default_message_notifications"]
            .as_u64()
            .unwrap_or(self.default_message_notifications);
        self.nsfw_level = value["nsfw_level"].as_u64().unwrap_or(self.nsfw_level);
        self.vanity_url_code = text(&value["vanity_url_code"]);
        if let Some(features) = value["features"].as_array() {
            self.features = features.iter().filter_map(text).collect();
        }
        self.invites_disabled_until = text(&value["incidents_data"]["invites_disabled_until"]);
        self.dms_disabled_until = text(&value["incidents_data"]["dms_disabled_until"]);
        self.lockdown_duration_hours = value["incidents_data"]["lockdown_duration_hours"].as_u64();
        self.premium_tier = value["premium_tier"].as_u64().unwrap_or(self.premium_tier);
        self.premium_subscription_count = value["premium_subscription_count"]
            .as_u64()
            .unwrap_or(self.premium_subscription_count);
        if let Some(roles) = value["roles"].as_array() {
            self.roles = roles
                .iter()
                .filter_map(|role| Some((id(&role["id"])?, text(&role["name"])?)))
                .collect();
            self.role_permissions = roles
                .iter()
                .filter_map(|role| {
                    Some((id(&role["id"])?, permissions::bits(&role["permissions"])))
                })
                .collect();
            self.role_info = RoleInfo::list(&value["roles"]);
            self.role_icons = RoleIcon::list(&value["roles"]);
        }
        if value["emojis"].is_array() {
            self.emojis = GuildEmoji::list(&value["emojis"]);
        }
    }

    /// The current user's member object: roles, screening and timeout.
    pub fn set_my_member(&mut self, member: &Value) {
        if let Some(flags) = member["flags"].as_u64() {
            self.my_flags = flags;
        }
        if let Some(joined) = member["joined_at"].as_str() {
            self.joined_at = joined.to_owned();
        }
        self.my_roles = member["roles"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(id)
            .collect();
        self.my_pending = member["pending"].as_bool().unwrap_or(false);
        self.my_timeout_until = member["communication_disabled_until"]
            .as_str()
            .and_then(|until| chrono::DateTime::parse_from_rfc3339(until).ok())
            .map(|until| until.timestamp_millis());
    }

    /// The join date as a snowflake, so it compares with message ids.
    pub fn joined_snowflake(&self) -> Id {
        chrono::DateTime::parse_from_rfc3339(&self.joined_at)
            .map(|time| read_state::snowflake_at(time.timestamp_millis().max(0) as u64))
            .unwrap_or(0)
    }

    pub fn has_emoji(&self, emoji: Id) -> bool {
        self.emojis.iter().any(|known| known.id == emoji)
    }

    fn has_feature(&self, feature: &str) -> bool {
        self.features.iter().any(|known| known == feature)
    }

    /// `K` in module 426983 over the traits from 176128.
    pub fn badge(&self) -> Badge {
        let verified = self.has_feature("VERIFIED");
        let partnered = self.has_feature("PARTNERED");
        let community = self.has_feature("COMMUNITY");
        match (verified, partnered) {
            _ if self.has_feature("INTERNAL_EMPLOYEE_ONLY") => Badge::Staff,
            (true, true) => Badge::VerifiedAndPartnered,
            (true, false) => Badge::Verified,
            (false, true) => Badge::Partnered,
            _ if community && self.has_feature("DISCOVERABLE") => Badge::Discoverable,
            _ if community => Badge::Community,
            _ => Badge::None,
        }
    }

    pub fn premium(&self) -> bool {
        self.premium_subscription_count > 0 || self.premium_tier > 0
    }

    /// The official server acronym: the first letter of each word.
    pub fn acronym(&self) -> String {
        Self::acronym_of(&self.name)
    }

    /// The acronym of any server name (an invite's guild need not be joined).
    pub fn acronym_of(name: &str) -> String {
        name.split_whitespace()
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
pub struct Reaction {
    /// The unicode emoji, or a custom emoji's name.
    pub name: String,
    pub emoji_id: Option<Id>,
    pub count: u64,
    pub me: bool,
}

impl Reaction {
    fn from_value(value: &Value) -> Option<Self> {
        Some(Self {
            name: text(&value["emoji"]["name"])?,
            emoji_id: id(&value["emoji"]["id"]),
            count: value["count"].as_u64().unwrap_or(1),
            me: value["me"].as_bool().unwrap_or(false),
        })
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Attachment {
    pub id: Id,
    pub filename: String,
    /// The CDN address, for downloads and text previews.
    pub url: String,
    pub proxy_url: String,
    pub content_type: Option<String>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub size: u64,
    pub flags: u64,
    /// The image description (alt text) the sender wrote.
    pub description: Option<String>,
    /// `duration_secs`: the length of a voice message, in seconds.
    pub duration_secs: Option<f64>,
    /// `waveform`: the base64 bytes a voice message's player draws its bars from.
    pub waveform: Option<String>,
}

impl Attachment {
    fn from_value(value: &Value) -> Option<Self> {
        Some(Self {
            id: id(&value["id"])?,
            filename: text(&value["filename"])?,
            url: text(&value["url"]).unwrap_or_default(),
            proxy_url: text(&value["proxy_url"]).or_else(|| text(&value["url"]))?,
            content_type: text(&value["content_type"]),
            width: value["width"].as_u64().map(|width| width as u32),
            height: value["height"].as_u64().map(|height| height as u32),
            size: value["size"].as_u64().unwrap_or(0),
            flags: value["flags"].as_u64().unwrap_or(0),
            description: text(&value["description"]).filter(|text| !text.is_empty()),
            duration_secs: value["duration_secs"].as_f64(),
            waveform: text(&value["waveform"]),
        })
    }

    pub fn is_image(&self) -> bool {
        self.width.is_some()
            && self.height.is_some()
            && self
                .content_type
                .as_deref()
                .is_some_and(|kind| kind.starts_with("image/"))
    }

    /// LazyImage (619517) and attachment flags (652215): GIF, or flagged WebP/AVIF.
    pub fn is_animated(&self) -> bool {
        let extension = self.filename.rsplit('.').next().unwrap_or("");
        extension.eq_ignore_ascii_case("gif")
            || self.flags & 32 != 0
                && (extension.eq_ignore_ascii_case("webp")
                    || extension.eq_ignore_ascii_case("avif"))
    }
}

/// Fits media into the message column without enlarging it. Official limits for a single image.
pub fn fit_media(width: u32, height: u32) -> (u32, u32) {
    const MAX_WIDTH: f64 = 550.;
    const MAX_HEIGHT: f64 = 350.;
    let (width, height) = (f64::from(width), f64::from(height));
    let scale = (MAX_WIDTH / width).min(MAX_HEIGHT / height).min(1.);
    (
        (width * scale).round() as u32,
        (height * scale).round() as u32,
    )
}

/// An image, thumbnail or video of an embed, as `O` in module 659674 keeps it.
#[derive(Clone, Debug, PartialEq)]
pub struct EmbedMedia {
    pub url: String,
    pub proxy_url: Option<String>,
    pub width: u32,
    pub height: u32,
}

impl EmbedMedia {
    /// `C` in module 659674: media without both sides is dropped.
    fn from_value(value: &Value) -> Option<Self> {
        let width = value["width"].as_u64().filter(|width| *width > 0)? as u32;
        let height = value["height"].as_u64().filter(|height| *height > 0)? as u32;
        Some(Self {
            url: text(&value["url"])?,
            proxy_url: text(&value["proxy_url"]),
            width,
            height,
        })
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct EmbedField {
    pub name: String,
    pub value: String,
    pub inline: bool,
}

/// A message embed after `fK` in module 659674.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Embed {
    pub kind: String,
    pub url: Option<String>,
    pub title: Option<String>,
    pub description: Option<String>,
    pub color: Option<u32>,
    pub provider: Option<String>,
    pub author: Option<String>,
    pub author_url: Option<String>,
    pub author_icon: Option<String>,
    pub footer: Option<String>,
    pub footer_icon: Option<String>,
    pub image: Option<EmbedMedia>,
    pub thumbnail: Option<EmbedMedia>,
    pub video: Option<EmbedMedia>,
    pub fields: Vec<EmbedField>,
}

impl Embed {
    pub fn from_value(value: &Value) -> Self {
        let kind = text(&value["type"]).unwrap_or_else(|| "rich".to_owned());
        let mut embed = Self {
            url: text(&value["url"]),
            title: text(&value["title"]),
            description: text(&value["description"]),
            color: value["color"].as_u64().map(|color| color as u32),
            provider: text(&value["provider"]["name"]),
            author: text(&value["author"]["name"]),
            author_url: text(&value["author"]["url"]),
            author_icon: text(&value["author"]["proxy_icon_url"])
                .or_else(|| text(&value["author"]["icon_url"])),
            footer: text(&value["footer"]["text"]),
            footer_icon: text(&value["footer"]["proxy_icon_url"])
                .or_else(|| text(&value["footer"]["icon_url"])),
            video: EmbedMedia::from_value(&value["video"]),
            ..Self::default()
        };
        // GIFV keeps its thumbnail as the poster while its separate video is loading.
        if let Some(thumbnail) = EmbedMedia::from_value(&value["thumbnail"]) {
            match kind.as_str() {
                "article" | "image" | "gifv" => embed.image = Some(thumbnail),
                _ => embed.thumbnail = Some(thumbnail),
            }
        }
        if let Some(image) = EmbedMedia::from_value(&value["image"]) {
            embed.image = Some(image);
        }
        // Only rich, auto-moderation and poll-result embeds carry fields (`u.pd`; the poll result row reads `poll_question_text` and the victor fields from them, module 117096).
        if matches!(
            kind.as_str(),
            "rich" | "auto_moderation_message" | "poll_result"
        ) {
            embed.fields = value["fields"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|field| {
                    Some(EmbedField {
                        name: text(&field["name"])?,
                        value: text(&field["value"])?,
                        inline: field["inline"].as_bool().unwrap_or(false),
                    })
                })
                .collect();
        }
        embed.kind = kind;
        embed
    }

    /// `ds` in module 659674 for embeds that are not TikTok or portrait YouTube.
    pub const MAX_MEDIA: (f64, f64) = (400., 300.);

    /// The image fitted into `MAX_MEDIA`, never enlarged.
    pub fn fitted(media: &EmbedMedia) -> (f64, f64) {
        let (max_width, max_height) = Self::MAX_MEDIA;
        let (width, height) = (f64::from(media.width), f64::from(media.height));
        let scale = (max_width / width).min(max_height / height).min(1.);
        (width * scale, height * scale)
    }

    /// `gifv` and `image` embeds whose URL is the whole message draw as bare media and hide the link text
    /// (`inlineMediaEmbed_XyW1ZT` in `design/dom/channel/gifv-darker-cozy.json`).
    pub fn is_inline_media(&self) -> bool {
        matches!(self.kind.as_str(), "gifv" | "image") && self.image.is_some()
    }

    /// `getMaxWidth`: an embed whose media is at least 300 wide is that wide plus 32.
    pub fn max_width(&self) -> Option<f64> {
        let (width, _) = Self::fitted(self.image.as_ref()?);
        (self.kind == "video" || width >= 300.).then_some(width + 32.)
    }
}

fn edited_at(value: &Value) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(value.as_str()?)
        .ok()
        .map(|time| time.with_timezone(&Utc))
}

#[derive(Clone, Debug, PartialEq)]
pub struct Message {
    pub id: Id,
    pub channel_id: Id,
    pub author: User,
    pub content: String,
    pub edited: bool,
    /// When it was last edited (`edited_timestamp`), for the tooltip of the "(edited)" mark.
    pub edited_at: Option<DateTime<Utc>>,
    /// The message this one replies to, as the server embedded it.
    pub referenced: Option<Box<Message>>,
    pub attachments: Vec<Attachment>,
    pub embeds: Vec<Embed>,
    pub reactions: Vec<Reaction>,
    pub timestamp: DateTime<Utc>,
    pub kind: u64,
    pub flags: u64,
    pub webhook_id: Option<Id>,
    pub application_id: Option<Id>,
    pub mentions: usize,
    pub mention_roles: usize,
    pub mention_everyone: bool,
    /// The ids in `mentions` and `mention_roles`, for the "mentions me" rule of the message row.
    pub mention_user_ids: Vec<Id>,
    pub mention_role_ids: Vec<Id>,
    pub blocked: bool,
    pub ignored: bool,
    /// A forwarded message's copy (`message_snapshots[0].message`) and where it came from.
    pub forwarded: Option<Box<Forwarded>>,
    pub pinned: bool,
    /// `message_reference.message_id`: what a reply, a forward or a pin notice points at.
    pub reference_id: Option<Id>,
    /// `message_reference.channel_id`: for THREAD_CREATED, the thread.
    pub reference_channel_id: Option<Id>,
    /// `sticker_items` (else `stickers`): the stickers sent with the message, as (id, name, `format_type`).
    pub sticker_items: Vec<(Id, String, u64)>,
    /// Data only some message types carry: group targets, calls, polls, purchases (see `message_kinds`).
    pub extra: message_kinds::MessageExtra,
}

/// What a forward carries: the original's content, files and embeds, and its `message_reference`.
#[derive(Clone, Debug, PartialEq)]
pub struct Forwarded {
    pub content: String,
    pub timestamp: DateTime<Utc>,
    pub attachments: Vec<Attachment>,
    pub embeds: Vec<Embed>,
    pub guild_id: Option<Id>,
    pub channel_id: Option<Id>,
    pub message_id: Option<Id>,
}

impl Forwarded {
    fn from_value(message: &Value) -> Option<Self> {
        // Only FORWARD references (`type: 1`) carry snapshots.
        if message["message_reference"]["type"].as_u64() != Some(1) {
            return None;
        }
        let snapshot = &message["message_snapshots"][0]["message"];
        if !snapshot.is_object() {
            return None;
        }
        Some(Self {
            content: text(&snapshot["content"]).unwrap_or_default(),
            timestamp: snapshot["timestamp"]
                .as_str()
                .and_then(|time| DateTime::parse_from_rfc3339(time).ok())
                .map(|time| time.with_timezone(&Utc))
                .unwrap_or_default(),
            attachments: snapshot["attachments"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Attachment::from_value)
                .collect(),
            embeds: snapshot["embeds"]
                .as_array()
                .into_iter()
                .flatten()
                .map(Embed::from_value)
                .collect(),
            guild_id: id(&message["message_reference"]["guild_id"]),
            channel_id: id(&message["message_reference"]["channel_id"]),
            message_id: id(&message["message_reference"]["message_id"]),
        })
    }

    pub fn local_time(&self) -> DateTime<Local> {
        self.timestamp.with_timezone(&Local)
    }
}

/// `IS_VOICE_MESSAGE` in the message flags (module 652215, `pr7`).
pub const VOICE_MESSAGE_FLAG: u64 = 1 << 13;

impl Message {
    /// A voice message: the message flag is set (`hasFlag(IS_VOICE_MESSAGE)`), the player is drawn for its attachment.
    pub fn is_voice_message(&self) -> bool {
        self.flags & VOICE_MESSAGE_FLAG != 0
    }

    /// Module 451919 `s`: `@everyone` or `@here` unless suppressed, or the account among the mentioned users, or (with
    /// roles not suppressed) one of the account's roles among the mentioned roles.
    pub fn mentions_me(
        &self,
        me: Id,
        my_roles: &[Id],
        suppress_everyone: bool,
        suppress_roles: bool,
    ) -> bool {
        if (self.mention_everyone && !suppress_everyone) || self.mention_user_ids.contains(&me) {
            return true;
        }
        !suppress_roles
            && self
                .mention_role_ids
                .iter()
                .any(|role| my_roles.contains(role))
    }

    /// The content is only the link its inline media embed shows, so the text is not drawn.
    pub fn content_is_inline_media(&self) -> bool {
        let content = self.content.trim();
        !content.is_empty()
            && !content.contains(char::is_whitespace)
            && self
                .embeds
                .iter()
                .any(|embed| embed.is_inline_media() && embed.url.as_deref() == Some(content))
    }

    pub fn from_value(value: &Value) -> Option<Self> {
        Some(Self {
            id: id(&value["id"])?,
            channel_id: id(&value["channel_id"])?,
            author: User::from_value(&value["author"])?,
            content: text(&value["content"]).unwrap_or_default(),
            edited: value["edited_timestamp"].is_string(),
            edited_at: edited_at(&value["edited_timestamp"]),
            referenced: Self::from_value(&value["referenced_message"]).map(Box::new),
            attachments: value["attachments"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Attachment::from_value)
                .collect(),
            embeds: value["embeds"]
                .as_array()
                .into_iter()
                .flatten()
                .map(Embed::from_value)
                .collect(),
            reactions: value["reactions"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Reaction::from_value)
                .collect(),
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
            mention_user_ids: value["mentions"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|user| id(&user["id"]))
                .collect(),
            mention_role_ids: value["mention_roles"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(id)
                .collect(),
            blocked: false,
            ignored: false,
            forwarded: Forwarded::from_value(value).map(Box::new),
            pinned: value["pinned"].as_bool().unwrap_or(false),
            reference_id: id(&value["message_reference"]["message_id"]),
            reference_channel_id: id(&value["message_reference"]["channel_id"]),
            extra: message_kinds::MessageExtra::from_value(value),
            sticker_items: value["sticker_items"]
                .as_array()
                .or_else(|| value["stickers"].as_array())
                .into_iter()
                .flatten()
                .filter_map(|sticker| {
                    Some((
                        id(&sticker["id"])?,
                        text(&sticker["name"])?,
                        sticker["format_type"].as_u64()?,
                    ))
                })
                .collect(),
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
    pub name: Option<String>,
    pub icon: Option<String>,
    pub application_id: Option<Id>,
    pub flags: u64,
    pub message_request: bool,
    pub spam: bool,
    pub request_timestamp: Option<DateTime<Utc>>,
}

impl PrivateChannel {
    fn from_value(value: &Value) -> Option<Self> {
        let mut channel = Self {
            id: id(&value["id"])?,
            kind: value["type"].as_u64()?,
            recipients: Vec::new(),
            last_message_id: 0,
            name: None,
            icon: None,
            application_id: None,
            flags: 0,
            message_request: false,
            spam: false,
            request_timestamp: None,
        };
        channel.update(value);
        Some(channel)
    }

    pub fn is_request(&self) -> bool {
        self.message_request || self.spam
    }

    fn update(&mut self, value: &Value) {
        if let Some(kind) = value["type"].as_u64() {
            self.kind = kind;
        }
        // READY lists ids; channel events and REST answers carry user objects.
        if let Some(ids) = value["recipient_ids"].as_array() {
            self.recipients = ids.iter().filter_map(id).collect();
        } else if let Some(users) = value["recipients"].as_array() {
            self.recipients = users.iter().filter_map(|user| id(&user["id"])).collect();
        }
        if value.get("last_message_id").is_some() {
            self.last_message_id = id(&value["last_message_id"]).unwrap_or(0);
        }
        if value.get("name").is_some() {
            self.name = text(&value["name"]);
        }
        if value.get("icon").is_some() {
            self.icon = text(&value["icon"]);
        }
        if value.get("application_id").is_some() {
            self.application_id = id(&value["application_id"]);
        }
        if let Some(flags) = value.get("flags") {
            self.flags = flags.as_u64().unwrap_or(0);
        }
        if let Some(request) = value["is_message_request"].as_bool() {
            self.message_request = request;
        }
        if let Some(spam) = value["is_spam"].as_bool() {
            self.spam = spam;
        }
        if value.get("is_message_request_timestamp").is_some() {
            self.request_timestamp = value["is_message_request_timestamp"]
                .as_str()
                .and_then(|stamp| DateTime::parse_from_rfc3339(stamp).ok())
                .map(|stamp| stamp.with_timezone(&Utc));
        }
    }
}

/// `T` in module 741961: how long a typing notice lasts.
pub const TYPING_DURATION: Duration = Duration::from_secs(10);

/// The private half of the READY user, which only the account settings show.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Account {
    pub nsfw_allowed: bool,
    pub email: Option<String>,
    pub phone: Option<String>,
    pub mfa_enabled: bool,
    pub verified: bool,
    /// `premium_type`: 1 Classic, 2 Nitro, 3 Basic; none without a subscription.
    pub premium_type: Option<u64>,
}

impl Account {
    fn from_value(value: &Value) -> Self {
        Self {
            nsfw_allowed: value["nsfw_allowed"].as_bool().unwrap_or(false),
            email: text(&value["email"]),
            phone: text(&value["phone"]),
            mfa_enabled: value["mfa_enabled"].as_bool().unwrap_or(false),
            verified: value["verified"].as_bool().unwrap_or(false),
            premium_type: value["premium_type"].as_u64().filter(|kind| *kind > 0),
        }
    }
}

/// One of the account's sessions, as READY's `sessions` and `SESSIONS_REPLACE` list them.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Session {
    pub session_id: String,
    pub active: bool,
    pub activities: Vec<Value>,
}

impl Session {
    fn list(value: &Value) -> Vec<Self> {
        value
            .as_array()
            .into_iter()
            .flatten()
            .map(|session| Self {
                session_id: text(&session["session_id"]).unwrap_or_default(),
                active: session["active"].as_bool().unwrap_or(false),
                activities: session["activities"]
                    .as_array()
                    .cloned()
                    .unwrap_or_default(),
            })
            .collect()
    }
}

/// One member's place in voice, from READY's `voice_states` and `VOICE_STATE_UPDATE`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct VoiceState {
    pub user_id: Id,
    pub channel_id: Id,
    pub session_id: String,
    pub self_mute: bool,
    pub self_deaf: bool,
    pub mute: bool,
    pub deaf: bool,
    /// Held back from speaking in a stage.
    pub suppress: bool,
    /// The camera is on (`self_video`).
    pub self_video: bool,
    /// Going live (`self_stream`).
    pub self_stream: bool,
}

impl VoiceState {
    fn from_value(value: &Value) -> Option<Self> {
        Some(Self {
            user_id: id(&value["user_id"])?,
            channel_id: id(&value["channel_id"])?,
            session_id: text(&value["session_id"]).unwrap_or_default(),
            self_mute: value["self_mute"].as_bool().unwrap_or(false),
            self_deaf: value["self_deaf"].as_bool().unwrap_or(false),
            mute: value["mute"].as_bool().unwrap_or(false),
            deaf: value["deaf"].as_bool().unwrap_or(false),
            suppress: value["suppress"].as_bool().unwrap_or(false),
            self_video: value["self_video"].as_bool().unwrap_or(false),
            self_stream: value["self_stream"].as_bool().unwrap_or(false),
        })
    }
}

/// `VOICE_SERVER_UPDATE`: where the voice connection for a guild goes. The token is a secret.
#[derive(Clone, PartialEq)]
pub struct VoiceServer {
    /// The guild, or 0 for a DM or group DM call (`VOICE_SERVER_UPDATE` carries `channel_id` and no `guild_id`).
    pub guild_id: Id,
    /// The channel of a DM or group DM call.
    pub channel_id: Option<Id>,
    pub endpoint: String,
    pub token: String,
}

#[derive(Default)]
pub struct Stores {
    /// This account's experiment assignments from READY (`ApexExperimentStore`).
    pub experiments: experiments::Assignments,
    /// Guild to its AFK voice channel (`afk_channel_id`), which voice join and leave sounds skip.
    pub afk_channels: HashMap<Id, Id>,
    /// Guild to its members in voice. A state without a channel removes the member.
    pub voice_states: HashMap<Id, Vec<VoiceState>>,
    /// The last voice server handed to this session.
    pub voice_server: Option<VoiceServer>,
    /// Calls in direct messages and group DMs, and the ring queue (`CallStore`, `IncomingCallStore`).
    pub calls: calls::CallStore,
    /// `appearance.theme` from the settings proto: 0 unset, 1 dark, 2 light, 3 darker, 4 midnight.
    pub theme: u64,
    pub account: Account,
    pub users: HashMap<Id, User>,
    /// Channel to who is typing and when that notice ends.
    pub typing: HashMap<Id, Vec<(Id, Instant)>>,
    /// User id to relationship type.
    pub relationships: HashMap<Id, u64>,
    pub relationship_nicknames: HashMap<Id, String>,
    pub relationship_since: HashMap<Id, String>,
    /// RelationshipStore's spam-request and ignored-pending counters are separate sets.
    pub spam_relationships: HashSet<Id>,
    pub ignored_pending_relationships: HashSet<Id>,
    /// Users this account ignores (`user_ignored` on a relationship; RelationshipStore `isIgnored`).
    pub ignored: HashSet<Id>,
    /// The account has 2FA (`mfa_enabled` on READY's user).
    pub mfa_enabled: bool,
    /// `safety_settings.ignore_profile_speedbump_disabled`.
    pub ignore_speedbump_disabled: bool,
    pub private_channels: Vec<PrivateChannel>,
    pub member_lists: HashMap<Id, member_list::MemberList>,
    /// User id to status, for people this session has presence for.
    pub presences: HashMap<Id, String>,
    pub presence: presence::PresenceStore,
    pub member_counts: guild_member_counts::GuildMemberCountStore,
    pub scheduled_events: scheduled_events::GuildScheduledEventStore,
    /// The user's scheduled messages and each channel's scheduled-message draft (`scheduledMessageStore`, `DraftStore`).
    pub scheduled_messages: scheduled_message::ScheduledMessageStore,
    pub scheduled_drafts: scheduled_message::ScheduledDrafts,
    pub content_inventory: content_inventory::ContentInventoryStore,
    pub current_user: Option<User>,
    /// This session's status as READY reports it.
    pub status: String,
    /// READY's `session_id`, which interactions carry.
    pub session_id: Option<String>,
    /// The account's sessions (`SessionsStore`, module 528767): READY's `sessions`, then `SESSIONS_REPLACE`.
    pub sessions: Vec<Session>,
    pub guild_order: Vec<Id>,
    /// Client preferences kept in the account's proto settings (`settings::PROTO_PREFS`), by row key: the varint as
    /// stored. A key that is absent was never set, and the preference keeps its default.
    pub prefs: HashMap<&'static str, u64>,
    /// `ConsentStore` (module 153488): what the account agreed to, by consent name (`usage_statistics`,
    /// `personalization`), from READY's `consents` and `GET /users/@me/consent`.
    pub consents: HashMap<String, bool>,
    /// The account's proto settings as the server last gave them, for a change to be built on.
    settings_raw: Vec<u8>,
    /// READY carried the account's settings, so `settings_raw` is the whole of them and a write may be built from it.
    settings_loaded: bool,
    pub guilds: Vec<Guild>,
    pub messages: HashMap<Id, Vec<Message>>,
    /// What `ChannelMessages` keeps beside each channel's loaded messages: `hasMoreBefore`, `hasMoreAfter`, the jump
    /// state and the caches (`message_window`).
    pub windows: HashMap<Id, message_window::ChannelWindow>,
    pub read_states: read_state::ReadStates,
    /// Bookmarks and reminders (`SavedMessagesStore`): (channel, message) to the reminder's `due_at`, `None` for a
    /// plain bookmark.
    pub saved: HashMap<(Id, Id), Option<String>>,
    /// Interactions in flight, the modal asked for and what was chosen on components (`InteractionStore`).
    pub interactions: interactions::InteractionStore,
    pub guild_settings: guild_settings::GuildSettingsStore,
    pub notification_flags: u64,
    /// The members seen so far, per guild (`GuildMemberStore`).
    pub members: members::GuildMembers,
    /// Guild stickers and the standard packs (`GuildStickersStore`, `StickersPackStore`).
    pub stickers: stickers::StickersStore,
    /// Channel or thread id to the guild that holds it. Lookups check the answer against the guild, so a guild
    /// changed directly (not through `dispatch`) still resolves, by the slow way.
    channel_guilds: HashMap<Id, Id>,
    /// Guilds the account only lurks in: their `joined_at` is null (260509 `DG`, 857071 `isLurking`).
    pub lurking_guilds: HashSet<Id>,
    /// Per guild, the last `unread::guild_counts` answer; cleared by every event that can change one.
    counts_cache: std::sync::Mutex<HashMap<Id, unread::CountsEntry>>,
}

impl Stores {
    pub fn dispatch(&mut self, name: &str, data: &Value) {
        if name == "READY" {
            // `CONNECTION_OPEN` (module 596344 `H`): every window is stale until it is loaded again.
            for window in self.windows.values_mut() {
                window.ready = false;
                window.loading_more = false;
            }
            self.presence.set_current_user(id(&data["user"]["id"]));
        }
        self.presence.dispatch(name, data);
        self.member_counts.dispatch(name, data);
        self.scheduled_events.dispatch(name, data);
        {
            let known: Vec<Id> = self
                .private_channels
                .iter()
                .map(|channel| channel.id)
                .collect();
            self.calls
                .dispatch(name, data, &|channel| known.contains(&channel));
        }
        if name == "READY" {
            self.content_inventory.connection_open();
            // `CONNECTION_OPEN` empties the scheduled-message list.
            self.scheduled_messages.reset();
        }
        self.members.dispatch(name, data);
        self.stickers.dispatch(name, data);
        self.interactions.dispatch(name, data);
        match name {
            "READY" => self.on_ready(data),
            "READY_SUPPLEMENTAL" => {
                // Who is in voice comes here for a user account, not in READY: without it, people already in a
                // channel stay invisible until their state next changes.
                for guild in data["guilds"].as_array().into_iter().flatten() {
                    if let (Some(guild_id), Some(states)) =
                        (id(&guild["id"]), guild["voice_states"].as_array())
                    {
                        let list = self.voice_states.entry(guild_id).or_default();
                        for state in states.iter().filter_map(VoiceState::from_value) {
                            list.retain(|known| known.user_id != state.user_id);
                            list.push(state);
                        }
                    }
                }
                let friends = data["merged_presences"]["friends"].as_array();
                for presence in friends.into_iter().flatten() {
                    self.set_presence(presence);
                }
            }
            "PRESENCE_UPDATE" => self.set_presence(data),
            "GUILD_MEMBERS_CHUNK" => {
                for member in data["members"].as_array().into_iter().flatten() {
                    if let Some(user) = User::from_value(&member["user"]) {
                        self.users.insert(user.id, user);
                    }
                }
                for presence in data["presences"].as_array().into_iter().flatten() {
                    self.set_presence(presence);
                }
            }
            "GUILD_MEMBER_LIST_UPDATE" => {
                if let Some(guild) = id(&data["guild_id"]) {
                    self.member_lists.entry(guild).or_default().apply(data);
                    self.members.apply_list_update(guild, data);
                    for op in data["ops"].as_array().into_iter().flatten() {
                        for item in op["items"]
                            .as_array()
                            .into_iter()
                            .flatten()
                            .chain(std::iter::once(&op["item"]))
                        {
                            if let Some(user) = User::from_value(&item["member"]["user"]) {
                                self.users.insert(user.id, user);
                            }
                        }
                    }
                }
            }
            "VOICE_STATE_UPDATE" => {
                // A DM or group DM call has no guild: its states live under guild 0, keyed by the call's channel.
                let guild = if data["guild_id"].is_null() {
                    Some(0)
                } else {
                    id(&data["guild_id"])
                };
                if let (Some(guild), Some(user)) = (guild, id(&data["user_id"])) {
                    let list = self.voice_states.entry(guild).or_default();
                    list.retain(|state| state.user_id != user);
                    if let Some(state) = VoiceState::from_value(data) {
                        list.push(state);
                    }
                }
            }
            "VOICE_SERVER_UPDATE" => {
                let guild = if data["guild_id"].is_null() {
                    Some(0)
                } else {
                    id(&data["guild_id"])
                };
                if let (Some(guild_id), Some(endpoint), Some(token)) =
                    (guild, text(&data["endpoint"]), text(&data["token"]))
                {
                    self.voice_server = Some(VoiceServer {
                        guild_id,
                        channel_id: id(&data["channel_id"]),
                        endpoint,
                        token,
                    });
                }
            }
            "GUILD_CREATE" => {
                self.note_afk_channel(data);
                if let Some(mut guild) = Guild::from_value(data) {
                    let me = self.current_user.as_ref().map(|user| user.id);
                    if let Some(member) = data["members"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .find(|member| me.is_some() && id(&member["user"]["id"]) == me)
                    {
                        guild.set_my_member(member);
                    }
                    for channel in &guild.channels {
                        if let Some(latest) = channel.last_message_id {
                            self.read_states.latest.insert(channel.id, latest);
                        }
                    }
                    if data.get("joined_at").is_some_and(Value::is_null) {
                        self.lurking_guilds.insert(guild.id);
                    } else {
                        self.lurking_guilds.remove(&guild.id);
                    }
                    for thread in &guild.threads {
                        self.note_thread_latest(thread);
                    }
                    self.guilds.retain(|known| known.id != guild.id);
                    self.guilds.push(guild);
                    self.sort_guilds();
                }
            }
            "GUILD_DELETE" => {
                // An outage (`unavailable`) keeps the guild; a removal drops it.
                if data["unavailable"] != true
                    && let Some(guild) = id(&data["id"])
                {
                    self.guilds.retain(|known| known.id != guild);
                    self.voice_states.remove(&guild);
                    self.afk_channels.remove(&guild);
                    self.lurking_guilds.remove(&guild);
                }
            }
            "SESSIONS_REPLACE" => self.sessions = Session::list(data),
            "NOTIFICATION_SETTINGS_UPDATE" => {
                // 225123 forwards `flags`; 543465 `Y` rereads USE_NEW_NOTIFICATIONS and MENTION_ON_ALL_MESSAGES.
                if let Some(flags) = data["flags"].as_u64() {
                    self.notification_flags = flags;
                }
            }
            "GUILD_UPDATE" => {
                self.note_afk_channel(data);
                if let Some(guild) = id(&data["id"])
                    .and_then(|known| self.guilds.iter_mut().find(|guild| guild.id == known))
                {
                    guild.apply_update(data);
                }
            }
            "GUILD_ROLE_CREATE" | "GUILD_ROLE_UPDATE" => {
                if let (Some(guild), Some(role)) = (
                    id(&data["guild_id"])
                        .and_then(|guild| self.guilds.iter_mut().find(|known| known.id == guild)),
                    id(&data["role"]["id"]),
                ) {
                    if let Some(name) = text(&data["role"]["name"]) {
                        guild.roles.insert(role, name);
                    }
                    guild
                        .role_permissions
                        .insert(role, permissions::bits(&data["role"]["permissions"]));
                    guild
                        .role_info
                        .insert(role, RoleInfo::from_value(&data["role"]));
                    match RoleIcon::from_value(&data["role"]) {
                        Some(icon) => guild.role_icons.insert(role, icon),
                        None => guild.role_icons.remove(&role),
                    };
                }
            }
            "GUILD_ROLE_DELETE" => {
                if let (Some(guild), Some(role)) = (
                    id(&data["guild_id"])
                        .and_then(|guild| self.guilds.iter_mut().find(|known| known.id == guild)),
                    id(&data["role_id"]),
                ) {
                    guild.roles.remove(&role);
                    guild.role_permissions.remove(&role);
                    guild.role_info.remove(&role);
                    guild.role_icons.remove(&role);
                    guild.my_roles.retain(|known| *known != role);
                }
            }
            "GUILD_MEMBER_UPDATE" => {
                let me = self.current_user.as_ref().map(|user| user.id);
                if me.is_some()
                    && id(&data["user"]["id"]) == me
                    && let Some(guild) = id(&data["guild_id"])
                        .and_then(|guild| self.guilds.iter_mut().find(|known| known.id == guild))
                {
                    guild.set_my_member(data);
                    if data["joined_at"].is_string() {
                        self.lurking_guilds.remove(&guild.id);
                    }
                }
            }
            "GUILD_EMOJIS_UPDATE" => {
                if let Some(guild) = id(&data["guild_id"])
                    .and_then(|guild| self.guilds.iter_mut().find(|known| known.id == guild))
                {
                    guild.emojis = GuildEmoji::list(&data["emojis"]);
                }
            }
            "TYPING_START" => {
                if let (Some(channel), Some(user)) = (id(&data["channel_id"]), id(&data["user_id"]))
                {
                    let list = self.typing.entry(channel).or_default();
                    list.retain(|(known, _)| *known != user);
                    list.push((user, Instant::now() + TYPING_DURATION));
                }
            }
            "MESSAGE_ACK" => self.read_states.ack(data),
            "USER_GUILD_SETTINGS_UPDATE" => self.guild_settings.update(data),
            "SAVED_MESSAGE_CREATE" => self.save(&data["save_data"]),
            "SAVED_MESSAGE_DELETE" => {
                let data = if data["save_data"].is_object() {
                    &data["save_data"]
                } else {
                    data
                };
                if let (Some(channel), Some(message)) =
                    (id(&data["channel_id"]), id(&data["message_id"]))
                {
                    self.saved.remove(&(channel, message));
                }
            }
            "MESSAGE_CREATE" => {
                if let Some(message) = Message::from_value(data) {
                    let me = self.current_user.as_ref().map(|user| user.id);
                    let mention = unread::message_mention(self, data, chrono::Utc::now());
                    self.read_states.message_created(
                        message.channel_id,
                        message.id,
                        mention,
                        Some(message.author.id) == me && message.kind != 46,
                    );
                    // A sent message ends its author's typing notice.
                    if let Some(list) = self.typing.get_mut(&message.channel_id) {
                        list.retain(|(user, _)| *user != message.author.id);
                    }
                    self.receive_message(message);
                }
            }
            "THREAD_CREATE" | "THREAD_UPDATE" => {
                if let (Some(guild), Some(mut thread)) =
                    (id(&data["guild_id"]), Thread::from_value(data))
                {
                    self.note_thread_latest(&thread);
                    if let Some(guild) = self.guilds.iter_mut().find(|known| known.id == guild) {
                        if let Some(known) =
                            guild.threads.iter().find(|known| known.id == thread.id)
                            && known.joined
                            && !thread.joined
                        {
                            // An update without a member keeps the one already known.
                            thread.joined = true;
                            thread.member_flags = known.member_flags;
                            thread.muted = known.muted;
                            thread.mute_end = known.mute_end;
                            thread.join_timestamp = known.join_timestamp.clone();
                        }
                        guild.threads.retain(|known| known.id != thread.id);
                        if !thread.archived {
                            guild.threads.push(thread);
                        }
                    }
                }
            }
            "THREAD_DELETE" => {
                if let (Some(guild), Some(thread)) = (id(&data["guild_id"]), id(&data["id"]))
                    && let Some(guild) = self.guilds.iter_mut().find(|known| known.id == guild)
                {
                    guild.threads.retain(|known| known.id != thread);
                }
            }
            // 152007: only the current user's member counts; the event carries one member's fields beside the id.
            "THREAD_MEMBER_UPDATE" => {
                let me = self.current_user.as_ref().map(|user| user.id);
                if let (Some(guild), Some(thread)) = (id(&data["guild_id"]), id(&data["id"]))
                    && id(&data["user_id"]).is_none_or(|user| Some(user) == me)
                    && let Some(guild) = self.guilds.iter_mut().find(|known| known.id == guild)
                    && let Some(known) = guild.threads.iter_mut().find(|known| known.id == thread)
                {
                    known.apply_member(data);
                }
            }
            "THREAD_MEMBERS_UPDATE" => {
                let me = self.current_user.as_ref().map(|user| user.id);
                if let (Some(guild), Some(thread)) = (id(&data["guild_id"]), id(&data["id"]))
                    && let Some(guild) = self.guilds.iter_mut().find(|known| known.id == guild)
                    && let Some(known) = guild.threads.iter_mut().find(|known| known.id == thread)
                {
                    if data["removed_member_ids"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .any(|user| id(user).is_some() && id(user) == me)
                    {
                        known.clear_member();
                    }
                    for member in data["added_members"].as_array().into_iter().flatten() {
                        if me.is_some() && id(&member["user_id"]) == me {
                            known.apply_member(member);
                        }
                    }
                }
            }
            "THREAD_LIST_SYNC" => {
                if let Some(guild) = id(&data["guild_id"]) {
                    let synced: Vec<Thread> = data["threads"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(Thread::from_value)
                        .collect();
                    for thread in &synced {
                        self.note_thread_latest(thread);
                    }
                    if let Some(guild) = self.guilds.iter_mut().find(|known| known.id == guild) {
                        for thread in synced {
                            guild.threads.retain(|known| known.id != thread.id);
                            guild.threads.push(thread);
                        }
                        // The account's own thread members come beside the threads, one per joined thread.
                        for member in data["members"].as_array().into_iter().flatten() {
                            if let Some(thread) = id(&member["id"])
                                && let Some(known) =
                                    guild.threads.iter_mut().find(|known| known.id == thread)
                            {
                                known.apply_member(member);
                            }
                        }
                    }
                }
            }
            "RELATIONSHIP_ADD" | "RELATIONSHIP_UPDATE" => {
                if let Some(user) = User::from_value(&data["user"]) {
                    self.users.insert(user.id, user);
                }
                if let (Some(user), Some(kind)) = (id(&data["id"]), data["type"].as_u64()) {
                    self.relationships.insert(user, kind);
                }
                if let Some(user) = id(&data["id"]) {
                    if let Some(nickname) = text(&data["nickname"]) {
                        self.relationship_nicknames.insert(user, nickname);
                    } else if name == "RELATIONSHIP_UPDATE" {
                        self.relationship_nicknames.remove(&user);
                    }
                    if let Some(since) = text(&data["since"]) {
                        self.relationship_since.insert(user, since);
                    } else if name == "RELATIONSHIP_UPDATE" {
                        self.relationship_since.remove(&user);
                    }
                    if data["is_spam_request"] == true {
                        self.spam_relationships.insert(user);
                    } else {
                        self.spam_relationships.remove(&user);
                    }
                    // 225123 normalizes absent flags to false before RelationshipStore receives them.
                    self.set_ignored(user, data["user_ignored"] == true);
                    if name == "RELATIONSHIP_ADD" && data["type"] == 1 {
                        self.ignored_pending_relationships.remove(&user);
                    }
                }
            }
            // A settings write from any session: type 1 is PreloadedUserSettings, sent whole.
            "USER_SETTINGS_PROTO_UPDATE" => {
                if data["settings"]["type"].as_u64() == Some(1)
                    && let Some(proto) = data["settings"]["proto"].as_str()
                {
                    // A partial update carries only what changed: absent there means unchanged.
                    let disabled = settings::ignore_speedbump_disabled(proto);
                    if data["partial"] != true || disabled {
                        self.ignore_speedbump_disabled = disabled;
                    }
                    if let Some(theme) = settings::theme(proto) {
                        self.theme = theme;
                    }
                    self.read_prefs(proto, data["partial"] == true);
                    settings::merge_update(
                        &mut self.settings_raw,
                        &settings::decode(proto),
                        data["partial"] == true,
                    );
                    // The status chosen in another session (or lapsed on the server) shows here too.
                    if let Some(status) = self.status_state().and_then(|state| state.status) {
                        self.status = status;
                    }
                }
            }
            "RELATIONSHIP_REMOVE" => {
                if let Some(user) = id(&data["id"]) {
                    self.relationships.remove(&user);
                    self.relationship_nicknames.remove(&user);
                    self.clear_relationship_request_flags(user);
                    self.relationship_since.remove(&user);
                    if data["user_ignored"] != true {
                        self.ignored.remove(&user);
                    }
                }
            }
            "CHANNEL_CREATE" | "CHANNEL_UPDATE" => match id(&data["guild_id"]) {
                Some(guild) => {
                    if let (Some(channel), Some(guild)) = (
                        Channel::from_value(data),
                        self.guilds.iter_mut().find(|known| known.id == guild),
                    ) {
                        guild.channels.retain(|known| known.id != channel.id);
                        guild.channels.push(channel);
                    }
                }
                None => self.add_private_channel(data),
            },
            "CHANNEL_DELETE" => {
                if let Some(channel) = id(&data["id"]) {
                    self.private_channels.retain(|known| known.id != channel);
                    for guild in &mut self.guilds {
                        guild.channels.retain(|known| known.id != channel);
                    }
                }
            }
            "CHANNEL_UPDATES" => {
                for channel in data["channels"].as_array().into_iter().flatten() {
                    self.dispatch("CHANNEL_UPDATE", channel);
                }
            }
            "CHANNEL_RECIPIENT_ADD" | "CHANNEL_RECIPIENT_REMOVE" => {
                if let (Some(channel), Some(user)) =
                    (id(&data["channel_id"]), id(&data["user"]["id"]))
                {
                    if let Some(known) = self
                        .private_channels
                        .iter_mut()
                        .find(|known| known.id == channel)
                    {
                        if name == "CHANNEL_RECIPIENT_ADD" {
                            if !known.recipients.contains(&user) {
                                known.recipients.push(user);
                            }
                        } else {
                            known.recipients.retain(|known| *known != user);
                        }
                    }
                    if let Some(user) = User::from_value(&data["user"]) {
                        self.users.entry(user.id).or_insert(user);
                    }
                }
            }
            "MESSAGE_UPDATE" => self.update_message(data),
            "MESSAGE_REACTION_ADD" => self.react(data, 1),
            "MESSAGE_REACTION_REMOVE" => self.react(data, -1),
            "MESSAGE_POLL_VOTE_ADD" => self.poll_vote(data, true),
            "MESSAGE_POLL_VOTE_REMOVE" => self.poll_vote(data, false),
            "MESSAGE_POLL_VOTE_ADD_MANY" => self.poll_vote_many(data),
            "MESSAGE_DELETE" => {
                if let (Some(channel), Some(message)) = (id(&data["channel_id"]), id(&data["id"]))
                    && let Some(list) = self.messages.get_mut(&channel)
                {
                    self.windows
                        .entry(channel)
                        .or_default()
                        .remove(list, message);
                }
            }
            _ => {}
        }
        self.after_dispatch(name, data);
    }

    /// Events that cannot change a guild's unread or mention counts.
    const COUNT_NEUTRAL: [&'static str; 12] = [
        "PRESENCE_UPDATE",
        "TYPING_START",
        "VOICE_STATE_UPDATE",
        "VOICE_SERVER_UPDATE",
        "GUILD_MEMBER_LIST_UPDATE",
        "GUILD_MEMBERS_CHUNK",
        "MESSAGE_REACTION_ADD",
        "MESSAGE_REACTION_REMOVE",
        "MESSAGE_POLL_VOTE_ADD",
        "MESSAGE_POLL_VOTE_REMOVE",
        "MESSAGE_POLL_VOTE_ADD_MANY",
        "ONLINE_GUILD_MEMBER_COUNT_UPDATE",
    ];

    fn after_dispatch(&mut self, name: &str, data: &Value) {
        match name {
            "READY" => self.rebuild_channel_index(),
            "GUILD_CREATE" => {
                if let Some(guild) = id(&data["id"]) {
                    self.index_guild(guild);
                }
            }
            "CHANNEL_CREATE" | "CHANNEL_UPDATE" | "THREAD_CREATE" | "THREAD_UPDATE"
            | "THREAD_LIST_SYNC" => {
                if let Some(guild) = id(&data["guild_id"]) {
                    self.index_guild(guild);
                }
            }
            "CHANNEL_DELETE" | "THREAD_DELETE" => {
                if let Some(channel) = id(&data["id"]) {
                    self.channel_guilds.remove(&channel);
                }
            }
            "GUILD_DELETE" => {
                if let Some(guild) = id(&data["id"]) {
                    self.channel_guilds.retain(|_, known| *known != guild);
                }
            }
            _ => {}
        }
        if !Self::COUNT_NEUTRAL.contains(&name) {
            self.invalidate_counts();
        }
    }

    /// Forgets the cached guild counts. `dispatch` does it for every event that can change them; a caller that
    /// changes the stores directly (a settings entry, an ack, a guild) calls it too.
    pub fn invalidate_counts(&mut self) {
        self.counts_cache
            .get_mut()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clear();
    }

    fn index_guild(&mut self, guild: Id) {
        if let Some(known) = self.guilds.iter().find(|known| known.id == guild) {
            for channel in &known.channels {
                self.channel_guilds.insert(channel.id, guild);
            }
            for thread in &known.threads {
                self.channel_guilds.insert(thread.id, guild);
            }
        }
    }

    fn rebuild_channel_index(&mut self) {
        self.channel_guilds.clear();
        let all: Vec<Id> = self.guilds.iter().map(|guild| guild.id).collect();
        for guild in all {
            self.index_guild(guild);
        }
    }

    /// The guild holding this channel or thread, by the channel index (checked, with a scan as the fallback).
    pub fn guild_of_channel(&self, channel: Id) -> Option<&Guild> {
        let holds = |guild: &Guild| {
            guild.channels.iter().any(|known| known.id == channel)
                || guild.threads.iter().any(|known| known.id == channel)
        };
        if let Some(guild) = self
            .channel_guilds
            .get(&channel)
            .and_then(|guild| self.guild(*guild))
            && holds(guild)
        {
            return Some(guild);
        }
        if self
            .private_channels
            .iter()
            .any(|known| known.id == channel)
        {
            return None;
        }
        self.guilds.iter().find(|guild| holds(guild))
    }

    /// Seeds a thread's newest message so its unread state can be told.
    fn note_thread_latest(&mut self, thread: &Thread) {
        if let Some(last) = thread.last_message_id {
            let latest = self.read_states.latest.entry(thread.id).or_insert(0);
            *latest = (*latest).max(last);
        }
    }

    /// `SessionsStore.getRemoteActivities`: the activities of the first active session that is not this one.
    pub fn remote_activities(&self) -> &[Value] {
        self.sessions
            .iter()
            .find(|session| {
                session.active && Some(session.session_id.as_str()) != self.session_id.as_deref()
            })
            .map_or(&[], |session| session.activities.as_slice())
    }

    pub fn clear_relationship_request_flags(&mut self, user: Id) {
        self.spam_relationships.remove(&user);
        self.ignored_pending_relationships.remove(&user);
    }

    /// `RELATIONSHIP_IGNORE_USER_SUCCESS` / `RELATIONSHIP_UNIGNORE_USER_SUCCESS`.
    pub fn set_ignored(&mut self, user: Id, ignored: bool) {
        if ignored {
            self.ignored.insert(user);
            if self.relationships.get(&user) == Some(&3) {
                self.ignored_pending_relationships.insert(user);
            }
        } else {
            self.ignored.remove(&user);
            self.ignored_pending_relationships.remove(&user);
        }
    }

    /// Who permissions are computed for.
    pub fn me(&self) -> Option<permissions::Me> {
        Some(permissions::Me {
            id: self.current_user.as_ref()?.id,
            mfa_enabled: self.mfa_enabled,
        })
    }

    /// RelationshipStore `isIgnored`: ignored and not blocked.
    pub fn is_ignored(&self, user: Id) -> bool {
        self.ignored.contains(&user) && self.relationships.get(&user) != Some(&2)
    }

    pub fn friend_nickname(&self, user: Id) -> Option<&str> {
        self.relationship_nicknames.get(&user).map(String::as_str)
    }

    pub fn add_private_channel(&mut self, data: &Value) {
        let existing = id(&data["id"]).and_then(|id| {
            self.private_channels
                .iter()
                .position(|known| known.id == id)
        });
        let kind = data["type"]
            .as_u64()
            .or_else(|| existing.map(|index| self.private_channels[index].kind));
        if !matches!(kind, Some(1 | 3)) {
            return;
        }
        if let (Some(channel), Some(message)) = (id(&data["id"]), id(&data["last_message_id"])) {
            let latest = self.read_states.latest.entry(channel).or_default();
            *latest = (*latest).max(message);
        }
        for user in data["recipients"].as_array().into_iter().flatten() {
            if let Some(user) = User::from_value(user) {
                self.users.entry(user.id).or_insert(user);
            }
        }
        if let Some(index) = existing {
            self.private_channels[index].update(data);
        } else if let Some(channel) = PrivateChannel::from_value(data) {
            self.private_channels.insert(0, channel);
        }
    }

    /// READY and GUILD_CREATE nest the guild's settings under `properties`; GUILD_UPDATE does not.
    fn note_afk_channel(&mut self, guild: &Value) {
        let properties = if guild["properties"].is_object() {
            &guild["properties"]
        } else {
            guild
        };
        if let Some(guild_id) = id(&guild["id"]) {
            match id(&properties["afk_channel_id"]) {
                Some(channel) => self.afk_channels.insert(guild_id, channel),
                None => self.afk_channels.remove(&guild_id),
            };
        }
    }

    /// Changes one client preference: the new value shows at once, and the PATCH body value comes back (the whole of
    /// the top-level message it lives in, see `settings::set_pref`).
    ///
    /// Empty until the account's settings have arrived (READY carries them): a patch built from nothing would
    /// replace the whole message on the server and wipe everything else in it. An empty value is never sent.
    pub fn set_pref(&mut self, pref: &'static settings::ProtoPref, value: u64) -> String {
        if !self.settings_loaded {
            return String::new();
        }
        let (patch, updated) = settings::set_pref(&self.settings_raw, pref, value);
        self.settings_raw = updated;
        self.prefs.insert(pref.key, value);
        patch
    }

    /// Takes a preference out of the account's settings (a default theme ends the colour theme): the PATCH body value
    /// comes back, empty until the account's settings have arrived.
    pub fn clear_pref(&mut self, pref: &'static settings::ProtoPref) -> String {
        self.prefs.remove(pref.key);
        if !self.settings_loaded {
            return String::new();
        }
        let (patch, updated) = settings::clear_pref(&self.settings_raw, pref);
        self.settings_raw = updated;
        patch
    }

    /// Takes the consents object (`{"usage_statistics": {"consented": true}, ...}`): the names it lists replace the
    /// store's, as `UPDATE_CONSENTS` does. Anything else is ignored.
    pub fn set_consents(&mut self, consents: &Value) {
        let Some(map) = consents.as_object() else {
            return;
        };
        self.consents = map
            .iter()
            .map(|(name, value)| (name.clone(), value["consented"].as_bool().unwrap_or(false)))
            .collect();
    }

    /// A guild list of the account's privacy settings (`DM_RESTRICTED_GUILDS` and friends).
    pub fn proto_ids(&self, list: &settings::IdList) -> Vec<u64> {
        settings::ids_in(&self.settings_raw, list)
    }

    /// Replaces a guild list; the PATCH body value comes back, empty until the account's settings have arrived.
    pub fn set_proto_ids(&mut self, list: &settings::IdList, ids: &[u64]) -> String {
        if !self.settings_loaded {
            return String::new();
        }
        let (patch, updated) = settings::set_ids(&self.settings_raw, list, ids);
        self.settings_raw = updated;
        patch
    }

    /// The varint at `field` of the settings message at `path`, for values that are not a page row (the friend
    /// request flags).
    pub fn proto_varint(&self, path: &[u64], field: u64) -> Option<u64> {
        settings::varint_in(&self.settings_raw, path, field)
    }

    pub fn content_dismissed(&self, content: u16) -> bool {
        settings::content_dismissed(&self.settings_raw, content)
    }

    /// Empty until the account's settings have arrived, as `set_pref`.
    pub fn dismiss_content(&mut self, content: u16) -> String {
        if !self.settings_loaded {
            return String::new();
        }
        let (patch, updated) = settings::dismiss_content(&self.settings_raw, content);
        self.settings_raw = updated;
        patch
    }

    /// The status the account's proto settings hold: the chosen one, when it lapses, and the custom status.
    pub fn status_state(&self) -> Option<settings::StatusState> {
        settings::read_status(&self.settings_raw)
    }

    /// Chooses a status (`online`, `idle`, `dnd`, `invisible`) until `expires_at_ms` (zero: until changed). The new
    /// status shows at once; the PATCH body value comes back.
    pub fn set_status(&mut self, status: &str, expires_at_ms: u64, now_ms: u64) -> String {
        if !self.settings_loaded {
            return String::new();
        }
        let (patch, updated) =
            settings::set_status(&self.settings_raw, status, expires_at_ms, now_ms);
        self.settings_raw = updated;
        self.status = status.to_owned();
        patch
    }

    /// Saves the custom status, or clears it with `None`; the PATCH body value comes back.
    pub fn set_custom_status(
        &mut self,
        custom: Option<&settings::CustomStatus>,
        now_ms: u64,
    ) -> String {
        if !self.settings_loaded {
            return String::new();
        }
        let (patch, updated) = settings::set_custom_status(&self.settings_raw, custom, now_ms);
        self.settings_raw = updated;
        patch
    }

    pub fn notification_quiet_mode(&self) -> bool {
        settings::notification_quiet_mode(&self.settings_raw)
    }

    /// Takes the client preferences out of a settings payload. A full one without a preference clears it (the account
    /// reset it); a partial one carries only what changed.
    fn read_prefs(&mut self, proto: &str, partial: bool) {
        for pref in settings::PROTO_PREFS {
            match settings::pref_value(proto, pref) {
                Some(value) => {
                    self.prefs.insert(pref.key, value);
                }
                None if !partial => {
                    self.prefs.remove(pref.key);
                }
                None => {}
            }
        }
    }

    fn on_ready(&mut self, data: &Value) {
        self.current_user = User::from_value(&data["user"]);
        self.experiments = experiments::Assignments::from_ready(
            &data["apex_experiments"],
            data["user"]["id"].as_str().unwrap_or_default(),
        );
        self.mfa_enabled = data["user"]["mfa_enabled"].as_bool().unwrap_or(false);
        self.account = Account::from_value(&data["user"]);
        self.theme = data["user_settings_proto"]
            .as_str()
            .and_then(settings::theme)
            .unwrap_or(0);
        self.ignore_speedbump_disabled = data["user_settings_proto"]
            .as_str()
            .is_some_and(settings::ignore_speedbump_disabled);
        self.guild_order = data["user_settings_proto"]
            .as_str()
            .and_then(settings::guild_order)
            .unwrap_or_default();
        self.prefs.clear();
        self.settings_raw.clear();
        self.settings_loaded = false;
        self.set_consents(&data["consents"]);
        if let Some(proto) = data["user_settings_proto"].as_str() {
            self.settings_raw = settings::decode(proto);
            self.settings_loaded = true;
            self.read_prefs(proto, false);
        }
        self.session_id = text(&data["session_id"]);
        self.sessions = Session::list(&data["sessions"]);
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
        self.relationship_nicknames = data["relationships"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|entry| Some((id(&entry["id"])?, text(&entry["nickname"])?)))
            .collect();
        self.relationship_since = data["relationships"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|entry| Some((id(&entry["id"])?, text(&entry["since"])?)))
            .collect();
        self.ignored = data["relationships"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|entry| entry["user_ignored"].as_bool() == Some(true))
            .filter_map(|entry| id(&entry["id"]))
            .collect();
        self.spam_relationships = data["relationships"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|entry| entry["is_spam_request"] == true)
            .filter_map(|entry| id(&entry["id"]))
            .collect();
        self.ignored_pending_relationships = data["relationships"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|entry| entry["user_ignored"] == true && entry["type"] == 3)
            .filter_map(|entry| id(&entry["id"]))
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
        // `merged_members` lines up with `guilds`: the current user's member object in each.
        let me = self.current_user.as_ref().map(|user| user.id);
        let merged: Vec<Option<Value>> = data["guilds"]
            .as_array()
            .into_iter()
            .flatten()
            .enumerate()
            .map(|(index, _)| {
                data["merged_members"][index]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .find(|member| me.is_none() || id(&member["user_id"]) == me)
                    .cloned()
            })
            .collect();
        let guild_ids: Vec<Option<Id>> = data["guilds"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|guild| id(&guild["id"]))
            .collect();
        self.afk_channels.clear();
        for guild in data["guilds"].as_array().into_iter().flatten() {
            self.note_afk_channel(guild);
        }
        for (guild_id, member) in guild_ids.into_iter().zip(merged) {
            if let (Some(guild), Some(member)) = (
                self.guilds
                    .iter_mut()
                    .find(|guild| Some(guild.id) == guild_id),
                member,
            ) {
                guild.set_my_member(&member);
            }
        }
        self.voice_states = data["guilds"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|guild| {
                let states = guild["voice_states"]
                    .as_array()?
                    .iter()
                    .filter_map(VoiceState::from_value)
                    .collect();
                Some((id(&guild["id"])?, states))
            })
            .collect();
        self.read_states.load(&data["read_state"]);
        self.guild_settings.load(&data["user_guild_settings"]);
        self.notification_flags = data["notification_settings"]["flags"].as_u64().unwrap_or(0);
        let latest: Vec<(Id, Id)> = self
            .guilds
            .iter()
            .flat_map(|guild| &guild.channels)
            .filter_map(|channel| Some((channel.id, channel.last_message_id?)))
            .chain(
                self.private_channels
                    .iter()
                    .map(|channel| (channel.id, channel.last_message_id)),
            )
            .collect();
        self.read_states.latest.extend(latest);
        let threads: Vec<Thread> = self
            .guilds
            .iter()
            .flat_map(|guild| guild.threads.iter().cloned())
            .collect();
        for thread in &threads {
            self.note_thread_latest(thread);
        }
        self.lurking_guilds = data["guilds"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|guild| guild.get("joined_at").is_some_and(Value::is_null))
            .filter_map(|guild| id(&guild["id"]))
            .collect();
        self.sort_guilds();
    }

    fn save(&mut self, save_data: &Value) {
        if let (Some(channel), Some(message)) =
            (id(&save_data["channel_id"]), id(&save_data["message_id"]))
        {
            self.saved
                .insert((channel, message), text(&save_data["due_at"]));
        }
    }

    /// `GET /users/@me/saved-messages`: `{results: [{message, save_data}]}`.
    pub fn load_saved(&mut self, body: &Value) {
        self.saved.clear();
        for entry in body["results"].as_array().into_iter().flatten() {
            self.save(&entry["save_data"]);
        }
    }

    /// Whether `channel` has messages past its ack. Guild channels never read count from the join date.
    pub fn is_unread(&self, channel: Id) -> bool {
        let joined = self
            .guild_of_channel(channel)
            .map_or(0, Guild::joined_snowflake);
        self.read_states.is_unread(channel, joined)
    }

    /// A guild is unread when one of its text channels is.
    pub fn guild_unread(&self, guild: &Guild) -> bool {
        unread::guild_counts(self, guild, chrono::Utc::now()).unread
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

    fn react(&mut self, data: &Value, change: i64) {
        let (Some(channel), Some(message_id), Some(name)) = (
            id(&data["channel_id"]),
            id(&data["message_id"]),
            text(&data["emoji"]["name"]),
        ) else {
            return;
        };
        let mine = self.current_user.as_ref().map(|user| user.id) == id(&data["user_id"]);
        let Some(message) = self
            .messages
            .get_mut(&channel)
            .and_then(|list| list.iter_mut().find(|known| known.id == message_id))
        else {
            return;
        };
        let emoji_id = id(&data["emoji"]["id"]);
        let found = message
            .reactions
            .iter()
            .position(|reaction| reaction.name == name && reaction.emoji_id == emoji_id);
        match (found, change > 0) {
            (Some(index), _) => {
                let reaction = &mut message.reactions[index];
                reaction.count = reaction.count.saturating_add_signed(change);
                if mine {
                    reaction.me = change > 0;
                }
                if reaction.count == 0 {
                    message.reactions.remove(index);
                }
            }
            (None, true) => message.reactions.push(Reaction {
                name,
                emoji_id,
                count: 1,
                me: mine,
            }),
            (None, false) => {}
        }
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
        let Some(known) = known else {
            return;
        };
        self.interactions.message_changed(message);
        if let Some(content) = text(&data["content"]) {
            known.content = content;
            known.edited = data["edited_timestamp"].is_string();
            known.edited_at = edited_at(&data["edited_timestamp"]);
        }
        if let Some(pinned) = data["pinned"].as_bool() {
            known.pinned = pinned;
        }
        // The answer to a deferred command arrives as an update with new flags, and a component message may be
        // edited into other controls (`R` in module 432005).
        if let Some(flags) = data["flags"].as_u64() {
            known.flags = flags;
        }
        if data["components"].is_array() {
            known.extra.components = components::parse(&data["components"]);
            known.extra.has_components = !known.extra.components.is_empty();
        }
        // A poll's results arrive finalized in the update that ends it.
        if data["poll"].is_object() {
            self.poll_update(channel, message, &data["poll"]);
        }
        let Some(known) = self
            .messages
            .get_mut(&channel)
            .and_then(|list| list.iter_mut().find(|known| known.id == message))
        else {
            return;
        };
        // Link previews arrive later, in an update that may carry only the embeds.
        if let Some(embeds) = data["embeds"].as_array() {
            known.embeds = embeds.iter().map(Embed::from_value).collect();
        }
    }

    /// Members in a voice channel, in the order they joined.
    pub fn voice_members(&self, guild: Id, channel: Id) -> Vec<&VoiceState> {
        self.voice_states
            .get(&guild)
            .map(|states| {
                states
                    .iter()
                    .filter(|state| state.channel_id == channel)
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Who is typing in a channel right now, without this client's own user.
    pub fn typing_users(&self, channel: Id, now: Instant) -> Vec<Id> {
        let me = self.current_user.as_ref().map(|user| user.id);
        self.typing
            .get(&channel)
            .map(|list| {
                list.iter()
                    .filter(|(user, ends)| *ends > now && Some(*user) != me)
                    .map(|(user, _)| *user)
                    .collect()
            })
            .unwrap_or_default()
    }

    fn set_presence(&mut self, data: &Value) {
        let user = id(&data["user_id"]).or_else(|| id(&data["user"]["id"]));
        if let (Some(user), Some(status)) = (user, text(&data["status"])) {
            self.presences.insert(user, status);
        }
    }

    /// `offline` when nothing says otherwise, as the official presence store answers.
    pub fn status(&self, user: Id) -> &str {
        self.presence
            .get(user, None)
            .map(|presence| presence.status.as_str())
            .or_else(|| self.presences.get(&user).map(String::as_str))
            .unwrap_or("offline")
    }

    pub fn friends(&self) -> Vec<&User> {
        let mut friends: Vec<&User> = self
            .relationships
            .iter()
            .filter(|(_, kind)| **kind == RELATIONSHIP_FRIEND)
            .filter_map(|(user, _)| self.users.get(user))
            .collect();
        friends.sort_by_key(|user| {
            self.friend_nickname(user.id)
                .unwrap_or(user.display_name())
                .to_lowercase()
        });
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

    /// `MESSAGE_CREATE` (module 596344): a message goes through `receiveMessage`, so a window that is detached from
    /// the newest message does not take it.
    pub fn receive_message(&mut self, message: Message) {
        let channel = message.channel_id;
        let list = self.messages.entry(channel).or_default();
        let window = self.windows.entry(channel).or_default();
        window.receive_message(list, message);
    }

    /// The window state of `channel` (`ChannelMessages`), as it is now.
    pub fn window(&self, channel: Id) -> message_window::ChannelWindow {
        self.windows.get(&channel).cloned().unwrap_or_default()
    }

    /// The first page of a channel: `fetchMessages` with no `before`, `after` or `around`, which replaces the window.
    pub fn load_messages(&mut self, channel: Id, page: &Value) {
        self.load_messages_since(channel, page, 0);
    }

    /// [`Stores::load_messages`] for a request that started at `started_ms` (Unix milliseconds): messages that
    /// arrived since stay.
    pub fn load_messages_since(&mut self, channel: Id, page: &Value, started_ms: u64) {
        let spec = message_window::FetchSpec {
            channel,
            limit: message_window::INITIAL_PAGE,
            jump: Some(message_window::Jump::default()),
            started_ms,
            ..message_window::FetchSpec::default()
        };
        self.messages_loaded(&spec, page);
    }

    /// `fetchMessages` before the request: the shortcuts of `_tryFetchMessagesCached` (a jump to a message the
    /// window covers, a page or the present the caches hold), else `loadStart`. Returns whether the request has to
    /// go to the server.
    pub fn fetch_messages(&mut self, spec: &message_window::FetchSpec) -> bool {
        use message_window::JumpPlan;
        let channel = spec.channel;
        let list = self.messages.entry(channel).or_default();
        let window = self.windows.entry(channel).or_default();
        let target = spec.jump.as_ref().and_then(|jump| jump.message_id);
        let present = spec.jump.as_ref().is_some_and(|jump| jump.present);
        if window.ready {
            if let (Some(target), Some(jump)) = (target, spec.jump.as_ref()) {
                if window.jump_plan(list, target) == JumpPlan::InWindow {
                    window.jump_to_message(jump);
                    return false;
                }
            } else if present && window.has_present() {
                window.jump_to_present(list, spec.limit as usize);
                return false;
            } else if let Some(before) = spec.before
                && !window.before_cache().is_empty()
                && list.first().is_some_and(|first| first.id == before)
            {
                window.load_from_cache(list, true, spec.limit as usize);
                return false;
            } else if let Some(after) = spec.after
                && !window.after_cache().is_empty()
                && list.last().is_some_and(|last| last.id == after)
            {
                window.load_from_cache(list, false, spec.limit as usize);
                return false;
            }
        }
        window.load_start(spec.jump.as_ref());
        true
    }

    /// `LOAD_MESSAGES_SUCCESS`: the page is newest first, as the route returns it.
    pub fn messages_loaded(&mut self, spec: &message_window::FetchSpec, page: &Value) {
        let channel = spec.channel;
        let guild = self
            .guilds
            .iter()
            .find(|guild| {
                guild.channels.iter().any(|known| known.id == channel)
                    || guild.threads.iter().any(|known| known.id == channel)
            })
            .map(|guild| guild.id);
        let mut fetched = Vec::new();
        for value in page.as_array().into_iter().flatten() {
            let Some(mut message) = Message::from_value(value) else {
                continue;
            };
            if let Some(guild) = guild
                && value["member"].is_object()
            {
                self.members
                    .apply_member(guild, &value["member"], Some(&value["author"]));
            }
            message.channel_id = channel;
            fetched.push(message);
        }
        let ids: Vec<Id> = fetched.iter().map(|message| message.id).collect();
        let (more_before, more_after) = message_window::has_more(spec, &ids);
        let loaded = message_window::Loaded {
            is_before: spec.is_before(),
            is_after: spec.is_after(),
            before: spec.before,
            after: spec.after,
            limit: Some(spec.limit as usize),
            jump: spec.jump.clone(),
            has_more_before: more_before,
            has_more_after: more_after,
            avoid_initial_scroll: false,
            request_start_ms: (spec.started_ms > 0).then_some(spec.started_ms),
        };
        let list = self.messages.entry(channel).or_default();
        let window = self.windows.entry(channel).or_default();
        window.load_complete(list, fetched, &loaded);
        if spec.truncate && spec.is_before() != spec.is_after() {
            window.truncate(list, spec.is_before(), spec.is_after());
        }
    }

    /// `LOAD_MESSAGES_FAILURE`.
    pub fn messages_failed(&mut self, channel: Id) {
        self.windows.entry(channel).or_default().load_failed();
    }

    /// `CLEAR_MESSAGES` (`clearChannel`): the window is forgotten.
    pub fn clear_messages(&mut self, channel: Id) {
        self.messages.remove(&channel);
        self.windows.remove(&channel);
    }

    pub fn guild(&self, guild: Id) -> Option<&Guild> {
        self.guilds.iter().find(|known| known.id == guild)
    }
}

#[cfg(test)]
mod tests {
    /// A voice message as the gateway delivers it: the flag, one `audio/ogg` attachment with its length and waveform.
    #[test]
    fn a_voice_message_payload_keeps_its_flag_length_and_waveform() {
        let payload: serde_json::Value =
            serde_json::from_str(include_str!("../../../tests/fixtures/voice_message.json"))
                .unwrap();
        let message = super::Message::from_value(&payload).unwrap();
        assert!(message.is_voice_message());
        assert_eq!(message.content, "");
        assert_eq!(message.attachments.len(), 1);
        let file = &message.attachments[0];
        assert_eq!(file.filename, "voice-message.ogg");
        assert_eq!(file.content_type.as_deref(), Some("audio/ogg"));
        assert_eq!(file.duration_secs, Some(2.4));
        assert_eq!(file.waveform.as_deref().map(str::len), Some(60));
        // An ordinary upload has neither field.
        let mut plain = payload.clone();
        plain["flags"] = 0.into();
        plain["attachments"][0]["duration_secs"] = serde_json::Value::Null;
        plain["attachments"][0]
            .as_object_mut()
            .unwrap()
            .remove("waveform");
        let plain = super::Message::from_value(&plain).unwrap();
        assert!(!plain.is_voice_message());
        assert_eq!(plain.attachments[0].duration_secs, None);
        assert_eq!(plain.attachments[0].waveform, None);
    }

    /// Module 451919: users, `@everyone`/`@here` and roles, with the two suppress settings.
    #[test]
    fn mentions_me_follows_the_official_rule() {
        let message = |extra: serde_json::Value| {
            let mut value = serde_json::json!({
                "id":"1","channel_id":"2","content":"x","timestamp":"2026-11-07T12:00:00Z",
                "author":{"id":"9","username":"other"},"mentions":[],"mention_roles":[],"mention_everyone":false
            });
            for (key, field) in extra.as_object().unwrap() {
                value[key] = field.clone();
            }
            super::Message::from_value(&value).unwrap()
        };
        let me = 5;
        let roles = [70, 71];
        let user = message(serde_json::json!({"mentions":[{"id":"5","username":"me"}]}));
        assert!(user.mentions_me(me, &roles, true, true));
        let other = message(serde_json::json!({"mentions":[{"id":"6","username":"you"}]}));
        assert!(!other.mentions_me(me, &roles, false, false));
        let everyone = message(serde_json::json!({"mention_everyone":true}));
        assert!(everyone.mentions_me(me, &roles, false, false));
        assert!(!everyone.mentions_me(me, &roles, true, false));
        let role = message(serde_json::json!({"mention_roles":["71"]}));
        assert!(role.mentions_me(me, &roles, false, false));
        assert!(!role.mentions_me(me, &roles, false, true));
        assert!(!role.mentions_me(me, &[], false, false));
    }

    #[test]
    fn guide_metadata_and_member_flags_preserve_partial_updates() {
        let mut guild = super::Guild::from_value(&serde_json::json!({
            "id":"100", "name":"fixture", "channels":[], "home_header":"banner", "rules_channel_id":"200"
        })).unwrap();
        guild.set_my_member(&serde_json::json!({"flags":10,"joined_at":"2026-11-01T00:00:00Z"}));
        guild.set_my_member(&serde_json::json!({"roles":[]}));
        assert_eq!(guild.my_flags, 10);
        assert_eq!(guild.joined_at, "2026-11-01T00:00:00Z");
        guild.apply_update(&serde_json::json!({"name":"renamed"}));
        assert_eq!(guild.home_header.as_deref(), Some("banner"));
        assert_eq!(guild.rules_channel_id, Some(200));
        guild.apply_update(&serde_json::json!({"home_header":null,"rules_channel_id":null}));
        assert!(guild.home_header.is_none());
        assert!(guild.rules_channel_id.is_none());
    }

    #[test]
    fn attachment_animation_flags_survive_message_parsing() {
        let attachment = super::Attachment::from_value(&serde_json::json!({
            "id":"1", "filename":"motion.webp", "url":"https://example.com/motion.webp",
            "width":160, "height":96, "content_type":"image/webp", "flags":32
        }))
        .unwrap();
        assert!(attachment.is_image());
        assert!(attachment.is_animated());
        assert_eq!(attachment.flags, 32);
    }

    #[test]
    fn gifv_keeps_video_and_proxy_separate_from_its_poster() {
        let embed = super::Embed::from_value(&serde_json::json!({
            "type": "gifv",
            "thumbnail": {"url":"https://example.com/poster.png", "width":498, "height":498},
            "video": {"url":"https://example.com/clip.mp4", "proxy_url":"https://example.com/proxy.mp4", "width":498, "height":498}
        }));
        assert!(embed.is_inline_media());
        assert_eq!(embed.image.unwrap().url, "https://example.com/poster.png");
        let video = embed.video.unwrap();
        assert_eq!(video.url, "https://example.com/clip.mp4");
        assert_eq!(
            video.proxy_url.as_deref(),
            Some("https://example.com/proxy.mp4")
        );
        assert!(
            super::Embed::from_value(
                &serde_json::json!({"type":"gifv", "video":{"url":"invalid"}})
            )
            .video
            .is_none()
        );
    }

    #[test]
    fn lazy_member_users_are_available_to_profiles() {
        use serde_json::json;
        let mut stores = super::Stores::default();
        stores.dispatch("GUILD_MEMBER_LIST_UPDATE", &json!({"guild_id":"100", "ops":[
            {"op":"SYNC", "range":[0,0], "items":[{"member":{"user":{"id":"200","username":"first"}}}]},
            {"op":"INSERT", "index":1, "item":{"member":{"user":{"id":"201","username":"second"}}}}
        ]}));
        assert_eq!(stores.users[&200].username, "first");
        assert_eq!(stores.users[&201].username, "second");
        stores.dispatch(
            "GUILD_MEMBERS_CHUNK",
            &json!({"guild_id":"100", "members":[
                {"user":{"id":"202","username":"guide-author"}}
            ]}),
        );
        assert_eq!(stores.users[&202].username, "guide-author");
        stores.dispatch("GUILD_MEMBER_LIST_UPDATE", &json!({"guild_id":"100", "ops":[
            {"op":"UPDATE", "index":0, "item":{"member":{"user":{"id":"200","username":"renamed"}}}}
        ]}));
        assert_eq!(stores.users[&200].username, "renamed");
    }

    #[test]
    fn member_color_uses_the_highest_colored_role_and_snowflake_ties() {
        let guild = super::Guild::from_value(&serde_json::json!({
            "id":"100", "name":"fixture", "channels":[], "roles":[
                {"id":"100", "name":"everyone", "position":999, "color":1},
                {"id":"101", "name":"lower", "position":1, "color":2},
                {"id":"102", "name":"colorless", "position":10, "color":0},
                {"id":"103", "name":"older", "position":5, "color":3},
                {"id":"104", "name":"newer", "position":5, "color":4}
            ]
        }))
        .unwrap();
        assert_eq!(guild.role_color(&[101, 102, 104, 103]), Some(3));
        assert_eq!(guild.role_color(&[100, 101]), Some(2));
        assert_eq!(guild.role_color(&[104, 101]), Some(4));
        assert_eq!(guild.role_color(&[102, 999]), None);
        assert_eq!(guild.role_color(&[]), None);
    }

    #[test]
    fn history_retains_member_roles_for_author_colors() {
        let mut stores = super::Stores::default();
        stores.dispatch(
            "GUILD_CREATE",
            &serde_json::json!({
                "id":"100", "name":"fixture", "channels":[{"id":"200", "name":"chat", "type":0}]
            }),
        );
        stores.load_messages(
            200,
            &serde_json::json!([{
            "id":"300", "channel_id":"200", "content":"history", "timestamp":"2026-11-09T12:00:00Z",
                "author":{"id":"400", "username":"member"},
                "member":{"nick":"nickname", "roles":["500"]}
            }]),
        );
        let member = stores.members.get(100, 400).unwrap();
        assert_eq!(member.roles, [500]);
        assert_eq!(member.nick.as_deref(), Some("nickname"));
        assert_eq!(stores.messages[&200][0].channel_id, 200);
    }

    #[test]
    fn guild_update_changes_the_guilds_own_properties_only() {
        let mut stores = super::Stores::default();
        stores.dispatch(
            "GUILD_CREATE",
            &serde_json::json!({"id": "5", "name": "old", "channels": [{"id": "9", "type": 0, "name": "x"}]}),
        );
        stores.dispatch(
            "GUILD_UPDATE",
            &serde_json::json!({
                "id": "5",
                "name": "new",
                "features": ["MEMBER_VERIFICATION_GATE_ENABLED"],
                "premium_tier": 2,
            }),
        );
        let guild = stores.guild(5).unwrap();
        assert_eq!(guild.name, "new");
        assert_eq!(guild.features, ["MEMBER_VERIFICATION_GATE_ENABLED"]);
        assert_eq!(guild.premium_tier, 2);
        assert_eq!(guild.channels.len(), 1);
    }

    use super::*;

    #[test]
    fn article_thumbnails_become_the_image_and_size_the_embed() {
        let embed = Embed::from_value(&json!({
            "type": "article",
            "url": "https://github.com/zed-industries/zed",
            "title": "zed",
            "provider": {"name": "GitHub"},
            "thumbnail": {"url": "https://x/y.png", "proxy_url": "https://p/y.png", "width": 1200, "height": 600}
        }));
        assert!(embed.thumbnail.is_none());
        let image = embed.image.as_ref().unwrap();
        assert_eq!(Embed::fitted(image), (400., 200.));
        assert_eq!(embed.max_width(), Some(432.));
    }

    #[test]
    fn link_embeds_keep_a_side_thumbnail_and_late_embeds_arrive_by_update() {
        let embed = Embed::from_value(&json!({
            "type": "link",
            "thumbnail": {"url": "https://x/y.png", "width": 80, "height": 80}
        }));
        assert!(embed.image.is_none() && embed.thumbnail.is_some());
        let mut stores = Stores::default();
        stores.dispatch("MESSAGE_CREATE", &message("5", "x"));
        stores.dispatch(
            "MESSAGE_UPDATE",
            &json!({"id": "5", "channel_id": "23", "embeds": [{"type": "link", "title": "t"}]}),
        );
        assert_eq!(
            stores.messages[&23][0].embeds[0].title.as_deref(),
            Some("t")
        );
    }

    #[test]
    fn people_already_in_voice_come_from_ready_supplemental() {
        let mut stores = Stores::default();
        stores.dispatch(
            "READY_SUPPLEMENTAL",
            &json!({"guilds": [{"id": "5", "voice_states": [
                {"user_id": "7", "channel_id": "9", "session_id": "s", "self_video": true},
                {"user_id": "8", "channel_id": "9", "session_id": "t"},
            ]}]}),
        );
        let members = stores.voice_members(5, 9);
        assert_eq!(members.len(), 2);
        assert!(members[0].self_video);
    }

    #[test]
    fn voice_states_follow_updates_and_leaves() {
        let mut stores = Stores::default();
        stores.dispatch(
            "VOICE_STATE_UPDATE",
            &json!({"guild_id": "1", "channel_id": "9", "user_id": "5", "session_id": "s", "self_mute": true}),
        );
        assert_eq!(stores.voice_members(1, 9)[0].session_id, "s");
        assert!(stores.voice_members(1, 9)[0].self_mute);
        stores.dispatch(
            "VOICE_STATE_UPDATE",
            &json!({"guild_id": "1", "channel_id": null, "user_id": "5"}),
        );
        assert!(stores.voice_members(1, 9).is_empty());
        stores.dispatch(
            "VOICE_SERVER_UPDATE",
            &json!({"guild_id": "1", "endpoint": "c-ams.discord.media", "token": "t"}),
        );
        assert_eq!(
            stores.voice_server.as_ref().unwrap().endpoint,
            "c-ams.discord.media"
        );
    }

    #[test]
    fn a_dm_call_keeps_its_voice_states_under_guild_zero() {
        let mut stores = Stores::default();
        // `CALL_CREATE` embeds the states with `guildId: null` (module 225123), and later updates have no guild.
        stores.dispatch(
            "VOICE_STATE_UPDATE",
            &json!({"guild_id": null, "channel_id": "500", "user_id": "5", "session_id": "s", "self_video": true}),
        );
        stores.dispatch(
            "VOICE_STATE_UPDATE",
            &json!({"channel_id": "500", "user_id": "6", "session_id": "t"}),
        );
        let members = stores.voice_members(0, 500);
        assert_eq!(members.len(), 2);
        assert!(
            members
                .iter()
                .any(|state| state.user_id == 5 && state.self_video)
        );
        stores.dispatch(
            "VOICE_STATE_UPDATE",
            &json!({"guild_id": null, "channel_id": null, "user_id": "5"}),
        );
        assert_eq!(stores.voice_members(0, 500).len(), 1);
        stores.dispatch(
            "VOICE_SERVER_UPDATE",
            &json!({"channel_id": "500", "endpoint": "c-ams.discord.media", "token": "t"}),
        );
        let server = stores.voice_server.as_ref().unwrap();
        assert_eq!((server.guild_id, server.channel_id), (0, Some(500)));
    }

    #[test]
    fn badges_follow_the_official_precedence() {
        let guild = |features: &[&str]| Guild {
            id: 1,
            name: String::new(),
            icon: None,
            owner_id: None,
            home_header: None,
            rules_channel_id: None,
            joined_at: String::new(),
            default_message_notifications: 0,
            nsfw_level: 0,
            channels: Vec::new(),
            roles: HashMap::new(),
            role_permissions: HashMap::new(),
            role_info: HashMap::new(),
            role_icons: HashMap::new(),
            my_roles: Vec::new(),
            my_pending: false,
            my_flags: 0,
            my_timeout_until: None,
            mfa_level: 0,
            vanity_url_code: None,
            invites_disabled_until: None,
            dms_disabled_until: None,
            lockdown_duration_hours: None,
            features: features
                .iter()
                .map(|feature| (*feature).to_owned())
                .collect(),
            premium_tier: 0,
            premium_subscription_count: 0,
            emojis: Vec::new(),
            threads: Vec::new(),
        };
        assert_eq!(guild(&[]).badge(), Badge::None);
        assert_eq!(guild(&["COMMUNITY"]).badge(), Badge::Community);
        assert_eq!(
            guild(&["COMMUNITY", "DISCOVERABLE"]).badge(),
            Badge::Discoverable
        );
        assert_eq!(guild(&["VERIFIED", "COMMUNITY"]).badge(), Badge::Verified);
        assert_eq!(
            guild(&["VERIFIED", "PARTNERED"]).badge(),
            Badge::VerifiedAndPartnered
        );
        assert_eq!(
            guild(&["PARTNERED", "INTERNAL_EMPLOYEE_ONLY"]).badge(),
            Badge::Staff
        );
    }
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
    fn ready_threads_keep_the_members_flags_mute_and_newest_message() {
        let mut stores = Stores::default();
        stores.dispatch(
            "READY",
            &json!({"user": {"id": "5", "username": "me"}, "guilds": [{"id": "1", "joined_at": "2026-01-01T00:00:00+00:00",
                "properties": {"name": "g"}, "channels": [{"id": "2", "type": 0, "name": "c"}],
                "threads": [
                    {"id": "7", "guild_id": "1", "parent_id": "2", "type": 11, "name": "in", "last_message_id": "70",
                     "thread_metadata": {"archived": false},
                     "member": {"id": "7", "user_id": "5", "flags": 4, "muted": true,
                         "mute_config": {"selected_time_window": 3600, "end_time": "2099-01-01T00:00:00+00:00"},
                         "join_timestamp": "2026-11-01T00:00:00+00:00"}},
                    {"id": "8", "guild_id": "1", "parent_id": "2", "type": 11, "name": "out",
                     "thread_metadata": {"archived": false}}]}]}),
        );
        let guild = &stores.guilds[0];
        let joined = &guild.threads[0];
        assert!(joined.joined && joined.muted && joined.is_muted(chrono::Utc::now()));
        assert_eq!(joined.member_flags, Some(THREAD_ONLY_MENTIONS));
        assert_eq!(
            joined.join_timestamp.as_deref(),
            Some("2026-11-01T00:00:00+00:00")
        );
        assert!(!guild.threads[1].joined && guild.threads[1].member_flags.is_none());
        assert_eq!(stores.read_states.latest.get(&7), Some(&70));
        assert_eq!(stores.guild_of_channel(8).map(|guild| guild.id), Some(1));
        // An update without a member keeps the one known.
        stores.dispatch(
            "THREAD_UPDATE",
            &json!({"id": "7", "guild_id": "1", "parent_id": "2", "type": 11, "name": "renamed",
                "thread_metadata": {"archived": false}}),
        );
        let kept = &stores.guilds[0].threads.iter().find(|t| t.id == 7).unwrap();
        assert_eq!(
            (kept.name.as_str(), kept.member_flags, kept.muted),
            ("renamed", Some(4), true)
        );
    }

    #[test]
    fn thread_events_keep_the_active_list() {
        let mut stores = Stores::default();
        stores.dispatch(
            "GUILD_CREATE",
            &json!({"id": "1", "name": "g", "channels": [], "threads": []}),
        );
        // As recorded in TRAFFIC/thread-create.json.
        stores.dispatch(
            "THREAD_CREATE",
            &json!({"guild_id": "1", "id": "9", "parent_id": "5", "name": "t", "type": 11, "newly_created": true,
                "owner_id": "3", "thread_metadata": {"archived": false}}),
        );
        let guild = stores.guilds.iter().find(|guild| guild.id == 1).unwrap();
        assert_eq!(guild.threads.len(), 1);
        assert!(guild.threads[0].joined);
        stores.dispatch("THREAD_DELETE", &json!({"guild_id": "1", "id": "9"}));
        assert!(stores.guilds[0].threads.is_empty());
    }

    #[test]
    fn settings_are_not_written_before_they_have_arrived() {
        let mut stores = Stores::default();
        // Nothing loaded: a patch would replace the whole message on the server.
        assert_eq!(stores.dismiss_content(133), "");
        assert_eq!(stores.set_pref(&settings::THEME, 2), "");
        assert_eq!(stores.set_status("idle", 0, 0), "");
        assert_eq!(stores.set_custom_status(None, 0), "");
        assert!(!stores.content_dismissed(133));
        // READY without the settings field leaves it so.
        stores.dispatch("READY", &json!({"users": []}));
        assert_eq!(stores.dismiss_content(133), "");
        // An account whose settings are empty has still sent them.
        stores.dispatch("READY", &json!({"users": [], "user_settings_proto": ""}));
        assert_ne!(stores.dismiss_content(133), "");
        assert!(stores.content_dismissed(133));
    }

    #[test]
    fn friendship_dates_follow_ready_add_update_and_removal() {
        let mut stores = Stores::default();
        let since = "2026-11-09T12:00:00.000000+00:00";
        stores.dispatch(
            "READY",
            &json!({
                "users":[{"id":"30","username":"Zed"},{"id":"31","username":"Beta"}],
                "relationships":[{"id":"30","type":1,"since":since},{"id":"31","type":1}]
            }),
        );
        assert_eq!(
            stores.relationship_since.get(&30).map(String::as_str),
            Some(since)
        );
        assert!(!stores.relationship_since.contains_key(&31));
        // 994500: ADD keeps a date it does not carry, UPDATE drops one it does not carry.
        stores.dispatch("RELATIONSHIP_ADD", &json!({"id":"30","type":1}));
        assert_eq!(
            stores.relationship_since.get(&30).map(String::as_str),
            Some(since)
        );
        stores.dispatch(
            "RELATIONSHIP_ADD",
            &json!({"id":"31","type":1,"since":"2026-11-10T08:00:00+00:00"}),
        );
        assert!(stores.relationship_since.contains_key(&31));
        stores.dispatch(
            "RELATIONSHIP_UPDATE",
            &json!({"id":"30","type":1,"since":null}),
        );
        assert!(!stores.relationship_since.contains_key(&30));
        stores.dispatch("RELATIONSHIP_REMOVE", &json!({"id":"31"}));
        assert!(!stores.relationship_since.contains_key(&31));
        // A new READY replaces everything.
        stores.dispatch(
            "RELATIONSHIP_ADD",
            &json!({"id":"30","type":1,"since":since}),
        );
        stores.dispatch("READY", &json!({"users":[],"relationships":[]}));
        assert!(stores.relationship_since.is_empty());
    }

    #[test]
    fn relationship_nicknames_follow_ready_add_update_and_removal() {
        let mut stores = Stores::default();
        stores.dispatch(
            "READY",
            &json!({
                "users":[{"id":"30","username":"Zed"},{"id":"31","username":"Beta"}],
                "relationships":[{"id":"30","type":1,"nickname":"private name"},{"id":"31","type":1}]
            }),
        );
        assert_eq!(stores.friend_nickname(30), Some("private name"));
        assert_eq!(
            stores
                .friends()
                .iter()
                .map(|user| user.id)
                .collect::<Vec<_>>(),
            [31, 30]
        );
        // 994500 ADD preserves an omitted/null nickname; UPDATE clears it.
        stores.dispatch(
            "RELATIONSHIP_ADD",
            &json!({"id":"30","type":1,"nickname":null}),
        );
        assert_eq!(stores.friend_nickname(30), Some("private name"));
        stores.dispatch(
            "RELATIONSHIP_UPDATE",
            &json!({"id":"30","type":1,"nickname":"aaa"}),
        );
        assert_eq!(stores.friend_nickname(30), Some("aaa"));
        assert_eq!(
            stores
                .friends()
                .iter()
                .map(|user| user.id)
                .collect::<Vec<_>>(),
            [30, 31]
        );
        stores.dispatch("RELATIONSHIP_UPDATE", &json!({"id":"30","type":1}));
        assert_eq!(stores.friend_nickname(30), None);
        stores.dispatch(
            "RELATIONSHIP_ADD",
            &json!({"id":"30","type":1,"nickname":""}),
        );
        assert_eq!(stores.friend_nickname(30), Some(""));
        stores.dispatch("RELATIONSHIP_REMOVE", &json!({"id":"30"}));
        assert_eq!(stores.friend_nickname(30), None);
        stores.dispatch(
            "RELATIONSHIP_ADD",
            &json!({"id":"30","type":1,"nickname":"stale"}),
        );
        stores.dispatch("READY", &json!({"relationships":[]}));
        assert!(stores.relationship_nicknames.is_empty());
    }

    #[test]
    fn relationship_and_dm_events_update_the_lists() {
        let mut stores = Stores::default();
        stores.dispatch(
            "RELATIONSHIP_ADD",
            &json!({"id": "30", "type": 3, "user": {"id": "30", "username": "someone"}}),
        );
        assert_eq!(stores.relationships.get(&30), Some(&3));
        assert!(stores.users.contains_key(&30));
        stores.dispatch("RELATIONSHIP_REMOVE", &json!({"id": "30", "type": 3}));
        assert!(!stores.relationships.contains_key(&30));
        stores.dispatch(
            "CHANNEL_CREATE",
            &json!({"id": "40", "type": 1, "recipients": [{"id": "30", "username": "someone"}]}),
        );
        assert_eq!(stores.private_channels[0].recipients, vec![30]);
        stores.dispatch(
            "GUILD_CREATE",
            &json!({"id": "1", "name": "g", "channels": []}),
        );
        stores.dispatch(
            "CHANNEL_CREATE",
            &json!({"id": "50", "guild_id": "1", "type": 0, "name": "new-channel", "position": 3}),
        );
        assert_eq!(stores.guilds[0].channels.len(), 1);
        stores.dispatch(
            "CHANNEL_DELETE",
            &json!({"id": "50", "guild_id": "1", "type": 0}),
        );
        assert!(stores.guilds[0].channels.is_empty());
        stores.dispatch("CHANNEL_DELETE", &json!({"id": "40", "type": 1}));
        assert!(stores.private_channels.is_empty());
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
    fn a_detached_window_ignores_gateway_messages_and_around_pages_replace_it() {
        use message_window::{FetchSpec, Jump};
        let mut stores = Stores::default();
        stores.dispatch("MESSAGE_CREATE", &message("900", "newest"));
        let spec = FetchSpec {
            channel: 23,
            limit: 30,
            jump: Some(Jump::to_message(115, true, None)),
            ..FetchSpec::default()
        };
        assert!(
            stores.fetch_messages(&spec),
            "a message that is not loaded needs a fetch"
        );
        let page: Vec<Value> = (100..130u64)
            .rev()
            .map(|id| message(&id.to_string(), "old"))
            .collect();
        stores.messages_loaded(&spec, &Value::Array(page));
        let window = stores.window(23);
        assert!(window.has_more_before && window.has_more_after);
        assert_eq!(window.jump_target_id, Some(115));
        assert_eq!(stores.messages[&23].len(), 30);
        // A message that arrives now only counts as unread: the window does not take it.
        stores.dispatch("MESSAGE_CREATE", &message("950", "new"));
        assert_eq!(stores.messages[&23].last().map(|m| m.id), Some(129));
        // A jump to a message the window holds needs no fetch.
        let inside = FetchSpec {
            channel: 23,
            limit: 30,
            jump: Some(Jump::to_message(120, true, None)),
            ..FetchSpec::default()
        };
        assert!(!stores.fetch_messages(&inside));
        assert_eq!(stores.window(23).jump_target_id, Some(120));
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
    fn media_shrinks_to_fit_and_never_grows() {
        assert_eq!(fit_media(400, 240), (400, 240));
        assert_eq!(fit_media(1100, 700), (550, 350));
        assert_eq!(fit_media(2000, 500), (550, 138));
        assert_eq!(fit_media(100, 1000), (35, 350));
    }

    #[test]
    fn attachments_parse_and_know_images() {
        let mut value = message("6", "x");
        value["attachments"] = json!([
            {"id": "1", "filename": "a.png", "proxy_url": "p", "content_type": "image/png", "width": 4, "height": 3, "size": 9},
            {"id": "2", "filename": "b.txt", "url": "u", "content_type": "text/plain", "size": 1},
        ]);
        let parsed = Message::from_value(&value).unwrap();
        assert!(parsed.attachments[0].is_image());
        assert!(!parsed.attachments[1].is_image());
        assert_eq!(parsed.attachments[1].proxy_url, "u");
    }

    fn reaction_event(user: &str) -> Value {
        json!({"channel_id": "23", "message_id": "5", "user_id": user, "emoji": {"name": "\u{1f44d}", "id": null}})
    }

    #[test]
    fn reactions_count_up_and_down_and_track_mine() {
        let mut stores = Stores {
            current_user: Some(User {
                id: 10,
                ..User::default()
            }),
            ..Stores::default()
        };
        stores.dispatch("MESSAGE_CREATE", &message("5", "x"));
        stores.dispatch("MESSAGE_REACTION_ADD", &reaction_event("10"));
        stores.dispatch("MESSAGE_REACTION_ADD", &reaction_event("11"));
        let reaction = &stores.messages[&23][0].reactions[0];
        assert_eq!((reaction.count, reaction.me), (2, true));
        stores.dispatch("MESSAGE_REACTION_REMOVE", &reaction_event("10"));
        let reaction = &stores.messages[&23][0].reactions[0];
        assert_eq!((reaction.count, reaction.me), (1, false));
        stores.dispatch("MESSAGE_REACTION_REMOVE", &reaction_event("11"));
        assert!(stores.messages[&23][0].reactions.is_empty());
    }

    #[test]
    fn removing_an_unknown_reaction_changes_nothing() {
        let mut stores = Stores::default();
        stores.dispatch("MESSAGE_CREATE", &message("5", "x"));
        stores.dispatch("MESSAGE_REACTION_REMOVE", &reaction_event("11"));
        assert!(stores.messages[&23][0].reactions.is_empty());
    }

    #[test]
    fn typing_expires_and_ignores_myself() {
        let mut stores = Stores {
            current_user: Some(User {
                id: 10,
                ..User::default()
            }),
            ..Stores::default()
        };
        stores.dispatch(
            "TYPING_START",
            &json!({"channel_id": "23", "user_id": "10"}),
        );
        stores.dispatch(
            "TYPING_START",
            &json!({"channel_id": "23", "user_id": "11"}),
        );
        let now = Instant::now();
        assert_eq!(stores.typing_users(23, now), [11]);
        assert!(
            stores
                .typing_users(23, now + TYPING_DURATION + Duration::from_secs(1))
                .is_empty()
        );
    }

    #[test]
    fn a_sent_message_ends_the_typing_notice() {
        let mut stores = Stores::default();
        stores.dispatch(
            "TYPING_START",
            &json!({"channel_id": "23", "user_id": "10"}),
        );
        stores.dispatch("MESSAGE_CREATE", &message("5", "x"));
        assert!(stores.typing_users(23, Instant::now()).is_empty());
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

    #[test]
    fn ignored_users_follow_ready_and_relationship_events() {
        let mut stores = Stores::default();
        stores.dispatch(
            "READY",
            &json!({
                "user": {"id": "1", "username": "me"},
                "relationships": [
                    {"id": "20", "type": 0, "user_ignored": true},
                    {"id": "21", "type": 1},
                    {"id": "22", "type": 2, "user_ignored": true}
                ]
            }),
        );
        assert!(stores.is_ignored(20));
        assert!(!stores.is_ignored(21));
        // Blocked wins over ignored, as `isIgnored` checks.
        assert!(!stores.is_ignored(22));
        stores.dispatch(
            "RELATIONSHIP_UPDATE",
            &json!({"id": "21", "type": 1, "user_ignored": true}),
        );
        assert!(stores.is_ignored(21));
        stores.set_ignored(21, false);
        assert!(!stores.is_ignored(21));
    }

    #[test]
    fn settings_proto_updates_read_the_speedbump() {
        let mut stores = Stores::default();
        let patch = settings::ignore_speedbump_disabled_patch();
        stores.dispatch(
            "USER_SETTINGS_PROTO_UPDATE",
            &json!({"settings": {"type": 1, "proto": patch}, "partial": true}),
        );
        assert!(stores.ignore_speedbump_disabled);
        // A partial update without the field leaves it alone; a full one without it clears it.
        stores.dispatch(
            "USER_SETTINGS_PROTO_UPDATE",
            &json!({"settings": {"type": 1, "proto": ""}, "partial": true}),
        );
        assert!(stores.ignore_speedbump_disabled);
        stores.dispatch(
            "USER_SETTINGS_PROTO_UPDATE",
            &json!({"settings": {"type": 1, "proto": ""}, "partial": false}),
        );
        assert!(!stores.ignore_speedbump_disabled);
    }

    #[test]
    fn notification_quiet_mode_follows_whole_message_replacement() {
        let mut stores = Stores::default();
        let quiet = settings::encode_varint_at(&[7, 5], 1, 1);
        stores.dispatch("READY", &json!({"user_settings_proto": quiet}));
        assert!(stores.notification_quiet_mode());
        for (proto, expected) in [
            (settings::encode_varint_at(&[13], 1, 3), true),
            (settings::encode_varint_at(&[7, 2], 1, 1), false),
        ] {
            stores.dispatch(
                "USER_SETTINGS_PROTO_UPDATE",
                &json!({"settings": {"type": 1, "proto": proto}, "partial": true}),
            );
            assert_eq!(stores.notification_quiet_mode(), expected);
        }
    }

    #[test]
    fn settings_proto_carries_client_preferences() {
        let mut stores = Stores::default();
        let embeds = settings::PROTO_PREFS
            .iter()
            .find(|pref| pref.key == "chat_embeds_render_embeds")
            .unwrap();
        let embeds_off = settings::pref_patch(embeds, 0);
        stores.dispatch(
            "USER_SETTINGS_PROTO_UPDATE",
            &json!({"settings": {"type": 1, "proto": embeds_off}, "partial": true}),
        );
        assert_eq!(stores.prefs.get("chat_embeds_render_embeds"), Some(&0));
        // A partial update without the key leaves it; a full one without it resets it.
        stores.dispatch(
            "USER_SETTINGS_PROTO_UPDATE",
            &json!({"settings": {"type": 1, "proto": ""}, "partial": true}),
        );
        assert_eq!(stores.prefs.get("chat_embeds_render_embeds"), Some(&0));
        stores.dispatch(
            "USER_SETTINGS_PROTO_UPDATE",
            &json!({"settings": {"type": 1, "proto": ""}, "partial": false}),
        );
        assert!(stores.prefs.is_empty());
    }

    #[test]
    fn a_change_is_built_on_what_the_server_sent() {
        let mut stores = Stores::default();
        // READY: appearance holds the theme DARKER.
        let (ready, _) = settings::set_pref(&[], &settings::THEME, 3);
        stores.dispatch("READY", &json!({"user_settings_proto": ready}));
        let hour = settings::PROTO_PREFS
            .iter()
            .find(|pref| pref.key == "time_format_setting")
            .unwrap();
        let patch = stores.set_pref(hour, 2);
        // The patch still names the theme, and the store shows the new hour cycle.
        assert_eq!(settings::varint_at(&patch, &[13], 1), Some(3));
        assert_eq!(settings::varint_at(&patch, hour.path, hour.field), Some(2));
        assert_eq!(stores.prefs.get("time_format_setting"), Some(&2));
    }

    #[test]
    fn a_status_choice_shows_at_once_and_follows_other_sessions() {
        let mut stores = Stores::default();
        stores.dispatch(
            "READY",
            &json!({"sessions": [{"status": "dnd"}], "user_settings_proto": ""}),
        );
        assert_eq!(stores.status, "dnd");
        let patch = stores.set_status("idle", 5_000, 1);
        assert_eq!(stores.status, "idle");
        let state = stores.status_state().expect("status");
        assert_eq!(
            (state.status.as_deref(), state.expires_at_ms),
            (Some("idle"), 5_000)
        );
        // The patch is the whole status message, so the server keeps the rest of it.
        let sent = settings::read_status(&settings::decode(&patch)).expect("patch");
        assert_eq!(sent, state);
        // Another session chooses invisible.
        let (other, _) = settings::set_status(&[], "invisible", 0, 2);
        stores.dispatch(
            "USER_SETTINGS_PROTO_UPDATE",
            &json!({"settings": {"type": 1, "proto": other}, "partial": true}),
        );
        assert_eq!(stores.status, "invisible");
        let custom = settings::CustomStatus {
            text: "afk".into(),
            ..Default::default()
        };
        stores.set_custom_status(Some(&custom), 3);
        assert_eq!(
            stores.status_state().and_then(|state| state.custom),
            Some(custom)
        );
    }
}
