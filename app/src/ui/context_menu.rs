//! Right-click menus of the server rail, channel rows, category rows, thread rows, DM rows and people. The entries,
//! their order, separators and heights come from the captures in `design/snapshot/dumps/menus/` (`guild-icon-context`,
//! `channel-text-context`, `channel-voice-context`, `category-context`, `thread-context`, `dm-row-context`,
//! `member-other-context`, `member-self-context`, `message-user-*-context`); which entries show for the account is
//! decided by `*_items`, from plain facts, so the rules are unit tested.

use super::{Root, color, icon};
use crate::net::Request;
use crate::parity::Parity;
use crate::spellcheck;
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
    InventoryFeed,
    InventoryFeedAtPointer,
    InventoryAuthor {
        user: Id,
    },
    Guild(Id),
    Channel(Id),
    Category(Id),
    Thread(Id),
    Dm(Id),
    User {
        user: Id,
        guild: Option<Id>,
    },
    /// The composer's text area (`textarea-context`, module 194400).
    Composer,
    /// The "..." of an application's page in the directory (module 586193 `l4`).
    AppDirectory(Id),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Act {
    InventoryAbout,
    InventoryHide,
    ReportApp,
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
    RemoveFriend,
    FriendNickname,
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
    /// Server-wide voice moderation of a person in a voice channel (module 291247): `voice-mute`, `voice-deafen`,
    /// `voice-disconnect`.
    ServerMute,
    ServerDeafen,
    Disconnect,
    /// "Roles" (module 180798): a submenu that gives and takes roles by `PATCH /guilds/:g/members/:u` with the list.
    Roles,
    /// "Move to" for a person in a voice channel (module 252726): the voice channels they can be moved to.
    MoveMember,
    /// "Change Nickname" of someone else (module 346247 opens module 752118).
    ChangeNickname,
    /// "Remove Timeout" of a member who is in timeout.
    RemoveTimeout,
    LeaveGuild,
    /// Voice: how loud a person is to us, and muting them (or their soundboard) here.
    Volume,
    VoiceMute,
    SoundboardMute,
    /// Composer text menu: a spelling suggestion (by position), the dictionary rows, the switch, the help page, and
    /// the clipboard rows.
    Correction(u8),
    AddToDictionary,
    RemoveFromDictionary,
    Spellcheck,
    SpellLanguages,
    Cut,
    Copy,
    Paste,
}

impl Act {
    /// Every row kind once (`Correction` with its first index). `ordinal` is an exhaustive match, so adding a variant
    /// without listing it here fails the build or the `act_list_is_complete` test.
    #[cfg(test)]
    pub(super) const ALL: [Act; 67] = [
        Act::InventoryAbout,
        Act::InventoryHide,
        Act::ReportApp,
        Act::MarkRead,
        Act::Invite,
        Act::Mute,
        Act::Notifications,
        Act::HideMuted,
        Act::ServerSettings,
        Act::PrivacySettings,
        Act::EditProfile,
        Act::CreateChannel,
        Act::CreateCategory,
        Act::CreateEvent,
        Act::MoveTo,
        Act::CopyLink,
        Act::CopyId,
        Act::EditChannel,
        Act::Duplicate,
        Act::DeleteChannel,
        Act::CollapseCategory,
        Act::CollapseAll,
        Act::PinChannel,
        Act::Favorite,
        Act::OpenChat,
        Act::HideNames,
        Act::CloseThread,
        Act::LockThread,
        Act::LeaveThread,
        Act::Profile,
        Act::Mention,
        Act::Message,
        Act::Call,
        Act::Note,
        Act::Apps,
        Act::AddFriend,
        Act::RemoveFriend,
        Act::FriendNickname,
        Act::Ignore,
        Act::Block,
        Act::CloseDm,
        Act::PinDm,
        Act::InviteToServer,
        Act::ModView,
        Act::Verify,
        Act::Timeout,
        Act::Kick,
        Act::Ban,
        Act::ServerMute,
        Act::ServerDeafen,
        Act::Disconnect,
        Act::LeaveGuild,
        Act::Volume,
        Act::VoiceMute,
        Act::SoundboardMute,
        Act::Correction(0),
        Act::AddToDictionary,
        Act::RemoveFromDictionary,
        Act::Spellcheck,
        Act::SpellLanguages,
        Act::Cut,
        Act::Copy,
        Act::Paste,
        Act::Roles,
        Act::MoveMember,
        Act::ChangeNickname,
        Act::RemoveTimeout,
    ];

    #[cfg(test)]
    pub(super) fn ordinal(self) -> usize {
        match self {
            Act::InventoryAbout => 0,
            Act::InventoryHide => 1,
            Act::ReportApp => 2,
            Act::MarkRead => 3,
            Act::Invite => 4,
            Act::Mute => 5,
            Act::Notifications => 6,
            Act::HideMuted => 7,
            Act::ServerSettings => 8,
            Act::PrivacySettings => 9,
            Act::EditProfile => 10,
            Act::CreateChannel => 11,
            Act::CreateCategory => 12,
            Act::CreateEvent => 13,
            Act::MoveTo => 14,
            Act::CopyLink => 15,
            Act::CopyId => 16,
            Act::EditChannel => 17,
            Act::Duplicate => 18,
            Act::DeleteChannel => 19,
            Act::CollapseCategory => 20,
            Act::CollapseAll => 21,
            Act::PinChannel => 22,
            Act::Favorite => 23,
            Act::OpenChat => 24,
            Act::HideNames => 25,
            Act::CloseThread => 26,
            Act::LockThread => 27,
            Act::LeaveThread => 28,
            Act::Profile => 29,
            Act::Mention => 30,
            Act::Message => 31,
            Act::Call => 32,
            Act::Note => 33,
            Act::Apps => 34,
            Act::AddFriend => 35,
            Act::RemoveFriend => 36,
            Act::FriendNickname => 37,
            Act::Ignore => 38,
            Act::Block => 39,
            Act::CloseDm => 40,
            Act::PinDm => 41,
            Act::InviteToServer => 42,
            Act::ModView => 43,
            Act::Verify => 44,
            Act::Timeout => 45,
            Act::Kick => 46,
            Act::Ban => 47,
            Act::ServerMute => 48,
            Act::ServerDeafen => 49,
            Act::Disconnect => 50,
            Act::LeaveGuild => 51,
            Act::Volume => 52,
            Act::VoiceMute => 53,
            Act::SoundboardMute => 54,
            Act::Correction(_) => 55,
            Act::AddToDictionary => 56,
            Act::RemoveFromDictionary => 57,
            Act::Spellcheck => 58,
            Act::SpellLanguages => 59,
            Act::Cut => 60,
            Act::Copy => 61,
            Act::Paste => 62,
            Act::Roles => 63,
            Act::MoveMember => 64,
            Act::ChangeNickname => 65,
            Act::RemoveTimeout => 66,
        }
    }
}

