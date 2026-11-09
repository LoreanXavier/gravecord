//! Right-click menus of the server rail, channel rows, category rows, thread rows, DM rows and people. The entries,
//! their order, separators and heights come from the captures in `design/snapshot/dumps/menus/` (`guild-icon-context`,
//! `channel-text-context`, `channel-voice-context`, `category-context`, `thread-context`, `dm-row-context`,
//! `member-other-context`, `member-self-context`, `message-user-*-context`); which entries show for the account is
//! decided by `*_items`, from plain facts, so the rules are unit tested.

use super::{Root, color, icon};
use crate::net::Request;
use dc_state::permissions::{self, CREATE_INSTANT_INVITE, MANAGE_CHANNELS, MANAGE_THREADS};
use dc_state::{Channel, ChannelKind, Id};
use gpui::{
    AnyElement, ClipboardItem, Context, FontWeight, IntoElement, Pixels, Point, SharedString,
    Window, anchored, deferred, div, prelude::*, px,
};
use serde_json::json;

/// What was right-clicked.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Target {
    Guild(Id),
    Channel(Id),
    Category(Id),
    Thread(Id),
    Dm(Id),
    User { user: Id, guild: Option<Id> },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Act {
    MarkRead,
    Invite,
    Mute,
    Notifications,
    HideMuted,
    ServerSettings,
    PrivacySettings,
    EditProfile,
    CreateChannel,
    CreateCategory,
    CreateEvent,
    MoveTo,
    CopyLink,
    CopyId,
    EditChannel,
    Duplicate,
    DeleteChannel,
    CollapseCategory,
    CollapseAll,
    PinChannel,
    Favorite,
    OpenChat,
    HideNames,
    CloseThread,
    LockThread,
    LeaveThread,
    Profile,
    Mention,
    Message,
    Call,
    Note,
    Apps,
    AddFriend,
    Ignore,
    Block,
    CloseDm,
    PinDm,
    InviteToServer,
    ModView,
    Verify,
    Timeout,
    Kick,
    Ban,
    LeaveGuild,
    /// Voice: how loud a person is to us, and muting them (or their soundboard) here.
    Volume,
    VoiceMute,
    SoundboardMute,
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct Row {
    pub act: Act,
    pub label: String,
    /// A second line under the label (Notification Settings shows the level).
    pub sub: Option<String>,
    pub icon: Option<&'static str>,
    pub caret: bool,
    pub check: Option<bool>,
    pub danger: bool,
    pub disabled: bool,
}

impl Row {
    fn new(act: Act, label: impl Into<String>) -> Self {
        Self {
            act,
            label: label.into(),
            sub: None,
            icon: None,
            caret: false,
            check: None,
            danger: false,
            disabled: false,
        }
    }

    fn caret(mut self) -> Self {
        self.caret = true;
        self
    }

    fn danger(mut self) -> Self {
        self.danger = true;
        self
    }

    fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    fn sub(mut self, sub: impl Into<String>) -> Self {
        self.sub = Some(sub.into());
        self
    }

    fn check(mut self, on: bool) -> Self {
        self.check = Some(on);
        self
    }

    fn icon(mut self, name: &'static str) -> Self {
        self.icon = Some(name);
        self
    }

    /// Row heights measured in the captures: 34, 36 with an icon, caret or checkbox, 52 with a second line.
    pub fn height(&self) -> f32 {
        if self.sub.is_some() {
            52.
        } else if self.icon.is_some() || self.caret || self.check.is_some() {
            36.
        } else {
            34.
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(super) enum Entry {
    Row(Row),
    Separator,
}

/// Drops separators that lead, trail or repeat, as the official lists do when a group is empty.
fn tidy(entries: Vec<Entry>) -> Vec<Entry> {
    let mut out: Vec<Entry> = Vec::new();
    for entry in entries {
        if entry == Entry::Separator && matches!(out.last(), None | Some(Entry::Separator)) {
            continue;
        }
        out.push(entry);
    }
    if out.last() == Some(&Entry::Separator) {
        out.pop();
    }
    out
}

fn group(entries: &mut Vec<Entry>, rows: Vec<Row>) {
    if rows.is_empty() {
        return;
    }
    entries.push(Entry::Separator);
    entries.extend(rows.into_iter().map(Entry::Row));
}

/// What a channel menu depends on.
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct ChannelFacts {
    pub voice: bool,
    pub unread: bool,
    pub can_manage: bool,
    pub can_invite: bool,
    pub developer: bool,
    pub hide_names: bool,
}

pub(super) fn channel_items(facts: ChannelFacts, level: &str) -> Vec<Entry> {
    let mut out = vec![Entry::Row(
        Row::new(Act::MarkRead, "Mark As Read").disabled(!facts.unread),
    )];
    group(
        &mut out,
        vec![Row::new(Act::Favorite, "Add to Favorites").icon("sb_star")],
    );
    let mut share = Vec::new();
    if facts.can_invite {
        share.push(Row::new(
            Act::Invite,
            if facts.voice {
                "Invite to Voice"
            } else {
                "Invite to Channel"
            },
        ));
    }
    share.push(Row::new(Act::PinChannel, "Pin Channel to Top"));
    share.push(Row::new(Act::CopyLink, "Copy Link"));
    group(&mut out, share);
    if facts.voice {
        group(
            &mut out,
            vec![
                Row::new(Act::OpenChat, "Open Chat"),
                Row::new(Act::HideNames, "Hide Names").check(facts.hide_names),
            ],
        );
    }
    group(
        &mut out,
        vec![
            Row::new(Act::Mute, "Mute Channel").caret(),
            Row::new(Act::Notifications, "Notification Settings")
                .sub(level)
                .caret(),
        ],
    );
    if facts.can_manage {
        group(&mut out, vec![Row::new(Act::MoveTo, "Move to...").caret()]);
        group(
            &mut out,
            vec![
                Row::new(Act::EditChannel, "Edit Channel"),
                Row::new(Act::Duplicate, "Duplicate Channel"),
                Row::new(
                    Act::CreateChannel,
                    if facts.voice {
                        "Create Voice Channel"
                    } else {
                        "Create Text Channel"
                    },
                ),
                Row::new(Act::DeleteChannel, "Delete Channel").danger(),
            ],
        );
    }
    if facts.developer {
        group(&mut out, vec![Row::new(Act::CopyId, "Copy Channel ID")]);
    }
    tidy(out)
}

pub(super) fn category_items(
    unread: bool,
    collapsed: bool,
    can_manage: bool,
    developer: bool,
    level: &str,
) -> Vec<Entry> {
    let mut out = vec![Entry::Row(
        Row::new(Act::MarkRead, "Mark As Read").disabled(!unread),
    )];
    group(
        &mut out,
        vec![
            Row::new(Act::CollapseCategory, "Collapse Category").check(collapsed),
            Row::new(Act::CollapseAll, "Collapse All Categories"),
        ],
    );
    group(
        &mut out,
        vec![
            Row::new(Act::Mute, "Mute Category").caret(),
            Row::new(Act::Notifications, "Notification Settings")
                .sub(level)
                .caret(),
        ],
    );
    if can_manage {
        group(&mut out, vec![Row::new(Act::MoveTo, "Move to...").caret()]);
        group(
            &mut out,
            vec![
                Row::new(Act::EditChannel, "Edit Category"),
                Row::new(Act::DeleteChannel, "Delete Category").danger(),
            ],
        );
    }
    if developer {
        group(&mut out, vec![Row::new(Act::CopyId, "Copy Category ID")]);
    }
    tidy(out)
}

pub(super) fn thread_items(
    unread: bool,
    manage: bool,
    owner: bool,
    joined: bool,
    developer: bool,
    level: &str,
) -> Vec<Entry> {
    let mut out = vec![Entry::Row(
        Row::new(Act::MarkRead, "Mark As Read").disabled(!unread),
    )];
    group(
        &mut out,
        vec![Row::new(Act::Favorite, "Add to Favorites").icon("sb_star")],
    );
    let mut own = Vec::new();
    if manage || owner {
        own.push(Row::new(Act::CloseThread, "Close Thread"));
    }
    if manage {
        own.push(Row::new(Act::LockThread, "Lock Thread"));
    }
    if manage || owner {
        own.push(Row::new(Act::EditChannel, "Edit Thread"));
    }
    own.push(Row::new(Act::CopyLink, "Copy Link"));
    group(&mut out, own);
    group(
        &mut out,
        vec![
            Row::new(Act::Mute, "Mute Thread").caret(),
            Row::new(Act::Notifications, "Notification Settings")
                .sub(level)
                .caret(),
        ],
    );
    if manage || owner {
        group(
            &mut out,
            vec![Row::new(Act::DeleteChannel, "Delete Thread").danger()],
        );
    }
    if joined {
        group(&mut out, vec![Row::new(Act::LeaveThread, "Leave Thread")]);
    }
    if developer {
        group(&mut out, vec![Row::new(Act::CopyId, "Copy Thread ID")]);
    }
    tidy(out)
}

/// What the server menu depends on.
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct GuildFacts {
    pub unread: bool,
    pub can_invite: bool,
    pub can_manage_channels: bool,
    pub can_manage_guild: bool,
    pub hide_muted: bool,
    pub owner: bool,
    pub developer: bool,
}

pub(super) fn guild_items(facts: GuildFacts, level: &str) -> Vec<Entry> {
    let mut out = vec![Entry::Row(
        Row::new(Act::MarkRead, "Mark As Read").disabled(!facts.unread),
    )];
    if facts.can_invite {
        group(&mut out, vec![Row::new(Act::Invite, "Invite to Server")]);
    }
    group(
        &mut out,
        vec![
            Row::new(Act::Mute, "Mute Server").caret(),
            Row::new(Act::Notifications, "Notification Settings")
                .sub(level)
                .caret(),
            Row::new(Act::HideMuted, "Hide Muted Channels").check(facts.hide_muted),
        ],
    );
    let mut settings = Vec::new();
    if facts.can_manage_guild {
        settings.push(Row::new(Act::ServerSettings, "Server Settings").caret());
    }
    settings.push(Row::new(Act::PrivacySettings, "Privacy Settings"));
    settings.push(Row::new(Act::EditProfile, "Edit Per-server Profile"));
    group(&mut out, settings);
    if facts.can_manage_channels {
        group(
            &mut out,
            vec![
                Row::new(Act::CreateChannel, "Create Channel"),
                Row::new(Act::CreateCategory, "Create Category"),
                Row::new(Act::CreateEvent, "Create Event"),
            ],
        );
    }
    group(&mut out, vec![Row::new(Act::MoveTo, "Move to...").caret()]);
    if !facts.owner {
        group(
            &mut out,
            vec![Row::new(Act::LeaveGuild, "Leave Server").danger()],
        );
    }
    if facts.developer {
        group(&mut out, vec![Row::new(Act::CopyId, "Copy Server ID")]);
    }
    tidy(out)
}

/// What a person's menu depends on.
#[derive(Clone, Debug, Default)]
pub(super) struct UserFacts {
    pub name: String,
    pub me: bool,
    pub in_guild: bool,
    pub friend: bool,
    pub blocked: bool,
    pub ignored: bool,
    pub can_nick: bool,
    pub can_moderate: bool,
    pub can_kick: bool,
    pub can_ban: bool,
    pub developer: bool,
    /// `Some(unread)` in a DM row, which adds the channel entries.
    pub dm: Option<bool>,
    pub invite_servers: bool,
    /// In the call we are in (never true unless a call is open): adds the voice rows.
    pub in_call: bool,
    pub voice_muted: bool,
    pub soundboard_muted: bool,
    /// The volume as a percentage, 0 to 200.
    pub volume: u32,
}

pub(super) fn user_items(facts: &UserFacts) -> Vec<Entry> {
    let mut out: Vec<Entry> = Vec::new();
    if let Some(unread) = facts.dm {
        out.push(Entry::Row(
            Row::new(Act::MarkRead, "Mark As Read").disabled(!unread),
        ));
        group(
            &mut out,
            vec![
                Row::new(Act::PinDm, "Pin").icon("menu_pin_message"),
                Row::new(Act::Favorite, "Add to Favorites").icon("sb_star"),
            ],
        );
    }
    let mut first = vec![Row::new(Act::Profile, "Profile")];
    if facts.dm.is_none() {
        first.push(Row::new(Act::Mention, "Mention"));
        if !facts.me {
            first.push(Row::new(Act::Message, "Message"));
        }
    }
    if !facts.me {
        first.push(Row::new(Act::Call, "Start a Call"));
        first.push(Row::new(Act::Note, "Add Note").sub("Only visible to you"));
    }
    if facts.dm.is_some() {
        first.push(Row::new(Act::CloseDm, "Close DM"));
    }
    if facts.dm.is_some() {
        group(&mut out, first);
    } else if out.is_empty() {
        out.extend(first.into_iter().map(Entry::Row));
    }
    if facts.in_call && !facts.me {
        let mut rows = vec![
            Row::new(Act::Volume, "User Volume").sub(format!("{}%", facts.volume)),
            Row::new(Act::VoiceMute, "Mute").check(facts.voice_muted),
            Row::new(
                Act::SoundboardMute,
                if facts.soundboard_muted {
                    "Unmute Soundboard"
                } else {
                    "Mute Soundboard"
                },
            ),
        ];
        rows[0].caret = false;
        out.push(Entry::Separator);
        out.extend(rows.into_iter().map(Entry::Row));
    }
    let mut mid = Vec::new();
    if facts.dm.is_none() && facts.in_guild && (facts.me || facts.can_nick) {
        mid.push(Row::new(
            Act::EditProfile,
            if facts.me {
                "Edit Per-server Profile"
            } else {
                "Change Nickname"
            },
        ));
    }
    mid.push(Row::new(Act::Apps, "Apps").caret());
    if facts.dm.is_some() && facts.invite_servers {
        mid.push(Row::new(Act::InviteToServer, "Invite to Server").caret());
    }
    if !facts.me {
        if !facts.friend {
            mid.push(Row::new(Act::AddFriend, "Add Friend"));
        }
        mid.push(Row::new(
            Act::Ignore,
            if facts.ignored { "Unignore" } else { "Ignore" },
        ));
        mid.push(Row::new(Act::Block, if facts.blocked { "Unblock" } else { "Block" }).danger());
    }
    if facts.dm.is_some() {
        group(&mut out, mid);
    } else if facts.me {
        // The own menu has no separator before the nickname row only when nothing precedes it.
        out.push(Entry::Separator);
        out.extend(mid.into_iter().map(Entry::Row));
    } else {
        out.push(Entry::Separator);
        out.extend(mid.into_iter().map(Entry::Row));
    }
    if facts.dm.is_none() && facts.in_guild {
        let mut moderation = vec![Row::new(Act::ModView, "Open in Mod View")];
        if !facts.me {
            if facts.can_moderate {
                moderation.push(Row::new(Act::Verify, "Verify Member"));
                moderation.push(Row::new(Act::Timeout, format!("Timeout {}", facts.name)).danger());
            }
            if facts.can_kick {
                moderation.push(Row::new(Act::Kick, format!("Kick {}", facts.name)).danger());
            }
            if facts.can_ban {
                moderation.push(Row::new(Act::Ban, format!("Ban {}", facts.name)).danger());
            }
        }
        group(&mut out, moderation);
    }
    if let Some(_unread) = facts.dm {
        group(
            &mut out,
            vec![Row::new(Act::Mute, format!("Mute @{}", facts.name)).caret()],
        );
    }
    if facts.developer {
        group(&mut out, vec![Row::new(Act::CopyId, "Copy User ID")]);
    }
    tidy(out)
}

pub(super) struct Open {
    pub target: Target,
    pub at: Point<Pixels>,
    pub items: Vec<Entry>,
    /// The open submenu: the row it hangs off, and where that row is.
    pub sub: Option<(Act, gpui::Bounds<Pixels>)>,
}

#[derive(Default)]
pub(super) struct State {
    pub open: Option<Open>,
    /// People whose soundboard sounds are muted here, for the whole session.
    pub soundboard_muted: std::collections::HashSet<Id>,
    /// Each person's volume (a factor) and local mute in the call.
    pub mix: std::collections::HashMap<Id, (f32, bool)>,
}

const MUTE_FOR: [(i64, &str); 6] = [
    (900, "For 15 Minutes"),
    (3600, "For 1 Hour"),
    (10800, "For 3 Hours"),
    (28800, "For 8 Hours"),
    (86400, "For 24 Hours"),
    (0, "Until I turn it back on"),
];

/// `message_notifications` values, with the labels of the notification submenu.
const LEVELS: [(u64, &str); 3] = [(0, "All Messages"), (1, "Only @mentions"), (2, "Nothing")];
const INHERIT: u64 = 3;

fn level_label(level: u64) -> &'static str {
    match level {
        0 => "All Messages",
        1 => "Only @mentions",
        2 => "Nothing",
        _ => "Use Category Default",
    }
}

impl Root {
    fn developer_mode(&self) -> bool {
        self.pref_on("developer_mode", false)
    }

    fn my_permission(&self, guild: Id, channel: Option<&Channel>, bit: u64) -> bool {
        let Some(me) = self.stores.me() else {
            return false;
        };
        let Some(guild) = self.stores.guild(guild) else {
            return false;
        };
        match channel {
            Some(channel) => permissions::can_in_channel(guild, channel, me, bit),
            None => permissions::can_in_guild(guild, me, bit),
        }
    }

    /// The guild a channel belongs to.
    fn guild_of_channel(&self, channel: Id) -> Option<Id> {
        self.stores.guilds.iter().find_map(|guild| {
            (guild.channels.iter().any(|known| known.id == channel)
                || guild.threads.iter().any(|known| known.id == channel))
            .then_some(guild.id)
        })
    }

    /// The level a channel's row shows: its own, else what it inherits from the server.
    fn channel_level_label(&self, guild: Id, channel: Id) -> &'static str {
        match self.channel_level(guild, channel) {
            INHERIT => self.guild_level_label(guild),
            own => level_label(own),
        }
    }

    fn channel_level(&self, guild: Id, channel: Id) -> u64 {
        self.stores
            .guild_settings
            .guilds
            .get(&guild)
            .and_then(|settings| settings.channels.get(&channel))
            .map_or(INHERIT, |entry| entry.message_notifications)
    }

    fn guild_level_label(&self, guild: Id) -> &'static str {
        level_label(
            self.stores
                .guild_settings
                .guilds
                .get(&guild)
                .map_or(1, |settings| settings.message_notifications),
        )
    }

    pub(super) fn open_context_menu(
        &mut self,
        target: Target,
        at: Point<Pixels>,
        cx: &mut Context<Self>,
    ) {
        let developer = self.developer_mode();
        let items = match target {
            Target::Guild(guild) => {
                let Some(known) = self.stores.guild(guild) else {
                    return;
                };
                let facts = GuildFacts {
                    unread: self.stores.guild_unread(known),
                    can_invite: self.my_permission(guild, None, CREATE_INSTANT_INVITE)
                        || known.vanity_url_code.is_some(),
                    can_manage_channels: self.my_permission(guild, None, MANAGE_CHANNELS),
                    can_manage_guild: self.my_permission(guild, None, permissions::MANAGE_GUILD),
                    hide_muted: self.hide_muted(guild),
                    owner: known.owner_id == self.stores.current_user.as_ref().map(|me| me.id),
                    developer,
                };
                guild_items(facts, self.guild_level_label(guild))
            }
            Target::Channel(id) | Target::Category(id) => {
                let Some(guild) = self.guild_of_channel(id) else {
                    return;
                };
                let Some(known) = self
                    .stores
                    .guild(guild)
                    .and_then(|guild| guild.channels.iter().find(|channel| channel.id == id))
                    .cloned()
                else {
                    return;
                };
                let can_manage = self.my_permission(guild, Some(&known), MANAGE_CHANNELS);
                let level = self.channel_level_label(guild, id);
                if known.kind == ChannelKind::Category {
                    let collapsed = self.stores.guild_settings.channel(guild, id).collapsed;
                    let unread = self.stores.guild(guild).is_some_and(|guild| {
                        guild
                            .channels
                            .iter()
                            .filter(|child| child.parent_id == Some(id))
                            .any(|child| self.stores.is_unread(child.id))
                    });
                    category_items(unread, collapsed, can_manage, developer, level)
                } else {
                    let facts = ChannelFacts {
                        voice: matches!(known.kind, ChannelKind::Voice | ChannelKind::Other(13)),
                        unread: self.stores.is_unread(id),
                        can_manage,
                        can_invite: self.my_permission(guild, Some(&known), CREATE_INSTANT_INVITE),
                        developer,
                        hide_names: false,
                    };
                    channel_items(facts, level)
                }
            }
            Target::Thread(id) => {
                let Some(guild) = self.guild_of_channel(id) else {
                    return;
                };
                let Some(thread) = self
                    .stores
                    .guild(guild)
                    .and_then(|guild| guild.threads.iter().find(|thread| thread.id == id))
                    .cloned()
                else {
                    return;
                };
                let parent = self
                    .stores
                    .guild(guild)
                    .and_then(|guild| {
                        guild
                            .channels
                            .iter()
                            .find(|channel| channel.id == thread.parent_id)
                    })
                    .cloned();
                let manage = parent
                    .as_ref()
                    .is_some_and(|parent| self.my_permission(guild, Some(parent), MANAGE_THREADS));
                let owner = thread.owner_id.is_some()
                    && thread.owner_id == self.stores.current_user.as_ref().map(|me| me.id);
                thread_items(
                    self.stores.is_unread(id),
                    manage,
                    owner,
                    thread.joined || owner,
                    developer,
                    self.channel_level_label(guild, id),
                )
            }
            Target::Dm(channel) => {
                let Some(dm) = self
                    .stores
                    .private_channels
                    .iter()
                    .find(|known| known.id == channel)
                else {
                    return;
                };
                let Some(user) = self.stores.dm_recipient(dm).cloned() else {
                    return;
                };
                let facts = UserFacts {
                    name: user.display_name().to_owned(),
                    me: false,
                    in_guild: false,
                    friend: self.stores.relationships.get(&user.id) == Some(&1),
                    blocked: self.stores.relationships.get(&user.id) == Some(&2),
                    ignored: self.stores.is_ignored(user.id),
                    developer,
                    dm: Some(self.stores.is_unread(channel)),
                    invite_servers: true,
                    ..UserFacts::default()
                };
                user_items(&facts)
            }
            Target::User { user, guild } => {
                let Some(known) = self
                    .stores
                    .users
                    .get(&user)
                    .cloned()
                    .or_else(|| self.stores.current_user.clone().filter(|me| me.id == user))
                else {
                    return;
                };
                let me = self
                    .stores
                    .current_user
                    .as_ref()
                    .is_some_and(|me| me.id == user);
                let facts = UserFacts {
                    name: known.username.clone(),
                    me,
                    in_guild: guild.is_some(),
                    friend: self.stores.relationships.get(&user) == Some(&1),
                    blocked: self.stores.relationships.get(&user) == Some(&2),
                    ignored: self.stores.is_ignored(user),
                    can_nick: guild.is_some_and(|guild| {
                        self.my_permission(guild, None, permissions::MANAGE_NICKNAMES)
                    }),
                    can_moderate: guild.is_some_and(|guild| {
                        self.my_permission(guild, None, permissions::MODERATE_MEMBERS)
                    }),
                    can_kick: guild.is_some_and(|guild| {
                        self.my_permission(guild, None, permissions::KICK_MEMBERS)
                    }),
                    can_ban: guild.is_some_and(|guild| {
                        self.my_permission(guild, None, permissions::BAN_MEMBERS)
                    }),
                    developer,
                    dm: None,
                    invite_servers: false,
                    in_call: self
                        .others_in_call()
                        .is_some_and(|others| others.contains(&user)),
                    voice_muted: self.context.mix.get(&user).is_some_and(|mix| mix.1),
                    soundboard_muted: self.context.soundboard_muted.contains(&user),
                    volume: self
                        .context
                        .mix
                        .get(&user)
                        .map_or(100, |mix| (mix.0 * 100.).round() as u32),
                };
                user_items(&facts)
            }
        };
        self.menu = None;
        self.context.open = Some(Open {
            target,
            at,
            items,
            sub: None,
        });
        cx.notify();
    }

    fn close_context_menu(&mut self, cx: &mut Context<Self>) {
        if self.context.open.take().is_some() {
            cx.notify();
        }
    }

    fn mark_channel_read(&mut self, channel: Id) {
        if !self.stores.is_unread(channel) {
            return;
        }
        let Some(latest) = self.stores.read_states.latest.get(&channel).copied() else {
            return;
        };
        self.stores.read_states.ack_local(channel, latest, false);
        let _ = self.requests.send(Request::Ack {
            channel,
            message: latest,
            manual: false,
            mention_count: 0,
            last_viewed: dc_state::read_state::days_since_epoch(super::picker::now_ms()),
        });
    }

    /// Runs a row against its target.
    fn run_context(
        &mut self,
        act: Act,
        target: Target,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match (act, target) {
            (Act::MarkRead, Target::Guild(guild)) => {
                let channels: Vec<Id> = self
                    .stores
                    .guild(guild)
                    .map(|guild| guild.channels.iter().map(|channel| channel.id).collect())
                    .unwrap_or_default();
                for channel in channels {
                    self.mark_channel_read(channel);
                }
            }
            (Act::MarkRead, Target::Category(category)) => {
                let children: Vec<Id> = self
                    .guild_of_channel(category)
                    .and_then(|guild| self.stores.guild(guild))
                    .map(|guild| {
                        guild
                            .channels
                            .iter()
                            .filter(|channel| channel.parent_id == Some(category))
                            .map(|channel| channel.id)
                            .collect()
                    })
                    .unwrap_or_default();
                for channel in children {
                    self.mark_channel_read(channel);
                }
            }
            (
                Act::MarkRead,
                Target::Channel(channel) | Target::Thread(channel) | Target::Dm(channel),
            ) => {
                self.mark_channel_read(channel);
            }
            (Act::Invite, Target::Guild(guild)) => {
                self.select(guild, None);
                self.open_invite(cx);
            }
            (Act::Invite, Target::Channel(channel)) => {
                if let Some(guild) = self.guild_of_channel(channel) {
                    self.select(guild, Some(channel));
                    self.open_invite(cx);
                }
            }
            (Act::HideMuted, Target::Guild(guild)) => {
                let hide = !self.hide_muted(guild);
                self.patch_guild_setting(guild, "hide_muted_channels", json!(hide));
            }
            (Act::ServerSettings, Target::Guild(guild)) => self.open_server_settings(guild, cx),
            (Act::PrivacySettings, Target::Guild(_)) => {
                self.open_settings_section("Data & Privacy", cx)
            }
            (Act::EditProfile, _) => self.open_settings_section("Account", cx),
            (Act::CreateChannel | Act::CreateCategory, Target::Guild(guild)) => {
                self.select(guild, None);
                self.open_create_channel(act == Act::CreateCategory, window, cx);
            }
            (Act::CreateChannel, Target::Channel(channel)) => {
                if let Some(guild) = self.guild_of_channel(channel) {
                    self.select(guild, None);
                    self.open_create_channel(false, window, cx);
                }
            }
            (Act::CopyLink, Target::Channel(channel) | Target::Thread(channel)) => {
                let guild = self.guild_of_channel(channel).unwrap_or_default();
                cx.write_to_clipboard(ClipboardItem::new_string(format!(
                    "https://discord.com/channels/{guild}/{channel}"
                )));
            }
            (Act::CopyId, target) => {
                let id = match target {
                    Target::Guild(id)
                    | Target::Channel(id)
                    | Target::Category(id)
                    | Target::Thread(id)
                    | Target::Dm(id) => id,
                    Target::User { user, .. } => user,
                };
                cx.write_to_clipboard(ClipboardItem::new_string(id.to_string()));
            }
            (
                Act::EditChannel,
                Target::Channel(channel) | Target::Category(channel) | Target::Thread(channel),
            ) => {
                self.open_channel_settings(channel, window, cx);
            }
            (Act::DeleteChannel, Target::Channel(channel) | Target::Category(channel)) => {
                self.open_channel_settings(channel, window, cx);
                if let Some(open) = self.channel_settings.as_mut() {
                    open.confirm_delete = true;
                }
            }
            (Act::DeleteChannel, Target::Thread(thread)) => {
                let _ = self.requests.send(Request::Rest {
                    key: None,
                    method: reqwest::Method::DELETE,
                    route: format!("/channels/{thread}"),
                    body: None,
                });
            }
            (Act::Duplicate, Target::Channel(channel)) => {
                if let Some(guild) = self.guild_of_channel(channel)
                    && let Some(known) = self
                        .stores
                        .guild(guild)
                        .and_then(|guild| guild.channels.iter().find(|known| known.id == channel))
                {
                    let kind = match known.kind {
                        ChannelKind::Text => 0,
                        ChannelKind::Voice => 2,
                        ChannelKind::Category => 4,
                        ChannelKind::Other(other) => other,
                    };
                    let body = json!({
                        "name": known.name,
                        "type": kind,
                        "topic": known.topic,
                        "parent_id": known.parent_id.map(|parent| parent.to_string()),
                        "permission_overwrites": known.overwrites.iter().map(|overwrite| json!({
                            "id": overwrite.id.to_string(),
                            "type": overwrite.kind,
                            "allow": overwrite.allow.to_string(),
                            "deny": overwrite.deny.to_string(),
                        })).collect::<Vec<_>>(),
                    });
                    let _ = self.requests.send(Request::Rest {
                        key: None,
                        method: reqwest::Method::POST,
                        route: format!("/guilds/{guild}/channels"),
                        body: Some(body),
                    });
                }
            }
            (Act::CollapseCategory, Target::Category(category)) => {
                if let Some(guild) = self.guild_of_channel(category) {
                    self.toggle_category(guild, category);
                }
            }
            (Act::CollapseAll, Target::Category(category)) => {
                if let Some(guild) = self.guild_of_channel(category) {
                    let categories: Vec<Id> = self
                        .stores
                        .guild(guild)
                        .map(|guild| {
                            guild
                                .channels
                                .iter()
                                .filter(|channel| channel.kind == ChannelKind::Category)
                                .map(|channel| channel.id)
                                .collect()
                        })
                        .unwrap_or_default();
                    for known in categories {
                        if !self.stores.guild_settings.channel(guild, known).collapsed {
                            self.toggle_category(guild, known);
                        }
                    }
                }
            }
            (Act::OpenChat, Target::Channel(channel)) => {
                if let Some(guild) = self.guild_of_channel(channel) {
                    self.select(guild, Some(channel));
                }
            }
            (Act::CloseThread | Act::LockThread, Target::Thread(thread)) => {
                let field = if act == Act::CloseThread {
                    "archived"
                } else {
                    "locked"
                };
                let _ = self.requests.send(Request::Rest {
                    key: None,
                    method: reqwest::Method::PATCH,
                    route: format!("/channels/{thread}"),
                    body: Some(json!({ field: true })),
                });
            }
            (Act::LeaveThread, Target::Thread(thread)) => {
                let _ = self.requests.send(Request::Rest {
                    key: None,
                    method: reqwest::Method::DELETE,
                    route: format!("/channels/{thread}/thread-members/@me"),
                    body: None,
                });
            }
            (Act::LeaveGuild, Target::Guild(guild)) => self.leave_guild = Some(guild),
            (Act::Profile, Target::User { user, .. }) => self.open_full_profile(user, window, cx),
            (Act::Profile | Act::Message, Target::Dm(channel)) => {
                let user = self
                    .stores
                    .private_channels
                    .iter()
                    .find(|known| known.id == channel)
                    .and_then(|dm| self.stores.dm_recipient(dm))
                    .map(|user| user.id);
                if let Some(user) = user {
                    if act == Act::Profile {
                        self.open_full_profile(user, window, cx);
                    } else {
                        self.open_dm_with(user, cx);
                    }
                }
            }
            (Act::Mention, Target::User { user, .. }) => {
                let text = format!(
                    "{}{} ",
                    self.composer.read(cx).text(),
                    format_args!("<@{user}>")
                );
                self.composer
                    .update(cx, |composer, _| composer.set_text(&text));
                window.focus(
                    &gpui::Focusable::focus_handle(self.composer.read(cx), cx),
                    cx,
                );
            }
            (Act::Message, Target::User { user, .. }) => self.open_dm_with(user, cx),
            (Act::AddFriend, Target::User { user, .. }) => self.set_relationship(user, Some(4), cx),
            (Act::AddFriend, Target::Dm(channel)) => {
                if let Some(user) = self.dm_user(channel) {
                    self.set_relationship(user, Some(4), cx);
                }
            }
            (Act::Ignore, Target::User { user, .. }) => {
                if self.stores.is_ignored(user) {
                    self.unignore_user(user, cx);
                } else {
                    self.ignore_user(user, cx);
                }
            }
            (Act::Ignore, Target::Dm(channel)) => {
                if let Some(user) = self.dm_user(channel) {
                    if self.stores.is_ignored(user) {
                        self.unignore_user(user, cx);
                    } else {
                        self.ignore_user(user, cx);
                    }
                }
            }
            (Act::Block, Target::User { user, .. }) => self.toggle_block(user, cx),
            (Act::Block, Target::Dm(channel)) => {
                if let Some(user) = self.dm_user(channel) {
                    self.toggle_block(user, cx);
                }
            }
            (Act::CloseDm, Target::Dm(channel)) => self.close_dm(channel, cx),
            (Act::VoiceMute, Target::User { user, .. }) => {
                let (volume, muted) = self.context.mix.get(&user).copied().unwrap_or((1., false));
                self.set_user_mix(user, volume, !muted);
            }
            (Act::SoundboardMute, Target::User { user, .. }) => {
                if self.context.soundboard_muted.contains(&user) {
                    self.context.soundboard_muted.remove(&user);
                } else {
                    self.context.soundboard_muted.insert(user);
                }
            }
            // Calls are voice: never started from here. The rest have no page yet (see the report).
            _ => {}
        }
    }

    /// Applies a person's volume and mute to the call's mixer, and remembers them for the menu.
    pub(super) fn set_user_mix(&mut self, user: Id, volume: f32, muted: bool) {
        if volume == 1. && !muted {
            self.context.mix.remove(&user);
        } else {
            self.context.mix.insert(user, (volume, muted));
        }
        if let Some(call) = &self.call {
            call.controls.set_user_mix(user, volume, muted);
        }
    }

    fn dm_user(&self, channel: Id) -> Option<Id> {
        self.stores
            .private_channels
            .iter()
            .find(|known| known.id == channel)
            .and_then(|dm| self.stores.dm_recipient(dm))
            .map(|user| user.id)
    }

    fn toggle_block(&mut self, user: Id, cx: &mut Context<Self>) {
        if self.stores.relationships.get(&user) == Some(&2) {
            self.unblock_user(user, cx);
        } else {
            self.block_user(user, cx);
        }
    }

    /// The rows of a submenu: mute durations, or notification levels.
    fn context_sub_rows(&self, act: Act, target: Target) -> Vec<(String, bool)> {
        match act {
            Act::Mute => MUTE_FOR
                .iter()
                .map(|(_, label)| ((*label).to_owned(), false))
                .collect(),
            Act::Notifications => {
                let channel = matches!(target, Target::Channel(_) | Target::Thread(_));
                let current = match target {
                    Target::Channel(id) | Target::Thread(id) => self
                        .guild_of_channel(id)
                        .map_or(INHERIT, |guild| self.channel_level(guild, id)),
                    Target::Guild(guild) => self
                        .stores
                        .guild_settings
                        .guilds
                        .get(&guild)
                        .map_or(1, |settings| settings.message_notifications),
                    _ => INHERIT,
                };
                let mut rows = Vec::new();
                if channel {
                    rows.push(("Use Category Default".to_owned(), current == INHERIT));
                }
                rows.extend(
                    LEVELS
                        .iter()
                        .map(|(value, label)| ((*label).to_owned(), current == *value)),
                );
                rows
            }
            _ => Vec::new(),
        }
    }

    fn run_context_sub(&mut self, act: Act, target: Target, index: usize, cx: &mut Context<Self>) {
        match act {
            Act::Mute => {
                let seconds = MUTE_FOR[index].0;
                let end = (seconds > 0).then(|| {
                    (chrono::Utc::now() + chrono::Duration::seconds(seconds))
                        .format("%Y-%m-%dT%H:%M:%S%.3fZ")
                        .to_string()
                });
                let config = json!({"selected_time_window": if seconds > 0 { seconds } else { -1 }, "end_time": end});
                match target {
                    Target::Guild(guild) => {
                        self.patch_guild_setting(guild, "muted", json!(true));
                        self.patch_guild_setting(guild, "mute_config", config);
                    }
                    Target::Channel(id) | Target::Category(id) | Target::Thread(id) => {
                        if let Some(guild) = self.guild_of_channel(id) {
                            self.patch_channel_override(
                                guild,
                                id,
                                json!({"muted": true, "mute_config": config}),
                            );
                        }
                    }
                    _ => {}
                }
            }
            Act::Notifications => match target {
                Target::Guild(guild) => {
                    let value = LEVELS[index].0;
                    self.patch_guild_setting(guild, "message_notifications", json!(value));
                }
                Target::Channel(id) | Target::Category(id) | Target::Thread(id) => {
                    let channel = matches!(target, Target::Channel(_) | Target::Thread(_));
                    let value = if channel && index == 0 {
                        INHERIT
                    } else {
                        LEVELS[index - usize::from(channel)].0
                    };
                    if let Some(guild) = self.guild_of_channel(id) {
                        self.patch_channel_override(
                            guild,
                            id,
                            json!({"message_notifications": value}),
                        );
                    }
                }
                _ => {}
            },
            _ => {}
        }
        let _ = cx;
    }

    pub(super) fn context_menu_layer(
        &self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let open = self.context.open.as_ref()?;
        let target = open.target;
        let mut rows: Vec<AnyElement> = Vec::new();
        for (index, entry) in open.items.iter().enumerate() {
            match entry {
                Entry::Separator => rows.push(
                    div()
                        .my(px(8.))
                        .h(px(1.))
                        .flex_none()
                        .bg(color("--border-subtle"))
                        .into_any_element(),
                ),
                Entry::Row(row) => rows.push(self.context_row(index, row, target, cx)),
            }
        }
        let menu = deferred(
            anchored()
                .position(open.at)
                .snap_to_window_with_margin(px(8.))
                .child(
                    div()
                        .id("context-menu-generic")
                        .occlude()
                        .rounded(px(8.))
                        .bg(color("--background-surface-higher"))
                        .border_1()
                        .border_color(color("--border-subtle"))
                        .shadow(vec![gpui::BoxShadow {
                            color: gpui::hsla(0., 0., 0., 0.24),
                            offset: gpui::point(px(0.), px(12.)),
                            blur_radius: px(24.),
                            spread_radius: px(0.),
                            inset: false,
                        }])
                        .on_mouse_down_out(cx.listener(
                            |this, event: &gpui::MouseDownEvent, _, cx| {
                                let in_sub = this
                                    .tip_anchors
                                    .borrow()
                                    .get("submenu")
                                    .is_some_and(|bounds| bounds.contains(&event.position));
                                if !in_sub {
                                    this.close_context_menu(cx);
                                }
                            },
                        ))
                        .child(div().p(px(8.)).flex().flex_col().children(rows)),
                ),
        )
        .with_priority(2)
        .into_any_element();
        let sub = open.sub.and_then(|(act, row)| {
            let labels = self.context_sub_rows(act, target);
            if labels.is_empty() {
                return None;
            }
            let rows: Vec<(AnyElement, f32)> = labels
                .into_iter()
                .enumerate()
                .map(|(index, (label, on))| {
                    (
                        self.submenu_item(
                            SharedString::from(format!("context-sub-{index}")),
                            on.then(|| {
                                icon("checkbox_check", 16., "--text-link").into_any_element()
                            }),
                            label,
                            34.,
                        )
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.run_context_sub(act, target, index, cx);
                            this.close_context_menu(cx);
                        }))
                        .into_any_element(),
                        34.,
                    )
                })
                .collect();
            Some(self.submenu_frame(rows, row, window))
        });
        Some(div().child(menu).children(sub).into_any_element())
    }

    /// User Volume: the name, the percentage and a track a click sets (0 to 200%).
    fn volume_row(&self, row: &Row, target: Target, cx: &mut Context<Self>) -> AnyElement {
        let Target::User { user, .. } = target else {
            return div().into_any_element();
        };
        let (factor, _) = self.context.mix.get(&user).copied().unwrap_or((1., false));
        div()
            .flex_none()
            .p(px(8.))
            .flex()
            .flex_col()
            .gap(px(8.))
            .child(
                div()
                    .flex()
                    .justify_between()
                    .text_size(px(14.))
                    .line_height(px(18.))
                    .font_weight(FontWeight(500.))
                    .text_color(color("--text-strong"))
                    .child(row.label.clone())
                    .child(
                        div()
                            .text_color(color("--text-muted"))
                            .child(format!("{}%", (factor * 100.).round() as u32)),
                    ),
            )
            .child(
                div()
                    .id("context-volume-track")
                    .relative()
                    .h(px(8.))
                    .w_full()
                    .rounded(px(4.))
                    .cursor_pointer()
                    .bg(color("--background-mod-strong"))
                    .child(self.anchor_probe("context-volume"))
                    .child(
                        div()
                            .h_full()
                            .rounded(px(4.))
                            .bg(color("--control-brand-foreground"))
                            .w(gpui::relative(factor / 2.)),
                    )
                    .on_mouse_down(
                        gpui::MouseButton::Left,
                        cx.listener(move |this, event: &gpui::MouseDownEvent, _, cx| {
                            let track = this.tip_anchors.borrow().get("context-volume").copied();
                            if let Some(track) = track {
                                let along = f32::from(event.position.x - track.left())
                                    / f32::from(track.size.width);
                                let volume = (along.clamp(0., 1.) * 2. * 20.).round() / 20.;
                                let muted = this.context.mix.get(&user).is_some_and(|mix| mix.1);
                                this.set_user_mix(user, volume, muted);
                                cx.notify();
                            }
                        }),
                    ),
            )
            .into_any_element()
    }

    fn context_row(
        &self,
        index: usize,
        row: &Row,
        target: Target,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if row.act == Act::Volume {
            return self.volume_row(row, target, cx);
        }
        let act = row.act;
        let danger = row.danger;
        let disabled = row.disabled;
        let key = format!("context-row-{index}");
        let probe = key.clone();
        let hover_key = key.clone();
        let tint = if danger {
            color("--text-feedback-critical")
        } else if disabled {
            color("--text-muted")
        } else {
            color("--text-strong")
        };
        let has_sub = row.caret;
        div()
            .id(SharedString::from(key))
            .relative()
            .child(self.anchor_probe(probe))
            .h(px(row.height()))
            .flex_none()
            .p(px(8.))
            .rounded(px(2.))
            .flex()
            .items_center()
            .when(!disabled, |this| {
                this.cursor_pointer().hover(move |style| {
                    style
                        .bg(if danger {
                            color("--background-feedback-mod-critical")
                        } else {
                            color("--interactive-background-hover")
                        })
                        .rounded(px(4.))
                })
            })
            .on_hover(cx.listener(move |this, inside: &bool, _, cx| {
                if !*inside {
                    return;
                }
                let bounds = this.tip_anchors.borrow().get(hover_key.as_str()).copied();
                if let Some(open) = this.context.open.as_mut() {
                    open.sub = if has_sub {
                        bounds.map(|bounds| (act, bounds))
                    } else {
                        None
                    };
                }
                cx.notify();
            }))
            .on_click(cx.listener(move |this, _, window, cx| {
                if disabled {
                    return;
                }
                if has_sub {
                    // A click opens the submenu, as the hover does.
                    let bounds = this
                        .tip_anchors
                        .borrow()
                        .get(format!("context-row-{index}").as_str())
                        .copied();
                    if let (Some(open), Some(bounds)) = (this.context.open.as_mut(), bounds) {
                        open.sub = Some((act, bounds));
                    }
                    if matches!(act, Act::Notifications) && matches!(target, Target::Guild(_)) {
                        // The server's levels live on its own modal too; the submenu is the quick path.
                    }
                    cx.notify();
                    return;
                }
                this.close_context_menu(cx);
                this.run_context(act, target, window, cx);
                cx.notify();
            }))
            .children(row.icon.map(|name| {
                div()
                    .mr(px(8.))
                    .child(icon(name, 18., "--interactive-text-default"))
            }))
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .text_size(px(14.))
                            .line_height(px(18.))
                            .font_weight(FontWeight(500.))
                            .text_color(tint)
                            .child(row.label.clone()),
                    )
                    .children(row.sub.clone().map(|sub| {
                        div()
                            .text_size(px(12.))
                            .line_height(px(16.))
                            .font_weight(FontWeight(400.))
                            .text_color(color("--text-muted"))
                            .child(sub)
                    })),
            )
            .children(row.check.map(|on| {
                div()
                    .ml(px(16.))
                    .size(px(20.))
                    .rounded(px(4.))
                    .border_1()
                    .border_color(color("--interactive-text-default"))
                    .flex()
                    .items_center()
                    .justify_center()
                    .when(on, |this| {
                        this.bg(color("--control-brand-foreground")).child(icon(
                            "checkbox_check",
                            14.,
                            "--white",
                        ))
                    })
            }))
            .when(row.caret, |this| {
                this.child(div().ml(px(10.)).child(icon(
                    "menu_caret",
                    18.,
                    "--interactive-text-default",
                )))
            })
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn labels(entries: &[Entry]) -> Vec<String> {
        entries
            .iter()
            .map(|entry| match entry {
                Entry::Row(row) => row.label.clone(),
                Entry::Separator => "-".to_owned(),
            })
            .collect()
    }

    #[test]
    fn a_text_channel_menu_for_a_manager_is_the_captured_list() {
        let items = channel_items(
            ChannelFacts {
                can_manage: true,
                can_invite: true,
                ..ChannelFacts::default()
            },
            "All Messages",
        );
        assert_eq!(
            labels(&items),
            [
                "Mark As Read",
                "-",
                "Add to Favorites",
                "-",
                "Invite to Channel",
                "Pin Channel to Top",
                "Copy Link",
                "-",
                "Mute Channel",
                "Notification Settings",
                "-",
                "Move to...",
                "-",
                "Edit Channel",
                "Duplicate Channel",
                "Create Text Channel",
                "Delete Channel",
            ]
        );
    }

    #[test]
    fn without_permissions_the_management_and_invite_rows_go() {
        let items = channel_items(ChannelFacts::default(), "All Messages");
        let shown = labels(&items);
        assert!(!shown.contains(&"Edit Channel".to_owned()));
        assert!(!shown.contains(&"Invite to Channel".to_owned()));
        assert!(!shown.contains(&"Move to...".to_owned()));
        assert!(shown.contains(&"Copy Link".to_owned()));
        // No separator is left leading, trailing or doubled.
        assert_ne!(items.first(), Some(&Entry::Separator));
        assert_ne!(items.last(), Some(&Entry::Separator));
        assert!(
            !items
                .windows(2)
                .any(|pair| pair[0] == Entry::Separator && pair[1] == Entry::Separator)
        );
    }

    #[test]
    fn mark_as_read_is_disabled_when_nothing_is_unread() {
        let items = channel_items(ChannelFacts::default(), "All Messages");
        assert!(matches!(&items[0], Entry::Row(row) if row.disabled));
        let items = channel_items(
            ChannelFacts {
                unread: true,
                ..ChannelFacts::default()
            },
            "x",
        );
        assert!(matches!(&items[0], Entry::Row(row) if !row.disabled));
    }

    #[test]
    fn heights_follow_the_captures() {
        let items = channel_items(ChannelFacts::default(), "All Messages");
        let heights: Vec<f32> = items
            .iter()
            .filter_map(|entry| match entry {
                Entry::Row(row) => Some(row.height()),
                Entry::Separator => None,
            })
            .collect();
        // Mark As Read 34, Favorites 36, Pin 34, Copy Link 34, Mute 36, Notifications 52.
        assert_eq!(heights, [34., 36., 34., 34., 36., 52.]);
    }

    #[test]
    fn the_category_menu_names_the_category() {
        let items = category_items(false, false, true, false, "All Messages");
        let shown = labels(&items);
        assert!(shown.contains(&"Mute Category".to_owned()));
        assert!(shown.contains(&"Edit Category".to_owned()));
        assert!(shown.contains(&"Delete Category".to_owned()));
    }

    #[test]
    fn a_thread_owner_without_manage_threads_can_still_close_and_edit_it() {
        let items = thread_items(false, false, true, true, false, "x");
        let shown = labels(&items);
        assert!(shown.contains(&"Close Thread".to_owned()));
        assert!(shown.contains(&"Edit Thread".to_owned()));
        assert!(!shown.contains(&"Lock Thread".to_owned()));
        assert!(shown.contains(&"Leave Thread".to_owned()));
    }

    #[test]
    fn the_server_menu_hides_what_the_account_may_not_do() {
        let items = guild_items(
            GuildFacts {
                owner: true,
                ..GuildFacts::default()
            },
            "Only @mentions",
        );
        let shown = labels(&items);
        assert!(!shown.contains(&"Invite to Server".to_owned()));
        assert!(!shown.contains(&"Create Channel".to_owned()));
        assert!(!shown.contains(&"Server Settings".to_owned()));
        assert!(!shown.contains(&"Leave Server".to_owned()));
        let admin = guild_items(
            GuildFacts {
                can_invite: true,
                can_manage_channels: true,
                can_manage_guild: true,
                ..GuildFacts::default()
            },
            "x",
        );
        let shown = labels(&admin);
        assert!(shown.contains(&"Create Event".to_owned()));
        assert!(shown.contains(&"Leave Server".to_owned()));
    }

    #[test]
    fn a_person_menu_depends_on_who_and_where() {
        let other = UserFacts {
            name: "ann".to_owned(),
            in_guild: true,
            can_kick: true,
            can_ban: true,
            can_moderate: true,
            ..UserFacts::default()
        };
        let shown = labels(&user_items(&other));
        for want in [
            "Profile",
            "Mention",
            "Message",
            "Add Friend",
            "Block",
            "Timeout ann",
            "Kick ann",
            "Ban ann",
        ] {
            assert!(shown.contains(&want.to_owned()), "{want}");
        }
        let own = UserFacts {
            me: true,
            in_guild: true,
            ..UserFacts::default()
        };
        let shown = labels(&user_items(&own));
        assert!(shown.contains(&"Edit Per-server Profile".to_owned()));
        assert!(!shown.contains(&"Block".to_owned()));
        assert!(!shown.contains(&"Message".to_owned()));
        let friend = UserFacts {
            friend: true,
            ..other
        };
        assert!(!labels(&user_items(&friend)).contains(&"Add Friend".to_owned()));
    }

    #[test]
    fn people_in_our_call_get_volume_and_mute_rows() {
        let in_call = UserFacts {
            name: "ann".to_owned(),
            in_call: true,
            volume: 150,
            ..UserFacts::default()
        };
        let items = user_items(&in_call);
        let shown = labels(&items);
        for want in ["User Volume", "Mute", "Mute Soundboard"] {
            assert!(shown.contains(&want.to_owned()), "{want}");
        }
        assert!(
            items.iter().any(
                |entry| matches!(entry, Entry::Row(row) if row.sub.as_deref() == Some("150%"))
            )
        );
        let muted = UserFacts {
            soundboard_muted: true,
            voice_muted: true,
            ..in_call.clone()
        };
        let items = user_items(&muted);
        assert!(labels(&items).contains(&"Unmute Soundboard".to_owned()));
        assert!(items.iter().any(|entry| matches!(entry, Entry::Row(row) if row.label == "Mute" && row.check == Some(true))));
        // Not in a call, or ourselves: none of them.
        let outside = UserFacts {
            in_call: false,
            ..in_call.clone()
        };
        assert!(!labels(&user_items(&outside)).contains(&"User Volume".to_owned()));
        let own = UserFacts {
            me: true,
            ..in_call
        };
        assert!(!labels(&user_items(&own)).contains(&"User Volume".to_owned()));
    }

    #[test]
    fn a_dm_row_menu_has_the_channel_entries_first() {
        let facts = UserFacts {
            name: "ann".to_owned(),
            dm: Some(false),
            invite_servers: true,
            ..UserFacts::default()
        };
        let shown = labels(&user_items(&facts));
        assert_eq!(shown[0], "Mark As Read");
        assert!(shown.contains(&"Pin".to_owned()));
        assert!(shown.contains(&"Close DM".to_owned()));
        assert!(shown.contains(&"Mute @ann".to_owned()));
        assert!(shown.contains(&"Invite to Server".to_owned()));
    }
}
