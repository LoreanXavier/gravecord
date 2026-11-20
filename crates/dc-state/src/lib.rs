//! Flux stores. M0 slice: the parts of UserStore, GuildStore, ChannelStore and MessageStore the first screen reads.
//! Names and getters follow `STORES`; the rest of each store is tracked as open rows in `FEATURES.md`.

pub mod account_events;
pub mod autocomplete;
pub mod automod_messages;
pub mod calls;
pub mod channel_member_counts;
pub mod channel_status;
pub mod collectibles;
pub mod components;
pub mod confusables;
pub mod content_inventory;
pub mod content_inventory_manager;
pub mod deleted_entities;
pub mod display_name_styles;
pub mod emoji_packs;
pub mod entitlements;
pub mod experiments;
pub mod folders;
pub mod frecency;
pub mod game_relationships;
mod gateway_extra;
pub mod grouping;
pub mod guild_bans;
pub mod guild_integrations;
pub mod guild_join_requests;
pub mod guild_member_counts;
pub mod guild_settings;
pub mod interactions;
pub mod local_activity;
pub mod location_metadata;
pub mod member_list;
pub mod members;
pub mod message_kinds;
pub mod message_requests;
pub mod message_window;
pub mod notes;
pub mod notification_center;
pub mod notifications;
pub mod now_playing;
pub mod permissions;
pub mod pins_unread;
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
pub mod spotify;
pub mod stage;
pub mod stickers;
pub mod thread_list;
pub mod thread_member_list;
pub mod thread_members;
pub mod thread_messages;
pub mod thread_previews;
pub mod unread;
pub mod unread_notice;