/// Whether a row's action exists in the app. A row whose feature is missing is drawn disabled (never a live-looking
/// row that does nothing); the name says what is missing. Every `Act` is classified here, so a new row cannot be added
/// without deciding it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Status {
    Wired,
    Missing(&'static str),
}

pub(super) fn status(act: Act) -> Status {
    use Status::{Missing, Wired};
    match act {
        Act::MoveTo => Missing("channel and category reordering"),
        Act::PinChannel => Missing("pinned channels (user settings proto)"),
        Act::Favorite => Missing("favorite channels (user settings proto)"),
        Act::PinDm => Missing("pinned direct messages (user settings proto)"),
        Act::HideNames => Missing("Hide Names for a voice channel"),
        Act::Apps => Missing("the user-app commands submenu"),
        Act::ModView => Missing("Mod View"),
        Act::InventoryAbout
        | Act::InventoryHide
        | Act::ReportApp
        | Act::MarkRead
        | Act::Invite
        | Act::Mute
        | Act::Notifications
        | Act::HideMuted
        | Act::ServerSettings
        | Act::PrivacySettings
        | Act::EditProfile
        | Act::CreateChannel
        | Act::CreateCategory
        | Act::CreateEvent
        | Act::CopyLink
        | Act::CopyId
        | Act::EditChannel
        | Act::Duplicate
        | Act::DeleteChannel
        | Act::CollapseCategory
        | Act::CollapseAll
        | Act::OpenChat
        | Act::CloseThread
        | Act::LockThread
        | Act::LeaveThread
        | Act::Profile
        | Act::Mention
        | Act::Message
        | Act::Call
        | Act::Note
        | Act::AddFriend
        | Act::RemoveFriend
        | Act::FriendNickname
        | Act::Ignore
        | Act::Block
        | Act::CloseDm
        | Act::Timeout
        | Act::Kick
        | Act::Ban
        | Act::ServerMute
        | Act::ServerDeafen
        | Act::Disconnect
        | Act::Verify
        | Act::Roles
        | Act::MoveMember
        | Act::ChangeNickname
        | Act::RemoveTimeout
        | Act::InviteToServer
        | Act::LeaveGuild
        | Act::Volume
        | Act::VoiceMute
        | Act::SoundboardMute
        | Act::Correction(_)
        | Act::AddToDictionary
        | Act::RemoveFromDictionary
        | Act::Spellcheck
        | Act::SpellLanguages
        | Act::Cut
        | Act::Copy
        | Act::Paste => Wired,
    }
}

/// A row of a submenu: its label, whether it is ticked, a colour dot (roles), and the id it stands for.
#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct SubRow {
    pub label: String,
    pub on: bool,
    pub disabled: bool,
    /// Shown but never acted on: a member's roles when we cannot manage roles.
    pub plain: bool,
    /// A role's colour (`0xRRGGBB`, 0 for the default dot).
    pub dot: Option<u32>,
    pub id: Id,
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
    /// The key combination shown at the right (`shortcut` of the menu item, module 165862).
    pub shortcut: Option<&'static str>,
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
            disabled: matches!(status(act), Status::Missing(_)),
            shortcut: None,
        }
    }

    fn shortcut(mut self, keys: &'static str) -> Self {
        self.shortcut = Some(keys);
        self
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

/// The help page the composer menu's Languages row opens: `getArticleURL(SPELLCHECK)` (module 413756).
const SPELLCHECK_HELP: &str = "https://support.discord.com/hc/en-us/articles/360000877191";

/// What the composer's text menu was opened on.
#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct ComposerMenu {
    /// The misspelled word under the click: its byte range and text.
    pub word: Option<(std::ops::Range<usize>, String)>,
    pub suggestions: Vec<String>,
    /// A learned word that stands in the text (`findLearnedWordIn`).
    pub learned: Option<String>,
    pub selection: bool,
    pub enabled: bool,
}

/// `textarea-context` (module 194400) with the spelling section of module 413756: the corrections, then Add to
/// Dictionary (or Remove from Dictionary when the click is on no misspelling and the text holds a learned word), the
/// Spellcheck switch and Languages, then Cut and Copy with a selection, then Paste. The Suggestions submenu, the send
/// button switch and the AI rows of the official menu are account features this client does not have yet.
/// `m` in module 586193: Report App (danger, a flag), then Copy Application ID in its own group when developer mode is on
/// (`devmode-copy-id`, module 50268). The page's Copy Link button covers the wide layout, so the menu has no such row.
pub(super) fn app_directory_items(developer: bool) -> Vec<Entry> {
    let mut out = vec![Entry::Row(
        Row::new(Act::ReportApp, "Report App")
            .icon("menu_flag")
            .danger(),
    )];
    group(
        &mut out,
        if developer {
            vec![Row::new(Act::CopyId, "Copy Application ID").icon("menu_id")]
        } else {
            Vec::new()
        },
    );
    out
}

