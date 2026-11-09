//! Flux stores. M0 slice: the parts of UserStore, GuildStore, ChannelStore and MessageStore the first screen reads.
//! Names and getters follow `STORES`; the rest of each store is tracked as open rows in `FEATURES.md`.

pub mod autocomplete;
pub mod confusables;
pub mod experiments;
pub mod frecency;
pub mod grouping;
pub mod guild_settings;
pub mod member_list;
pub mod members;
pub mod notifications;
pub mod permissions;
pub mod premium;
pub mod read_state;
pub mod slowmode;
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
}

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

#[derive(Clone, Debug, PartialEq)]
pub struct Guild {
    pub id: Id,
    pub name: String,
    pub icon: Option<String>,
    pub owner_id: Option<Id>,
    pub joined_at: String,
    pub default_message_notifications: u64,
    pub nsfw_level: u64,
    pub channels: Vec<Channel>,
    pub roles: HashMap<Id, String>,
    /// Role id to its permission bits (the @everyone role has the guild's id).
    pub role_permissions: HashMap<Id, u64>,
    /// What a role's mention and list order read: position, colour and whether anyone may mention it.
    pub role_info: HashMap<Id, RoleInfo>,
    /// The current user's roles here (READY `merged_members`, then GUILD_MEMBER_UPDATE).
    pub my_roles: Vec<Id>,
    /// The current user has not passed the rules screening yet (`pending` on their member).
    pub my_pending: bool,
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
}

impl Thread {
    /// A pinned forum post (`ChannelFlags.PINNED`).
    pub fn pinned(&self) -> bool {
        self.flags & 2 != 0
    }

    pub fn from_value(value: &Value) -> Option<Self> {
        let kind = value["type"].as_u64()?;
        if !matches!(kind, 10..=12) {
            return None;
        }
        Some(Self {
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
        })
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

/// The parts of a role the composer's mention list and the member list read.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RoleInfo {
    pub position: i64,
    /// The role's colour as `0xRRGGBB`; 0 is none.
    pub color: u32,
    pub mentionable: bool,
}

impl RoleInfo {
    fn from_value(role: &Value) -> Self {
        Self {
            position: role["position"].as_i64().unwrap_or(0),
            color: role["color"].as_u64().unwrap_or(0) as u32,
            mentionable: role["mentionable"].as_bool().unwrap_or(false),
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
            my_roles: Vec::new(),
            my_pending: false,
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
        }
        if value["emojis"].is_array() {
            self.emojis = GuildEmoji::list(&value["emojis"]);
        }
    }