use chrono::{DateTime, Local, Utc};
use dc_proto::settings;
use serde_json::{Map, Value, json};
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
    /// `system`: an official Discord account; a DM with one is a system DM (`isSystemDM`, module 95701).
    pub system: bool,
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
    /// `isNonUserBot` of module 619019: the system account, or a bot whose tag is `0000`.
    pub fn non_user_bot(&self) -> bool {
        self.system || (self.bot && self.discriminator == "0000")
    }

    /// `isVerifiedBot` of module 619019: the system account, or `VERIFIED_BOT` (1 << 16) in the user's flags.
    pub fn verified_bot(&self) -> bool {
        self.system || (self.flags | self.public_flags) & (1 << 16) != 0
    }

    pub fn from_value(value: &Value) -> Option<Self> {
        Some(Self {
            id: id(&value["id"])?,
            username: text(&value["username"])?,
            global_name: text(&value["global_name"]),
            avatar: text(&value["avatar"]),
            discriminator: text(&value["discriminator"]).unwrap_or_else(|| "0".to_owned()),
            tag: ServerTag::from_value(&value["primary_guild"])
                .or_else(|| ServerTag::from_value(&value["clan"])),
            bot: value["bot"].as_bool().unwrap_or(false),
            system: value["system"].as_bool().unwrap_or(false),
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

    /// Updates only fields whose keys are present on `value` (`287809` `mergeUser` / `es`), so partial `{ "id": ... }`
    /// payloads never clobber existing username, display name, avatar or collectibles.
    pub fn merge_value(&mut self, value: &Value) {
        if let Some(username) = text(&value["username"]).filter(|name| !name.is_empty()) {
            self.username = username;
        }
        if let Some(field) = value.get("global_name") {
            self.global_name = text(field);
        }
        if let Some(field) = value.get("avatar") {
            self.avatar = text(field);
        }
        if let Some(discriminator) = text(&value["discriminator"]).filter(|disc| !disc.is_empty()) {
            self.discriminator = discriminator;
        }
        if value.get("primary_guild").is_some() || value.get("clan").is_some() {
            self.tag = ServerTag::from_value(&value["primary_guild"])
                .or_else(|| ServerTag::from_value(&value["clan"]));
        }
        if let Some(bot) = value["bot"].as_bool() {
            self.bot = bot;
        }
        if let Some(collectibles) = value.get("collectibles") {
            self.nameplate = collectibles::Nameplate::from_value(&collectibles["nameplate"]);
        }
        if let Some(decoration) = value.get("avatar_decoration_data") {
            self.avatar_decoration =
                collectibles::AvatarDecoration::from_value(decoration).map(Box::new);
        }
        if let Some(flags) = value["flags"].as_u64() {
            self.flags = flags;
        }
        if let Some(public_flags) = value["public_flags"].as_u64() {
            self.public_flags = public_flags;
        }
        if let Some(styles) = value.get("display_name_styles") {
            self.display_name_styles =
                display_name_styles::DisplayNameStyles::from_value(styles).map(Box::new);
        }
    }

    /// Fills in fields missing on `self` from an embedded secondary payload without clearing existing values.
    fn enrich_from_value(&mut self, value: &Value) {
        if self.username.is_empty()
            && let Some(username) = text(&value["username"]).filter(|name| !name.is_empty())
        {
            self.username = username;
        }
        if self.global_name.is_none() {
            self.global_name = text(&value["global_name"]);
        }
        if self.avatar.is_none() {
            self.avatar = text(&value["avatar"]);
        }
        if self.discriminator == "0"
            && let Some(discriminator) =
                text(&value["discriminator"]).filter(|disc| !disc.is_empty() && disc != "0")
        {
            self.discriminator = discriminator;
        }
        if self.tag.is_none() {
            self.tag = ServerTag::from_value(&value["primary_guild"])
                .or_else(|| ServerTag::from_value(&value["clan"]));
        }
        if !self.bot && value["bot"].as_bool() == Some(true) {
            self.bot = true;
        }
        if self.nameplate.is_none() {
            self.nameplate =
                collectibles::Nameplate::from_value(&value["collectibles"]["nameplate"]);
        }
        if self.avatar_decoration.is_none() {
            self.avatar_decoration =
                collectibles::AvatarDecoration::from_value(&value["avatar_decoration_data"])
                    .map(Box::new);
        }
        if self.flags == 0
            && let Some(flags) = value["flags"].as_u64()
        {
            self.flags = flags;
        }
        if self.public_flags == 0
            && let Some(public_flags) = value["public_flags"].as_u64()
        {
            self.public_flags = public_flags;
        }
        if self.display_name_styles.is_none() {
            self.display_name_styles =
                display_name_styles::DisplayNameStyles::from_value(&value["display_name_styles"])
                    .map(Box::new);
        }
    }

    /// 619019 `hasFlag`: `(flags | publicFlags) & flag === flag`.
    pub fn has_flag(&self, flag: u64) -> bool {
        (self.flags | self.public_flags) & flag == flag
    }

    /// Serializes this user into the JSON shape accepted by [`Self::from_value`] for start-cache persistence.
    pub fn to_cache_value(&self) -> Value {
        let mut out = Map::new();
        out.insert("id".to_owned(), json!(self.id.to_string()));
        out.insert("username".to_owned(), json!(self.username));
        out.insert("global_name".to_owned(), json!(self.global_name));
        out.insert("avatar".to_owned(), json!(self.avatar));
        out.insert("discriminator".to_owned(), json!(self.discriminator));
        out.insert("bot".to_owned(), json!(self.bot));
        out.insert("flags".to_owned(), json!(self.flags));
        out.insert("public_flags".to_owned(), json!(self.public_flags));
        if let Some(tag) = &self.tag {
            out.insert(
                "primary_guild".to_owned(),
                json!({
                    "identity_enabled": true,
                    "identity_guild_id": tag.guild_id.to_string(),
                    "badge": tag.badge,
                    "tag": tag.tag,
                }),
            );
        }
        if let Some(nameplate) = &self.nameplate {
            out.insert(
                "collectibles".to_owned(),
                json!({
                    "nameplate": {
                        "sku_id": nameplate.sku_id.to_string(),
                        "label": nameplate.label,
                        "palette": nameplate.palette,
                    }
                }),
            );
        }
        if let Some(decoration) = &self.avatar_decoration {
            out.insert(
                "avatar_decoration_data".to_owned(),
                json!({
                    "sku_id": decoration.sku_id.to_string(),
                    "asset": decoration.asset,
                    "expires_at": decoration.expires_at,
                }),
            );
        }
        if let Some(styles) = &self.display_name_styles {
            out.insert(
                "display_name_styles".to_owned(),
                json!({
                    "font_id": styles.font_id,
                    "effect_id": styles.effect_id,
                    "colors": styles.colors,
                }),
            );
        }
        Value::Object(out)
    }

    pub fn display_name(&self) -> &str {
        self.global_name.as_deref().unwrap_or(&self.username)
    }

    /// `(id >> 22) % 6` for pomelo users, `discriminator % 5` for legacy ones.
    /// A bot (a webhook's author) without an avatar and with discriminator "0000" always shows the first default
    /// (486020 `getUserAvatarURL`: `if (bot && avatar == null && discriminator === "0000") return defaults[0]`).
    pub fn default_avatar_index(&self) -> u64 {
        if self.bot && self.avatar.is_none() && self.discriminator == "0000" {
            return 0;
        }
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
    /// The server banner's hash (`banner`), for the plugins that open it (ViewIcons).
    pub banner: Option<String>,
    /// `description`, `preferred_locale` and `verification_level`, for ServerInfo.
    pub description: Option<String>,
    pub preferred_locale: Option<String>,
    pub verification_level: Option<u64>,
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
    /// View Server As Role (ImpersonateStore, module 164956 `getViewingRoles`): while set, permissions here are
    /// computed as a non-owner holding @everyone and only these roles. Purely local; nothing is sent.
    pub viewing_roles: Option<Vec<Id>>,
    /// 1 when moderation needs 2FA (`mfa_level` ELEVATED).
    pub mfa_level: u64,
    pub vanity_url_code: Option<String>,
    pub features: Vec<String>,
    /// `incidents_data`: invites and DMs paused until (ISO times) and the last lockdown length in hours.
    pub invites_disabled_until: Option<String>,
    pub dms_disabled_until: Option<String>,
    pub lockdown_duration_hours: Option<u64>,
    /// `incidents_data.raid_detected_at` and `dm_spam_detected_at` (ISO times): an incident is active for two hours
    /// after either (`k$` of module 903093).
    pub raid_detected_at: Option<String>,
    pub dm_spam_detected_at: Option<String>,
    /// `safety_alerts_channel_id`, else `public_updates_channel_id`: where a raid report goes (`B` of 859706).
    pub alerts_channel: Option<Id>,
    pub premium_tier: u64,
    pub premium_subscription_count: u64,
    /// Stage video is available only when the guild's advertised capacity is positive (module 279250).
    pub max_stage_video_channel_users: u64,
    /// `max_video_channel_users`: the video seats of a voice channel (`maxVideoChannelUsers`, `Pd` of module 147036).
    pub max_video_channel_users: u64,
    /// `premium_progress_bar_enabled`: the boost progress bar in the channel list.
    pub premium_progress_bar_enabled: bool,
    /// `system_channel_flags`: which system messages the guild silences (SUPPRESS_JOIN_NOTIFICATION_REPLIES is
    /// 1 << 3, the "Wave to say hi!" button).
    pub system_channel_flags: u64,
    pub emojis: Vec<GuildEmoji>,
    /// Active threads this session knows (READY's `threads`, then THREAD_* events).
    pub threads: Vec<Thread>,
    /// `profile.tag` / `clan.tag`: server tag offered by guilds with `GUILD_TAGS` (module 318785 / 537975).
    pub tag: Option<String>,
    /// `profile.badge` / `clan.badge`: clan badge hash for the guild's server tag.
    pub tag_badge: Option<String>,
}

fn guild_tag_pair(value: &Value) -> (Option<String>, Option<String>) {
    let source = if value["profile"].is_object() {
        &value["profile"]
    } else if value["clan"].is_object() {
        &value["clan"]
    } else {
        return (None, None);
    };
    let tag = text(&source["tag"]).filter(|tag| !tag.is_empty());
    let badge = tag
        .as_ref()
        .and_then(|_| text(&source["badge"]).filter(|badge| !badge.is_empty()));
    (tag, badge)
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
    /// `thread_metadata.invitable` of a private thread: members other than moderators may add people (absent: true).
    pub invitable: bool,
    /// The current user's thread member flags (`member.flags`: 2 all messages, 4 only mentions, 8 nothing). `None`
    /// when no member data arrived: JoinedThreadsStore `flags()` is then null.
    pub member_flags: Option<u64>,
    /// `member.muted` and `member.mute_config.end_time`.
    pub muted: bool,
    pub mute_end: Option<DateTime<Utc>>,
    pub join_timestamp: Option<String>,
    /// `thread_metadata.auto_archive_duration` in minutes (0 when absent).
    pub auto_archive_duration: u64,
    /// `thread_metadata.archive_timestamp` and `last_non_message_activity_timestamp`, in Unix milliseconds.
    pub archive_timestamp_ms: Option<i64>,
    pub last_non_message_activity_ms: Option<i64>,
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
            joined: value["member"].is_object(),
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
            invitable: value["thread_metadata"]["invitable"]
                .as_bool()
                .unwrap_or(true),
            auto_archive_duration: value["thread_metadata"]["auto_archive_duration"]
                .as_u64()
                .unwrap_or(0),
            archive_timestamp_ms: thread_list::time_ms(
                &value["thread_metadata"]["archive_timestamp"],
            ),
            last_non_message_activity_ms: thread_list::time_ms(
                &value["last_non_message_activity_timestamp"],
            ),
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
    /// `colors.secondary_color` of the enhanced role styles (module 176201 `K3`): with `color` the gradient pair.
    pub secondary_color: Option<u32>,
    /// `colors.tertiary_color`: the third stop of the holographic style.
    pub tertiary_color: Option<u32>,
}

/// A role's colours as module 176201 `K3` reads them: the primary, then the optional second and third.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RoleColors {
    pub primary: u32,
    pub secondary: Option<u32>,
    pub tertiary: Option<u32>,
}

impl RoleInfo {
    fn from_value(role: &Value) -> Self {
        let colors = &role["colors"];
        Self {
            position: role["position"].as_i64().unwrap_or(0),
            color: role["color"].as_u64().unwrap_or(0) as u32,
            mentionable: role["mentionable"].as_bool().unwrap_or(false),
            guild_connection: role["tags"].get("guild_connections").is_some(),
            managed: role["managed"].as_bool().unwrap_or(false),
            secondary_color: colors["secondary_color"].as_u64().map(|c| c as u32),
            tertiary_color: colors["tertiary_color"].as_u64().map(|c| c as u32),
        }
    }

    /// The colours of the gradient name, when the role has a second colour (the `primaryColor`/`secondaryColor`
    /// test of module 967144).
    pub fn gradient(&self) -> Option<RoleColors> {
        Some(RoleColors {
            primary: self.color,
            secondary: Some(self.secondary_color?),
            tertiary: self.tertiary_color,
        })
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
        self.color_role(roles).map(|(_, role)| role.color)
    }

    /// One role's gradient pair under the same rule (`X_` of module 967144: the server feature and a second colour).
    pub fn role_colors_of(&self, role: Id) -> Option<RoleColors> {
        if !self
            .features
            .iter()
            .any(|feature| feature == "ENHANCED_ROLE_COLORS")
        {
            return None;
        }
        self.role_info.get(&role)?.gradient()
    }

    fn color_role(&self, roles: &[Id]) -> Option<(Id, &RoleInfo)> {
        roles
            .iter()
            .filter_map(|id| self.role_info.get(id).map(|role| (*id, role)))
            .filter(|(_, role)| role.color != 0)
            .min_by_key(|(id, role)| (*id == self.id, std::cmp::Reverse(role.position), *id))
    }

    /// The colour role's gradient pair for a name: only in a server with `ENHANCED_ROLE_COLORS` and a role that
    /// has a second colour (modules 676608 and 967144 `gn`/`X_`).
    pub fn role_gradient(&self, roles: &[Id]) -> Option<RoleColors> {
        if !self
            .features
            .iter()
            .any(|feature| feature == "ENHANCED_ROLE_COLORS")
        {
            return None;
        }
        self.color_role(roles)?.1.gradient()
    }

    /// READY nests name and icon under `properties`; GUILD_CREATE may not.
    pub fn from_value(value: &Value) -> Option<Self> {
        let properties = if value["properties"].is_object() {
            &value["properties"]
        } else {
            value
        };
        let channels = value["channels"].as_array()?;
        let (tag, tag_badge) = guild_tag_pair(properties);
        Some(Self {
            id: id(&value["id"])?,
            name: text(&properties["name"])?,
            icon: text(&properties["icon"]),
            banner: text(&properties["banner"]),
            description: text(&properties["description"]),
            preferred_locale: text(&properties["preferred_locale"]),
            verification_level: properties["verification_level"].as_u64(),
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
            viewing_roles: None,
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
            raid_detected_at: text(&properties["incidents_data"]["raid_detected_at"]),
            dm_spam_detected_at: text(&properties["incidents_data"]["dm_spam_detected_at"]),
            alerts_channel: id(&properties["safety_alerts_channel_id"])
                .or_else(|| id(&properties["public_updates_channel_id"])),
            premium_tier: properties["premium_tier"].as_u64().unwrap_or(0),
            premium_subscription_count: value["premium_subscription_count"]
                .as_u64()
                .or(properties["premium_subscription_count"].as_u64())
                .unwrap_or(0),
            max_stage_video_channel_users: value["max_stage_video_channel_users"]
                .as_u64()
                .or(properties["max_stage_video_channel_users"].as_u64())
                .unwrap_or(0),
            max_video_channel_users: value["max_video_channel_users"]
                .as_u64()
                .or(properties["max_video_channel_users"].as_u64())
                .unwrap_or(0),
            premium_progress_bar_enabled: value["premium_progress_bar_enabled"]
                .as_bool()
                .or(properties["premium_progress_bar_enabled"].as_bool())
                .unwrap_or(false),
            system_channel_flags: value["system_channel_flags"]
                .as_u64()
                .or(properties["system_channel_flags"].as_u64())
                .unwrap_or(0),
            emojis: GuildEmoji::list(&value["emojis"]),
            tag,
            tag_badge,
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
        self.raid_detected_at = text(&value["incidents_data"]["raid_detected_at"]);
        self.dm_spam_detected_at = text(&value["incidents_data"]["dm_spam_detected_at"]);
        if value.get("safety_alerts_channel_id").is_some()
            || value.get("public_updates_channel_id").is_some()
        {
            self.alerts_channel = id(&value["safety_alerts_channel_id"])
                .or_else(|| id(&value["public_updates_channel_id"]));
        }
        self.premium_tier = value["premium_tier"].as_u64().unwrap_or(self.premium_tier);
        self.premium_subscription_count = value["premium_subscription_count"]
            .as_u64()
            .unwrap_or(self.premium_subscription_count);
        self.max_stage_video_channel_users = value["max_stage_video_channel_users"]
            .as_u64()
            .unwrap_or(self.max_stage_video_channel_users);
        self.max_video_channel_users = value["max_video_channel_users"]
            .as_u64()
            .unwrap_or(self.max_video_channel_users);
        self.premium_progress_bar_enabled = value["premium_progress_bar_enabled"]
            .as_bool()
            .unwrap_or(self.premium_progress_bar_enabled);
        self.system_channel_flags = value["system_channel_flags"]
            .as_u64()
            .unwrap_or(self.system_channel_flags);
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
        if value.get("profile").is_some() || value.get("clan").is_some() {
            let (tag, badge) = guild_tag_pair(value);
            self.tag = tag;
            self.tag_badge = badge;
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
        if let Some(roles) = member["roles"].as_array() {
            self.my_roles = roles.iter().filter_map(id).collect();
        }
        if let Some(pending) = member["pending"].as_bool() {
            self.my_pending = pending;
        }
        if member.get("communication_disabled_until").is_some() {
            self.my_timeout_until = member["communication_disabled_until"]
                .as_str()
                .and_then(|until| chrono::DateTime::parse_from_rfc3339(until).ok())
                .map(|until| until.timestamp_millis());
        }
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

    /// Module 318785 (`b`): eligible server tag when `GUILD_TAGS` is present, joined, not pending, and `profile.tag` exists.
    pub fn selectable_server_tag(&self) -> Option<(&str, Option<&str>)> {
        let tag = self.tag.as_deref().filter(|tag| !tag.is_empty())?;
        if !self.has_feature("GUILD_TAGS") || self.joined_at.is_empty() || self.my_pending {
            return None;
        }
        Some((
            tag,
            self.tag_badge.as_deref().filter(|badge| !badge.is_empty()),
        ))
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

fn attachment_scan_versions() -> &'static std::sync::Mutex<HashMap<Id, i64>> {
    static MAP: std::sync::OnceLock<std::sync::Mutex<HashMap<Id, i64>>> =
        std::sync::OnceLock::new();
    MAP.get_or_init(|| std::sync::Mutex::new(HashMap::new()))
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Attachment {
    pub id: Id,
    pub filename: String,
    pub title: Option<String>,
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
    pub fn from_value(value: &Value) -> Option<Self> {
        let id = id(&value["id"])?;
        if let Ok(mut map) = attachment_scan_versions().lock() {
            match value.get("content_scan_version").and_then(Value::as_i64) {
                Some(version) => {
                    map.insert(id, version);
                }
                None => {
                    map.remove(&id);
                }
            }
        }
        Some(Self {
            id,
            filename: text(&value["filename"])?,
            title: text(&value["title"]),
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

    /// Module 465856: a supplied title keeps the filename's final extension, except for dotfiles.
    pub fn display_name(&self) -> String {
        match &self.title {
            Some(title) => {
                let extension = self
                    .filename
                    .rfind('.')
                    .filter(|index| *index > 0)
                    .map_or("", |index| &self.filename[index..]);
                format!("{title}{extension}")
            }
            None => self.filename.clone(),
        }
    }

    /// `content_scan_version` from the gateway/REST payload, for explicit-media pending scan checks (module 282108).
    pub fn content_scan_version(&self) -> Option<i64> {
        attachment_scan_versions()
            .lock()
            .ok()
            .and_then(|map| map.get(&self.id).copied())
    }

    pub fn is_image(&self) -> bool {
        self.width.is_some()
            && self.height.is_some()
            && self
                .content_type
                .as_deref()
                .is_some_and(|kind| kind.starts_with("image/"))
    }

    /// Modules 949244 (`NI`) and 273185 (`AE`): non-thumbnail attachment with explicit dimensions,
    /// non-empty proxy/source URL and a playable container extension (`.mp4`/`.webm`/`.mov`/`.qt`).
    pub fn is_video(&self) -> bool {
        if self.flags & 2 != 0
            || self.proxy_url.is_empty()
            || !self.width.is_some_and(|w| w > 0)
            || !self.height.is_some_and(|h| h > 0)
        {
            return false;
        }
        let extension = self.filename.rsplit('.').next().unwrap_or("");
        extension.eq_ignore_ascii_case("mp4")
            || extension.eq_ignore_ascii_case("webm")
            || extension.eq_ignore_ascii_case("mov")
            || extension.eq_ignore_ascii_case("qt")
    }

    /// Module 949244 (`NI`): video flagged `IS_CLIP` (`1 << 2`).
    pub fn is_clip(&self) -> bool {
        self.is_video() && self.flags & 4 != 0
    }

    /// Module 548146 (`E`): poster image derived from `proxy_url` with `format=webp`.
    pub fn video_poster_url(&self) -> Option<String> {
        self.is_video()
            .then(|| with_query(&self.proxy_url, "format", "webp"))
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
    /// The alt text (`description`), empty treated as none.
    pub description: Option<String>,
    /// `flags`; `IS_ANIMATED` (32) is `srcIsAnimated`.
    pub flags: u64,
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
            description: text(&value["description"]).filter(|text| !text.is_empty()),
            flags: value["flags"].as_u64().unwrap_or(0),
        })
    }

    /// `E` in module 644447: the proxy address when there is one, else the original.
    pub fn src(&self) -> &str {
        self.proxy_url
            .as_deref()
            .filter(|proxy| !proxy.is_empty())
            .unwrap_or(&self.url)
    }

    pub fn is_animated(&self) -> bool {
        // LazyImage (619517) also treats GIF URLs as animated when the payload has no animation flag.
        let original = if self.url.is_empty() {
            self.src()
        } else {
            &self.url
        };
        self.flags & 32 != 0
            || original
                .split(['?', '#'])
                .next()
                .and_then(|path| path.rsplit_once('.'))
                .is_some_and(|(_, ext)| ext.eq_ignore_ascii_case("gif"))
    }
}

/// `Uj` in module 515718: scales down to the width first, then to the height, rounding at each step.
pub fn fit_rounded(
    width: f64,
    height: f64,
    max_width: f64,
    max_height: f64,
    min_width: f64,
    min_height: f64,
) -> (f64, f64) {
    let (mut width, mut height) = (width, height);
    if width != max_width || height != max_height {
        let first = if width > max_width {
            max_width / width
        } else {
            1.
        };
        width = (width * first).round().max(min_width);
        height = (height * first).round().max(min_height);
        let second = if height > max_height {
            max_height / height
        } else {
            1.
        };
        width = (width * second).round().max(min_width);
        height = (height * second).round().max(min_height);
    }
    (width, height)
}

/// The snowflake's creation time in milliseconds since the Unix epoch.
fn snowflake_ms(id: Id) -> u64 {
    (id >> 22) + 1_420_070_400_000
}

/// `A` in module 659674: providers whose videos played before 2017-04-23 (`0x15b7e4a5bfb`).
fn legacy_video_provider(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    [
        "youtube",
        "steam",
        "imgur",
        "vimeo",
        "sketchfab",
        "soundcloud",
        "streamable",
        "twitch",
        "vid.me",
        "twitter",
    ]
    .iter()
    .any(|known| name.contains(known))
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct EmbedField {
    pub name: String,
    pub value: String,
    pub inline: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EmbedAudio {
    pub url: Option<String>,
    pub proxy_url: String,
    pub content_type: Option<String>,
    pub description: Option<String>,
}

impl EmbedAudio {
    /// `fK` in module 659674: audio is populated only when `proxy_url` is present and non-empty.
    fn from_value(value: &Value) -> Option<Self> {
        let proxy_url = text(&value["proxy_url"]).filter(|proxy| !proxy.is_empty())?;
        Some(Self {
            url: text(&value["url"]),
            proxy_url,
            content_type: text(&value["content_type"]),
            description: text(&value["description"]),
        })
    }
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
    pub audio: Option<EmbedAudio>,
    pub fields: Vec<EmbedField>,
    /// `provider.url`: the provider's name links there.
    pub provider_url: Option<String>,
    /// `timestamp`, shown after the footer text.
    pub timestamp: Option<DateTime<Utc>>,
    /// `images` of `nh` in module 659674: the pictures of later embeds with the same `url`, merged into this one
    /// (the first is this embed's own image). Empty when nothing was merged.
    pub images: Vec<EmbedMedia>,
    /// The embed's own `flags`.
    pub flags: u64,
    /// `content_scan_version` (`contentScanVersion` in module 659674), for sensitive-media scan gating.
    pub content_scan_version: Option<i64>,
}

impl Embed {
    pub fn from_value(value: &Value) -> Self {
        Self::from_value_in(value, None)
    }

    /// `fK` in module 659674 for an embed of the message `message` (its id decides the pre-2017 video rule).
    pub fn from_value_in(value: &Value, message: Option<Id>) -> Self {
        let kind = text(&value["type"]).unwrap_or_else(|| "rich".to_owned());
        let mut embed = Self {
            url: text(&value["url"]),
            title: text(&value["title"]),
            description: text(&value["description"]),
            color: value["color"].as_u64().map(|color| color as u32),
            provider: text(&value["provider"]["name"]),
            provider_url: text(&value["provider"]["name"])
                .and_then(|_| text(&value["provider"]["url"])),
            author: text(&value["author"]["name"]),
            author_url: text(&value["author"]["name"]).and_then(|_| text(&value["author"]["url"])),
            // The icon drawn is `proxy_icon_url` (`iconProxyURL`); without one there is no icon.
            author_icon: text(&value["author"]["name"])
                .and_then(|_| text(&value["author"]["proxy_icon_url"])),
            footer: text(&value["footer"]["text"]),
            footer_icon: text(&value["footer"]["proxy_icon_url"]),
            audio: EmbedAudio::from_value(&value["audio"]),
            timestamp: value["timestamp"]
                .as_str()
                .and_then(|time| DateTime::parse_from_rfc3339(time).ok())
                .map(|time| time.with_timezone(&Utc)),
            flags: value.get("flags").and_then(Value::as_u64).unwrap_or(0),
            content_scan_version: value.get("content_scan_version").and_then(Value::as_i64),
            ..Self::default()
        };
        // Articles and images show their thumbnail as the image; every other type keeps it as the thumbnail
        // (a GIFV's is its poster).
        if let Some(thumbnail) = EmbedMedia::from_value(&value["thumbnail"]) {
            match kind.as_str() {
                "article" | "image" => embed.image = Some(thumbnail),
                _ => embed.thumbnail = Some(thumbnail),
            }
        }
        if let Some(image) = EmbedMedia::from_value(&value["image"]) {
            embed.image = Some(image);
        }
        let video = &value["video"];
        if video.is_object() {
            let sized = video["width"].as_u64().is_some_and(|width| width > 0)
                && video["height"].as_u64().is_some_and(|height| height > 0);
            // A video without a thumbnail gets one from its own proxy address as WebP.
            if embed.thumbnail.is_none()
                && let Some(proxy) = text(&video["proxy_url"])
                && sized
            {
                embed.thumbnail = Some(EmbedMedia {
                    url: with_query(&proxy, "format", "webp"),
                    proxy_url: None,
                    width: video["width"].as_u64().unwrap_or(0) as u32,
                    height: video["height"].as_u64().unwrap_or(0) as u32,
                    description: None,
                    flags: 0,
                });
            }
            let provider = text(&value["provider"]["name"]);
            if embed.thumbnail.is_some()
                && sized
                && let Some(media) = EmbedMedia::from_value(video)
            {
                let sketchfab = provider
                    .as_deref()
                    .is_some_and(|name| name.to_ascii_lowercase().contains("sketchfab"))
                    || media
                        .url
                        .to_ascii_lowercase()
                        .starts_with("https://sketchfab.com");
                let mut playable = media.proxy_url.is_some()
                    || media.url.to_ascii_lowercase().starts_with("https:");
                if message.is_some_and(|id| snowflake_ms(id) < 0x15b_7e4a_5bfb) {
                    playable &= provider.as_deref().is_some_and(legacy_video_provider);
                }
                if !sketchfab && playable {
                    embed.video = Some(media);
                }
            }
        }
        // Only rich, auto-moderation and poll-result embeds carry fields (`u.pd`; the poll result row reads `poll_question_text` and the victor fields from them, module 117096).
        if matches!(
            kind.as_str(),
            "rich"
                | "auto_moderation_message"
                | "auto_moderation_notification"
                | "safety_policy_notice"
                | "safety_system_notification"
                | "age_verification_system_notification"
                | "voice_channel"
                | "poll_result"
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
    /// `ds` for TikTok and for a portrait YouTube video.
    pub const TALL_MEDIA: (f64, f64) = (400., 450.);

    /// `QY` in module 659674: "YouTube" or "TikTok" by provider name, else by the video's host.
    pub fn video_provider(&self) -> Option<String> {
        let name = self.provider.clone();
        if matches!(name.as_deref(), Some("YouTube" | "TikTok")) {
            return name;
        }
        let host = self
            .video
            .as_ref()
            .and_then(|video| video.url.split("://").nth(1))
            .and_then(|rest| rest.split(['/', '?', '#']).next())
            .map(|host| host.to_ascii_lowercase());
        match host.as_deref() {
            Some("www.youtube.com") => Some("YouTube".to_owned()),
            Some("www.tiktok.com") => Some("TikTok".to_owned()),
            _ => name,
        }
    }

    /// `ds` in module 659674: the box media of this embed is fitted into.
    pub fn max_media(&self) -> (f64, f64) {
        let tall_youtube = self
            .video
            .as_ref()
            .is_some_and(|video| video.height > video.width)
            && self.video_provider().as_deref() == Some("YouTube");
        if self.provider.as_deref() == Some("TikTok") || tall_youtube {
            Self::TALL_MEDIA
        } else {
            Self::MAX_MEDIA
        }
    }

    /// `media` fitted into this embed's `max_media` (`Uj`), never enlarged.
    pub fn fit(&self, media: &EmbedMedia) -> (f64, f64) {
        let (max_width, max_height) = self.max_media();
        fit_rounded(
            f64::from(media.width),
            f64::from(media.height),
            max_width,
            max_height,
            0.,
            0.,
        )
    }

    /// The image fitted into `MAX_MEDIA`, never enlarged (`Uj`).
    pub fn fitted(media: &EmbedMedia) -> (f64, f64) {
        let (max_width, max_height) = Self::MAX_MEDIA;
        fit_rounded(
            f64::from(media.width),
            f64::from(media.height),
            max_width,
            max_height,
            0.,
            0.,
        )
    }

    /// `NV` in module 659674 (the component's `isInline` without `hideMedia`): media, and either a GIFV or a type
    /// other than rich with no author and no title. Such an embed is drawn as the bare media
    /// (`inlineMediaEmbed_XyW1ZT`).
    pub fn is_inline_media(&self) -> bool {
        (self.image.is_some() || self.video.is_some())
            && (self.kind == "gifv"
                || self.kind != "rich" && self.author.is_none() && self.title.is_none())
    }

    /// `getMaxWidth(false)` of module 265705: a video embed, or media at least 300 wide, is that wide plus 32; a
    /// rich embed with a merged gallery is 520; a GIFV is its fitted video (or poster) width.
    pub fn max_width(&self) -> Option<f64> {
        self.max_width_for(false)
    }

    /// `getMaxWidth(true)`, for the inline media embed: only the gallery and GIFV rules.
    pub fn inline_max_width(&self) -> Option<f64> {
        self.max_width_for(true)
    }

    fn max_width_for(&self, inline: bool) -> Option<f64> {
        let media = self.image.as_ref().or(self.video.as_ref())?;
        let (width, _) = self.fit(media);
        if !inline && (self.kind == "video" || width >= 300.) {
            return Some(width + 32.);
        }
        if self.kind == "rich" && !self.images.is_empty() {
            return Some(520.);
        }
        if self.kind == "gifv" {
            let sized = self.video.as_ref().or(self.thumbnail.as_ref());
            let (max_width, max_height) = self.max_media();
            let (width, _) = fit_rounded(
                sized.map_or(0., |media| f64::from(media.width)),
                sized.map_or(0., |media| f64::from(media.height)),
                max_width,
                max_height,
                0.,
                0.,
            );
            return Some(width);
        }
        None
    }

    /// `nh` in module 659674: embeds with the same `url` become one, the later ones' images joining the first's
    /// `images` (the first's own image leading).
    pub fn merge(embeds: Vec<Embed>) -> Vec<Embed> {
        let mut out: Vec<Embed> = Vec::new();
        for embed in embeds {
            let Some(url) = embed.url.clone() else {
                out.push(embed);
                continue;
            };
            let Some(first) = out
                .iter_mut()
                .find(|known| known.url.as_deref() == Some(url.as_str()))
            else {
                out.push(embed);
                continue;
            };
            if let Some(image) = embed.image {
                if first.images.is_empty()
                    && let Some(own) = first.image.clone()
                {
                    first.images.push(own);
                }
                first.images.push(image);
            }
        }
        out
    }

    /// A message's `embeds` array as the record keeps it (`y` in module 432005): `fK` for each, then `nh`.
    pub fn list(value: &Value, message: Option<Id>) -> Vec<Embed> {
        Self::merge(
            value
                .as_array()
                .into_iter()
                .flatten()
                .map(|embed| Self::from_value_in(embed, message))
                .collect(),
        )
    }
}

/// `url` with `key=value` set in its query (`URL.searchParams.set`).
fn with_query(url: &str, key: &str, value: &str) -> String {
    let (base, fragment) = match url.split_once('#') {
        Some((base, fragment)) => (base, Some(fragment)),
        None => (url, None),
    };
    let (path, query) = match base.split_once('?') {
        Some((path, query)) => (path, query),
        None => (base, ""),
    };
    let mut pairs: Vec<String> = query
        .split('&')
        .filter(|pair| !pair.is_empty())
        .filter(|pair| pair.split('=').next() != Some(key))
        .map(str::to_owned)
        .collect();
    pairs.push(format!("{key}={value}"));
    let mut out = format!("{path}?{}", pairs.join("&"));
    if let Some(fragment) = fragment {
        out.push('#');
        out.push_str(fragment);
    }
    out
}

fn edited_at(value: &Value) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(value.as_str()?)
        .ok()
        .map(|time| time.with_timezone(&Utc))
}

/// `MessageStates` (module 652215). Local rows retain their state until the server replaces them.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MessageState {
    #[default]
    Sent,
    Sending,
    SendFailed,
}

impl MessageState {
    fn from_value(value: &Value) -> Self {
        match value.as_str() {
            Some("SENDING") => Self::Sending,
            Some("SEND_FAILED") => Self::SendFailed,
            _ => Self::Sent,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Message {
    pub id: Id,
    pub channel_id: Id,
    pub state: MessageState,
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
    /// `message_reference.guild_id`: for a crossposted announcement, the server it was published from.
    pub reference_guild_id: Option<Id>,
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
    /// The original's message flags (the copy of a voice message keeps `IS_VOICE_MESSAGE`).
    pub flags: u64,
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
            embeds: Embed::list(&snapshot["embeds"], None),
            flags: snapshot["flags"].as_u64().unwrap_or(0),
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

/// `SUPPRESS_NOTIFICATIONS` in the message flags (module 652215, `pr7`): an `@silent` message.
pub const SUPPRESS_NOTIFICATIONS_FLAG: u64 = 1 << 12;

/// The `@silent` prefix (module 677413, `Ay`, the send side of the silentPrefix rule of 182490/605275): text that
/// starts with `@silent` followed by whitespace or the end is sent without it, trimmed, and with
/// [`SUPPRESS_NOTIFICATIONS_FLAG`] (`_sendMessage` of module 138399). Anything else is returned as it is.
pub fn split_silent_prefix(content: &str) -> (bool, &str) {
    const PREFIX: &str = "@silent";
    match content.strip_prefix(PREFIX) {
        Some(rest) if rest.is_empty() || rest.starts_with(char::is_whitespace) => {
            (true, rest.trim())
        }
        _ => (false, content),
    }
}

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
        if self.kind == 24 {
            return false;
        }
        if (self.mention_everyone && !suppress_everyone) || self.mention_user_ids.contains(&me) {
            return true;
        }
        !suppress_roles
            && self
                .mention_role_ids
                .iter()
                .any(|role| my_roles.contains(role))
    }

    /// `hideSimpleEmbedContent` of module 354777 with `k5` and `iz` of module 448767: the text is not drawn when the
    /// message has exactly one (merged) embed, an image or GIFV that draws as inline media, and the content is one
    /// link and nothing else.
    pub fn content_is_inline_media(&self) -> bool {
        let [embed] = self.embeds.as_slice() else {
            return false;
        };
        if !matches!(embed.kind.as_str(), "image" | "gifv") || !embed.is_inline_media() {
            return false;
        }
        let content = self.content.trim();
        let link = content
            .strip_prefix('<')
            .and_then(|inner| inner.strip_suffix('>'))
            .unwrap_or(content);
        !link.is_empty()
            && !link.contains(char::is_whitespace)
            && !link.contains(['<', '>'])
            && (link.starts_with("https://") || link.starts_with("http://"))
    }

    pub fn from_value(value: &Value) -> Option<Self> {
        Some(Self {
            id: id(&value["id"])?,
            channel_id: id(&value["channel_id"])?,
            state: MessageState::from_value(&value["state"]),
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
            embeds: Embed::list(&value["embeds"], id(&value["id"])),
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
            reference_guild_id: id(&value["message_reference"]["guild_id"]),
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
    /// The group's owner (`owner_id`): the member list crowns them.
    pub owner_id: Option<Id>,
    pub flags: u64,
    pub message_request: bool,
    pub spam: bool,
    pub request_timestamp: Option<DateTime<Utc>>,
    /// `blocked_user_warning_dismissed`: the "Leave group?" warning about blocked or ignored members was dismissed.
    pub blocked_user_warning_dismissed: bool,
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
            owner_id: None,
            flags: 0,
            message_request: false,
            spam: false,
            request_timestamp: None,
            blocked_user_warning_dismissed: false,
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
        if value.get("owner_id").is_some() {
            self.owner_id = id(&value["owner_id"]);
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
        if let Some(dismissed) = value["blocked_user_warning_dismissed"].as_bool() {
            self.blocked_user_warning_dismissed = dismissed;
        }
        if value.get("is_message_request_timestamp").is_some() {
            self.request_timestamp = value["is_message_request_timestamp"]
                .as_str()
                .and_then(|stamp| DateTime::parse_from_rfc3339(stamp).ok())
                .map(|stamp| stamp.with_timezone(&Utc));
        }
    }

    /// Serializes this private channel into the JSON shape accepted by [`Self::from_value`].
    pub fn to_cache_value(&self) -> Value {
        let mut out = Map::new();
        out.insert("id".to_owned(), json!(self.id.to_string()));
        out.insert("type".to_owned(), json!(self.kind));
        out.insert(
            "recipient_ids".to_owned(),
            Value::Array(
                self.recipients
                    .iter()
                    .map(|id| json!(id.to_string()))
                    .collect(),
            ),
        );
        out.insert(
            "last_message_id".to_owned(),
            if self.last_message_id > 0 {
                json!(self.last_message_id.to_string())
            } else {
                Value::Null
            },
        );
        out.insert("name".to_owned(), json!(self.name));
        out.insert("icon".to_owned(), json!(self.icon));
        if let Some(application_id) = self.application_id {
            out.insert(
                "application_id".to_owned(),
                json!(application_id.to_string()),
            );
        }
        if let Some(owner_id) = self.owner_id {
            out.insert("owner_id".to_owned(), json!(owner_id.to_string()));
        }
        out.insert("flags".to_owned(), json!(self.flags));
        out.insert("is_message_request".to_owned(), json!(self.message_request));
        out.insert("is_spam".to_owned(), json!(self.spam));
        out.insert(
            "is_message_request_timestamp".to_owned(),
            json!(self.request_timestamp.map(|stamp| stamp.to_rfc3339())),
        );
        Value::Object(out)
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
    /// When this member asked to speak in a Stage (`request_to_speak_timestamp`, ISO 8601), if they did.
    pub request_to_speak_timestamp: Option<String>,
    /// The camera is on (`self_video`).
    pub self_video: bool,
    /// Going live (`self_stream`).
    pub self_stream: bool,
    /// Explicitly false voice states are excluded from fully private Active Now groups.
    pub discoverable: Option<bool>,
    /// When the member joined the channel (`connected_at`, unix seconds; the record's `connectedAt`, modules 225123
    /// and 977997), for the voice user popout's timer (module 772800).
    pub connected_at: Option<u64>,
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
            request_to_speak_timestamp: text(&value["request_to_speak_timestamp"]),
            self_video: value["self_video"].as_bool().unwrap_or(false),
            self_stream: value["self_stream"].as_bool().unwrap_or(false),
            discoverable: value["discoverable"].as_bool(),
            connected_at: value["connected_at"].as_u64(),
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

/// A server this account belongs to that is not available in its region (`{id, name, icon}` of module 543897).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GeoRestrictedGuild {
    pub id: Id,
    pub name: String,
    pub icon: Option<String>,
}

impl GeoRestrictedGuild {
    fn from_value(value: &Value) -> Option<Self> {
        Some(Self {
            id: id(&value["id"])?,
            name: text(&value["name"]).unwrap_or_default(),
            icon: text(&value["icon"]),
        })
    }
}

#[derive(Default)]
pub struct Stores {
    /// Not in the pinned client: MessageLogger's `handleDelete`. While the app's plugin is on, a deleted message this
    /// answers true for stays in its channel's list and the window's caches; the rest of `MESSAGE_DELETE` (mention
    /// counts, the latest message) happens as usual.
    pub keep_deleted: Option<fn(&Message) -> bool>,
    /// This account's experiment assignments from READY (`ApexExperimentStore`).
    pub experiments: experiments::Assignments,
    /// READY's `guild_experiments` (module 343446), evaluated per guild by `guild_experiment_bucket`.
    pub guild_experiments: experiments::GuildExperiments,
    /// Guild to its AFK voice channel (`afk_channel_id`), which voice join and leave sounds skip.
    pub afk_channels: HashMap<Id, Id>,
    /// Guild to its `afk_timeout` in seconds (the idle move of module 596339 `handleIdleUpdate`).
    pub afk_timeouts: HashMap<Id, u64>,
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
    /// The message sent with a friend request (`relationship.note`, RelationshipStore `getNote` of module 994500).
    pub relationship_notes: HashMap<Id, String>,
    /// The game a friend request was sent from (`origin_application_id`, `getOriginApplicationId`).
    pub relationship_origin_apps: HashMap<Id, Id>,
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
    /// Optimistic accepts and previews of message requests (`MessageRequestPreviewStore`).
    pub message_requests: message_requests::MessageRequests,
    /// `EntitlementStore`.
    pub entitlements: entitlements::Entitlements,
    /// `GuildAutomodMessageStore`.
    pub automod: automod_messages::AutomodMessages,
    /// `NotificationCenterItemsStore`.
    pub notification_center: notification_center::NotificationCenter,
    /// `UserGuildJoinRequestStore` and `GuildJoinRequestStoreV2`.
    pub join_requests: guild_join_requests::JoinRequests,
    /// `LAST_MESSAGES` previews by channel (`MESSAGE_PREVIEWS_LOADED`).
    pub previews: HashMap<Id, Message>,
    /// `NoteStore`.
    pub notes: notes::NoteStore,
    pub game_relationships: game_relationships::GameRelationshipStore,
    /// The Bans page's list (`GuildSettingsStore`).
    pub guild_bans: guild_bans::GuildBans,
    /// Integrations, webhooks and the reads their events ask for (`WebhooksStore`, `MyGuildApplicationsStore`).
    pub integrations: guild_integrations::GuildIntegrations,
    /// `ChannelMemberCountStore`.
    pub channel_member_counts: channel_member_counts::ChannelMemberCounts,
    /// `ThreadMemberListStore`.
    pub thread_member_lists: thread_member_list::ThreadMemberLists,
    /// Required action, friend suggestions, forum unread counts, pending bulk bans.
    pub account_events: account_events::AccountEvents,
    pub member_lists: HashMap<Id, member_list::MemberList>,
    /// User id to status, for people this session has presence for.
    pub presences: HashMap<Id, String>,
    pub presence: presence::PresenceStore,
    pub spotify: spotify::SpotifyStore,
    pub member_counts: guild_member_counts::GuildMemberCountStore,
    /// The connection's country (`LocationMetadataStore`).
    pub location_metadata: location_metadata::LocationMetadataStore,
    pub scheduled_events: scheduled_events::GuildScheduledEventStore,
    /// Voice channel statuses and call start times (`ChannelStatusStore`, `VoiceChannelStartTimeStore`).
    pub channel_status: channel_status::ChannelStatusStore,
    /// Live Stage instances (`StageInstanceStore`).
    pub stage_instances: stage::StageInstanceStore,
    /// The user's scheduled messages and each channel's scheduled-message draft (`scheduledMessageStore`, `DraftStore`).
    pub scheduled_messages: scheduled_message::ScheduledMessageStore,
    pub scheduled_drafts: scheduled_message::ScheduledDrafts,
    pub content_inventory: content_inventory::ContentInventoryStore,
    pub current_user: Option<User>,
    /// This session's status as READY reports it.
    pub status: String,
    /// READY's `session_id`, which interactions carry.
    pub session_id: Option<String>,
    /// READY's `auth_session_id_hash`: which of the account's logged-in devices this client is (module 158935 `NF`,
    /// `getAuthSessionIdHash`). Unknown until READY says.
    pub auth_session_id_hash: Option<String>,
    /// The account's sessions (`SessionsStore`, module 528767): READY's `sessions`, then `SESSIONS_REPLACE`.
    pub sessions: Vec<Session>,
    pub guild_order: Vec<Id>,
    /// Server folders and the rail order (`SortedGuildStore`, `ExpandedGuildFolderStore`).
    pub folders: folders::GuildFolders,
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
    /// The threads the current user has joined (`JoinedThreadsStore`).
    pub thread_members: thread_members::ThreadMembers,
    /// `ThreadMembersStore` (module 802958): member counts and previews of threads.
    pub thread_previews: thread_previews::ThreadMemberPreviews,
    pub thread_messages: thread_messages::ThreadMessages,
    /// Newest and acknowledged pin times per channel (`hasUnreadPins`).
    pub pin_states: pins_unread::PinStates,
    /// Guild stickers and the standard packs (`GuildStickersStore`, `StickersPackStore`).
    pub stickers: stickers::StickersStore,
    /// Nitro emoji packs (`EmojiPackStore`, module 956396).
    pub emoji_packs: emoji_packs::EmojiPackStore,
    /// Channel or thread id to the guild that holds it. Lookups check the answer against the guild, so a guild
    /// changed directly (not through `dispatch`) still resolves, by the slow way.
    channel_guilds: HashMap<Id, Id>,
    /// Guilds the account only lurks in: their `joined_at` is null (260509 `DG`, 857071 `isLurking`).
    pub lurking_guilds: HashSet<Id>,
    /// Guilds READY listed as `{id, unavailable: true}` (an outage) and no GUILD_CREATE has filled in yet. They keep
    /// their place in the folders: `SortedGuildStore` (711014 `N`) drops a guild only when it is neither known nor
    /// unavailable, so a reorder during an outage must not write them out of the account's settings.
    pub unavailable_guilds: HashSet<Id>,
    /// GeoRestrictedGuildStore (module 543897): the servers READY lists with `geo_restricted` (`CONNECTION_OPEN`'s
    /// `geoRestrictedGuilds`) and those a `GUILD_DELETE` takes away with `geo_restricted` (`GUILD_GEO_RESTRICTED`).
    pub geo_restricted_guilds: Vec<GeoRestrictedGuild>,
    /// Current user's `merged_members` entry per guild (including unavailable stubs), so delayed
    /// `GUILD_CREATE` or partial updates preserve roles and join date.
    pub my_members_by_guild: HashMap<Id, Value>,
    /// True when `READY` for the same account carried an empty `private_channels` list while cached/prior DMs
    /// remained loaded, so `READY_SUPPLEMENTAL` replaces them cleanly without a blank flash.
    cached_private_channels_pending: bool,
    /// Per guild, the last `unread::guild_counts` answer; cleared by every event that can change one.
    counts_cache: std::sync::Mutex<HashMap<Id, unread::CountsEntry>>,
}

impl Stores {
    pub fn dispatch(&mut self, name: &str, data: &Value) {
        // Only complete INITIAL_GUILD snapshots can use the replacement path (225123).
        // Partial payloads require cache hydration before they can become GUILD_CREATE.
        let name = if name == "INITIAL_GUILD" && data["data_mode"] != "partial" {
            "GUILD_CREATE"
        } else {
            name
        };
        if name == "VOICE_STATE_UPDATE_BATCH" {
            self.voice_state_batch(data);
            return;
        }
        if name == "READY" {
            // `CONNECTION_OPEN` (module 596344 `H`): every window is stale until it is loaded again.
            for window in self.windows.values_mut() {
                window.ready = false;
                window.loading_more = false;
            }
            self.presence.set_current_user(id(&data["user"]["id"]));
        }
        self.presence.dispatch(name, data);
        self.spotify
            .dispatch(name, data, chrono::Utc::now().timestamp_millis());
        self.member_counts.dispatch(name, data);
        self.location_metadata.dispatch(name, data);
        self.scheduled_events.dispatch(name, data);
        self.channel_status.dispatch(name, data);
        self.stage_instances.dispatch(name, data);
        if name == "READY" {
            self.content_inventory.connection_open();
            // `CONNECTION_OPEN` empties the scheduled-message list.
            self.scheduled_messages.reset();
        }
        self.members.dispatch(name, data);
        self.stickers.dispatch(name, data);
        self.interactions.dispatch(name, data);
        let me = self.current_user.as_ref().map(|user| user.id);
        self.thread_members.dispatch(name, data, me);
        self.thread_previews.dispatch(name, data);
        self.thread_messages.dispatch(name, data);
        self.pin_states.dispatch(name, data);
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
                if let Some(lazy) = data["lazy_private_channels"].as_array() {
                    if self.cached_private_channels_pending {
                        self.private_channels.clear();
                        self.cached_private_channels_pending = false;
                    }
                    for channel in lazy {
                        self.add_private_channel(channel);
                        for recipient in channel["recipients"].as_array().into_iter().flatten() {
                            self.merge_embedded_user(recipient);
                        }
                    }
                    self.private_channels
                        .sort_by_key(|channel| std::cmp::Reverse(channel.last_message_id));
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
            "CALL_CREATE" => {
                // Module 225123: the call's `voice_states` are dispatched as `VOICE_STATE_UPDATES` with no guild.
                // They are who is in a call that began before this connection asked for it.
                let list = self.voice_states.entry(0).or_default();
                for state in data["voice_states"].as_array().into_iter().flatten() {
                    if let Some(user) = id(&state["user_id"]) {
                        list.retain(|known| known.user_id != user);
                        if let Some(state) = VoiceState::from_value(state) {
                            list.push(state);
                        }
                    }
                }
            }
            "CALL_DELETE" => {
                // `VoiceStateStore` (module 977997): the call's channel has no voice states left.
                if let Some(channel) = id(&data["channel_id"])
                    && let Some(list) = self.voice_states.get_mut(&0)
                {
                    list.retain(|state| state.channel_id != channel);
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
                } else if let Some(guild_id) = guild
                    && self.voice_server.as_ref().is_some_and(|server| {
                        server.guild_id == guild_id
                            && (guild_id != 0 || server.channel_id == id(&data["channel_id"]))
                    })
                {
                    // `endpoint: null`: the voice server is gone and another is being allocated. The old one is
                    // not connected to again.
                    self.voice_server = None;
                }
            }
            "GUILD_CREATE" => {
                self.note_afk_channel(data);
                if let Some(mut guild) = Guild::from_value(data) {
                    if let Some(cached) = self.my_members_by_guild.get(&guild.id) {
                        guild.set_my_member(cached);
                    } else if let Some(prior) =
                        self.guilds.iter().find(|known| known.id == guild.id)
                    {
                        if guild.joined_at.is_empty() {
                            guild.joined_at = prior.joined_at.clone();
                        }
                        if guild.my_roles.is_empty() {
                            guild.my_roles = prior.my_roles.clone();
                        }
                        guild.my_pending = prior.my_pending;
                        guild.my_flags = prior.my_flags;
                        guild.my_timeout_until = prior.my_timeout_until;
                    }
                    let me = self.current_user.as_ref().map(|user| user.id);
                    if let Some(member) =
                        data["members"]
                            .as_array()
                            .into_iter()
                            .flatten()
                            .find(|member| {
                                me.is_some()
                                    && (id(&member["user"]["id"]) == me
                                        || id(&member["user_id"]) == me)
                            })
                    {
                        guild.set_my_member(member);
                        self.my_members_by_guild.insert(guild.id, member.clone());
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
                    // 225123 replaces the guild then dispatches its complete voice snapshot; 977997 clears the old one.
                    self.voice_states.insert(
                        guild.id,
                        data["voice_states"]
                            .as_array()
                            .into_iter()
                            .flatten()
                            .filter_map(VoiceState::from_value)
                            .collect(),
                    );
                    self.unavailable_guilds.remove(&guild.id);
                    // ImpersonateStore (164956) keeps a View Server As Role preview across a guild's replacement.
                    if let Some(prior) = self.guilds.iter().find(|known| known.id == guild.id) {
                        guild.viewing_roles = prior.viewing_roles.clone();
                    }
                    self.guilds.retain(|known| known.id != guild.id);
                    self.guilds.push(guild);
                    self.note_guild_scheduled_events(data);
                    self.sort_guilds();
                }
            }
            "GUILD_DELETE" => {
                // Module 543897: `GUILD_DELETE` drops a server, then `GUILD_GEO_RESTRICTED` adds it (replacing an
                // earlier entry) when it went away for its region; `GUILD_DELETE` alone removes the entry.
                if let Some(guild) = id(&data["id"]) {
                    self.geo_restricted_guilds.retain(|known| known.id != guild);
                }
                if data["geo_restricted"] == true
                    && let Some(geo) = GeoRestrictedGuild::from_value(data)
                {
                    self.guilds.retain(|known| known.id != geo.id);
                    self.unavailable_guilds.remove(&geo.id);
                    self.geo_restricted_guilds.push(geo);
                }
                // An outage (`unavailable`) keeps the guild; a removal drops it.
                if data["unavailable"] != true
                    && let Some(guild) = id(&data["id"])
                {
                    self.guilds.retain(|known| known.id != guild);
                    self.unavailable_guilds.remove(&guild);
                    self.voice_states.remove(&guild);
                    self.afk_channels.remove(&guild);
                    self.afk_timeouts.remove(&guild);
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
                    // 164956 `GUILD_ROLE_DELETE`: the role leaves a View Server As Role preview too.
                    if let Some(viewing) = guild.viewing_roles.as_mut() {
                        viewing.retain(|known| *known != role);
                    }
                }
            }
            "GUILD_MEMBER_UPDATE" => {
                if let Some(list) =
                    id(&data["guild_id"]).and_then(|g| self.member_lists.get_mut(&g))
                {
                    list.member_updated(data);
                }
                let me = self.current_user.as_ref().map(|user| user.id);
                if me.is_some()
                    && (id(&data["user"]["id"]) == me || id(&data["user_id"]) == me)
                    && let Some(guild) = id(&data["guild_id"])
                        .and_then(|guild| self.guilds.iter_mut().find(|known| known.id == guild))
                {
                    guild.set_my_member(data);
                    self.my_members_by_guild.insert(guild.id, data.clone());
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
                    if let Some(me) = me
                        && message.mention_user_ids.contains(&me)
                    {
                        self.read_states
                            .track_direct_mention(message.channel_id, message.id);
                    }
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
                if let (Some(guild), Some(thread)) =
                    (id(&data["guild_id"]), Thread::from_value(data))
                {
                    self.note_thread_latest(&thread);
                    if let Some(guild) = self.guilds.iter_mut().find(|known| known.id == guild) {
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
                    }
                }
            }
            "RELATIONSHIP_ADD" | "RELATIONSHIP_UPDATE" => {
                if data["user"].is_object() {
                    self.merge_user(&data["user"]);
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
                    // 994500: ADD keeps an absent note or origin, UPDATE drops it.
                    if let Some(note) = text(&data["note"]) {
                        self.relationship_notes.insert(user, note);
                    } else if name == "RELATIONSHIP_UPDATE" {
                        self.relationship_notes.remove(&user);
                    }
                    if let Some(app) = id(&data["origin_application_id"]) {
                        self.relationship_origin_apps.insert(user, app);
                    } else if name == "RELATIONSHIP_UPDATE" {
                        self.relationship_origin_apps.remove(&user);
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
                    self.refresh_guild_order();
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
                    self.relationship_notes.remove(&user);
                    self.relationship_origin_apps.remove(&user);
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
                if let (Some(channel), Some(message)) = (id(&data["channel_id"]), id(&data["id"])) {
                    self.delete_messages(channel, &[message]);
                }
            }
            "MESSAGE_DELETE_BULK" => self.message_delete_bulk(data),
            "MESSAGE_REACTION_REMOVE_ALL" => self.reactions_removed(data, false),
            "MESSAGE_REACTION_REMOVE_EMOJI" => self.reactions_removed(data, true),
            "MESSAGE_REACTION_ADD_MANY" => self.reaction_add_many(data),
            "USER_UPDATE" => self.user_update(data),
            "PASSIVE_UPDATE_V1" | "PASSIVE_UPDATE_V2" => self.passive_update(data),
            "GUILD_FEATURE_ACK" => {
                if let (Some(guild), Some(kind)) = (id(&data["id"]), data["ack_type"].as_u64()) {
                    let now = read_state::snowflake_at(
                        chrono::Utc::now().timestamp_millis().max(0) as u64,
                    );
                    self.read_states
                        .non_channel_ack(guild, kind, id(&data["acked_id"]), now);
                }
            }
            "USER_NON_CHANNEL_ACK" => {
                if let (Some(me), Some(kind)) = (
                    self.current_user.as_ref().map(|u| u.id),
                    data["ack_type"].as_u64(),
                ) {
                    let now = read_state::snowflake_at(
                        chrono::Utc::now().timestamp_millis().max(0) as u64,
                    );
                    self.read_states
                        .non_channel_ack(me, kind, id(&data["acked_id"]), now);
                }
            }
            "GUILD_SCHEDULED_EVENT_CREATE" => {
                if let (Some(guild), Some(event_id)) = (id(&data["guild_id"]), id(&data["id"])) {
                    let me = self.current_user.as_ref().map(|u| u.id);
                    let creator = id(&data["creator_id"]);
                    let mute_events = self
                        .guild_settings
                        .guilds
                        .get(&guild)
                        .is_some_and(|s| s.mute_scheduled_events);
                    let state = self.read_states.typed.entry((guild, 1)).or_default();
                    state.last = state.last.max(event_id);
                    if me.is_some() && creator == me {
                        self.read_states
                            .non_channel_ack(guild, 1, Some(event_id), event_id);
                    } else if !mute_events {
                        state.mention_count = state.mention_count.saturating_add(1);
                    }
                }
            }
            "GUILD_SCHEDULED_EVENT_UPDATE" | "GUILD_SCHEDULED_EVENT_DELETE" => {
                if let Some(guild) = id(&data["guild_id"]) {
                    let gone = name == "GUILD_SCHEDULED_EVENT_DELETE"
                        || matches!(data["status"].as_u64(), Some(3 | 4));
                    if gone && let Some(state) = self.read_states.typed.get_mut(&(guild, 1)) {
                        state.mention_count = state.mention_count.saturating_sub(1);
                        if let Some(event_id) = id(&data["id"])
                            && state.last == event_id
                        {
                            state.last = self
                                .scheduled_events
                                .get_guild_scheduled_events_for_guild(guild)
                                .iter()
                                .map(|event| event.id)
                                .max()
                                .unwrap_or(state.acked);
                        }
                    }
                }
            }
            _ => {}
        }
        // Populate `private_channels` from `READY`, `READY_SUPPLEMENTAL`, and `CHANNEL_CREATE` before call dispatch
        // checks `channel_known`.
        {
            let known: Vec<Id> = self
                .private_channels
                .iter()
                .map(|channel| channel.id)
                .collect();
            self.calls
                .dispatch(name, data, &|channel| known.contains(&channel));
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
        self.message_requests_dispatch(name, data);
        self.entitlements_dispatch(name, data);
        self.automod_dispatch(name, data);
        self.notification_center_dispatch(name, data);
        self.join_requests_dispatch(name, data);
        self.deleted_entities_dispatch(name, data);
        self.notes_dispatch(name, data);
        self.game_relationships_dispatch(name, data);
        self.guild_bans_dispatch(name, data);
        self.guild_integrations_dispatch(name, data);
        self.channel_member_counts_dispatch(name, data);
        self.thread_member_list_dispatch(name, data);
        self.account_events_dispatch(name, data);
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
        if thread_members::ThreadMembers::AFFECTS_THREADS.contains(&name) {
            self.sync_thread_members();
        }
        if !Self::COUNT_NEUTRAL.contains(&name) {
            self.invalidate_counts();
        }
    }

    /// Every known thread takes its membership from `thread_members` (`JoinedThreadsStore.hasJoined` and friends).
    pub fn sync_thread_members(&mut self) {
        for guild in &mut self.guilds {
            for thread in &mut guild.threads {
                thread.set_member(self.thread_members.get(thread.id));
            }
        }
    }

    /// The `members` of a message-search, thread-search or archived-threads response body (`SEARCH_MESSAGES_SUCCESS`,
    /// `LOAD_THREADS_SUCCESS`, `LOAD_ARCHIVED_THREADS_SUCCESS`).
    pub fn thread_members_from_response(&mut self, guild: Option<Id>, body: &Value) {
        self.thread_members.add_response(guild, body);
        self.sync_thread_members();
    }

    /// `THREAD_MEMBER_LOCAL_UPDATE`: this client joined or left a forum post and waits for the server's word.
    pub fn thread_member_local_update(&mut self, guild: Id, thread: Id, joining: bool) {
        self.thread_members.local_update(thread, guild, joining);
        self.sync_thread_members();
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
            self.merge_embedded_user(user);
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
            match properties["afk_timeout"].as_u64() {
                Some(timeout) => self.afk_timeouts.insert(guild_id, timeout),
                None => self.afk_timeouts.remove(&guild_id),
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

    /// `voiceAndVideo.videoBackgroundFilterDesktop` as the account holds it, `None` when no member is set.
    pub fn video_background(&self) -> Option<settings::VideoBackgroundFilter> {
        settings::video_background_in(&self.settings_raw)
    }

    /// Sets `videoBackgroundFilterDesktop`: the new value shows at once and the PATCH body value (the whole
    /// `voice_and_video` message) comes back, empty until the account's settings have arrived.
    pub fn set_video_background(
        &mut self,
        filter: Option<&settings::VideoBackgroundFilter>,
    ) -> String {
        if !self.settings_loaded {
            return String::new();
        }
        let (patch, updated) = settings::set_video_background(&self.settings_raw, filter);
        self.settings_raw = updated;
        patch
    }

    /// The custom theme the account holds (`customUserThemeSettings`), if it has colours.
    pub fn custom_theme(&self) -> Option<settings::CustomTheme> {
        settings::custom_theme_in(&self.settings_raw)
    }

    /// Applies a custom theme to the account the way `u_` does (the theme and the client theme settings together,
    /// the colour theme preset goes): the PATCH body value comes back, empty until the account's settings have
    /// arrived.
    pub fn set_custom_theme(&mut self, theme: u64, custom: &settings::CustomTheme) -> String {
        if !self.settings_loaded {
            return String::new();
        }
        let (patch, updated) = settings::set_custom_theme(&self.settings_raw, theme, custom);
        self.settings_raw = updated;
        self.theme = theme;
        self.prefs.remove(settings::CLIENT_THEME_PRESET.key);
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

    /// Whether the account's settings have arrived (dismissals are unknown before).
    pub fn settings_loaded(&self) -> bool {
        self.settings_loaded
    }

    pub fn content_dismissed(&self, content: u16) -> bool {
        settings::content_dismissed(&self.settings_raw, content)
    }

    /// The account has an entrance sound for this guild, or one for every guild (module 984813 `mz`).
    pub fn join_sound_set(&self, guild: Id) -> bool {
        settings::join_sound_set(&self.settings_raw, guild)
    }

    /// Sets the entrance sound of `guild` (`0` for everywhere); empty until the settings have arrived, as `set_pref`.
    pub fn set_join_sound(&mut self, guild: u64, sound: u64, sound_guild: u64) -> String {
        if !self.settings_loaded {
            return String::new();
        }
        let (patch, updated) =
            settings::set_join_sound(&self.settings_raw, guild, sound, sound_guild);
        self.settings_raw = updated;
        patch
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

    /// A recurring dismissible content dismissed less than `cooldown_ms` ago (`FZ` of module 543862). Before the
    /// settings arrive it counts as dismissed, so nothing shows that the account had put away.
    pub fn recurring_dismissed(&self, content: u64, cooldown_ms: u64, now_ms: u64) -> bool {
        !self.settings_loaded
            || settings::recurring_dismissed(&self.settings_raw, content, cooldown_ms, now_ms)
    }

    /// Dismisses a recurring content now (`uh`); empty until the settings have arrived, as `dismiss_content`.
    pub fn dismiss_recurring(&mut self, content: u64, now_ms: u64) -> String {
        if !self.settings_loaded {
            return String::new();
        }
        let (patch, updated) = settings::dismiss_recurring(&self.settings_raw, content, now_ms);
        self.settings_raw = updated;
        patch
    }

    /// The in-app feedback state of one survey type (`inAppFeedbackSettings`, read by module 220162).
    pub fn in_app_feedback(&self, kind: u64) -> settings::InAppFeedbackState {
        settings::in_app_feedback_state(&self.settings_raw, kind)
    }

    /// Sets one survey type's state and returns the PATCH value; empty before the settings arrived.
    pub fn set_in_app_feedback(
        &mut self,
        kind: u64,
        state: settings::InAppFeedbackState,
    ) -> String {
        if !self.settings_loaded {
            return String::new();
        }
        let (patch, updated) = settings::set_in_app_feedback(&self.settings_raw, kind, state);
        self.settings_raw = updated;
        patch
    }

    /// The status the account's proto settings hold: the chosen one, when it lapses, and the custom status.
    pub fn status_state(&self) -> Option<settings::StatusState> {
        settings::read_status(&self.settings_raw)
    }

    /// The status to show and send at `now_ms`: the saved one, or `online` once a status that was set for a time has
    /// lapsed (`status_expires_at_ms`), even when the server's settings update that clears it was missed.
    pub fn status_at(&self, now_ms: u64) -> String {
        match self.status_state() {
            Some(state) if state.expires_at_ms > 0 && state.expires_at_ms <= now_ms => {
                "online".to_owned()
            }
            _ => self.status.clone(),
        }
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
        let incoming_user = User::from_value(&data["user"]);
        let same_account = self
            .current_user
            .as_ref()
            .zip(incoming_user.as_ref())
            .is_some_and(|(prev, next)| prev.id == next.id);
        self.current_user = incoming_user;
        self.experiments = experiments::Assignments::from_ready(
            &data["apex_experiments"],
            data["user"]["id"].as_str().unwrap_or_default(),
        )
        .with_legacy(&data["experiments"]);
        self.guild_experiments = experiments::GuildExperiments::from_ready(&data["guild_experiments"]);
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
        self.auth_session_id_hash = text(&data["auth_session_id_hash"]);
        self.sessions = Session::list(&data["sessions"]);
        // Module 894768: the account's own status is the one saved in its settings (`online` when none is saved),
        // never the status this session happened to connect with.
        self.status = self
            .status_state()
            .and_then(|state| state.status)
            .filter(|status| !status.is_empty() && status != "unknown")
            .unwrap_or_else(|| "online".to_owned());
        for user in data["users"].as_array().into_iter().flatten() {
            if let Some(user) = User::from_value(user) {
                self.users.insert(user.id, user);
            }
        }
        for entry in data["relationships"].as_array().into_iter().flatten() {
            if entry["user"].is_object() {
                self.merge_embedded_user(&entry["user"]);
            }
        }
        for channel in data["private_channels"].as_array().into_iter().flatten() {
            for recipient in channel["recipients"].as_array().into_iter().flatten() {
                self.merge_embedded_user(recipient);
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
        self.relationship_notes = data["relationships"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|entry| Some((id(&entry["id"])?, text(&entry["note"])?)))
            .collect();
        self.relationship_origin_apps = data["relationships"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|entry| Some((id(&entry["id"])?, id(&entry["origin_application_id"])?)))
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
        let incoming_private: Vec<PrivateChannel> = data["private_channels"]
            .as_array()
            .map(|channels| {
                channels
                    .iter()
                    .filter_map(PrivateChannel::from_value)
                    .collect()
            })
            .unwrap_or_default();
        if incoming_private.is_empty() && same_account && !self.private_channels.is_empty() {
            // Live READY omits `private_channels` for user accounts until `READY_SUPPLEMENTAL`:
            // retain prior/cached DMs in the meantime so the sidebar does not flicker empty.
            self.cached_private_channels_pending = true;
        } else {
            self.cached_private_channels_pending = false;
            self.private_channels = incoming_private;
            // Most recent conversation first, as the official list is.
            self.private_channels
                .sort_by_key(|channel| std::cmp::Reverse(channel.last_message_id));
        }
        // ImpersonateStore (164956) does not listen to READY: a View Server As Role preview survives a reconnect of
        // the same account (never handed to another one).
        let viewing: Vec<(Id, Vec<Id>)> = if same_account {
            self.guilds
                .iter()
                .filter_map(|guild| Some((guild.id, guild.viewing_roles.clone()?)))
                .collect()
        } else {
            Vec::new()
        };
        self.guilds = data["guilds"]
            .as_array()
            .map(|guilds| guilds.iter().filter_map(Guild::from_value).collect())
            .unwrap_or_default();
        for (id, roles) in viewing {
            if let Some(guild) = self.guilds.iter_mut().find(|guild| guild.id == id) {
                guild.viewing_roles = Some(roles);
            }
        }
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
        self.afk_timeouts.clear();
        for guild in data["guilds"].as_array().into_iter().flatten() {
            self.note_afk_channel(guild);
        }
        self.my_members_by_guild.clear();
        for (guild_id, member) in guild_ids.into_iter().zip(merged) {
            if let (Some(guild_id), Some(member)) = (guild_id, member) {
                if let Some(guild) = self.guilds.iter_mut().find(|guild| guild.id == guild_id) {
                    guild.set_my_member(&member);
                }
                self.my_members_by_guild.insert(guild_id, member);
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
        for guild in data["guilds"].as_array().into_iter().flatten() {
            self.note_guild_scheduled_events(guild);
        }
        self.lurking_guilds = data["guilds"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|guild| guild.get("joined_at").is_some_and(Value::is_null))
            .filter_map(|guild| id(&guild["id"]))
            .collect();
        // A stub has no channels, so `Guild::from_value` skipped it; GUILD_CREATE fills it in later.
        // `CONNECTION_OPEN` of module 225123: the unavailable servers leave out the geo-restricted ones, which go to
        // GeoRestrictedGuildStore.
        self.geo_restricted_guilds = data["guilds"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|guild| guild["geo_restricted"] == true)
            .filter_map(GeoRestrictedGuild::from_value)
            .collect();
        self.unavailable_guilds = data["guilds"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|guild| guild["unavailable"] == true && guild["geo_restricted"] != true)
            .filter_map(|guild| id(&guild["id"]))
            .filter(|guild| self.guilds.iter().all(|known| known.id != *guild))
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

    /// `GET /users/@me/saved-messages?bookmark_ids=true`: `{results: [{message, save_data}], bookmark_ids}`.
    pub fn load_saved(&mut self, body: &Value) {
        self.saved.clear();
        for entry in body["results"].as_array().into_iter().flatten() {
            self.save(&entry["save_data"]);
        }
        for entry in body["bookmark_ids"].as_array().into_iter().flatten() {
            let pair = id(&entry["channel_id"])
                .zip(id(&entry["message_id"]))
                .or_else(|| {
                    entry
                        .as_array()
                        .filter(|pair| pair.len() == 2)
                        .and_then(|pair| id(&pair[0]).zip(id(&pair[1])))
                })
                .or_else(|| {
                    entry.as_str().and_then(|text| {
                        let (channel, message) = text.split_once('-')?;
                        Some((channel.parse().ok()?, message.parse().ok()?))
                    })
                });
            if let Some((channel, message)) = pair {
                self.saved.entry((channel, message)).or_insert(None);
            }
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

    /// The order from the settings proto. Guilds it does not list yet go on top, newest first. The folders are
    /// rebuilt from the same settings (`SortedGuildStore` `N`).
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
        let (entries, positions) = settings::guild_folders(&self.settings_raw);
        let positions = if entries.is_empty() && positions.is_empty() {
            self.guild_order.clone()
        } else {
            positions
        };
        let listed: std::collections::HashSet<Id> = if entries.is_empty() {
            positions.iter().copied().collect()
        } else {
            entries
                .iter()
                .flat_map(|entry| entry.guild_ids.iter().copied())
                .collect()
        };
        let mut known: Vec<Id> = self.guilds.iter().map(|guild| guild.id).collect();
        known.extend(self.unavailable_guilds.iter().copied());
        let unlisted: Vec<Id> = known
            .iter()
            .copied()
            .filter(|id| !listed.contains(id))
            .collect();
        self.folders
            .rebuild(&entries, &positions, &known, &unlisted);
        let flat = self.folders.flattened();
        self.guilds
            .sort_by_key(|guild| flat.iter().position(|id| *id == guild.id));
    }

    /// The order and folders from the settings as they are now (a settings update from another session).
    fn refresh_guild_order(&mut self) {
        let (entries, positions) = settings::guild_folders(&self.settings_raw);
        self.guild_order = if entries.is_empty() {
            positions
        } else {
            entries
                .into_iter()
                .flat_map(|entry| entry.guild_ids)
                .collect()
        };
        self.sort_guilds();
    }

    /// Writes the tree's entries into the account's settings and re-sorts the rail. The PATCH body value comes back
    /// (`updateAsync("guildFolders", ...)`, module 817281 `um`); empty until the settings have arrived, and the tree
    /// still changes locally then.
    pub fn commit_guild_folders(&mut self) -> String {
        let mut entries = self.folders.to_entries();
        // The tree holds only the servers this client knows. A server that is in the saved order but has not
        // arrived (an outage, a slow start) keeps its place in its folder, or goes to the end, so that a drag
        // can never write it out of the account's folders.
        let (stored, _) = settings::guild_folders(&self.settings_raw);
        let written: HashSet<Id> = entries
            .iter()
            .flat_map(|entry| entry.guild_ids.iter().copied())
            .collect();
        for old in stored {
            for guild in old.guild_ids {
                if written.contains(&guild) || self.guilds.iter().any(|known| known.id == guild) {
                    continue;
                }
                match entries
                    .iter_mut()
                    .find(|entry| old.id.is_some() && entry.id == old.id)
                {
                    Some(folder) => folder.guild_ids.push(guild),
                    None => entries.push(settings::GuildFolderEntry {
                        guild_ids: vec![guild],
                        ..Default::default()
                    }),
                }
            }
        }
        self.guild_order = self.folders.flattened();
        let flat = self.guild_order.clone();
        self.guilds
            .sort_by_key(|guild| flat.iter().position(|id| *id == guild.id));
        if !self.settings_loaded {
            return String::new();
        }
        let (patch, updated) = settings::set_guild_folders(&self.settings_raw, &entries);
        self.settings_raw = updated;
        patch
    }

    /// The account's favourite channels (`FavoritesStore`, module 181079), read from the settings.
    pub fn favorites(&self) -> settings::Favorites {
        settings::read_favorites(&self.settings_raw)
    }

    /// Writes the favourites into the account's settings (`updateAsync("favorites", ...)`, module 146092 `K`). The
    /// PATCH body value comes back; empty until the settings have arrived.
    pub fn write_favorites(&mut self, favorites: &settings::Favorites) -> String {
        if !self.settings_loaded {
            return String::new();
        }
        let (patch, updated) = settings::set_favorites(&self.settings_raw, favorites);
        self.settings_raw = updated;
        patch
    }

    /// A flat order of plain servers, listed ones first (a capture script's layout; the folders are dropped).
    pub fn set_rail_order(&mut self, ids: &[Id]) {
        let entries: Vec<settings::GuildFolderEntry> = ids
            .iter()
            .map(|id| settings::GuildFolderEntry {
                guild_ids: vec![*id],
                ..settings::GuildFolderEntry::default()
            })
            .collect();
        let known: Vec<Id> = self.guilds.iter().map(|guild| guild.id).collect();
        self.folders.rebuild(&entries, &[], &known, &[]);
    }

    /// "Move to... First in List / Last in List" (`moveFirst`, `moveLast`).
    pub fn move_guild(&mut self, id: Id, placement: folders::Placement) -> String {
        if !self.folders.move_guild(id, placement) {
            return String::new();
        }
        self.commit_guild_folders()
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
                if mine && reaction.me == (change > 0) {
                    return;
                }
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

    /// Applies or reverts an optimistic reaction change by the current user before the gateway echo arrives.
    pub fn apply_own_reaction(
        &mut self,
        channel: Id,
        message_id: Id,
        name: &str,
        emoji_id: Option<Id>,
        add: bool,
    ) {
        let Some(message) = self
            .messages
            .get_mut(&channel)
            .and_then(|list| list.iter_mut().find(|known| known.id == message_id))
        else {
            return;
        };
        let found = message
            .reactions
            .iter()
            .position(|reaction| reaction.name == name && reaction.emoji_id == emoji_id);
        match (found, add) {
            (Some(index), true) => {
                let reaction = &mut message.reactions[index];
                if !reaction.me {
                    reaction.count += 1;
                    reaction.me = true;
                }
            }
            (Some(index), false) => {
                let reaction = &mut message.reactions[index];
                if reaction.me {
                    reaction.count = reaction.count.saturating_sub(1);
                    reaction.me = false;
                    if reaction.count == 0 {
                        message.reactions.remove(index);
                    }
                }
            }
            (None, true) => message.reactions.push(Reaction {
                name: name.to_owned(),
                emoji_id,
                count: 1,
                me: true,
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
        if let Some(everyone) = data["mention_everyone"].as_bool() {
            known.mention_everyone = everyone;
        }
        if let Some(mentions) = data["mentions"].as_array() {
            known.mentions = mentions.len();
            known.mention_user_ids = mentions.iter().filter_map(|user| id(&user["id"])).collect();
        }
        if let Some(roles) = data["mention_roles"].as_array() {
            known.mention_roles = roles.len();
            known.mention_role_ids = roles.iter().filter_map(id).collect();
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
        if data["embeds"].is_array() {
            known.embeds = Embed::list(&data["embeds"], Some(message));
        }
        // Removing or replacing attachments arrives on MESSAGE_UPDATE with the new list.
        if let Some(attachments) = data["attachments"].as_array() {
            known.attachments = attachments
                .iter()
                .filter_map(Attachment::from_value)
                .collect();
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

    /// Merges a user object into `UserStore` (`287809` `mergeUser` / `USER_PROFILE_FETCH_SUCCESS`), updating present
    /// fields on an existing entry without clobbering omitted ones.
    pub fn merge_user(&mut self, value: &Value) -> Option<&User> {
        let user_id = id(&value["id"])?;
        if let Some(existing) = self.users.get_mut(&user_id) {
            existing.merge_value(value);
            return self.users.get(&user_id);
        }
        let user = User::from_value(value)?;
        self.users.insert(user_id, user);
        self.users.get(&user_id)
    }

    fn merge_embedded_user(&mut self, value: &Value) {
        let Some(user_id) = id(&value["id"]) else {
            return;
        };
        if let Some(existing) = self.users.get_mut(&user_id) {
            existing.enrich_from_value(value);
        } else if let Some(user) = User::from_value(value) {
            self.users.insert(user_id, user);
        }
    }

    fn set_presence(&mut self, data: &Value) {
        if data["user"].is_object() {
            self.merge_user(&data["user"]);
        }
        let user = id(&data["user_id"]).or_else(|| id(&data["user"]["id"]));
        if let (Some(user), Some(status)) = (user, text(&data["status"])) {
            self.presences.insert(user, status);
        }
    }

    /// Known users across all relationship types (friends, pending incoming/outgoing, blocked), ignored users, and
    /// private channel recipients (1:1 DMs, group DMs, message requests), deduplicated by id.
    pub fn relationship_and_channel_users(&self) -> Vec<&User> {
        let mut ids = std::collections::BTreeSet::new();
        ids.extend(self.relationships.keys().copied());
        ids.extend(self.ignored.iter().copied());
        for channel in &self.private_channels {
            ids.extend(channel.recipients.iter().copied());
        }
        ids.into_iter()
            .filter_map(|id| self.users.get(&id))
            .collect()
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
        // The key is built once per friend, not once per comparison: the friends page asks on every frame.
        friends.sort_by_cached_key(|user| {
            self.friend_nickname(user.id)
                .unwrap_or(user.display_name())
                .to_lowercase()
        });
        friends
    }

    /// Whether any friend is known, without sorting the list (`friends().is_empty()`, drawn every frame).
    pub fn has_friends(&self) -> bool {
        self.relationships
            .iter()
            .any(|(user, kind)| *kind == RELATIONSHIP_FRIEND && self.users.contains_key(user))
    }

    /// Whether any friend is not offline, without sorting the list.
    pub fn has_online_friends(&self) -> bool {
        self.relationships.iter().any(|(user, kind)| {
            *kind == RELATIONSHIP_FRIEND
                && self.users.contains_key(user)
                && self.status(*user) != "offline"
        })
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

    /// `isSystemDM` of the channel record (module 95701): a 1:1 DM whose first recipient is the system account
    /// (`system: true`; there can be more than one such channel).
    pub fn is_system_dm(&self, channel: &PrivateChannel) -> bool {
        channel.kind == 1
            && channel
                .recipients
                .first()
                .and_then(|id| self.users.get(id))
                .is_some_and(|user| user.system)
    }

    pub fn insert_message(&mut self, message: Message) {
        let list = self.messages.entry(message.channel_id).or_default();
        message_window::insert_in_order(list, message);
    }

    pub fn set_message_state(&mut self, channel: Id, id: Id, state: MessageState) {
        let list = self.messages.entry(channel).or_default();
        self.windows
            .entry(channel)
            .or_default()
            .update(list, id, |message| {
                message.state = state;
            });
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
        if let Some(&newest) = ids.iter().max() {
            let latest = self.read_states.latest.entry(channel).or_insert(0);
            *latest = (*latest).max(newest);
        }
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

    /// `ExperimentStore.getGuildExperimentBucket(name, guild)`: the guild's bucket, `None` when it is not eligible.
    pub fn guild_experiment_bucket(&self, name: &str, guild: Id, now_ms: u64) -> Option<i64> {
        let known = self.guild(guild);
        let facts = experiments::GuildFacts {
            id: guild,
            member_count: self.member_counts.get_member_count(guild),
            features: known.map_or(&[][..], |known| known.features.as_slice()),
            hub_type: None,
            has_vanity_url: known.is_some_and(|known| known.vanity_url_code.is_some()),
            now_ms,
        };
        self.guild_experiments.bucket(name, &facts)
    }

    pub fn guild(&self, guild: Id) -> Option<&Guild> {
        self.guilds.iter().find(|known| known.id == guild)
    }

    pub(crate) fn delete_messages(&mut self, channel: Id, ids: &[Id]) {
        if ids.is_empty() {
            return;
        }
        let joined = self
            .guild_of_channel(channel)
            .map_or(0, Guild::joined_snowflake);
        let acked = self.read_states.acked(channel, joined);
        let now = chrono::Utc::now();
        let mention_drops: u64 = self.messages.get(&channel).map_or(0, |list| {
            list.iter()
                .filter(|msg| ids.contains(&msg.id) && msg.id > acked)
                .filter(|msg| unread::stored_message_mention(self, msg, now).is_some())
                .count() as u64
        });
        if mention_drops > 0
            && let Some(state) = self.read_states.states.get_mut(&channel)
        {
            state.mention_count = state.mention_count.saturating_sub(mention_drops);
            if state.mention_count == 0 {
                state.low_importance = false;
            }
        }
        if let Some(list) = self.messages.get_mut(&channel) {
            let window = self.windows.entry(channel).or_default();
            for &message in ids {
                // Not in the pinned client: MessageLogger keeps it (`keep_deleted`).
                if let Some(keep) = self.keep_deleted
                    && list
                        .iter()
                        .chain(window.before_cache().messages())
                        .chain(window.after_cache().messages())
                        .find(|known| known.id == message)
                        .is_some_and(keep)
                {
                    continue;
                }
                window.remove(list, message);
            }
            // ReferencedMessageStore (module 9842) on MESSAGE_DELETE: a reply to a deleted message now reads
            // "Original message was deleted".
            for reply in list.iter_mut() {
                if matches!(reply.kind, 19 | 21 | 23)
                    && reply
                        .reference_id
                        .is_some_and(|reference| ids.contains(&reference))
                {
                    reply.referenced = None;
                    reply.extra.reference_deleted = true;
                }
            }
        }
        if self
            .read_states
            .latest
            .get(&channel)
            .copied()
            .is_some_and(|latest| ids.contains(&latest))
        {
            if let Some(newest) = self
                .messages
                .get(&channel)
                .and_then(|list| list.last())
                .map(|m| m.id)
            {
                self.read_states.latest.insert(channel, newest);
            } else if let Some(acked) = self.read_states.states.get(&channel).map(|s| s.acked) {
                self.read_states.latest.insert(channel, acked);
            } else {
                self.read_states.latest.remove(&channel);
            }
        }
    }

    fn passive_update(&mut self, data: &Value) {
        let guild_id = id(&data["guild_id"]);
        for member in data["updated_members"].as_array().into_iter().flatten() {
            self.merge_user(&member["user"]);
        }
        if let Some(guild) = guild_id {
            let voices = self.voice_states.entry(guild).or_default();
            for value in data["updated_voice_states"]
                .as_array()
                .into_iter()
                .flatten()
            {
                if let Some(user) = id(&value["user_id"]) {
                    voices.retain(|voice| voice.user_id != user);
                    if let Some(voice) = VoiceState::from_value(value) {
                        voices.push(voice);
                    }
                }
            }
            for user in data["removed_voice_states"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(id)
            {
                voices.retain(|voice| voice.user_id != user);
            }
        }
        let entries = data["updated_channels"]
            .as_array()
            .or_else(|| data["channels"].as_array());
        for entry in entries.into_iter().flatten() {
            let Some(channel_id) = id(&entry["id"]) else {
                continue;
            };
            if let Some(last) = id(&entry["last_message_id"]) {
                let latest = self.read_states.latest.entry(channel_id).or_insert(0);
                *latest = (*latest).max(last);
                if let Some(guild) =
                    guild_id.and_then(|gid| self.guilds.iter_mut().find(|known| known.id == gid))
                {
                    if let Some(channel) = guild.channels.iter_mut().find(|c| c.id == channel_id) {
                        if channel.last_message_id.is_none_or(|prev| last > prev) {
                            channel.last_message_id = Some(last);
                        }
                    } else if let Some(thread) =
                        guild.threads.iter_mut().find(|t| t.id == channel_id)
                        && thread.last_message_id.is_none_or(|prev| last > prev)
                    {
                        thread.last_message_id = Some(last);
                    }
                }
            }
        }
    }

    fn note_guild_scheduled_events(&mut self, guild_value: &Value) {
        let Some(gid) = id(&guild_value["id"]) else {
            return;
        };
        let events = guild_value["guild_scheduled_events"].as_array();
        let max_id = events
            .into_iter()
            .flatten()
            .filter_map(|event| id(&event["id"]))
            .max();
        if max_id.is_none() && !self.read_states.typed.contains_key(&(gid, 1)) {
            return;
        }
        let joined = self
            .guilds
            .iter()
            .find(|g| g.id == gid)
            .map_or(0, Guild::joined_snowflake);
        let state = self.read_states.typed.entry((gid, 1)).or_default();
        if state.acked == 0 && joined > 0 {
            state.acked = joined;
        }
        if let Some(max_id) = max_id {
            state.last = state.last.max(max_id);
        }
    }
}

#[cfg(test)]
mod tests {
    /// A forward of a voice message: the copy keeps the original's flag, so it draws as a player.
    #[test]
    fn a_forwarded_voice_message_keeps_its_flag() {
        let original: serde_json::Value =
            serde_json::from_str(include_str!("../../../tests/fixtures/voice_message.json"))
                .unwrap();
        let forward = serde_json::json!({
            "id": "5", "channel_id": "7", "type": 0, "content": "", "flags": 16384,
            "timestamp": "2026-11-11T10:00:00.000000+00:00",
            "author": {"id": "1", "username": "a"},
            "message_reference": {"type": 1, "message_id": "4", "channel_id": "7"},
            "message_snapshots": [{"message": {
                "content": "", "flags": original["flags"], "timestamp": original["timestamp"],
                "attachments": original["attachments"], "embeds": [],
            }}],
        });
        let message = super::Message::from_value(&forward).unwrap();
        let copy = message.forwarded.unwrap();
        assert_eq!(
            copy.flags & super::VOICE_MESSAGE_FLAG,
            super::VOICE_MESSAGE_FLAG
        );
        assert_eq!(copy.attachments[0].duration_secs, Some(6.0065));
    }

    /// Module 677413: `^@silent(\s|$)`, then the rest trimmed.
    #[test]
    fn silent_prefix_is_split_only_before_whitespace_or_the_end() {
        use super::split_silent_prefix as split;
        assert_eq!(split("@silent hello  "), (true, "hello"));
        assert_eq!(split("@silent\nline"), (true, "line"));
        assert_eq!(split("@silent"), (true, ""));
        assert_eq!(split("@silent @silent x"), (true, "@silent x"));
        assert_eq!(split("@silently"), (false, "@silently"));
        assert_eq!(split(" @silent x"), (false, " @silent x"));
        assert_eq!(split("hi @silent"), (false, "hi @silent"));
    }

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
        assert_eq!(file.duration_secs, Some(6.0065));
        assert_eq!(file.waveform.as_deref().map(str::len), Some(200));
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
        // `fK`: a GIFV's thumbnail stays its thumbnail (the poster), only articles and images move it to the image.
        assert!(embed.image.is_none());
        assert_eq!(
            embed.thumbnail.unwrap().url,
            "https://example.com/poster.png"
        );
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
    fn a_gradient_needs_the_server_feature_and_a_second_colour_on_the_colour_role() {
        let value = |features: serde_json::Value| {
            super::Guild::from_value(&serde_json::json!({
                "id":"100", "name":"fixture", "channels":[], "features": features, "roles":[
                    {"id":"101", "name":"pair", "position":3, "color":1,
                     "colors":{"primary_color":1,"secondary_color":2,"tertiary_color":null}},
                    {"id":"102", "name":"holo", "position":2, "color":4,
                     "colors":{"primary_color":4,"secondary_color":5,"tertiary_color":6}},
                    {"id":"103", "name":"solid", "position":9, "color":7,
                     "colors":{"primary_color":7,"secondary_color":null,"tertiary_color":null}}
                ]
            }))
            .unwrap()
        };
        let with = value(serde_json::json!(["ENHANCED_ROLE_COLORS"]));
        let colors = with.role_gradient(&[101]).unwrap();
        assert_eq!(
            (colors.primary, colors.secondary, colors.tertiary),
            (1, Some(2), None)
        );
        let holo = with.role_gradient(&[102]).unwrap();
        assert_eq!(holo.tertiary, Some(6));
        // The colour role is the highest coloured one; a solid role above hides a gradient one below.
        assert!(with.role_gradient(&[101, 103]).is_none());
        assert!(with.role_colors_of(103).is_none());
        assert!(value(serde_json::json!([])).role_gradient(&[101]).is_none());
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
    fn a_video_tweet_keeps_its_video_author_icon_footer_icon_and_time() {
        // The shape FxTwitter's unfurl takes for a video post (module 659674 `fK`).
        let embed = Embed::from_value_in(
            &json!({
                "type": "rich", "url": "https://fxtwitter.com/burakalpekn/status/1/video/1",
                "description": "text\n\n💬 17 🔁 2 ❤️ 273 👁 57.0K",
                "color": 6_465_461,
                "author": {"name": "Heisenberg (@burakalpekn)", "url": "https://x.com/burakalpekn",
                           "icon_url": "https://pbs.twimg.com/a.jpg", "proxy_icon_url": "https://images-ext-1.discordapp.net/a.jpg"},
                "footer": {"text": "FxTwitter", "icon_url": "https://x/f.png", "proxy_icon_url": "https://images-ext-1.discordapp.net/f.png"},
                "timestamp": "2026-11-11T08:28:00+00:00",
                "thumbnail": {"url": "https://pbs.twimg.com/t.jpg", "proxy_url": "https://images-ext-1.discordapp.net/t.jpg", "width": 720, "height": 1280},
                "video": {"url": "https://video.twimg.com/v.mp4", "proxy_url": "https://images-ext-1.discordapp.net/v.mp4", "width": 720, "height": 1280}
            }),
            Some(1_438_000_000_000_000_000),
        );
        assert!(embed.video.is_some() && embed.thumbnail.is_some());
        assert_eq!(
            embed.author_icon.as_deref(),
            Some("https://images-ext-1.discordapp.net/a.jpg")
        );
        assert_eq!(
            embed.footer_icon.as_deref(),
            Some("https://images-ext-1.discordapp.net/f.png")
        );
        assert!(embed.timestamp.is_some());
        assert!(!embed.is_inline_media(), "a rich embed is never inline");
        // A portrait video fits 169x300: under 300 wide, so the card keeps no fixed width.
        assert_eq!(embed.fit(embed.video.as_ref().unwrap()), (169., 300.));
        assert_eq!(embed.max_width(), None);
    }

    #[test]
    fn videos_without_a_thumbnail_borrow_their_proxy_and_old_ones_need_a_known_provider() {
        let raw = json!({
            "type": "video", "url": "https://example.com/v",
            "video": {"url": "https://example.com/v.mp4", "proxy_url": "https://proxy/v.mp4?x=1", "width": 1280, "height": 720}
        });
        let embed = Embed::from_value_in(&raw, None);
        assert_eq!(
            embed.thumbnail.as_ref().unwrap().url,
            "https://proxy/v.mp4?x=1&format=webp"
        );
        assert!(embed.video.is_some());
        assert!(embed.is_inline_media(), "a video with no author or title");
        assert_eq!(embed.max_width(), Some(432.));
        // Before 2017-04-23 only the listed providers keep a video.
        let old = (1_450_000_000_000u64 - 1_420_070_400_000) << 22;
        assert!(Embed::from_value_in(&raw, Some(old)).video.is_none());
        let mut youtube = raw.clone();
        youtube["provider"] = json!({"name": "YouTube"});
        assert!(Embed::from_value_in(&youtube, Some(old)).video.is_some());
        // Sketchfab never plays inline.
        let mut sketch = raw;
        sketch["provider"] = json!({"name": "Sketchfab"});
        assert!(Embed::from_value_in(&sketch, None).video.is_none());
        // TikTok and a portrait YouTube video fit into 400x450.
        let tall = Embed::from_value(&json!({
            "type": "video", "provider": {"name": "YouTube"},
            "thumbnail": {"url": "https://i.ytimg.com/t.jpg", "width": 720, "height": 1280},
            "video": {"url": "https://www.youtube.com/embed/x", "width": 720, "height": 1280}
        }));
        assert_eq!(tall.max_media(), Embed::TALL_MEDIA);
        assert_eq!(tall.fit(tall.video.as_ref().unwrap()), (253., 450.));
    }

    #[test]
    fn embeds_with_one_url_merge_into_a_gallery() {
        let picture = |name: &str| {
            json!({"type": "rich", "url": "https://x.com/p/1",
                   "image": {"url": format!("https://x/{name}.png"), "width": 600, "height": 400}})
        };
        let list = Embed::list(
            &json!([picture("a"), picture("b"), picture("c"), {"type": "link", "url": "https://other"}]),
            None,
        );
        assert_eq!(list.len(), 2);
        let urls: Vec<&str> = list[0]
            .images
            .iter()
            .map(|image| image.url.as_str())
            .collect();
        assert_eq!(
            urls,
            ["https://x/a.png", "https://x/b.png", "https://x/c.png"]
        );
        assert_eq!(
            list[0].max_width(),
            Some(432.),
            "a 400-wide image wins first"
        );
        let mut small = list[0].clone();
        small.image.as_mut().unwrap().width = 200;
        small.image.as_mut().unwrap().height = 100;
        assert_eq!(small.max_width(), Some(520.));
    }

    #[test]
    fn only_a_lone_link_to_one_inline_image_or_gifv_hides_the_text() {
        let mut stores = Stores::default();
        // A 2026 id: before 2017-04-23 a video needs a listed provider.
        let mut payload = message("1438000000000000000", "https://tenor.com/view/x");
        payload["embeds"] = json!([{"type": "gifv", "url": "https://tenor.com/view/x",
            "thumbnail": {"url": "https://t/p.png", "width": 498, "height": 280},
            "video": {"url": "https://t/v.mp4", "width": 498, "height": 280}}]);
        stores.dispatch("MESSAGE_CREATE", &payload);
        let known = &stores.messages[&23][0];
        assert!(known.content_is_inline_media());
        let mut with_text = known.clone();
        with_text.content = "look https://tenor.com/view/x".into();
        assert!(!with_text.content_is_inline_media());
        // A video embed keeps its link text.
        let mut video = known.clone();
        video.embeds[0].kind = "video".into();
        assert!(!video.content_is_inline_media());
        let mut two = known.clone();
        two.embeds.push(two.embeds[0].clone());
        assert!(!two.content_is_inline_media());
        assert_eq!(
            fit_rounded(720., 1280., 400., 300., 150., 144.),
            (169., 300.)
        );
        assert_eq!(fit_rounded(100., 50., 400., 300., 0., 0.), (100., 50.));
        assert_eq!(fit_rounded(100., 50., 400., 300., 150., 144.), (150., 144.));
        assert_eq!(
            fit_rounded(1000., 100., 400., 300., 150., 144.),
            (400., 144.)
        );
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
        // Another guild's server going away leaves this one; its own going away clears it.
        stores.dispatch(
            "VOICE_SERVER_UPDATE",
            &json!({"guild_id": "2", "endpoint": null, "token": "t"}),
        );
        assert!(stores.voice_server.is_some());
        stores.dispatch(
            "VOICE_SERVER_UPDATE",
            &json!({"guild_id": "1", "endpoint": null, "token": "t"}),
        );
        assert!(stores.voice_server.is_none());
    }

    #[test]
    fn a_call_that_was_asked_for_brings_who_is_in_it() {
        let mut stores = Stores::default();
        stores.dispatch(
            "CALL_CREATE",
            &json!({"channel_id": "500", "message_id": "1", "region": "x", "ongoing_rings": {},
                "voice_states": [
                    {"channel_id": "500", "user_id": "5", "session_id": "s", "self_stream": true},
                    {"channel_id": "500", "user_id": "6", "session_id": "t"}]}),
        );
        let members = stores.voice_members(0, 500);
        assert_eq!(members.len(), 2);
        assert!(
            members
                .iter()
                .any(|state| state.user_id == 5 && state.self_stream)
        );
    }

    #[test]
    fn a_call_that_ended_has_nobody_in_it() {
        let mut stores = Stores::default();
        stores.dispatch(
            "CALL_CREATE",
            &json!({"channel_id": "500", "message_id": "1", "region": "x", "ongoing_rings": {},
                "voice_states": [{"channel_id": "500", "user_id": "5", "session_id": "s"}]}),
        );
        stores.dispatch(
            "VOICE_STATE_UPDATE",
            &json!({"channel_id": "600", "user_id": "7", "session_id": "u"}),
        );
        stores.dispatch("CALL_DELETE", &json!({"channel_id": "500"}));
        assert!(stores.voice_members(0, 500).is_empty());
        assert_eq!(stores.voice_members(0, 600).len(), 1);
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
            banner: None,
            description: None,
            preferred_locale: None,
            verification_level: None,
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
            viewing_roles: None,
            mfa_level: 0,
            vanity_url_code: None,
            invites_disabled_until: None,
            dms_disabled_until: None,
            lockdown_duration_hours: None,
            raid_detected_at: None,
            dm_spam_detected_at: None,
            alerts_channel: None,
            features: features
                .iter()
                .map(|feature| (*feature).to_owned())
                .collect(),
            premium_tier: 0,
            premium_subscription_count: 0,
            max_stage_video_channel_users: 0,
            max_video_channel_users: 0,
            premium_progress_bar_enabled: false,
            system_channel_flags: 0,
            emojis: Vec::new(),
            threads: Vec::new(),
            tag: None,
            tag_badge: None,
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
    fn geo_restricted_servers_are_kept_apart_from_unavailable_ones() {
        let mut stores = Stores::default();
        stores.dispatch(
            "READY",
            &json!({"user": {"id": "5", "username": "me"}, "guilds": [
                {"id": "1", "unavailable": true},
                {"id": "2", "unavailable": true, "geo_restricted": true, "name": "Far", "icon": "abc"}]}),
        );
        assert!(stores.unavailable_guilds.contains(&1));
        assert!(!stores.unavailable_guilds.contains(&2));
        assert_eq!(
            stores.geo_restricted_guilds,
            vec![GeoRestrictedGuild {
                id: 2,
                name: "Far".to_owned(),
                icon: Some("abc".to_owned()),
            }]
        );
        stores.dispatch(
            "GUILD_DELETE",
            &json!({"id": "3", "unavailable": true, "geo_restricted": true, "name": "New"}),
        );
        assert_eq!(stores.geo_restricted_guilds.len(), 2);
        stores.dispatch("GUILD_DELETE", &json!({"id": "2"}));
        assert_eq!(stores.geo_restricted_guilds.len(), 1);
        assert_eq!(stores.geo_restricted_guilds[0].id, 3);
    }

    #[test]
    fn thread_events_keep_the_active_list() {
        let mut stores = Stores::default();
        stores.dispatch(
            "READY",
            &json!({"user": {"id": "3", "username": "me"}, "users": [], "guilds": []}),
        );
        stores.dispatch(
            "GUILD_CREATE",
            &json!({"id": "1", "name": "g", "channels": [], "threads": []}),
        );
        // As recorded in TRAFFIC/thread-create.json: the create names no member, a THREAD_MEMBER_UPDATE follows.
        stores.dispatch(
            "THREAD_CREATE",
            &json!({"guild_id": "1", "id": "9", "parent_id": "5", "name": "t", "type": 11, "newly_created": true,
                "owner_id": "3", "thread_metadata": {"archived": false}}),
        );
        let guild = stores.guilds.iter().find(|guild| guild.id == 1).unwrap();
        assert_eq!(guild.threads.len(), 1);
        assert!(!guild.threads[0].joined);
        stores.dispatch(
            "THREAD_MEMBER_UPDATE",
            &json!({"flags": 1, "guild_id": "1", "id": "9", "join_timestamp": "2026-11-08T02:57:03.036078+00:00",
                "mute_config": null, "muted": false, "user_id": "3"}),
        );
        assert!(stores.guilds[0].threads[0].joined);
        // An update of the thread keeps the membership; so does the archive and unarchive round trip.
        stores.dispatch(
            "THREAD_UPDATE",
            &json!({"guild_id": "1", "id": "9", "parent_id": "5", "name": "t2", "type": 11,
                "thread_metadata": {"archived": true}}),
        );
        assert!(stores.guilds[0].threads.is_empty());
        stores.dispatch(
            "THREAD_UPDATE",
            &json!({"guild_id": "1", "id": "9", "parent_id": "5", "name": "t2", "type": 11,
                "thread_metadata": {"archived": false}}),
        );
        assert!(stores.guilds[0].threads[0].joined);
        // A search response that names the membership of a thread seen later.
        stores
            .thread_members_from_response(Some(1), &json!({"members": [{"id": "10", "flags": 2}]}));
        stores.dispatch(
            "THREAD_UPDATE",
            &json!({"guild_id": "1", "id": "10", "parent_id": "5", "name": "u", "type": 11,
                "thread_metadata": {"archived": false}}),
        );
        let ten = stores.guilds[0]
            .threads
            .iter()
            .find(|thread| thread.id == 10)
            .unwrap();
        assert!(ten.joined && ten.member_flags == Some(2));
        stores.dispatch("THREAD_DELETE", &json!({"guild_id": "1", "id": "9"}));
        stores.dispatch("THREAD_DELETE", &json!({"guild_id": "1", "id": "10"}));
        assert!(stores.guilds[0].threads.is_empty());
        assert!(stores.thread_members.is_empty());
    }

    #[test]
    fn a_list_sync_without_members_keeps_the_memberships_ready_gave() {
        // READY lists the joined threads with their `member`; the THREAD_LIST_SYNC that follows (op 37 `threads`)
        // carries the same threads without one and must not unjoin them (`JoinedThreadsStore` is a separate map).
        let mut stores = Stores::default();
        let thread = |member: bool| {
            let mut value = json!({"id": "9", "guild_id": "1", "parent_id": "5", "name": "t", "type": 11,
                "thread_metadata": {"archived": false, "auto_archive_duration": 4320}});
            if member {
                value["member"] = json!({"flags": 1, "muted": false, "join_timestamp": "2026-11-08T02:57:03.036000+00:00"});
            }
            value
        };
        stores.dispatch(
            "READY",
            &json!({"user": {"id": "3", "username": "me"}, "users": [],
                "guilds": [{"id": "1", "name": "g", "channels": [], "threads": [thread(true)]}]}),
        );
        assert!(stores.guilds[0].threads[0].joined);
        stores.dispatch(
            "THREAD_LIST_SYNC",
            &json!({"guild_id": "1", "threads": [thread(false)], "most_recent_messages": []}),
        );
        let synced = &stores.guilds[0].threads[0];
        assert!(synced.joined && synced.member_flags == Some(1));
        assert_eq!(
            synced.join_timestamp.as_deref(),
            Some("2026-11-08T02:57:03.036000+00:00")
        );
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
    fn request_notes_and_origin_games_follow_ready_add_update_and_removal() {
        let mut stores = Stores::default();
        stores.dispatch(
            "READY",
            &json!({"users":[],"relationships":[
                {"id":"30","type":3,"note":"hi from the fair","origin_application_id":"700"},
                {"id":"31","type":3}
            ]}),
        );
        assert_eq!(
            stores.relationship_notes.get(&30).map(String::as_str),
            Some("hi from the fair")
        );
        assert_eq!(stores.relationship_origin_apps.get(&30), Some(&700));
        // ADD keeps what it does not carry; UPDATE replaces it.
        stores.dispatch("RELATIONSHIP_ADD", &json!({"id":"30","type":3}));
        assert!(stores.relationship_notes.contains_key(&30));
        stores.dispatch("RELATIONSHIP_UPDATE", &json!({"id":"30","type":3}));
        assert!(!stores.relationship_notes.contains_key(&30));
        assert!(!stores.relationship_origin_apps.contains_key(&30));
        stores.dispatch("RELATIONSHIP_ADD", &json!({"id":"31","type":3,"note":"x"}));
        stores.dispatch("RELATIONSHIP_REMOVE", &json!({"id":"31"}));
        assert!(stores.relationship_notes.is_empty());
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
    fn a_dm_with_an_official_account_knows_it_is_a_system_dm() {
        let mut stores = Stores::default();
        stores.dispatch(
            "CHANNEL_CREATE",
            &json!({"id": "41", "type": 1, "recipients": [{"id": "643945264868098049", "username": "discord", "system": true}]}),
        );
        stores.dispatch(
            "CHANNEL_CREATE",
            &json!({"id": "42", "type": 1, "recipients": [{"id": "31", "username": "someone"}]}),
        );
        let system = |channel: Id| {
            let known = stores
                .private_channels
                .iter()
                .find(|known| known.id == channel);
            stores.users[&known.unwrap().recipients[0]].system
        };
        assert!(system(41));
        assert!(!system(42));
        let channel = |id: Id| {
            stores
                .private_channels
                .iter()
                .find(|known| known.id == id)
                .unwrap()
        };
        assert!(stores.is_system_dm(channel(41)));
        assert!(!stores.is_system_dm(channel(42)));
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
    fn folders_come_from_the_settings_and_a_change_is_written_back() {
        use dc_proto::settings::{GuildFolderEntry, guild_folders, set_guild_folders};
        let mut stores = Stores::default();
        stores.dispatch("READY", &ready());
        stores.settings_loaded = true;
        let folder = GuildFolderEntry {
            id: Some(5),
            guild_ids: vec![1, 2],
            ..GuildFolderEntry::default()
        };
        stores.settings_raw = set_guild_folders(&[], &[folder]).1;
        stores.refresh_guild_order();
        assert_eq!(stores.folders.folder(5).unwrap().guilds, vec![1, 2]);
        assert_eq!(stores.guilds[0].id, 1);
        assert!(stores.folders.move_by_id(2, 5, false, false));
        let patch = stores.commit_guild_folders();
        assert!(!patch.is_empty());
        let (entries, _) = guild_folders(&settings::decode(&patch));
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].guild_ids, vec![2]);
        assert_eq!(entries[1].guild_ids, vec![1]);
        assert_eq!(stores.guild_order, vec![2, 1]);
        assert_eq!(stores.guilds[0].id, 2);
    }

    #[test]
    fn a_drag_keeps_the_servers_that_have_not_arrived() {
        use dc_proto::settings::{GuildFolderEntry, guild_folders, set_guild_folders};
        let mut stores = Stores::default();
        stores.dispatch("READY", &ready());
        let mut second = stores.guilds[0].clone();
        second.id = 2;
        stores.guilds.push(second);
        stores.settings_loaded = true;
        // 98 and 99 are in the account's saved order but unknown to this client (an outage, a slow start).
        let entries = [
            GuildFolderEntry {
                id: Some(5),
                guild_ids: vec![1, 99],
                ..GuildFolderEntry::default()
            },
            GuildFolderEntry {
                guild_ids: vec![2],
                ..GuildFolderEntry::default()
            },
            GuildFolderEntry {
                guild_ids: vec![98],
                ..GuildFolderEntry::default()
            },
        ];
        stores.settings_raw = set_guild_folders(&[], &entries).1;
        stores.refresh_guild_order();
        let patch = stores.commit_guild_folders();
        let (written, _) = guild_folders(&settings::decode(&patch));
        let all: Vec<u64> = written
            .iter()
            .flat_map(|entry| entry.guild_ids.iter().copied())
            .collect();
        assert!(all.contains(&99) && all.contains(&98), "{all:?}");
        assert!(all.contains(&1) && all.contains(&2), "{all:?}");
        assert_eq!(all.len(), 4, "{all:?}");
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
    fn message_update_replaces_or_clears_attachments() {
        let mut stores = Stores::default();
        let mut created = message("5", "with file");
        created["attachments"] = json!([
            {"id": "1", "filename": "a.png", "proxy_url": "p", "content_type": "image/png", "width": 4, "height": 3, "size": 9},
            {"id": "2", "filename": "b.txt", "url": "u", "size": 5},
        ]);
        stores.dispatch("MESSAGE_CREATE", &created);
        assert_eq!(stores.messages[&23][0].attachments.len(), 2);

        // Partial update without attachments keeps the existing ones.
        stores.dispatch(
            "MESSAGE_UPDATE",
            &json!({"id": "5", "channel_id": "23", "content": "edited"}),
        );
        assert_eq!(stores.messages[&23][0].attachments.len(), 2);

        // Update with a trimmed attachments array removes the dropped attachment.
        stores.dispatch(
            "MESSAGE_UPDATE",
            &json!({"id": "5", "channel_id": "23", "attachments": [
                {"id": "2", "filename": "b.txt", "url": "u", "size": 5}
            ]}),
        );
        assert_eq!(stores.messages[&23][0].attachments.len(), 1);
        assert_eq!(stores.messages[&23][0].attachments[0].id, 2);

        // Empty attachments array clears them.
        stores.dispatch(
            "MESSAGE_UPDATE",
            &json!({"id": "5", "channel_id": "23", "attachments": []}),
        );
        assert!(stores.messages[&23][0].attachments.is_empty());
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
    fn a_reply_to_a_deleted_message_reads_deleted() {
        let mut stores = Stores::default();
        stores.dispatch("MESSAGE_CREATE", &message("5", "b"));
        let mut reply = message("6", "re");
        reply["type"] = json!(19);
        reply["message_reference"] = json!({"message_id": "5", "channel_id": "23"});
        reply["referenced_message"] = message("5", "b");
        stores.dispatch("MESSAGE_CREATE", &reply);
        assert!(stores.messages[&23][1].referenced.is_some());
        assert!(!stores.messages[&23][1].extra.reference_deleted);
        stores.dispatch("MESSAGE_DELETE", &json!({"id": "5", "channel_id": "23"}));
        let reply = &stores.messages[&23][0];
        assert_eq!(reply.id, 6);
        assert!(reply.referenced.is_none());
        assert!(reply.extra.reference_deleted);
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
    fn a_webhook_author_without_an_avatar_shows_the_first_default() {
        let webhook = User {
            id: 4 << 22,
            discriminator: "0000".into(),
            bot: true,
            ..User::default()
        };
        assert_eq!(webhook.default_avatar_index(), 0);
        let human = User {
            bot: false,
            ..webhook.clone()
        };
        assert_eq!(human.default_avatar_index(), 4);
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
    fn ready_names_which_of_the_sessions_this_client_is() {
        let mut stores = Stores::default();
        assert_eq!(stores.auth_session_id_hash, None);
        stores.dispatch(
            "READY",
            &json!({"sessions": [], "user_settings_proto": "", "auth_session_id_hash": "abc"}),
        );
        assert_eq!(stores.auth_session_id_hash.as_deref(), Some("abc"));
    }

    #[test]
    fn a_timed_status_that_lapsed_is_online_again() {
        let mut stores = Stores::default();
        let (saved, _) = settings::set_status(&[], "dnd", 5_000, 1);
        stores.dispatch(
            "READY",
            &json!({"sessions": [], "user_settings_proto": saved}),
        );
        assert_eq!(stores.status_at(4_999), "dnd");
        assert_eq!(stores.status_at(5_000), "online");
        assert_eq!(stores.status_at(9_999_999), "online");
        // A status without an end never lapses.
        let (forever, _) = settings::set_status(&[], "dnd", 0, 1);
        stores.dispatch(
            "READY",
            &json!({"sessions": [], "user_settings_proto": forever}),
        );
        assert_eq!(stores.status_at(9_999_999), "dnd");
    }

    #[test]
    fn a_status_choice_shows_at_once_and_follows_other_sessions() {
        let mut stores = Stores::default();
        // No saved status: online, whatever another session of the account reports.
        stores.dispatch(
            "READY",
            &json!({"sessions": [{"status": "idle"}], "user_settings_proto": ""}),
        );
        assert_eq!(stores.status, "online");
        // The saved status is the account's status.
        let (saved, _) = settings::set_status(&[], "dnd", 0, 1);
        stores.dispatch(
            "READY",
            &json!({"sessions": [{"status": "online"}], "user_settings_proto": saved}),
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

    #[test]
    fn video_attachment_classification_and_poster_url() {
        for ext in ["mp4", "MP4", "webm", "mov", "qt"] {
            let att = Attachment::from_value(&json!({
                "id": "10",
                "filename": format!("clip.{ext}"),
                "url": format!("https://cdn.discordapp.com/attachments/1/2/clip.{ext}"),
                "proxy_url": format!("https://media.discordapp.net/attachments/1/2/clip.{ext}?ex=abc"),
                "width": 1280,
                "height": 720,
                "flags": 0
            }))
            .unwrap();
            assert!(att.is_video(), "{ext} should classify as video");
            assert!(!att.is_clip());
            assert_eq!(
                att.video_poster_url().as_deref(),
                Some(
                    format!(
                        "https://media.discordapp.net/attachments/1/2/clip.{ext}?ex=abc&format=webp"
                    )
                    .as_str()
                )
            );
        }

        // IS_CLIP (1 << 2 = 4)
        let clip = Attachment::from_value(&json!({
            "id": "11",
            "filename": "moment.mp4",
            "url": "https://cdn.discordapp.com/attachments/1/2/moment.mp4",
            "proxy_url": "https://media.discordapp.net/attachments/1/2/moment.mp4",
            "width": 640,
            "height": 360,
            "flags": 4
        }))
        .unwrap();
        assert!(clip.is_video());
        assert!(clip.is_clip());

        // IS_THUMBNAIL (1 << 1 = 2) and missing dimensions are excluded.
        let thumb = Attachment::from_value(&json!({
            "id": "12",
            "filename": "thumb.mp4",
            "url": "https://cdn.discordapp.com/attachments/1/2/thumb.mp4",
            "proxy_url": "https://media.discordapp.net/attachments/1/2/thumb.mp4",
            "width": 640,
            "height": 360,
            "flags": 2
        }))
        .unwrap();
        assert!(!thumb.is_video());
        assert!(!thumb.is_clip());
        assert!(thumb.video_poster_url().is_none());

        let no_dims = Attachment::from_value(&json!({
            "id": "13",
            "filename": "unsized.mp4",
            "url": "https://cdn.discordapp.com/attachments/1/2/unsized.mp4",
            "proxy_url": "https://media.discordapp.net/attachments/1/2/unsized.mp4",
            "width": 0,
            "height": 720,
            "flags": 0
        }))
        .unwrap();
        assert!(!no_dims.is_video());

        let other = Attachment::from_value(&json!({
            "id": "14",
            "filename": "archive.mkv",
            "url": "https://cdn.discordapp.com/attachments/1/2/archive.mkv",
            "proxy_url": "https://media.discordapp.net/attachments/1/2/archive.mkv",
            "width": 1280,
            "height": 720,
            "flags": 0
        }))
        .unwrap();
        assert!(!other.is_video());
    }

    #[test]
    fn guild_server_tag_parsing_and_selectable_filter() {
        let mut stores = Stores::default();
        stores.dispatch(
            "READY",
            &json!({
                "user": {"id": "5", "username": "me"},
                "guilds": [{
                    "id": "100",
                    "joined_at": "2026-10-01T00:00:00Z",
                    "properties": {
                        "name": "Tag Guild",
                        "features": ["GUILD_TAGS"],
                        "profile": {"tag": "PTPC", "badge": "hash123"}
                    },
                    "channels": []
                }],
                "merged_members": [[{"user_id": "5", "joined_at": "2026-10-01T00:00:00Z", "pending": false}]]
            }),
        );
        assert_eq!(stores.guilds[0].tag.as_deref(), Some("PTPC"));
        assert_eq!(stores.guilds[0].tag_badge.as_deref(), Some("hash123"));
        assert_eq!(
            stores.guilds[0].selectable_server_tag(),
            Some(("PTPC", Some("hash123")))
        );

        // Pending member cannot select server tag.
        stores.guilds[0].my_pending = true;
        assert_eq!(stores.guilds[0].selectable_server_tag(), None);
        stores.guilds[0].my_pending = false;

        // GUILD_UPDATE replaces or clears profile.tag.
        stores.dispatch(
            "GUILD_UPDATE",
            &json!({
                "id": "100",
                "name": "Tag Guild",
                "features": ["GUILD_TAGS"],
                "profile": {"tag": "NEWT", "badge": null}
            }),
        );
        assert_eq!(
            stores.guilds[0].selectable_server_tag(),
            Some(("NEWT", None))
        );

        stores.dispatch(
            "GUILD_UPDATE",
            &json!({
                "id": "100",
                "name": "Tag Guild",
                "features": ["GUILD_TAGS"],
                "profile": null
            }),
        );
        assert_eq!(stores.guilds[0].selectable_server_tag(), None);
    }

    #[test]
    fn ready_hydrates_embedded_relationship_and_channel_users_without_stub_clobber() {
        let mut stores = Stores::default();
        stores.dispatch(
            "READY",
            &json!({
                "user": {"id": "1", "username": "me"},
                "users": [
                    {
                        "id": "10",
                        "username": "yeya",
                        "global_name": "Yeya",
                        "avatar": "full_hash_10",
                        "public_flags": 64
                    }
                ],
                "relationships": [
                    {"id": "10", "type": 3, "user": {"id": "10"}},
                    {
                        "id": "11",
                        "type": 3,
                        "user": {
                            "id": "11",
                            "username": "incoming_embed",
                            "global_name": "Incoming",
                            "avatar": "hash_11"
                        }
                    },
                    {
                        "id": "12",
                        "type": 4,
                        "user": {
                            "id": "12",
                            "username": "outgoing_embed",
                            "avatar": "hash_12"
                        }
                    }
                ],
                "private_channels": [
                    {
                        "id": "500",
                        "type": 3,
                        "recipients": [
                            {"id": "10"},
                            {"id": "13", "username": "group_member", "avatar": "hash_13"}
                        ]
                    }
                ]
            }),
        );
        let yeya = stores.users.get(&10).expect("yeya preserved");
        assert_eq!(yeya.username, "yeya");
        assert_eq!(yeya.global_name.as_deref(), Some("Yeya"));
        assert_eq!(yeya.avatar.as_deref(), Some("full_hash_10"));
        assert_eq!(yeya.public_flags, 64);

        let incoming = stores.users.get(&11).expect("embedded incoming hydrated");
        assert_eq!(incoming.username, "incoming_embed");
        assert_eq!(incoming.avatar.as_deref(), Some("hash_11"));

        let outgoing = stores.users.get(&12).expect("embedded outgoing hydrated");
        assert_eq!(outgoing.username, "outgoing_embed");
        assert_eq!(outgoing.avatar.as_deref(), Some("hash_12"));

        let group_member = stores.users.get(&13).expect("embedded recipient hydrated");
        assert_eq!(group_member.username, "group_member");
        assert_eq!(group_member.avatar.as_deref(), Some("hash_13"));

        stores.dispatch(
            "READY_SUPPLEMENTAL",
            &json!({
                "lazy_private_channels": [
                    {
                        "id": "501",
                        "type": 1,
                        "recipients": [
                            {"id": "10"},
                            {"id": "14", "username": "lazy_dm", "avatar": "hash_14"}
                        ]
                    }
                ]
            }),
        );
        assert_eq!(stores.users[&10].avatar.as_deref(), Some("full_hash_10"));
        assert_eq!(stores.users[&14].avatar.as_deref(), Some("hash_14"));

        let preload_ids: Vec<Id> = stores
            .relationship_and_channel_users()
            .into_iter()
            .map(|u| u.id)
            .collect();
        assert_eq!(preload_ids, vec![10, 11, 12, 13, 14]);
        assert!(stores.private_channels.iter().any(|dm| dm.id == 501));

        stores.dispatch(
            "CALL_CREATE",
            &json!({
                "channel_id": "501",
                "message_id": "901",
                "region": "us-east",
                "ongoing_rings": {"1": "14"},
                "voice_states": []
            }),
        );
        assert_eq!(stores.calls.get_incoming_calls(false).len(), 1);
        assert_eq!(stores.calls.get_incoming_calls(false)[0].channel, 501);
    }

    #[test]
    fn presence_update_merges_partial_user_fields_without_clobbering_username() {
        let mut stores = Stores::default();
        stores.dispatch(
            "READY",
            &json!({
                "user": {"id": "1", "username": "me"},
                "users": [
                    {
                        "id": "20",
                        "username": "yeya",
                        "global_name": "Yeya",
                        "avatar": "old_avatar"
                    }
                ],
                "relationships": [{"id": "20", "type": 3}]
            }),
        );

        // Status-only presence carries `{ "id": "20" }`: existing user fields remain untouched.
        stores.dispatch(
            "PRESENCE_UPDATE",
            &json!({
                "user": {"id": "20"},
                "status": "online"
            }),
        );
        assert_eq!(stores.users[&20].username, "yeya");
        assert_eq!(stores.users[&20].global_name.as_deref(), Some("Yeya"));
        assert_eq!(stores.users[&20].avatar.as_deref(), Some("old_avatar"));

        // Partial presence user update updates avatar and server tag without clobbering username.
        stores.dispatch(
            "PRESENCE_UPDATE",
            &json!({
                "user": {
                    "id": "20",
                    "avatar": "new_avatar",
                    "clan": {
                        "identity_guild_id": "999",
                        "identity_enabled": true,
                        "tag": "CUTE",
                        "badge": "clan_badge_hash"
                    }
                },
                "status": "idle"
            }),
        );
        let updated = &stores.users[&20];
        assert_eq!(updated.username, "yeya");
        assert_eq!(updated.global_name.as_deref(), Some("Yeya"));
        assert_eq!(updated.avatar.as_deref(), Some("new_avatar"));
        assert_eq!(updated.tag.as_ref().map(|t| t.tag.as_str()), Some("CUTE"));
        assert_eq!(stores.status(20), "idle");
    }

    #[test]
    fn embed_audio_requires_non_empty_proxy_url_and_scan_versions_parse() {
        let with_audio = Embed::from_value(&json!({
            "type": "audio",
            "title": "Song",
            "flags": 16,
            "content_scan_version": 4,
            "audio": {
                "url": "https://cdn.example/song.mp3",
                "proxy_url": "https://media.example/song.mp3",
                "content_type": "audio/mpeg",
                "description": "Track preview"
            }
        }));
        assert_eq!(with_audio.flags, 16);
        assert_eq!(with_audio.content_scan_version, Some(4));
        assert_eq!(
            with_audio.audio,
            Some(EmbedAudio {
                url: Some("https://cdn.example/song.mp3".into()),
                proxy_url: "https://media.example/song.mp3".into(),
                content_type: Some("audio/mpeg".into()),
                description: Some("Track preview".into()),
            })
        );

        let empty_proxy = Embed::from_value(&json!({
            "type": "audio",
            "content_scan_version": -1,
            "audio": {
                "url": "https://cdn.example/song.mp3",
                "proxy_url": ""
            }
        }));
        assert!(empty_proxy.audio.is_none());
        assert_eq!(empty_proxy.content_scan_version, Some(-1));

        let missing_proxy = Embed::from_value(&json!({
            "type": "audio",
            "audio": {
                "url": "https://cdn.example/song.mp3"
            }
        }));
        assert!(missing_proxy.audio.is_none());

        let attachment = Attachment::from_value(&json!({
            "id": "99",
            "filename": "clip.png",
            "url": "https://cdn.example/clip.png",
            "proxy_url": "https://media.example/clip.png",
            "size": 1200,
            "content_scan_version": 2
        }))
        .expect("attachment");
        assert_eq!(attachment.content_scan_version(), Some(2));
    }

    #[test]
    fn load_saved_merges_results_and_bookmark_ids() {
        let mut stores = Stores::default();
        stores.load_saved(&json!({
            "results": [{
                "save_data": {
                    "channel_id": "10",
                    "message_id": "20",
                    "due_at": "2026-12-01T00:00:00Z"
                }
            }],
            "bookmark_ids": [
                {"channel_id": "10", "message_id": "20"},
                {"channel_id": "10", "message_id": "21"},
                ["10", "22"],
                "10-23"
            ]
        }));
        assert_eq!(
            stores.saved.get(&(10, 20)),
            Some(&Some("2026-12-01T00:00:00Z".to_owned()))
        );
        assert_eq!(stores.saved.get(&(10, 21)), Some(&None));
        assert_eq!(stores.saved.get(&(10, 22)), Some(&None));
        assert_eq!(stores.saved.get(&(10, 23)), Some(&None));
    }

    #[test]
    fn optimistic_own_reaction_is_idempotent_with_gateway_echo() {
        let mut stores = Stores {
            current_user: Some(User {
                id: 10,
                ..User::default()
            }),
            ..Stores::default()
        };
        stores.dispatch("MESSAGE_CREATE", &message("5", "x"));

        // Optimistic add followed by Gateway echo keeps count at 1.
        stores.apply_own_reaction(23, 5, "\u{1f44d}", None, true);
        stores.dispatch("MESSAGE_REACTION_ADD", &reaction_event("10"));
        let reaction = &stores.messages[&23][0].reactions[0];
        assert_eq!((reaction.count, reaction.me), (1, true));

        // Another user reacts; optimistic remove drops count to 1 and me=false; Gateway echo is idempotent.
        stores.dispatch("MESSAGE_REACTION_ADD", &reaction_event("11"));
        assert_eq!(stores.messages[&23][0].reactions[0].count, 2);
        stores.apply_own_reaction(23, 5, "\u{1f44d}", None, false);
        stores.dispatch("MESSAGE_REACTION_REMOVE", &reaction_event("10"));
        let reaction = &stores.messages[&23][0].reactions[0];
        assert_eq!((reaction.count, reaction.me), (1, false));
    }

    #[test]
    fn message_update_delete_loaded_and_passive_updates_keep_mentions_and_latest_accurate() {
        let mut stores = Stores::default();
        stores.dispatch(
            "READY",
            &json!({
                "user": {"id": "1", "username": "me"},
                "guilds": [{
                    "id": "10",
                    "properties": {"name": "g", "default_message_notifications": 0},
                    "roles": [{"id": "10", "permissions": "1024"}, {"id": "50", "name": "mods"}],
                    "channels": [{"id": "20", "name": "chat", "type": 0}]
                }],
                "merged_members": [[{"user_id": "1", "roles": ["50"]}]]
            }),
        );
        assert_eq!(stores.guilds[0].my_roles, vec![50]);
        assert_eq!(stores.my_members_by_guild[&10]["roles"], json!(["50"]));

        // Mention added on MESSAGE_UPDATE updates stored mention fields.
        stores.dispatch(
            "MESSAGE_CREATE",
            &json!({
                "id": "200",
                "channel_id": "20",
                "timestamp": "2026-11-09T12:00:00Z",
                "author": {"id": "2", "username": "sender"},
                "content": "hi"
            }),
        );
        stores.dispatch(
            "MESSAGE_UPDATE",
            &json!({
                "id": "200",
                "channel_id": "20",
                "content": "<@&50>",
                "mention_roles": ["50"],
                "mention_everyone": false
            }),
        );
        assert!(stores.messages[&20][0].mentions_me(1, &[50], false, false));

        // Mentioned message created & deleted decrements mention_count and rewinds latest.
        stores.dispatch(
            "MESSAGE_CREATE",
            &json!({
                "id": "201",
                "channel_id": "20",
                "timestamp": "2026-11-09T12:01:00Z",
                "author": {"id": "2", "username": "sender"},
                "content": "@everyone",
                "mention_everyone": true
            }),
        );
        assert_eq!(stores.read_states.mentions(20), 1);
        assert_eq!(stores.read_states.latest.get(&20), Some(&201));
        stores.dispatch("MESSAGE_DELETE", &json!({"id": "201", "channel_id": "20"}));
        assert_eq!(stores.read_states.mentions(20), 0);
        assert_eq!(stores.read_states.latest.get(&20), Some(&200));

        // PASSIVE_UPDATE_V2 and messages_loaded update `latest` monotonically.
        stores.dispatch(
            "PASSIVE_UPDATE_V2",
            &json!({"guild_id": "10", "updated_channels": [{"id": "20", "last_message_id": "250"}]}),
        );
        assert_eq!(stores.read_states.latest.get(&20), Some(&250));
        assert_eq!(stores.guilds[0].channels[0].last_message_id, Some(250));

        let spec = crate::message_window::FetchSpec {
            channel: 20,
            limit: 50,
            ..Default::default()
        };
        stores.messages_loaded(
            &spec,
            &json!([{
                "id": "260",
                "channel_id": "20",
                "timestamp": "2026-11-09T12:02:00Z",
                "author": {"id": "2", "username": "sender"},
                "content": "loaded"
            }]),
        );
        assert_eq!(stores.read_states.latest.get(&20), Some(&260));
    }

    #[test]
    fn user_and_private_channel_cache_roundtrip_and_ready_keeps_cached_dms_until_supplemental() {
        let mut stores = Stores::default();
        stores.dispatch(
            "READY",
            &json!({
                "user": {"id": "1", "username": "me"},
                "users": [
                    {
                        "id": "42",
                        "username": "alice",
                        "global_name": "Alice",
                        "avatar": "av42",
                        "primary_guild": {
                            "identity_enabled": true,
                            "identity_guild_id": "77",
                            "badge": "b1",
                            "tag": "TAG"
                        },
                        "display_name_styles": {
                            "font_id": 2,
                            "effect_id": 1,
                            "colors": [0xFF0000]
                        }
                    }
                ],
                "private_channels": [
                    {
                        "id": "900",
                        "type": 1,
                        "recipient_ids": ["42"],
                        "last_message_id": "1000"
                    },
                    {
                        "id": "901",
                        "type": 3,
                        "recipient_ids": ["42"],
                        "last_message_id": "999",
                        "name": "Squad",
                        "icon": "grp1",
                        "owner_id": "1"
                    }
                ]
            }),
        );
        let serialized_user = stores.users[&42].to_cache_value();
        let reparsed_user = User::from_value(&serialized_user).expect("user roundtrips");
        assert_eq!(reparsed_user, stores.users[&42]);

        let serialized_group = stores.private_channels[1].to_cache_value();
        let reparsed_group =
            PrivateChannel::from_value(&serialized_group).expect("channel roundtrips");
        assert_eq!(reparsed_group, stores.private_channels[1]);

        // Live READY with empty private_channels on the same account preserves cached DMs.
        stores.dispatch(
            "READY",
            &json!({
                "user": {"id": "1", "username": "me"},
                "private_channels": []
            }),
        );
        assert_eq!(stores.private_channels.len(), 2);

        // READY_SUPPLEMENTAL replaces cached stale DMs with the authoritative server list.
        stores.dispatch(
            "READY_SUPPLEMENTAL",
            &json!({
                "lazy_private_channels": [
                    {
                        "id": "900",
                        "type": 1,
                        "last_message_id": "1100",
                        "recipients": [{"id": "42", "username": "alice"}]
                    }
                ]
            }),
        );
        assert_eq!(stores.private_channels.len(), 1);
        assert_eq!(stores.private_channels[0].id, 900);
        assert_eq!(stores.private_channels[0].last_message_id, 1100);
    }
}