pub(super) fn composer_items(menu: &ComposerMenu) -> Vec<Entry> {
    let mut out = Vec::new();
    group(
        &mut out,
        menu.suggestions
            .iter()
            .take(spellcheck::MAX_SUGGESTIONS)
            .enumerate()
            .map(|(index, word)| Row::new(Act::Correction(index as u8), word.clone()))
            .collect(),
    );
    let mut spelling = Vec::new();
    if menu.word.is_some() {
        spelling.push(Row::new(Act::AddToDictionary, "Add to Dictionary"));
    }
    if menu.learned.is_some() && menu.word.is_none() {
        spelling.push(Row::new(
            Act::RemoveFromDictionary,
            "Remove from Dictionary",
        ));
    }
    spelling.push(Row::new(Act::Spellcheck, "Spellcheck").check(menu.enabled));
    if menu.enabled {
        spelling.push(Row::new(Act::SpellLanguages, "Languages"));
    }
    group(&mut out, spelling);
    let mut clipboard = Vec::new();
    if menu.selection {
        clipboard.push(Row::new(Act::Cut, "Cut").shortcut("Ctrl+X"));
        clipboard.push(Row::new(Act::Copy, "Copy").shortcut("Ctrl+C"));
    }
    clipboard.push(Row::new(Act::Paste, "Paste").shortcut("Ctrl+V"));
    group(&mut out, clipboard);
    tidy(out)
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
    pub bot: bool,
    pub in_guild: bool,
    pub friend: bool,
    pub nickname: bool,
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
    /// The person is in a voice channel of this server (module 291247 `Z`).
    pub server_voice: Option<ServerVoice>,
    /// The person is in timeout and we may lift it.
    pub in_timeout: bool,
    /// Verify Member is offered: `Some(true)` when the member already bypasses verification (module 291247).
    pub verify: Option<bool>,
    /// The Roles submenu has something to show: the member's roles, or the ones we may give.
    pub roles: bool,
    pub voice_muted: bool,
    pub soundboard_muted: bool,
    /// The volume as a percentage, 0 to 200.
    pub volume: u32,
}

/// `GuildMemberFlags.BYPASSES_VERIFICATION` (module 340837).
const BYPASSES_VERIFICATION: u64 = 4;