    /// The current user's member object: roles, screening and timeout.
    pub fn set_my_member(&mut self, member: &Value) {
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
            ..Self::default()
        };
        // Articles, images and GIFs (`gifv`, drawn from the thumbnail's still frame) show it as the large image.
        if let Some(thumbnail) = EmbedMedia::from_value(&value["thumbnail"]) {
            match kind.as_str() {
                "article" | "image" | "gifv" => embed.image = Some(thumbnail),
                _ => embed.thumbnail = Some(thumbnail),
            }
        }
        if let Some(image) = EmbedMedia::from_value(&value["image"]) {
            embed.image = Some(image);
        }
        // Only rich and auto-moderation embeds carry fields (`u.pd`).
        if matches!(kind.as_str(), "rich" | "auto_moderation_message") {
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

#[derive(Clone, Debug, PartialEq)]
pub struct Message {
    pub id: Id,
    pub channel_id: Id,
    pub author: User,
    pub content: String,
    pub edited: bool,
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
    pub blocked: bool,
    pub ignored: bool,
    /// A forwarded message's copy (`message_snapshots[0].message`) and where it came from.
    pub forwarded: Option<Box<Forwarded>>,
    pub pinned: bool,
    /// `message_reference.message_id`: what a reply, a forward or a pin notice points at.
    pub reference_id: Option<Id>,
    /// `message_reference.channel_id`: for THREAD_CREATED, the thread.
    pub reference_channel_id: Option<Id>,
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

impl Message {
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
            blocked: false,
            ignored: false,
            forwarded: Forwarded::from_value(value).map(Box::new),
            pinned: value["pinned"].as_bool().unwrap_or(false),
            reference_id: id(&value["message_reference"]["message_id"]),
            reference_channel_id: id(&value["message_reference"]["channel_id"]),
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
    pub application_id: Option<Id>,
    pub message_request: bool,
}

impl PrivateChannel {
    fn from_value(value: &Value) -> Option<Self> {
        Some(Self {
            id: id(&value["id"])?,
            kind: value["type"].as_u64()?,
            // READY lists `recipient_ids`; CHANNEL_CREATE and `POST /users/@me/channels` send `recipients` users.
            recipients: match value["recipient_ids"].as_array() {
                Some(ids) => ids.iter().filter_map(id).collect(),
                None => value["recipients"]
                    .as_array()
                    .map(|users| users.iter().filter_map(|user| id(&user["id"])).collect())
                    .unwrap_or_default(),
            },
            last_message_id: id(&value["last_message_id"]).unwrap_or(0),
            name: text(&value["name"]),
            application_id: id(&value["application_id"]),
            message_request: value["is_message_request"].as_bool().unwrap_or(false)
                || value["is_spam"].as_bool().unwrap_or(false),
        })
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
    pub guild_id: Id,
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
    /// `appearance.theme` from the settings proto: 0 unset, 1 dark, 2 light, 3 darker, 4 midnight.
    pub theme: u64,
    pub account: Account,
    pub users: HashMap<Id, User>,
    /// Channel to who is typing and when that notice ends.
    pub typing: HashMap<Id, Vec<(Id, Instant)>>,
    /// User id to relationship type.
    pub relationships: HashMap<Id, u64>,
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
    pub current_user: Option<User>,
    /// This session's status as READY reports it.
    pub status: String,
    /// READY's `session_id`, which interactions carry.
    pub session_id: Option<String>,
    pub guild_order: Vec<Id>,
    /// Client preferences kept in the account's proto settings (`settings::PROTO_PREFS`), by row key: the varint as
    /// stored. A key that is absent was never set, and the preference keeps its default.
    pub prefs: HashMap<&'static str, u64>,
    /// The account's proto settings as the server last gave them, for a change to be built on.
    settings_raw: Vec<u8>,
    pub guilds: Vec<Guild>,
    pub messages: HashMap<Id, Vec<Message>>,
    pub read_states: read_state::ReadStates,
    /// Bookmarks and reminders (`SavedMessagesStore`): (channel, message) to the reminder's `due_at`, `None` for a
    /// plain bookmark.
    pub saved: HashMap<(Id, Id), Option<String>>,
    pub guild_settings: guild_settings::GuildSettingsStore,
    pub notification_flags: u64,
    /// The members seen so far, per guild (`GuildMemberStore`).
    pub members: members::GuildMembers,
}

impl Stores {
    pub fn dispatch(&mut self, name: &str, data: &Value) {
        self.members.dispatch(name, data);
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
                for presence in data["presences"].as_array().into_iter().flatten() {
                    self.set_presence(presence);
                }
            }
            "GUILD_MEMBER_LIST_UPDATE" => {
                if let Some(guild) = id(&data["guild_id"]) {
                    self.member_lists.entry(guild).or_default().apply(data);
                    self.members.apply_list_update(guild, data);
                }
            }
            "VOICE_STATE_UPDATE" => {
                if let (Some(guild), Some(user)) = (id(&data["guild_id"]), id(&data["user_id"])) {
                    let list = self.voice_states.entry(guild).or_default();
                    list.retain(|state| state.user_id != user);
                    if let Some(state) = VoiceState::from_value(data) {
                        list.push(state);
                    }
                }
            }
            "VOICE_SERVER_UPDATE" => {
                if let (Some(guild_id), Some(endpoint), Some(token)) = (
                    id(&data["guild_id"]),
                    text(&data["endpoint"]),
                    text(&data["token"]),
                ) {
                    self.voice_server = Some(VoiceServer {
                        guild_id,
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
                    self.guilds.retain(|known| known.id != guild.id);
                    self.guilds.push(guild);
                    self.sort_guilds();
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
                    self.insert_message(message);
                }
            }
            "THREAD_CREATE" | "THREAD_UPDATE" => {
                if let (Some(guild), Some(mut thread)) =
                    (id(&data["guild_id"]), Thread::from_value(data))
                    && let Some(guild) = self.guilds.iter_mut().find(|known| known.id == guild)
                {
                    if let Some(known) = guild.threads.iter().find(|known| known.id == thread.id) {
                        thread.joined |= known.joined;
                    }
                    guild.threads.retain(|known| known.id != thread.id);
                    if !thread.archived {
                        guild.threads.push(thread);
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
            "THREAD_MEMBER_UPDATE" => {
                if let (Some(guild), Some(thread)) = (id(&data["guild_id"]), id(&data["id"]))
                    && let Some(guild) = self.guilds.iter_mut().find(|known| known.id == guild)
                    && let Some(known) = guild.threads.iter_mut().find(|known| known.id == thread)
                {
                    known.joined = true;
                }
            }
            "THREAD_LIST_SYNC" => {
                if let Some(guild) = id(&data["guild_id"])
                    && let Some(guild) = self.guilds.iter_mut().find(|known| known.id == guild)
                {
                    for thread in data["threads"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(Thread::from_value)
                    {
                        guild.threads.retain(|known| known.id != thread.id);
                        guild.threads.push(thread);
                    }
                    // The account's own thread members come beside the threads, one per joined thread.
                    for member in data["members"].as_array().into_iter().flatten() {
                        if let Some(thread) = id(&member["id"])
                            && let Some(known) =
                                guild.threads.iter_mut().find(|known| known.id == thread)
                        {
                            known.joined = true;
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
                if let (Some(user), Some(ignored)) =
                    (id(&data["id"]), data["user_ignored"].as_bool())
                {
                    self.set_ignored(user, ignored);
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
            "MESSAGE_UPDATE" => self.update_message(data),
            "MESSAGE_REACTION_ADD" => self.react(data, 1),
            "MESSAGE_REACTION_REMOVE" => self.react(data, -1),
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

    /// A DM or group DM from CHANNEL_CREATE or the `POST /users/@me/channels` answer, put first in the list.
    /// `RELATIONSHIP_IGNORE_USER_SUCCESS` / `RELATIONSHIP_UNIGNORE_USER_SUCCESS`.
    pub fn set_ignored(&mut self, user: Id, ignored: bool) {
        if ignored {
            self.ignored.insert(user);
        } else {
            self.ignored.remove(&user);
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

    pub fn add_private_channel(&mut self, data: &Value) {
        if !matches!(data["type"].as_u64(), Some(1 | 3)) {
            return;
        }
        for user in data["recipients"].as_array().into_iter().flatten() {
            if let Some(user) = User::from_value(user) {
                self.users.entry(user.id).or_insert(user);
            }
        }
        if let Some(channel) = PrivateChannel::from_value(data)
            && !self
                .private_channels
                .iter()
                .any(|known| known.id == channel.id)
        {
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
    pub fn set_pref(&mut self, pref: &'static settings::ProtoPref, value: u64) -> String {
        let (patch, updated) = settings::set_pref(&self.settings_raw, pref, value);
        self.settings_raw = updated;
        self.prefs.insert(pref.key, value);
        patch
    }

    /// The status the account's proto settings hold: the chosen one, when it lapses, and the custom status.
    pub fn status_state(&self) -> Option<settings::StatusState> {
        settings::read_status(&self.settings_raw)
    }

    /// Chooses a status (`online`, `idle`, `dnd`, `invisible`) until `expires_at_ms` (zero: until changed). The new
    /// status shows at once; the PATCH body value comes back.
    pub fn set_status(&mut self, status: &str, expires_at_ms: u64, now_ms: u64) -> String {
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
            match settings::varint_at(proto, pref.path, pref.field) {
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
        if let Some(proto) = data["user_settings_proto"].as_str() {
            self.settings_raw = settings::decode(proto);
            self.read_prefs(proto, false);
        }
        self.session_id = text(&data["session_id"]);
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
        self.ignored = data["relationships"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|entry| entry["user_ignored"].as_bool() == Some(true))
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
            .guilds
            .iter()
            .find(|guild| guild.channels.iter().any(|known| known.id == channel))
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
        if let Some(content) = text(&data["content"]) {
            known.content = content;
            known.edited = data["edited_timestamp"].is_string();
        }
        if let Some(pinned) = data["pinned"].as_bool() {
            known.pinned = pinned;
        }
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
        let guild = self
            .guilds
            .iter()
            .find(|guild| {
                guild.channels.iter().any(|known| known.id == channel)
                    || guild.threads.iter().any(|known| known.id == channel)
            })
            .map(|guild| guild.id);
        for value in page.as_array().into_iter().flatten() {
            let Some(mut message) = Message::from_value(value) else {
                continue;
            };
            if let Some(guild) = guild
                && value["member"].is_object()
                && let Some(member) =
                    members::Member::from_value(&value["member"], Some(&value["author"]))
            {
                self.members.upsert(guild, member);
            }
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
    fn badges_follow_the_official_precedence() {
        let guild = |features: &[&str]| Guild {
            id: 1,
            name: String::new(),
            icon: None,
            owner_id: None,
            joined_at: String::new(),
            default_message_notifications: 0,
            nsfw_level: 0,
            channels: Vec::new(),
            roles: HashMap::new(),
            role_permissions: HashMap::new(),
            role_info: HashMap::new(),
            my_roles: Vec::new(),
            my_pending: false,
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
        stores.dispatch("READY", &json!({"sessions": [{"status": "dnd"}]}));
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
