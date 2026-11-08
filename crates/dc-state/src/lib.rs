//! Flux stores. M0 slice: the parts of UserStore, GuildStore, ChannelStore and MessageStore the first screen reads.
//! Names and getters follow `STORES`; the rest of each store is tracked as open rows in `FEATURES.md`.

pub mod frecency;
pub mod grouping;
pub mod guild_settings;
pub mod member_list;
pub mod read_state;

use chrono::{DateTime, Local, Utc};
use dc_proto::settings;
use serde_json::Value;
use std::collections::HashMap;
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
}

impl User {
    pub fn from_value(value: &Value) -> Option<Self> {
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
    /// The newest message when the channel arrived; `read_states.latest` follows it after that.
    pub last_message_id: Option<Id>,
}

impl Channel {
    fn from_value(value: &Value) -> Option<Self> {
        Some(Self {
            id: id(&value["id"])?,
            name: text(&value["name"]).unwrap_or_default(),
            kind: ChannelKind::from_u64(value["type"].as_u64()?),
            position: value["position"].as_i64().unwrap_or(0),
            parent_id: id(&value["parent_id"]),
            last_message_id: id(&value["last_message_id"]),
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
    pub roles: HashMap<Id, String>,
    pub features: Vec<String>,
    pub premium_tier: u64,
    pub premium_subscription_count: u64,
    pub emojis: Vec<GuildEmoji>,
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
            roles: value["roles"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|role| Some((id(&role["id"])?, text(&role["name"])?)))
                .collect(),
            features: properties["features"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(text)
                .collect(),
            premium_tier: properties["premium_tier"].as_u64().unwrap_or(0),
            premium_subscription_count: value["premium_subscription_count"]
                .as_u64()
                .or(properties["premium_subscription_count"].as_u64())
                .unwrap_or(0),
            emojis: GuildEmoji::list(&value["emojis"]),
        })
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
        // Articles and images show their thumbnail as the large image.
        if let Some(thumbnail) = EmbedMedia::from_value(&value["thumbnail"]) {
            match kind.as_str() {
                "article" | "image" => embed.image = Some(thumbnail),
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

/// `T` in module 741961: how long a typing notice lasts.
pub const TYPING_DURATION: Duration = Duration::from_secs(10);

/// The private half of the READY user, which only the account settings show.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Account {
    pub email: Option<String>,
    pub phone: Option<String>,
    pub mfa_enabled: bool,
    pub verified: bool,
}

impl Account {
    fn from_value(value: &Value) -> Self {
        Self {
            email: text(&value["email"]),
            phone: text(&value["phone"]),
            mfa_enabled: value["mfa_enabled"].as_bool().unwrap_or(false),
            verified: value["verified"].as_bool().unwrap_or(false),
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
    pub guilds: Vec<Guild>,
    pub messages: HashMap<Id, Vec<Message>>,
    pub read_states: read_state::ReadStates,
    /// Bookmarks and reminders (`SavedMessagesStore`): (channel, message) to the reminder's `due_at`, `None` for a
    /// plain bookmark.
    pub saved: HashMap<(Id, Id), Option<String>>,
    pub guild_settings: guild_settings::GuildSettingsStore,
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
                if let Some(guild) = Guild::from_value(data) {
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
                    let mentions_me = data["mention_everyone"].as_bool().unwrap_or(false)
                        || data["mentions"]
                            .as_array()
                            .into_iter()
                            .flatten()
                            .any(|user| id(&user["id"]).is_some_and(|user| Some(user) == me));
                    self.read_states.message_created(
                        message.channel_id,
                        message.id,
                        mentions_me,
                        Some(message.author.id) == me,
                    );
                    // A sent message ends its author's typing notice.
                    if let Some(list) = self.typing.get_mut(&message.channel_id) {
                        list.retain(|(user, _)| *user != message.author.id);
                    }
                    self.insert_message(message);
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

    fn on_ready(&mut self, data: &Value) {
        self.current_user = User::from_value(&data["user"]);
        self.account = Account::from_value(&data["user"]);
        self.theme = data["user_settings_proto"]
            .as_str()
            .and_then(settings::theme)
            .unwrap_or(0);
        self.guild_order = data["user_settings_proto"]
            .as_str()
            .and_then(settings::guild_order)
            .unwrap_or_default();
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
        let joined = guild.joined_snowflake();
        guild
            .channels
            .iter()
            .filter(|channel| channel.kind == ChannelKind::Text)
            .any(|channel| self.read_states.is_unread(channel.id, joined))
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
            channels: Vec::new(),
            roles: HashMap::new(),
            features: features
                .iter()
                .map(|feature| (*feature).to_owned())
                .collect(),
            premium_tier: 0,
            premium_subscription_count: 0,
            emojis: Vec::new(),
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
}