/// A person's place in a voice channel of the open server, and what we may do to them there.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct ServerVoice {
    pub mute: bool,
    pub deaf: bool,
    pub can_mute: bool,
    pub can_deafen: bool,
    pub can_disconnect: bool,
    /// There is a voice channel to move them to.
    pub can_move: bool,
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
        if facts.friend {
            first.push(Row::new(
                Act::FriendNickname,
                if facts.nickname {
                    "Change Friend Nickname"
                } else {
                    "Add Friend Nickname"
                },
            ));
        }
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
            if facts.me {
                Act::EditProfile
            } else {
                Act::ChangeNickname
            },
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
        if !facts.bot {
            mid.push(if facts.friend {
                Row::new(Act::RemoveFriend, "Remove Friend")
            } else {
                Row::new(Act::AddFriend, "Add Friend").disabled(facts.blocked)
            });
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
        // Module 107632: Roles (180798) and Move to (252726) share a group before the moderation one.
        let mut place = Vec::new();
        if facts.roles {
            place.push(Row::new(Act::Roles, "Roles").caret());
        }
        if facts.server_voice.is_some_and(|voice| voice.can_move) {
            place.push(Row::new(Act::MoveMember, "Move to").caret());
        }
        group(&mut out, place);
    }
    if facts.dm.is_none() && facts.in_guild {
        let mut moderation = vec![Row::new(Act::ModView, "Open in Mod View")];
        if !facts.me {
            if let Some(verified) = facts.verify {
                moderation.push(Row::new(
                    Act::Verify,
                    if verified {
                        "Unverify Member"
                    } else {
                        "Verify Member"
                    },
                ));
            }
            // Module 291247 lists the voice rows between Verify and Timeout, in the same group.
            if let Some(voice) = facts.server_voice {
                if voice.can_mute {
                    moderation.push(
                        Row::new(Act::ServerMute, "Server Mute")
                            .check(voice.mute)
                            .danger(),
                    );
                }
                if voice.can_deafen {
                    moderation.push(
                        Row::new(Act::ServerDeafen, "Server Deafen")
                            .check(voice.deaf)
                            .danger(),
                    );
                }
                if voice.can_disconnect {
                    moderation.push(Row::new(Act::Disconnect, "Disconnect").danger());
                }
            }
            if facts.can_moderate {
                moderation.push(if facts.in_timeout {
                    Row::new(
                        Act::RemoveTimeout,
                        format!("Remove Timeout From {}", facts.name),
                    )
                    .danger()
                } else {
                    Row::new(Act::Timeout, format!("Timeout {}", facts.name)).danger()
                });
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
    /// What the composer menu was opened on, kept while it is open.
    pub composer: Option<ComposerMenu>,
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

    /// Module 291247: Verify Member needs a member who joined, a server that ever had onboarding, and the right to
    /// manage them (moderate, kick and ban, or manage the server or roles). `Some(true)` when already verified.
    fn verify_state(&self, guild: Id, user: Id, me: bool) -> Option<bool> {
        let known = self.stores.guild(guild)?;
        let member = self.stores.members.get(guild, user)?;
        if me
            || member.joined_at.is_none()
            || !known
                .features
                .iter()
                .any(|feature| feature == "GUILD_ONBOARDING_EVER_ENABLED")
            || !self.outranks(guild, user)
        {
            return None;
        }
        let can = |bit| self.my_permission(guild, None, bit);
        let allowed = (can(permissions::MODERATE_MEMBERS)
            && can(permissions::KICK_MEMBERS)
            && can(permissions::BAN_MEMBERS))
            || can(permissions::MANAGE_GUILD)
            || can(permissions::MANAGE_ROLES);
        allowed.then_some(member.flags & BYPASSES_VERIFICATION != 0)
    }

    /// The member is in timeout now (`communication_disabled_until` in the future).
    fn is_in_timeout(&self, guild: Id, user: Id) -> bool {
        self.stores
            .members
            .get(guild, user)
            .and_then(|member| member.timeout_until.as_deref())
            .and_then(|until| chrono::DateTime::parse_from_rfc3339(until).ok())
            .is_some_and(|until| until > chrono::Utc::now())
    }

    /// Whether our roles outrank everything the person holds, and they do not own the server: what
    /// `canManageUser` asks (module 576705).
    fn outranks(&self, guild: Id, user: Id) -> bool {
        let Some(me) = self.stores.current_user.as_ref().map(|me| me.id) else {
            return false;
        };
        let Some(known) = self.stores.guild(guild) else {
            return false;
        };
        if known.owner_id == Some(user) {
            return false;
        }
        let theirs = self
            .stores
            .members
            .get(guild, user)
            .map(|member| member.roles.clone())
            .unwrap_or_default();
        theirs.iter().all(|role| {
            *role == guild || permissions::role_is_higher(known, me, &known.my_roles, *role)
        })
    }

    /// The Roles submenu (module 180798): with Manage Roles, every role below our highest (a role above is listed
    /// only when the person holds it, and is disabled), checked when held; without it, just the roles they hold.
    fn role_sub_rows(&self, guild: Id, user: Id) -> Vec<SubRow> {
        let (Some(known), Some(member), Some(me)) = (
            self.stores.guild(guild),
            self.stores.members.get(guild, user),
            self.stores.current_user.as_ref().map(|me| me.id),
        ) else {
            return Vec::new();
        };
        let manage = self.my_permission(guild, None, permissions::MANAGE_ROLES);
        let mut roles: Vec<(i64, Id)> = known
            .roles
            .keys()
            .copied()
            .filter(|id| *id != guild)
            .map(|id| (known.role_info.get(&id).map_or(0, |info| info.position), id))
            .collect();
        roles.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
        roles
            .into_iter()
            .filter_map(|(_, id)| {
                let info = known.role_info.get(&id).copied().unwrap_or_default();
                let held = member.roles.contains(&id);
                let locked =
                    info.managed || !permissions::role_is_higher(known, me, &known.my_roles, id);
                if manage && locked && !held {
                    return None;
                }
                if !manage && !held {
                    return None;
                }
                Some(SubRow {
                    label: known.roles.get(&id).cloned().unwrap_or_default(),
                    on: manage && held,
                    disabled: manage && locked,
                    plain: !manage,
                    dot: Some(info.color),
                    id,
                })
            })
            .collect()
    }

    /// Module 252726: the voice channels a person can be moved to: not the one they are in, where we may move
    /// members and they may connect.
    fn move_targets(&self, guild: Id, user: Id) -> Vec<Id> {
        let (Some(known), Some(state)) = (
            self.stores.guild(guild),
            self.stores
                .voice_states
                .get(&guild)
                .and_then(|states| states.iter().find(|state| state.user_id == user)),
        ) else {
            return Vec::new();
        };
        let roles = self
            .stores
            .members
            .get(guild, user)
            .map(|member| member.roles.clone())
            .unwrap_or_default();
        let mut channels: Vec<&Channel> = known
            .channels
            .iter()
            .filter(|channel| channel.kind == ChannelKind::Voice && channel.id != state.channel_id)
            .filter(|channel| {
                self.my_permission(guild, Some(channel), permissions::MOVE_MEMBERS)
                    && permissions::member_channel_permissions(known, channel, user, &roles)
                        & permissions::CONNECT
                        != 0
            })
            .collect();
        channels.sort_by_key(|channel| (channel.position, channel.id));
        channels.into_iter().map(|channel| channel.id).collect()
    }

    /// Module 291247: the voice rows need the person in a voice channel of this server and the permission in that
    /// channel; Disconnect also needs the person to be someone else.
    fn server_voice_of(&self, guild: Id, user: Id, me: bool) -> Option<ServerVoice> {
        let state = self
            .stores
            .voice_states
            .get(&guild)?
            .iter()
            .find(|state| state.user_id == user)?;
        let channel = self
            .stores
            .guild(guild)?
            .channels
            .iter()
            .find(|channel| channel.id == state.channel_id)?;
        Some(ServerVoice {
            mute: state.mute,
            deaf: state.deaf,
            can_mute: self.my_permission(guild, Some(channel), permissions::MUTE_MEMBERS),
            can_deafen: self.my_permission(guild, Some(channel), permissions::DEAFEN_MEMBERS),
            can_disconnect: !me
                && self.my_permission(guild, Some(channel), permissions::MOVE_MEMBERS),
            can_move: !me
                && self.my_permission(guild, Some(channel), permissions::MOVE_MEMBERS)
                && !self.move_targets(guild, user).is_empty(),
        })
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

    /// Harness: `context-user` opens the menu of the first other known person in the fixture server, `context-dm` the
    /// one of the first direct message, at a fixed place.
    #[cfg(feature = "capture")]
    pub(crate) fn script_context(&mut self, kind: &str, cx: &mut Context<Self>) -> String {
        let me = self.stores.current_user.as_ref().map(|me| me.id);
        let target = match kind {
            "user" => {
                let Some(user) = self
                    .stores
                    .users
                    .keys()
                    .copied()
                    .filter(|id| Some(*id) != me)
                    .min()
                else {
                    return "no other person known".to_owned();
                };
                Target::User {
                    user,
                    guild: Some(self.guild.unwrap_or(1_568_692_564_637_524_109)),
                }
            }
            "self" => {
                let Some(user) = me else {
                    return "no current user".to_owned();
                };
                self.open_context_menu(
                    Target::User {
                        user,
                        guild: Some(self.guild.unwrap_or(1_568_692_564_637_524_109)),
                    },
                    gpui::point(px(980.), px(465.)),
                    cx,
                );
                return "menu open for self".to_owned();
            }
            "dm" => {
                let Some(channel) = self.stores.private_channels.first().map(|dm| dm.id) else {
                    return "no direct message".to_owned();
                };
                Target::Dm(channel)
            }
            _ => return "usage: context-user | context-dm".to_owned(),
        };
        self.open_context_menu(target, gpui::point(px(520.), px(120.)), cx);
        format!("menu open for {target:?}")
    }

    /// Harness: whether a submenu is open and how many rows it holds.
    #[cfg(feature = "capture")]
    pub(crate) fn script_context_sub_state(&self) -> String {
        let Some(open) = self.context.open.as_ref() else {
            return "no menu".to_owned();
        };
        match open.sub {
            Some((act, row)) => format!(
                "sub {act:?}: {} rows, row {:?}, submenu anchor {:?}",
                self.context_sub_rows(act, open.target).len(),
                row,
                self.tip_anchors.borrow().get("submenu")
            ),
            None => "no sub open".to_owned(),
        }
    }

    pub(super) fn open_context_menu(
        &mut self,
        target: Target,
        at: Point<Pixels>,
        cx: &mut Context<Self>,
    ) {
        let developer = self.developer_mode();
        let items = match target {
            Target::InventoryAuthor { .. } => {
                let mut items = vec![Entry::Row(Row::new(Act::Profile, "Profile"))];
                if developer {
                    items.push(Entry::Separator);
                    items.push(Entry::Row(Row::new(Act::CopyId, "Copy User ID")));
                }
                items
            }
            Target::InventoryFeed | Target::InventoryFeedAtPointer => vec![
                Entry::Row(
                    Row::new(Act::InventoryAbout, "About Recent Activity").icon("circle_info"),
                ),
                Entry::Row(
                    Row::new(Act::InventoryHide, "Hide Activity Cards")
                        .icon("eye_slash")
                        .check(self.inventory_hidden()),
                ),
            ],
            Target::Composer => composer_items(&self.context.composer.clone().unwrap_or_default()),
            Target::AppDirectory(_) => app_directory_items(developer),
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
                    name: self.private_channel_name(dm),
                    me: self
                        .stores
                        .current_user
                        .as_ref()
                        .is_some_and(|me| me.id == user.id),
                    bot: user.bot,
                    in_guild: false,
                    friend: self.stores.relationships.get(&user.id) == Some(&1),
                    nickname: self.nickname_for(user.id).is_some(),
                    blocked: self.stores.relationships.get(&user.id) == Some(&2),
                    ignored: self.stores.is_ignored(user.id),
                    developer,
                    dm: Some(self.stores.is_unread(channel)),
                    invite_servers: !self.invite_servers().is_empty(),
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
                    bot: known.bot,
                    in_guild: guild.is_some(),
                    friend: self.stores.relationships.get(&user) == Some(&1),
                    nickname: self.nickname_for(user).is_some(),
                    blocked: self.stores.relationships.get(&user) == Some(&2),
                    ignored: self.stores.is_ignored(user),
                    can_nick: guild.is_some_and(|guild| {
                        self.my_permission(guild, None, permissions::MANAGE_NICKNAMES)
                            && self.outranks(guild, user)
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
                    server_voice: guild.and_then(|guild| self.server_voice_of(guild, user, me)),
                    in_timeout: guild.is_some_and(|guild| self.is_in_timeout(guild, user))
                        && guild.is_some_and(|guild| {
                            self.my_permission(guild, None, permissions::MODERATE_MEMBERS)
                        }),
                    roles: guild.is_some_and(|guild| !self.role_sub_rows(guild, user).is_empty()),
                    verify: guild.and_then(|guild| self.verify_state(guild, user, me)),
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

    /// A right click in the composer: the field selects the misspelled word under the pointer, the checker supplies its
    /// suggestions (off the interface thread; the menu opens when they are back), and the menu lists them first.
    pub(super) fn open_composer_menu(
        &mut self,
        at: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let click = self
            .composer
            .update(cx, |composer, cx| composer.context_click(at, window, cx));
        let text = self.composer.read(cx).text().to_owned();
        let enabled = spellcheck::is_enabled();
        let menu = ComposerMenu {
            learned: click
                .word
                .is_none()
                .then(|| spellcheck::find_learned_word_in(&text))
                .flatten(),
            word: click.word.clone().filter(|_| enabled),
            suggestions: Vec::new(),
            selection: !click.selected.is_empty(),
            enabled,
        };
        let Some((_, word)) = menu.word.clone() else {
            self.context.composer = Some(menu);
            self.open_context_menu(Target::Composer, at, cx);
            return;
        };
        cx.spawn(async move |this, cx| {
            let suggestions = cx
                .background_executor()
                .spawn(async move {
                    spellcheck::ensure_languages();
                    spellcheck::service()
                        .suggest(&word)
                        .recv()
                        .ok()
                        .unwrap_or_default()
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                let mut menu = menu;
                menu.suggestions = suggestions;
                this.context.composer = Some(menu);
                this.open_context_menu(Target::Composer, at, cx);
            });
        })
        .detach();
    }

    pub(super) fn close_context_menu(&mut self, cx: &mut Context<Self>) {
        if self.context.open.take().is_some() {
            cx.notify();
        }
    }

    pub(super) fn mark_channel_read(&mut self, channel: Id) {
        if !self.stores.is_unread(channel) {
            return;
        }
        let Some(latest) = self.stores.read_states.latest.get(&channel).copied() else {
            return;
        };
        self.stores.read_states.ack_local(channel, latest, false);
        self.data_revision += 1;
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
            (Act::ReportApp, Target::AppDirectory(app)) => {
                self.open_report_app(app, "app_directory", cx);
            }
            (Act::InventoryAbout, Target::InventoryFeed | Target::InventoryFeedAtPointer) => {
                self.inventory.about = true
            }
            (Act::InventoryHide, Target::InventoryFeed | Target::InventoryFeedAtPointer) => {
                self.toggle_inventory_hidden()
            }
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
                    Target::User { user, .. } | Target::InventoryAuthor { user } => user,
                    Target::AppDirectory(id) => id,
                    Target::InventoryFeed | Target::InventoryFeedAtPointer | Target::Composer => {
                        return;
                    }
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
            (Act::Profile, Target::User { user, .. } | Target::InventoryAuthor { user }) => {
                self.close_inventory_popout();
                self.open_full_profile(user, window, cx)
            }
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
            // "Start a Call" of the DM row's menu (`dm-row-context.json`): a voice call that rings.
            (Act::Call, Target::Dm(channel)) => {
                self.select_home(Some(channel));
                let active = self.stores.calls.is_call_active(channel, None);
                self.start_call(channel, false, !active, cx);
            }
            // "Start a Call" of a person's menu: their DM opens and the voice call rings there.
            (Act::Call, Target::User { user, .. }) => self.start_dm_call_with(user, false, cx),
            // "Add Note": the note field of the full profile.
            (Act::Note, Target::User { user, .. }) => self.open_full_profile(user, window, cx),
            (Act::Note, Target::Dm(channel)) => {
                if let Some(user) = self
                    .stores
                    .private_channels
                    .iter()
                    .find(|known| known.id == channel)
                    .and_then(|known| known.recipients.first().copied())
                {
                    self.open_full_profile(user, window, cx);
                }
            }
            (
                Act::Verify,
                Target::User {
                    user,
                    guild: Some(guild),
                },
            ) => {
                // Module 778858 `setMemberFlags`: the flags go back with the bit flipped.
                let flags = self
                    .stores
                    .members
                    .get(guild, user)
                    .map_or(0, |member| member.flags);
                let _ = self.requests.send(super::member_actions::member_patch(
                    guild,
                    user,
                    "flags",
                    json!(flags ^ BYPASSES_VERIFICATION),
                ));
            }
            (
                Act::ChangeNickname,
                Target::User {
                    user,
                    guild: Some(guild),
                },
            ) => self.open_nickname_dialog(guild, user, window, cx),
            (
                Act::RemoveTimeout,
                Target::User {
                    user,
                    guild: Some(guild),
                },
            ) => self.open_remove_timeout(guild, user, window, cx),
            (Act::CreateEvent, Target::Guild(guild)) => {
                self.select(guild, None);
                self.open_guild_events(cx);
                self.open_create_event(window, cx);
            }
            (
                Act::Timeout | Act::Kick | Act::Ban,
                Target::User {
                    user,
                    guild: Some(guild),
                },
            ) => {
                let kind = match act {
                    Act::Timeout => super::member_actions::Kind::Timeout,
                    Act::Kick => super::member_actions::Kind::Kick,
                    _ => super::member_actions::Kind::Ban,
                };
                self.open_member_action(kind, guild, user, window, cx);
            }
            (
                Act::ServerMute | Act::ServerDeafen | Act::Disconnect,
                Target::User {
                    user,
                    guild: Some(guild),
                },
            ) => {
                let state = self
                    .stores
                    .voice_states
                    .get(&guild)
                    .and_then(|states| states.iter().find(|state| state.user_id == user))
                    .cloned();
                let request = match (act, state) {
                    (Act::ServerMute, Some(state)) => {
                        super::member_actions::member_patch(guild, user, "mute", json!(!state.mute))
                    }
                    (Act::ServerDeafen, Some(state)) => {
                        super::member_actions::member_patch(guild, user, "deaf", json!(!state.deaf))
                    }
                    (_, _) => super::member_actions::member_patch(
                        guild,
                        user,
                        "channel_id",
                        serde_json::Value::Null,
                    ),
                };
                let _ = self.requests.send(request);
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
            (Act::RemoveFriend, Target::User { user, .. }) => {
                self.confirm_remove_friend = Some(super::friends::RemoveFriend::Menu(user));
            }
            (Act::FriendNickname, Target::User { user, .. }) => {
                self.open_friend_nickname(user, window, cx)
            }
            (Act::FriendNickname, Target::Dm(channel)) => {
                if let Some(user) = self.dm_user(channel) {
                    self.open_friend_nickname(user, window, cx);
                }
            }
            (Act::RemoveFriend, Target::Dm(channel)) => {
                if let Some(user) = self.dm_user(channel) {
                    self.confirm_remove_friend = Some(super::friends::RemoveFriend::Menu(user));
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
            (Act::Correction(index), Target::Composer) => {
                let menu = self.context.composer.clone().unwrap_or_default();
                if let (Some((range, word)), Some(correction)) =
                    (menu.word, menu.suggestions.get(usize::from(index)))
                {
                    let correction = correction.clone();
                    self.composer.update(cx, |composer, cx| {
                        // The word must still stand where it was; else the occurrence nearest the caret is used.
                        let text = composer.text().to_owned();
                        let range = if text.get(range.clone()) == Some(word.as_str()) {
                            Some(range)
                        } else {
                            text.match_indices(word.as_str())
                                .map(|(at, _)| at..at + word.len())
                                .min_by_key(|found| {
                                    let caret = composer.collapsed_caret().unwrap_or(range.start);
                                    found.start.abs_diff(caret)
                                })
                        };
                        if let Some(range) = range {
                            composer.replace_range(range, &correction, cx);
                        }
                    });
                    window.focus(
                        &gpui::Focusable::focus_handle(self.composer.read(cx), cx),
                        cx,
                    );
                }
            }
            (Act::AddToDictionary, Target::Composer) => {
                if let Some((_, word)) = self.context.composer.as_ref().and_then(|m| m.word.clone())
                {
                    spellcheck::learn_word(&word);
                    self.refresh_spelling(cx);
                }
            }
            (Act::RemoveFromDictionary, Target::Composer) => {
                if let Some(word) = self
                    .context
                    .composer
                    .as_ref()
                    .and_then(|m| m.learned.clone())
                {
                    spellcheck::unlearn_word(&word);
                    self.refresh_spelling(cx);
                }
            }
            (Act::Spellcheck, Target::Composer) => {
                spellcheck::toggle();
                self.refresh_spelling(cx);
            }
            (Act::SpellLanguages, Target::Composer) => cx.open_url(SPELLCHECK_HELP),
            (Act::Cut, Target::Composer) => self
                .composer
                .update(cx, |composer, cx| composer.cut_selection(cx)),
            (Act::Copy, Target::Composer) => self
                .composer
                .update(cx, |composer, cx| composer.copy_selection(cx)),
            (Act::Paste, Target::Composer) => self
                .composer
                .update(cx, |composer, cx| composer.paste_clipboard(cx)),
            // Rows of a missing feature are drawn disabled (`Row::new`) and never reach here; any other pair is a row
            // built for a target it has no handler for, which the debug assertion below reports.
            (act, target) => {
                debug_assert!(
                    matches!(status(act), Status::Missing(_)),
                    "context row {act:?} has no handler for {target:?}"
                );
            }
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

    /// The store changed: the composer draws its underlines again (a learned word is gone from them at once).
    fn refresh_spelling(&mut self, cx: &mut Context<Self>) {
        self.composer.update(cx, |_, cx| cx.notify());
        cx.notify();
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

    /// The rows of a submenu: mute durations, notification levels, roles, voice channels or servers.
    fn context_sub_rows(&self, act: Act, target: Target) -> Vec<SubRow> {
        let plain = |label: &str, on: bool| SubRow {
            label: label.to_owned(),
            on,
            ..SubRow::default()
        };
        match act {
            Act::Roles => match target {
                Target::User {
                    user,
                    guild: Some(guild),
                } => self.role_sub_rows(guild, user),
                _ => Vec::new(),
            },
            Act::MoveMember => match target {
                Target::User {
                    user,
                    guild: Some(guild),
                } => self
                    .move_targets(guild, user)
                    .into_iter()
                    .filter_map(|id| {
                        let name = self
                            .stores
                            .guild(guild)?
                            .channels
                            .iter()
                            .find(|channel| channel.id == id)?
                            .name
                            .clone();
                        Some(SubRow {
                            label: name,
                            id,
                            ..SubRow::default()
                        })
                    })
                    .collect(),
                _ => Vec::new(),
            },
            Act::InviteToServer => self
                .invite_servers()
                .into_iter()
                .map(|(id, name, _)| SubRow {
                    label: name,
                    id,
                    ..SubRow::default()
                })
                .collect(),
            Act::Mute => MUTE_FOR
                .iter()
                .map(|(_, label)| plain(label, false))
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
                    rows.push(plain("Use Category Default", current == INHERIT));
                }
                rows.extend(
                    LEVELS
                        .iter()
                        .map(|(value, label)| plain(label, current == *value)),
                );
                rows
            }
            _ => Vec::new(),
        }
    }

    /// Servers we may create an invite in, with the channel to invite to (module 777933's invite list): a text
    /// channel we can see, in a server where we hold Create Invite.
    pub(super) fn invite_servers(&self) -> Vec<(Id, String, Id)> {
        self.stores
            .guilds
            .iter()
            .filter(|guild| self.my_permission(guild.id, None, permissions::CREATE_INSTANT_INVITE))
            .filter_map(|guild| {
                let channel = guild.channels.iter().find(|channel| {
                    channel.kind == ChannelKind::Text
                        && self.my_permission(
                            guild.id,
                            Some(channel),
                            permissions::CREATE_INSTANT_INVITE,
                        )
                })?;
                Some((guild.id, guild.name.clone(), channel.id))
            })
            .collect()
    }

    fn run_context_sub(&mut self, act: Act, target: Target, index: usize, cx: &mut Context<Self>) {
        let row = self.context_sub_rows(act, target).into_iter().nth(index);
        match act {
            Act::Roles => {
                let (
                    Some(row),
                    Target::User {
                        user,
                        guild: Some(guild),
                    },
                ) = (row, target)
                else {
                    return;
                };
                if row.plain || row.disabled {
                    return;
                }
                // Module 180798: the whole list goes back, with the role added or taken out.
                let mut roles: Vec<Id> = self
                    .stores
                    .members
                    .get(guild, user)
                    .map(|member| member.roles.clone())
                    .unwrap_or_default();
                if row.on {
                    roles.retain(|id| *id != row.id);
                } else {
                    roles.push(row.id);
                }
                let list: Vec<String> = roles.iter().map(ToString::to_string).collect();
                let _ = self.requests.send(super::member_actions::member_patch(
                    guild,
                    user,
                    "roles",
                    json!(list),
                ));
                return;
            }
            Act::MoveMember => {
                if let (
                    Some(row),
                    Target::User {
                        user,
                        guild: Some(guild),
                    },
                ) = (row, target)
                {
                    let _ = self.requests.send(super::member_actions::member_patch(
                        guild,
                        user,
                        "channel_id",
                        json!(row.id.to_string()),
                    ));
                }
                return;
            }
            Act::InviteToServer => {
                let Target::Dm(channel) = target else {
                    return;
                };
                let (Some(row), Some(user)) = (row, self.dm_user(channel)) else {
                    return;
                };
                self.send_server_invite(row.id, user);
                return;
            }
            _ => {}
        }
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
                .anchor(
                    if matches!(target, Target::InventoryFeed | Target::AppDirectory(_)) {
                        gpui::Anchor::TopRight
                    } else {
                        gpui::Anchor::TopLeft
                    },
                )
                .position(open.at)
                .snap_to_window_with_margin(px(8.))
                .child(
                    div()
                        .parity_with_border("context-menu", 1.)
                        .id("context-menu-generic")
                        .when(matches!(target, Target::InventoryAuthor { .. }), |d| {
                            d.min_w(px(144.))
                        })
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
                                if !in_sub
                                    && this.press_outside(super::keyboard::Layer::ContextMenu, cx)
                                {
                                    cx.stop_propagation();
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
                .map(|(index, sub)| {
                    let SubRow {
                        label,
                        on,
                        disabled,
                        plain,
                        dot,
                        ..
                    } = sub;
                    let role = dot.is_some();
                    let height = if role { 36. } else { 34. };
                    let leading = match dot {
                        Some(rgb) => Some(
                            div()
                                .size(px(20.))
                                .flex()
                                .items_center()
                                .justify_center()
                                .child(
                                    div()
                                        .size(px(12.))
                                        .rounded_full()
                                        .bg(gpui::rgb(if rgb == 0 { 0x99_aab5 } else { rgb })),
                                )
                                .into_any_element(),
                        ),
                        None => on
                            .then(|| icon("checkbox_check", 16., "--text-link").into_any_element()),
                    };
                    let mut item = self
                        .submenu_item(
                            SharedString::from(format!("context-sub-{index}")),
                            leading,
                            label,
                            height,
                        )
                        .opacity(if disabled { 0.5 } else { 1. });
                    if role {
                        // The role's check box (module 180798): brand and checked, or a faint empty square.
                        item = item.child(div().flex_1()).child(
                            div()
                                .size(px(20.))
                                .rounded(px(4.))
                                .border_1()
                                .flex()
                                .items_center()
                                .justify_center()
                                .border_color(if on {
                                    color("--checkbox-border-selected-default")
                                } else {
                                    color("--checkbox-border-default")
                                })
                                .bg(if on {
                                    color("--checkbox-background-selected-default")
                                } else {
                                    color("--checkbox-background-default")
                                })
                                .when(on, |d| d.child(icon("checkbox_check", 16., "--white"))),
                        );
                    }
                    if !plain && !disabled {
                        item = item.on_click(cx.listener(move |this, _, _, cx| {
                            this.run_context_sub(act, target, index, cx);
                            this.close_context_menu(cx);
                        }));
                    }
                    (item.into_any_element(), height)
                })
                .collect();
            Some(self.submenu_frame(rows, row, window))
        });
        Some(
            div()
                .absolute()
                .top(px(0.))
                .left(px(0.))
                .child(menu)
                .children(sub)
                .into_any_element(),
        )
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
        let label = gpui::StyledText::new(row.label.clone()).with_highlights(
            (act == Act::Mute && matches!(target, Target::Dm(_)))
                .then(|| row.label.find('@'))
                .flatten()
                .map(|start| {
                    (
                        start..row.label.len(),
                        gpui::HighlightStyle {
                            font_weight: Some(FontWeight(600.)),
                            ..Default::default()
                        },
                    )
                }),
        );
        self.menu_stop(
            div().id(SharedString::from(key.clone())),
            &key,
            if danger {
                color("--background-feedback-mod-critical")
            } else {
                color("--interactive-background-hover")
            },
            cx,
        )
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
            div().mr(px(8.)).child(icon(
                name,
                if matches!(
                    target,
                    Target::InventoryFeed | Target::InventoryFeedAtPointer
                ) {
                    20.
                } else {
                    18.
                },
                "--interactive-text-default",
            ))
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
                        .child(label),
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
        .children(row.shortcut.map(|keys| {
            div()
                .ml(px(8.))
                .flex_none()
                .text_size(px(14.))
                .line_height(px(18.))
                .font_weight(FontWeight(500.))
                .text_color(tint)
                .child(keys)
        }))
        .children(row.check.map(|on| {
            div()
                .ml(px(
                    if matches!(
                        target,
                        Target::InventoryFeed | Target::InventoryFeedAtPointer
                    ) {
                        8.
                    } else {
                        16.
                    },
                ))
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

    use super::{Act, Status, status};

    #[test]
    fn act_list_is_complete() {
        let mut seen = vec![false; Act::ALL.len()];
        for act in Act::ALL {
            let slot = &mut seen[act.ordinal()];
            assert!(!*slot, "{act:?} listed twice");
            *slot = true;
        }
        assert!(seen.iter().all(|seen| *seen));
    }

    #[test]
    fn missing_rows_are_disabled() {
        for act in Act::ALL {
            if let Status::Missing(_) = status(act) {
                assert!(
                    super::Row::new(act, "x").disabled,
                    "{act:?} would look alive"
                );
            }
        }
    }
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
    fn relationship_menu_actions_respect_friend_bot_self_and_blocked_state() {
        for dm in [None, Some(false)] {
            let mut facts = UserFacts {
                friend: true,
                dm,
                ..Default::default()
            };
            let items = user_items(&facts);
            assert!(labels(&items).contains(&"Add Friend Nickname".into()));
            facts.nickname = true;
            assert!(labels(&user_items(&facts)).contains(&"Change Friend Nickname".into()));
            let remove = items
                .iter()
                .position(|entry| matches!(entry, Entry::Row(row) if row.act == Act::RemoveFriend))
                .unwrap();
            let ignore = items
                .iter()
                .position(|entry| matches!(entry, Entry::Row(row) if row.act == Act::Ignore))
                .unwrap();
            assert!(remove < ignore);
            assert!(
                !items
                    .iter()
                    .any(|entry| matches!(entry, Entry::Row(row) if row.act == Act::AddFriend))
            );
            facts.bot = true;
            assert!(!user_items(&facts).iter().any(|entry| matches!(entry, Entry::Row(row) if matches!(row.act, Act::AddFriend | Act::RemoveFriend))));
            facts.bot = false;
            facts.me = true;
            assert!(!labels(&user_items(&facts)).contains(&"Remove Friend".into()));
            facts.me = false;
            facts.friend = false;
            facts.blocked = true;
            assert!(!labels(&user_items(&facts)).contains(&"Change Friend Nickname".into()));
            assert!(user_items(&facts).iter().any(|entry| matches!(entry, Entry::Row(row) if row.act == Act::AddFriend && row.disabled)));
            assert!(!labels(&user_items(&facts)).contains(&"Remove Friend".into()));
        }
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

    #[test]
    fn composer_menu_lists_corrections_dictionary_switch_then_clipboard() {
        let menu = ComposerMenu {
            word: Some((0..4, "helo".to_owned())),
            suggestions: ["hello", "halo", "helot", "hele", "help", "held"]
                .map(str::to_owned)
                .to_vec(),
            learned: None,
            selection: true,
            enabled: true,
        };
        assert_eq!(
            labels(&composer_items(&menu)),
            [
                "hello",
                "halo",
                "helot",
                "hele",
                "help",
                "-",
                "Add to Dictionary",
                "Spellcheck",
                "Languages",
                "-",
                "Cut",
                "Copy",
                "Paste"
            ]
        );
        // No misspelling under the click, a learned word in the text, the switch off, nothing selected.
        let menu = ComposerMenu {
            learned: Some("zorp".to_owned()),
            enabled: false,
            ..ComposerMenu::default()
        };
        assert_eq!(
            labels(&composer_items(&menu)),
            ["Remove from Dictionary", "Spellcheck", "-", "Paste"]
        );
    }
}
