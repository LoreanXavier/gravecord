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
    /// Not in the pinned client: the account panel (AccountPanelServerProfile, `plugin_hooks/account_panel.rs`).
    AccountPanel,
    /// Not in the pinned client: a role's group header in the member list (BetterRoleContext,
    /// `plugin_hooks/better_role_context.rs`).
    Role {
        role: Id,
        guild: Id,
    },
    /// Not in the pinned client: a PinDMs category header, by its place (`plugin_hooks/pin_dms.rs`).
    PinCategory(u16),
    /// Not in the pinned client: a GlobalBadges badge (`plugin_hooks/global_badges.rs` keeps which).
    PluginBadge,
    /// Not in the pinned client: an entry of PermissionsViewer's dialog, a role or a member
    /// (`plugin_hooks/permissions_viewer.rs`).
    PluginPermissionRole {
        role: Id,
        guild: Id,
    },
    PluginPermissionUser(Id),
    /// Not in the pinned client: the "Wave to say hi!" button of a join message (GreetStickerPicker,
    /// `plugin_hooks/greet_sticker_picker.rs`).
    Greet {
        channel: Id,
        message: Id,
    },
    InventoryFeed,
    InventoryFeedAtPointer,
    InventoryAuthor {
        user: Id,
    },
    SpotifyActivity {
        user: Id,
        popout: bool,
    },
    /// The "..." of a game's live card (module 285909 for a `PLAYING` activity); the game it showed is kept in
    /// `State::game_activity`.
    GameActivity {
        user: Id,
        popout: bool,
        embedded: bool,
    },
    Guild(Id),
    Channel(Id),
    Category(Id),
    Thread(Id),
    Dm(Id),
    /// A `#channel` mention in a message (`channel-mention-context`, module 223998).
    ChannelMention(Id),
    /// An attachment chip in a message (`attachment-link-context`, module 740024); its address and file name are kept
    /// in `State::attachment_link`.
    AttachmentLink,
    /// A forum tag (`forum-tag`, module 960015): Copy Tag ID in developer mode.
    ForumTag(Id),
    /// A role chip of a profile (`dev-context`, module 646938, opened by module 166005 `K`): Copy Role ID in
    /// developer mode.
    RoleChip(Id),
    /// The "+N" of an invite's roles in Server Settings > Invites (`invite-roles-menu`, module 551309): every role of
    /// the invite, kept in `State::invite_roles`.
    InviteRoles,
    User {
        user: Id,
        guild: Option<Id>,
    },
    /// The composer's text area (`textarea-context`, module 194400).
    Composer,
    /// The "..." of an application's page in the directory (module 586193 `l4`).
    AppDirectory(Id),
    /// The "..." of an app's page in the App Launcher (`app-details-more-menu` of module 621377).
    LauncherApp(Id),
    /// The "..." of a game's profile (`game-profile-context` of module 581745).
    GameProfile(Id),
    /// The game profile's Add to Profile menu.
    GameAddToProfile(Id),
    /// The Favorites server's star in the rail (module 132598): Thread Settings, Hide Favorites.
    Favorites,
    /// An emoji of the emoji picker (`expression-picker`, module 233503); the emoji is kept in
    /// `State::picker_emoji`.
    PickerEmoji,
    /// A sticker of the sticker picker (the same `expression-picker` menu).
    PickerSticker(Id),
    /// A soundboard sound of the picker or of a server's Soundboard page (`sound-button-context`, module 923819);
    /// the sound is kept in `State::picker_sound`.
    PickerSound,
    /// The Widget Actions button of a Board widget on someone else's profile (module 40989 `Y`): Report Widget.
    ProfileWidget {
        user: Id,
        widget: Id,
    },
    /// The voice panel's channel line (`rtc-channel`, module 576701).
    RtcChannel {
        guild: Id,
        channel: Id,
    },
    /// A channel of an Audit Log entry (`channel-context` of module 194195, from module 989433's
    /// `onChannelContextMenu` and `onTargetContextMenu`).
    AuditChannel(Id),
    /// A role or member of a channel's advanced permissions (`member-permissions-menu` / `role-permissions-menu`,
    /// module 495603).
    PermissionRow {
        channel: Id,
        target: Id,
        role: bool,
        everyone: bool,
    },
    /// The channel list's boost progress bar (`progress-bar-context`, module 709843).
    ProgressBar(Id),
    /// Actions of AutoMod's raid notification (`moderation-raid-context`, module 495372).
    RaidAlert(Id),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Act {
    InventoryAbout,
    InventoryHide,
    ViewAllActivity,
    ManageSpotifyPrivacy,
    ReportGameIssue,
    ViewGameProfile,
    ReportApp,
    ReportWidget,
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
    /// "Join Without Entrance Sound" (module 217563 `join-muted-custom-join-sound`).
    JoinQuiet,
    /// "Report Stage" (module 304694): a live Stage's report.
    ReportStage,
    HideNames,
    /// "Clear Channel Status" (module 671483 `clear-status`): `updateVoiceChannelStatus(id, "")`.
    ClearStatus,
    CloseThread,
    /// "Open Thread" / "Open Post" of an archived thread (module 182061 `unarchiveThread`).
    OpenThread,
    LockThread,
    /// "Unlock Thread" / "Unlock post" (module 707792 `unlockThread`).
    UnlockThread,
    /// "Pin Post" / "Unpin Post" of a forum post (module 41402).
    PinPost,
    UnpinPost,
    /// "Anyone can invite" of a private thread (module 707653 `toggle-thread-invitable`).
    ToggleInvitable,
    /// The voice panel channel menu (`rtc-channel`, module 576701): the input mode radios of module 1611.
    RtcVoiceActivity,
    RtcPushToTalk,
    /// Input Volume and Output Volume (modules 486487, 298242): the menu sliders.
    InputVolume,
    OutputVolume,
    /// Pop Out (`popout-call`): the call in its own window.
    PopOutCall,
    /// Show Voice Users (`show-voice-states`, the `debug.rtcPanelShowVoiceStates` setting).
    ShowVoiceUsers,
    LeaveThread,
    /// "Join Thread" (module 660273): `joinThread`, shown for a thread the account has not joined.
    JoinThread,
    Profile,
    Mention,
    Message,
    Call,
    /// 313998: "Ring" for a friend who is not in the call.
    RingUser,
    /// 313998: "Stop Ringing" for a friend the call is ringing.
    StopRinging,
    Note,
    Apps,
    AddFriend,
    RemoveFriend,
    FriendNickname,
    Ignore,
    Block,
    CloseDm,
    /// "Edit Group" and "Leave Group" of a group DM (module 4027).
    EditGroup,
    LeaveGroup,
    Invites,
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
    /// `report-raid` (module 495372): Report a Raid (module 935074).
    ReportRaid,
    /// `nav-server-settings` (module 495372): Server Settings on its Safety Setup page.
    ModerationSettings,
    /// `progress-bar-enabled` (module 709843): the boost progress bar's Enable Progress Bar checkbox.
    ToggleProgressBar,
    /// `remove-permission` (module 495603): Remove User / Remove Role of a channel permission row, after its confirm.
    RemovePermission,
    /// `server-lockdown` (module 664967): Security Actions, Edit Security Actions during a pause.
    SecurityActions,
    /// Voice: how loud a person is to us, and muting them (or their soundboard) here.
    Volume,
    VoiceMute,
    /// Our own row's Deafen (module 198229 `toggleSelfDeaf`).
    VoiceDeafen,
    SoundboardMute,
    /// Composer text menu: a spelling suggestion (by position), the dictionary rows, the switch, the help page, and
    /// the clipboard rows.
    Correction(u8),
    AddToDictionary,
    RemoveFromDictionary,
    Spellcheck,
    /// The composer menu's Suggestions submenu (Slash Commands).
    Suggestions,
    /// The composer menu's Send Message Button checkbox (`AccessibilityStore.isSubmitButtonEnabled`).
    SubmitButton,
    SpellLanguages,
    Cut,
    Copy,
    Paste,
    /// The Favorites server (module 132598): its rail menu and the menu of a channel inside it.
    GoToServer,
    FavoritesNickname,
    FavoritesSettings,
    HideFavorites,
    DisableVideo,
    StageSpeak,
    StageInvite,
    StageLeave,
    StageMoveToAudience,
    /// Favorite / Unfavorite Emoji of the picker's emoji (module 253925 `isInExpressionPicker`).
    FavoriteExpression,
    /// Copy Emoji: a unicode emoji's characters (developer mode).
    CopyEmoji,
    /// Copy Emoji Image Link: a custom emoji's picture address (developer mode).
    CopyExpressionLink,
    /// A game widget row of Add to Profile, by its place in the menu.
    GameWidget(u8),
    /// The composer's AI rows (staff only).
    AiFixGrammar,
    AiTranslate,
    AiTransforms,
    /// A picker sound's menu (`sound-button-context`, module 923819): Edit Sound, Set as Server / Global Entrance
    /// Sound, Download Sound.
    EditSound,
    EntranceServer,
    EntranceGlobal,
    DownloadSound,
    /// A row that only closes its menu (the roles of `invite-roles-menu`, module 551309: `onSelect` does nothing).
    Noop,
    /// Not in the pinned client: AccountPanelServerProfile's rows (`plugin_hooks/account_panel.rs`).
    PluginAlternateProfile,
    PluginPrioritizeServerProfile,
    /// Not in the pinned client: MuteChannelMentions' check (`plugin_hooks/mute_channel_mentions.rs`).
    PluginHideMentionBadge,
    /// Not in the pinned client: BetterRoleContext's rows (`plugin_hooks/better_role_context.rs`).
    PluginEditRole,
    PluginCopyRoleColor,
    PluginViewRoleIcon,
    PluginViewRoleMembers,
    /// Not in the pinned client: ViewIcons' rows (`plugin_hooks/view_icons.rs`).
    PluginViewAvatar,
    PluginViewServerAvatar,
    PluginViewDecoration,
    PluginViewIcon,
    PluginViewBanner,
    /// Not in the pinned client: ServerInfo's row (`plugin_hooks/server_info.rs`).
    PluginServerInfo,
    /// Not in the pinned client: BiggerStreamPreview's row (`plugin_hooks/bigger_stream_preview.rs`).
    PluginViewStreamPreview,
    /// Not in the pinned client: PinDMs' rows (`plugin_hooks/pin_dms.rs`).
    PluginPinDms,
    PluginPinEditCategory,
    PluginPinCategoryUp,
    PluginPinCategoryDown,
    PluginPinDeleteCategory,
    /// Not in the pinned client: NewGuildSettings' row (`plugin_hooks/new_guild_settings.rs`).
    PluginApplyNewGuildSettings,
    /// Not in the pinned client: GreetStickerPicker's rows: Message mode (true) or Greet, and a sticker by its place
    /// (`plugin_hooks/greet_sticker_picker.rs`).
    PluginGreetMode(bool),
    PluginGreetSticker(u8),
    /// Not in the pinned client: BypassStatus' Add / Remove Status Bypass (`plugin_hooks/bypass_status.rs`).
    PluginStatusBypass,
    /// Not in the pinned client: UserPFP's Set Avatar (`plugin_hooks/userpfp.rs`).
    PluginSetAvatar,
    /// Not in the pinned client: GlobalBadges' badge menu (`plugin_hooks/global_badges.rs`).
    PluginCopyBadgeName,
    PluginCopyBadgeLink,
    /// Not in the pinned client: PermissionsViewer's View Permissions (`plugin_hooks/permissions_viewer.rs`).
    PluginViewPermissions,
}

impl Act {
    /// Every row kind once (`Correction` with its first index). `ordinal` is an exhaustive match, so adding a variant
    /// without listing it here fails the build or the `act_list_is_complete` test.
    #[cfg(test)]
    pub(super) const ALL: [Act; 143] = [
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
        Act::JoinQuiet,
        Act::ReportStage,
        Act::HideNames,
        Act::ClearStatus,
        Act::CloseThread,
        Act::LockThread,
        Act::LeaveThread,
        Act::JoinThread,
        Act::Profile,
        Act::Mention,
        Act::Message,
        Act::Call,
        Act::RingUser,
        Act::StopRinging,
        Act::Note,
        Act::Apps,
        Act::AddFriend,
        Act::RemoveFriend,
        Act::FriendNickname,
        Act::Ignore,
        Act::Block,
        Act::CloseDm,
        Act::EditGroup,
        Act::LeaveGroup,
        Act::Invites,
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
        Act::GoToServer,
        Act::FavoritesNickname,
        Act::FavoritesSettings,
        Act::HideFavorites,
        Act::VoiceDeafen,
        Act::DisableVideo,
        Act::StageSpeak,
        Act::StageInvite,
        Act::StageLeave,
        Act::StageMoveToAudience,
        Act::ViewAllActivity,
        Act::ManageSpotifyPrivacy,
        Act::ReportGameIssue,
        Act::Suggestions,
        Act::SubmitButton,
        Act::ViewGameProfile,
        Act::FavoriteExpression,
        Act::CopyEmoji,
        Act::CopyExpressionLink,
        Act::GameWidget(0),
        Act::AiFixGrammar,
        Act::AiTranslate,
        Act::AiTransforms,
        Act::ReportWidget,
        Act::OpenThread,
        Act::UnlockThread,
        Act::PinPost,
        Act::UnpinPost,
        Act::ToggleInvitable,
        Act::RtcVoiceActivity,
        Act::RtcPushToTalk,
        Act::InputVolume,
        Act::OutputVolume,
        Act::PopOutCall,
        Act::ShowVoiceUsers,
        Act::EditSound,
        Act::EntranceServer,
        Act::EntranceGlobal,
        Act::DownloadSound,
        Act::Noop,
        Act::PluginAlternateProfile,
        Act::PluginPrioritizeServerProfile,
        Act::PluginHideMentionBadge,
        Act::PluginEditRole,
        Act::PluginCopyRoleColor,
        Act::PluginViewRoleIcon,
        Act::PluginViewRoleMembers,
        Act::PluginViewAvatar,
        Act::PluginViewServerAvatar,
        Act::PluginViewDecoration,
        Act::PluginViewIcon,
        Act::PluginViewBanner,
        Act::PluginServerInfo,
        Act::PluginViewStreamPreview,
        Act::SecurityActions,
        Act::RemovePermission,
        Act::ToggleProgressBar,
        Act::ModerationSettings,
        Act::ReportRaid,
        Act::PluginPinDms,
        Act::PluginPinEditCategory,
        Act::PluginPinCategoryUp,
        Act::PluginPinCategoryDown,
        Act::PluginPinDeleteCategory,
        Act::PluginApplyNewGuildSettings,
        Act::PluginGreetMode(false),
        Act::PluginGreetSticker(0),
        Act::PluginStatusBypass,
        Act::PluginSetAvatar,
        Act::PluginCopyBadgeName,
        Act::PluginCopyBadgeLink,
        Act::PluginViewPermissions,
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
            Act::RingUser => 67,
            Act::StopRinging => 68,
            Act::Note => 33,
            Act::Apps => 34,
            Act::AddFriend => 35,
            Act::RemoveFriend => 36,
            Act::FriendNickname => 37,
            Act::Ignore => 38,
            Act::Block => 39,
            Act::CloseDm => 40,
            Act::EditGroup => 69,
            Act::LeaveGroup => 70,
            Act::Invites => 76,
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
            Act::GoToServer => 71,
            Act::FavoritesNickname => 72,
            Act::FavoritesSettings => 73,
            Act::HideFavorites => 74,
            Act::VoiceDeafen => 75,
            Act::JoinThread => 77,
            Act::ClearStatus => 78,
            Act::DisableVideo => 79,
            Act::StageSpeak => 80,
            Act::StageInvite => 81,
            Act::StageLeave => 82,
            Act::StageMoveToAudience => 83,
            Act::ViewAllActivity => 84,
            Act::ManageSpotifyPrivacy => 85,
            Act::ReportGameIssue => 86,
            Act::Suggestions => 87,
            Act::SubmitButton => 88,
            Act::ViewGameProfile => 89,
            Act::FavoriteExpression => 90,
            Act::CopyEmoji => 91,
            Act::CopyExpressionLink => 92,
            Act::GameWidget(_) => 93,
            Act::AiFixGrammar => 94,
            Act::AiTranslate => 95,
            Act::AiTransforms => 96,
            Act::ReportWidget => 97,
            Act::OpenThread => 98,
            Act::UnlockThread => 99,
            Act::PinPost => 100,
            Act::UnpinPost => 101,
            Act::JoinQuiet => 102,
            Act::ReportStage => 103,
            Act::ToggleInvitable => 104,
            Act::RtcVoiceActivity => 105,
            Act::RtcPushToTalk => 106,
            Act::InputVolume => 107,
            Act::OutputVolume => 108,
            Act::PopOutCall => 109,
            Act::ShowVoiceUsers => 110,
            Act::EditSound => 111,
            Act::EntranceServer => 112,
            Act::EntranceGlobal => 113,
            Act::DownloadSound => 114,
            Act::Noop => 115,
            Act::PluginAlternateProfile => 116,
            Act::PluginPrioritizeServerProfile => 117,
            Act::PluginHideMentionBadge => 118,
            Act::PluginEditRole => 119,
            Act::PluginCopyRoleColor => 120,
            Act::PluginViewRoleIcon => 121,
            Act::PluginViewRoleMembers => 122,
            Act::PluginViewAvatar => 123,
            Act::PluginViewServerAvatar => 124,
            Act::PluginViewDecoration => 125,
            Act::PluginViewIcon => 126,
            Act::PluginViewBanner => 127,
            Act::PluginServerInfo => 128,
            Act::PluginViewStreamPreview => 129,
            Act::SecurityActions => 130,
            Act::RemovePermission => 131,
            Act::ToggleProgressBar => 132,
            Act::ModerationSettings => 133,
            Act::ReportRaid => 134,
            Act::PluginPinDms => 130,
            Act::PluginPinEditCategory => 131,
            Act::PluginPinCategoryUp => 132,
            Act::PluginPinCategoryDown => 133,
            Act::PluginPinDeleteCategory => 134,
            Act::PluginApplyNewGuildSettings => 135,
            Act::PluginGreetMode(_) => 136,
            Act::PluginGreetSticker(_) => 137,
            Act::PluginStatusBypass => 138,
            Act::PluginSetAvatar => 139,
            Act::PluginCopyBadgeName => 140,
            Act::PluginCopyBadgeLink => 141,
            Act::PluginViewPermissions => 142,
        }
    }
}

/// Whether a row's action exists in the app. A row whose feature is missing is drawn disabled (never a live-looking
/// row that does nothing); the name says what is missing. Every `Act` is classified here, so a new row cannot be added
/// without deciding it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Status {
    Wired,
}

pub(super) fn status(act: Act) -> Status {
    use Status::Wired;
    match act {
        Act::PinDm | Act::Favorite | Act::MoveTo | Act::PinChannel | Act::HideNames => Wired,
        Act::JoinQuiet | Act::ReportStage => Wired,
        Act::Apps => Wired,
        Act::ModView => Wired,
        Act::ReportRaid => Wired,
        Act::ModerationSettings => Wired,
        Act::ToggleProgressBar => Wired,
        Act::RemovePermission => Wired,
        Act::SecurityActions => Wired,
        Act::PluginAlternateProfile
        | Act::PluginPrioritizeServerProfile
        | Act::PluginHideMentionBadge
        | Act::PluginEditRole
        | Act::PluginCopyRoleColor
        | Act::PluginViewRoleIcon
        | Act::PluginViewRoleMembers
        | Act::PluginViewAvatar
        | Act::PluginViewServerAvatar
        | Act::PluginViewDecoration
        | Act::PluginViewIcon
        | Act::PluginViewBanner
        | Act::PluginServerInfo
        | Act::PluginViewStreamPreview
        | Act::PluginPinDms
        | Act::PluginPinEditCategory
        | Act::PluginPinCategoryUp
        | Act::PluginPinCategoryDown
        | Act::PluginPinDeleteCategory
        | Act::PluginApplyNewGuildSettings
        | Act::PluginGreetMode(_)
        | Act::PluginGreetSticker(_)
        | Act::PluginStatusBypass
        | Act::PluginSetAvatar
        | Act::PluginCopyBadgeName
        | Act::PluginCopyBadgeLink
        | Act::PluginViewPermissions => Wired,
        Act::InventoryAbout
        | Act::InventoryHide
        | Act::ViewAllActivity
        | Act::ManageSpotifyPrivacy
        | Act::ReportGameIssue
        | Act::ViewGameProfile
        | Act::FavoriteExpression
        | Act::CopyEmoji
        | Act::CopyExpressionLink
        | Act::GameWidget(_)
        | Act::AiFixGrammar
        | Act::AiTranslate
        | Act::AiTransforms
        | Act::ReportApp
        | Act::ReportWidget
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
        | Act::OpenThread
        | Act::LockThread
        | Act::UnlockThread
        | Act::PinPost
        | Act::UnpinPost
        | Act::ToggleInvitable
        | Act::LeaveThread
        | Act::JoinThread
        | Act::Profile
        | Act::Mention
        | Act::Message
        | Act::Call
        | Act::RingUser
        | Act::StopRinging
        | Act::Note
        | Act::AddFriend
        | Act::RemoveFriend
        | Act::FriendNickname
        | Act::Ignore
        | Act::Block
        | Act::CloseDm
        | Act::EditGroup
        | Act::Invites
        | Act::LeaveGroup
        | Act::Timeout
        | Act::Kick
        | Act::Ban
        | Act::ServerMute
        | Act::ServerDeafen
        | Act::Disconnect
        | Act::ClearStatus
        | Act::Verify
        | Act::Roles
        | Act::MoveMember
        | Act::ChangeNickname
        | Act::RemoveTimeout
        | Act::InviteToServer
        | Act::LeaveGuild
        | Act::Volume
        | Act::VoiceMute
        | Act::VoiceDeafen
        | Act::SoundboardMute
        | Act::DisableVideo
        | Act::StageSpeak
        | Act::StageInvite
        | Act::StageLeave
        | Act::StageMoveToAudience
        | Act::Correction(_)
        | Act::AddToDictionary
        | Act::RemoveFromDictionary
        | Act::Spellcheck
        | Act::SpellLanguages
        | Act::Suggestions
        | Act::SubmitButton
        | Act::Cut
        | Act::Copy
        | Act::Paste
        | Act::GoToServer
        | Act::FavoritesNickname
        | Act::FavoritesSettings
        | Act::HideFavorites => Wired,
        Act::RtcVoiceActivity
        | Act::RtcPushToTalk
        | Act::InputVolume
        | Act::OutputVolume
        | Act::PopOutCall
        | Act::ShowVoiceUsers
        | Act::EditSound
        | Act::EntranceServer
        | Act::EntranceGlobal
        | Act::DownloadSound
        | Act::Noop => Wired,
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
    /// A second line under the label (the level a notification choice inherits).
    pub sub: Option<String>,
    /// Drawn as a radio choice (notification levels) instead of a tick.
    pub radio: bool,
    /// Opens a nested list, as the "Category" row of a channel's Move to.
    pub caret: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct Row {
    pub act: Act,
    pub label: String,
    /// A second line under the label (Notification Settings shows the level).
    pub sub: Option<String>,
    pub icon: Option<&'static str>,
    /// An icon after the label (the `icon` of a menu item; `leadingAccessory` is the one before it).
    pub trailing: Option<&'static str>,
    pub caret: bool,
    pub check: Option<bool>,
    pub danger: bool,
    pub disabled: bool,
    /// The key combination shown at the right (`shortcut` of the menu item, module 165862).
    pub shortcut: Option<&'static str>,
    /// A `MenuRadioItem`: the check is drawn as a radio.
    pub radio: bool,
    /// The `roleDot` leading accessory (module 165862): the role's 12px circle in a 20px box, 8px before the label.
    pub role_dot: Option<u32>,
    /// Not in the pinned client: BypassStatus' 18px ring as the leading accessory and the icon (`plugin_hooks/
    /// bypass_status.rs`): the row's colour round a black centre when on, the danger colour round a white one when off.
    pub status_dot: Option<bool>,
}

/// BypassStatus' `EnabledIcon` / `DisabledIcon`: a 16px disc with a 7.5px one in its middle, in an 18px box.
fn status_dot(on: bool, tint: gpui::Hsla) -> gpui::Div {
    div()
        .size(px(18.))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .child(
            div()
                .size(px(16.))
                .rounded_full()
                .flex()
                .items_center()
                .justify_center()
                .bg(if on {
                    tint
                } else {
                    color("--status-danger").into()
                })
                .child(div().size(px(7.5)).rounded_full().bg(if on {
                    gpui::black()
                } else {
                    gpui::white()
                })),
        )
}

impl Row {
    pub(super) fn new(act: Act, label: impl Into<String>) -> Self {
        Self {
            act,
            label: label.into(),
            sub: None,
            icon: None,
            trailing: None,
            caret: false,
            check: None,
            danger: false,
            disabled: false,
            shortcut: None,
            radio: false,
            role_dot: None,
            status_dot: None,
        }
    }

    /// The `roleDot` accessory in the role's colour (0: `DEFAULT_ROLE_COLOR`).
    pub(super) fn role_dot(mut self, color: u32) -> Self {
        self.role_dot = Some(color);
        self
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

    pub(super) fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub(super) fn sub(mut self, sub: impl Into<String>) -> Self {
        self.sub = Some(sub.into());
        self
    }

    fn check(mut self, on: bool) -> Self {
        self.check = Some(on);
        self
    }

    pub(super) fn icon(mut self, name: &'static str) -> Self {
        self.icon = Some(name);
        self
    }

    pub(super) fn trailing(mut self, name: &'static str) -> Self {
        self.trailing = Some(name);
        self
    }

    /// Row heights measured in the captures: 34, 36 with an icon, caret or checkbox, 52 with a second line,
    /// 58 for the menu volume slider.
    pub fn height(&self) -> f32 {
        if matches!(self.act, Act::Volume | Act::InputVolume | Act::OutputVolume) {
            58.
        } else if self.sub.is_some() {
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
    /// Not in the pinned client's menus here: a group's label (`MenuGroup label`), for plugins' menus.
    Heading(&'static str),
}

/// A menu's height: its rows, 17px per separator, and the 8px padding and 1px border at the top and bottom.
/// A group label: 12/16 semibold muted text with 4px above and below and 8px at the sides.
const HEADING_HEIGHT: f32 = 24.;

fn menu_height(items: &[Entry]) -> f32 {
    items
        .iter()
        .map(|entry| match entry {
            Entry::Row(row) => row.height(),
            Entry::Separator => 17.,
            Entry::Heading(_) => HEADING_HEIGHT,
        })
        .sum::<f32>()
        + 18.
}

/// The layer's `autoInvert`: a menu that does not fit under the pointer opens above it, its bottom at the pointer, when
/// it fits there (the composer menu, `menus/composer-context-empty`); else it keeps the pointer and the window snaps it
/// 8px from the edge (the member menu).
fn flip_above(at: Point<Pixels>, height: f32, window_height: f32) -> Point<Pixels> {
    let y = f32::from(at.y);
    if y + height > window_height - 8. && y - height >= 8. {
        gpui::point(at.x, px(y - height))
    } else {
        at
    }
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

/// Live Spotify entries of 285909: the popout links to all activity; only our own card has privacy settings.
pub(super) fn spotify_activity_items(own: bool, popout: bool) -> Vec<Entry> {
    let mut items = Vec::new();
    if popout {
        items.push(Entry::Row(Row::new(
            Act::ViewAllActivity,
            dc_intl::tr("View All Activity"),
        )));
    }
    if own {
        items.push(Entry::Row(Row::new(
            Act::ManageSpotifyPrivacy,
            dc_intl::tr("Manage Privacy"),
        )));
    }
    items
}

/// Module 285909 for a live `PLAYING` activity: View All Activity in a popout (`pD1L1u`), Report Issue
/// (`game-detection-report-issue` of module 534465), then Manage Privacy on our own card when the game is not an
/// embedded activity (module 939075, Activity Privacy on desktop). View Game Profile (534465 `Ay`) comes after View
/// All Activity when the game's record says its profile opens.
pub(super) fn game_activity_items(
    own: bool,
    popout: bool,
    embedded: bool,
    profile: bool,
) -> Vec<Entry> {
    let mut items = Vec::new();
    if popout {
        items.push(Entry::Row(Row::new(
            Act::ViewAllActivity,
            dc_intl::tr("View All Activity"),
        )));
    }
    if profile {
        items.push(Entry::Row(Row::new(
            Act::ViewGameProfile,
            dc_intl::tr("View Game Profile"),
        )));
    }
    items.push(Entry::Row(Row::new(
        Act::ReportGameIssue,
        dc_intl::tr("Report Issue"),
    )));
    if own && !embedded {
        items.push(Entry::Row(Row::new(
            Act::ManageSpotifyPrivacy,
            dc_intl::tr("Manage Privacy"),
        )));
    }
    items
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
    /// The send button shows in the composer (`isSubmitButtonEnabled`).
    pub send_button: bool,
    /// A staff account with text selected: the AI rows (module 194400 `F`), and what is in flight.
    pub ai: bool,
    pub ai_busy: Option<super::composer_ai::Busy>,
}

/// `textarea-context` (module 194400) with the spelling section of module 413756: the corrections, then Add to
/// Dictionary (or Remove from Dictionary when the click is on no misspelling and the text holds a learned word), the
/// Spellcheck switch and Languages, then Cut and Copy with a selection, then Paste; above them, ungrouped, Suggestions (a
/// submenu with the Slash Commands checkbox) and the Send Message Button checkbox, which a chat composer always has
/// (`commands.enabled`, `submit.button`). The AI rows of the official menu are account features this client does not
/// have.
/// `m` in module 586193: Report App (danger, a flag), then Copy Application ID in its own group when developer mode is on
/// (`devmode-copy-id`, module 50268). The page's Copy Link button covers the wide layout, so the menu has no such row.
/// `Y` of module 40989 (`user-profile-widget-context-menu`): Report Widget, danger, with a flag.
pub(super) fn profile_widget_items() -> Vec<Entry> {
    vec![Entry::Row(
        Row::new(Act::ReportWidget, dc_intl::t!("D4GvHE"))
            .icon("menu_flag")
            .danger(),
    )]
}

pub(super) fn app_directory_items(developer: bool) -> Vec<Entry> {
    let mut out = vec![Entry::Row(
        Row::new(Act::ReportApp, dc_intl::tr("Report App"))
            .icon("menu_flag")
            .danger(),
    )];
    group(
        &mut out,
        if developer {
            vec![Row::new(Act::CopyId, dc_intl::tr("Copy Application ID")).icon("menu_id")]
        } else {
            Vec::new()
        },
    );
    out
}

/// `app-details-more-menu` of module 621377: Report App (danger), then Copy App ID (`+NP/b2`) in its own group in
/// developer mode. Open Store and Add App need the storefront and the install flow of the page, which this client does
/// not have yet.
pub(super) fn launcher_app_items(developer: bool) -> Vec<Entry> {
    let mut out = vec![Entry::Row(
        Row::new(Act::ReportApp, dc_intl::tr("Report App")).danger(),
    )];
    group(
        &mut out,
        if developer {
            vec![Row::new(Act::CopyId, dc_intl::tr("Copy App ID")).icon("menu_id")]
        } else {
            Vec::new()
        },
    );
    out
}

/// `game-profile-context` of module 581745: Report Issue (danger, `game-profile-something-wrong`), then Copy Game ID in
/// its own group in developer mode.
/// Module 233503: the favourite row (Favorite or Unfavorite Emoji, `None` when it cannot be a favourite), then in
/// developer mode Copy Emoji for a unicode emoji, or Copy Emoji ID and Copy Emoji Image Link for a custom one.
pub(super) fn picker_emoji_items(
    favorited: Option<bool>,
    custom: bool,
    developer: bool,
) -> Vec<Entry> {
    let mut out = Vec::new();
    match favorited {
        Some(true) => out.push(Entry::Row(
            Row::new(Act::FavoriteExpression, dc_intl::tr("Unfavorite Emoji"))
                .icon("clips_star_outline"),
        )),
        Some(false) => out.push(Entry::Row(
            Row::new(Act::FavoriteExpression, dc_intl::tr("Favorite Emoji")).icon("clips_star"),
        )),
        None => {}
    }
    if developer {
        if custom {
            out.push(Entry::Row(
                Row::new(Act::CopyId, dc_intl::tr("Copy Emoji ID")).icon("menu_id"),
            ));
            out.push(Entry::Row(
                Row::new(
                    Act::CopyExpressionLink,
                    dc_intl::tr("Copy Emoji Image Link"),
                )
                .icon("menu_copy_message_link"),
            ));
        } else {
            out.push(Entry::Row(Row::new(
                Act::CopyEmoji,
                dc_intl::tr("Copy Emoji"),
            )));
        }
    }
    out
}

/// Module 233503 for a sticker: the favourite row, then in developer mode Copy Sticker ID and Copy Sticker Image Link.
pub(super) fn picker_sticker_items(favorited: Option<bool>, developer: bool) -> Vec<Entry> {
    let mut out = Vec::new();
    match favorited {
        Some(true) => out.push(Entry::Row(
            Row::new(Act::FavoriteExpression, dc_intl::tr("Unfavorite Sticker"))
                .icon("clips_star_outline"),
        )),
        Some(false) => out.push(Entry::Row(
            Row::new(Act::FavoriteExpression, dc_intl::tr("Favorite Sticker")).icon("clips_star"),
        )),
        None => {}
    }
    if developer {
        out.push(Entry::Row(
            Row::new(Act::CopyId, dc_intl::tr("Copy Sticker ID")).icon("menu_id"),
        ));
        out.push(Entry::Row(
            Row::new(
                Act::CopyExpressionLink,
                dc_intl::tr("Copy Sticker Image Link"),
            )
            .icon("menu_copy_message_link"),
        ));
    }
    out
}

/// What decides a sound's `sound-button-context` rows (module 923819).
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct SoundFacts {
    /// `canManageGuildExpressions` of the sound's own server (`null` for the defaults).
    pub can_manage: bool,
    /// `canUseCustomCallSounds` (Nitro) and a call's server (`activeCallGuildId`).
    pub entrance: bool,
    /// A default sound (guild `0`): no Download Sound.
    pub default_sound: bool,
    pub developer: bool,
}

/// Module 923819 in one group: Edit Sound, the two entrance rows, Download Sound, Copy Sound ID (developer mode,
/// module 50268). No icons: the rows are plain `MenuItem`s.
pub(super) fn sound_items(facts: SoundFacts) -> Vec<Entry> {
    let mut out = Vec::new();
    if facts.can_manage {
        out.push(Entry::Row(Row::new(Act::EditSound, dc_intl::t("ponZcG"))));
    }
    if facts.entrance {
        out.push(Entry::Row(Row::new(
            Act::EntranceServer,
            dc_intl::t("p2hUt7"),
        )));
        out.push(Entry::Row(Row::new(
            Act::EntranceGlobal,
            dc_intl::t("/yA6Qd"),
        )));
    }
    if !facts.default_sound {
        out.push(Entry::Row(Row::new(
            Act::DownloadSound,
            dc_intl::t("/fzLLK"),
        )));
    }
    if facts.developer {
        out.push(Entry::Row(
            Row::new(Act::CopyId, dc_intl::t("HJikXp")).icon("menu_id"),
        ));
    }
    out
}

pub(super) fn game_profile_items(developer: bool) -> Vec<Entry> {
    let mut out = vec![Entry::Row(
        Row::new(Act::ReportGameIssue, dc_intl::tr("Report Issue"))
            .icon("shop_circle_error")
            .danger(),
    )];
    group(
        &mut out,
        if developer {
            vec![Row::new(Act::CopyId, dc_intl::tr("Copy Game ID")).icon("menu_id")]
        } else {
            Vec::new()
        },
    );
    out
}

pub(super) fn composer_items(menu: &ComposerMenu) -> Vec<Entry> {
    let mut out = vec![
        Entry::Row(Row::new(Act::Suggestions, dc_intl::tr("Suggestions")).caret()),
        Entry::Row(
            Row::new(Act::SubmitButton, dc_intl::tr("Send Message Button")).check(menu.send_button),
        ),
    ];
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
        spelling.push(Row::new(
            Act::AddToDictionary,
            dc_intl::tr("Add to Dictionary"),
        ));
    }
    if menu.learned.is_some() && menu.word.is_none() {
        spelling.push(Row::new(
            Act::RemoveFromDictionary,
            dc_intl::tr("Remove from Dictionary"),
        ));
    }
    spelling.push(Row::new(Act::Spellcheck, dc_intl::tr("Spellcheck")).check(menu.enabled));
    if menu.enabled {
        spelling.push(Row::new(Act::SpellLanguages, dc_intl::tr("Languages")));
    }
    if menu.ai {
        use super::composer_ai::Busy;
        let busy = menu.ai_busy.is_some();
        spelling.push(
            Row::new(
                Act::AiFixGrammar,
                dc_intl::tr(if menu.ai_busy == Some(Busy::Grammar) {
                    "Fixing Grammar"
                } else {
                    "Fix Grammar"
                }),
            )
            .trailing("magic_wand")
            .disabled(busy),
        );
        spelling.push(
            Row::new(
                Act::AiTranslate,
                dc_intl::tr(if matches!(menu.ai_busy, Some(Busy::Translate(_))) {
                    "Translating..."
                } else {
                    "Translate"
                }),
            )
            .caret()
            .disabled(busy),
        );
        spelling.push(
            Row::new(Act::AiTransforms, dc_intl::tr("Transforms"))
                .trailing("magic_wand")
                .caret(),
        );
    }
    group(&mut out, spelling);
    let mut clipboard = Vec::new();
    if menu.selection {
        clipboard.push(Row::new(Act::Cut, dc_intl::tr("Cut")).shortcut("Ctrl+X"));
        clipboard.push(Row::new(Act::Copy, dc_intl::tr("Copy")).shortcut("Ctrl+C"));
    }
    clipboard.push(Row::new(Act::Paste, dc_intl::tr("Paste")).shortcut("Ctrl+V"));
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
    /// A voice channel with a status that the manager, outside the channel, may clear (module 671483).
    pub clear_status: bool,
    /// Join Without Entrance Sound (module 217563): a voice or Stage channel the account may connect to and is not in,
    /// with an entrance sound set for this guild or everywhere.
    pub join_quiet: bool,
    /// Report Stage (module 304694): a Stage channel whose Stage is live.
    pub report_stage: bool,
}

pub(super) fn channel_items(facts: ChannelFacts, level: &str) -> Vec<Entry> {
    let mut out = vec![Entry::Row(
        Row::new(Act::MarkRead, dc_intl::tr("Mark As Read")).disabled(!facts.unread),
    )];
    group(
        &mut out,
        vec![Row::new(Act::Favorite, dc_intl::tr("Add to Favorites")).icon("clips_star")],
    );
    let mut share = Vec::new();
    if facts.can_invite {
        share.push(Row::new(
            Act::Invite,
            if facts.voice {
                dc_intl::tr("Invite to Voice")
            } else {
                dc_intl::tr("Invite to Channel")
            },
        ));
    }
    share.push(Row::new(Act::PinChannel, dc_intl::tr("Pin Channel to Top")));
    share.push(Row::new(Act::CopyLink, dc_intl::tr("Copy Link")));
    group(&mut out, share);
    if facts.voice {
        // `voice-actions` of module 428631: Join Without Entrance Sound, Open Chat, the status row, Hide Names, then
        // Report Stage.
        let mut voice = Vec::new();
        if facts.join_quiet {
            voice.push(Row::new(Act::JoinQuiet, dc_intl::t("saLMWc")));
        }
        voice.push(Row::new(Act::OpenChat, dc_intl::tr("Open Chat")));
        if facts.clear_status {
            voice.push(Row::new(
                Act::ClearStatus,
                dc_intl::tr("Clear Channel Status"),
            ));
        }
        voice.push(Row::new(Act::HideNames, dc_intl::tr("Hide Names")).check(facts.hide_names));
        if facts.report_stage {
            voice.push(
                Row::new(Act::ReportStage, dc_intl::t("JGj6Ci"))
                    .icon("menu_flag")
                    .danger(),
            );
        }
        group(&mut out, voice);
    }
    // A voice channel's menu has no Notification Settings (its capture, `voice-row`).
    let mut alerts = vec![Row::new(Act::Mute, dc_intl::tr("Mute Channel")).caret()];
    if !facts.voice {
        alerts.push(
            Row::new(Act::Notifications, dc_intl::tr("Notification Settings"))
                .sub(level)
                .caret(),
        );
    }
    group(&mut out, alerts);
    if facts.can_manage {
        group(
            &mut out,
            vec![Row::new(Act::MoveTo, dc_intl::tr("Move to...")).caret()],
        );
        group(
            &mut out,
            vec![
                Row::new(Act::EditChannel, dc_intl::tr("Edit Channel")),
                Row::new(Act::Duplicate, dc_intl::tr("Duplicate Channel")),
                Row::new(
                    Act::CreateChannel,
                    if facts.voice {
                        dc_intl::tr("Create Voice Channel")
                    } else {
                        dc_intl::tr("Create Text Channel")
                    },
                ),
                Row::new(Act::DeleteChannel, dc_intl::tr("Delete Channel")).danger(),
            ],
        );
    }
    if facts.developer {
        group(
            &mut out,
            vec![Row::new(Act::CopyId, dc_intl::tr("Copy Channel ID"))],
        );
    }
    tidy(out)
}

/// Module 113255 (`CHANNEL_ACTIVITY_FEED_VOICE_MENU`, "Channel Actions"): a voice channel's menu in Active Now. Edit
/// Channel; Invite to Voice, Duplicate Channel, Create Voice Channel; Delete Channel; Copy Channel ID (the hooks of
/// modules 317910, 475777, 533957, 477190, 307623 and 50268, each behind its own permission).
pub(super) fn activity_voice_items(
    can_manage: bool,
    can_invite: bool,
    developer: bool,
) -> Vec<Entry> {
    let mut out = Vec::new();
    if can_manage {
        group(
            &mut out,
            vec![Row::new(Act::EditChannel, dc_intl::tr("Edit Channel"))],
        );
    }
    let mut actions = Vec::new();
    if can_invite {
        actions.push(Row::new(Act::Invite, dc_intl::tr("Invite to Voice")));
    }
    if can_manage {
        actions.push(Row::new(Act::Duplicate, dc_intl::tr("Duplicate Channel")));
        actions.push(Row::new(
            Act::CreateChannel,
            dc_intl::tr("Create Voice Channel"),
        ));
    }
    group(&mut out, actions);
    if can_manage {
        group(
            &mut out,
            vec![Row::new(Act::DeleteChannel, dc_intl::tr("Delete Channel")).danger()],
        );
    }
    if developer {
        group(
            &mut out,
            vec![Row::new(Act::CopyId, dc_intl::tr("Copy Channel ID"))],
        );
    }
    tidy(out)
}

/// The menu of the Favorites star (frame menus2/favorites-rail-menu).
/// `channel-mention-context` (module 223998): Copy Link with the link icon, then the developer Copy Channel ID.
pub(super) fn channel_mention_items(developer: bool) -> Vec<Entry> {
    let mut out = vec![Entry::Row(
        Row::new(Act::CopyLink, dc_intl::tr("Copy Link")).icon("menu_copy_message_link"),
    )];
    if developer {
        group(
            &mut out,
            vec![Row::new(Act::CopyId, dc_intl::tr("Copy Channel ID")).icon("menu_id")],
        );
    }
    tidy(out)
}

/// The menu's accessible name (`aria-label` of its `Menu`): "Thread Actions" (module 949488, `thread-context`), "Link
/// Options" (module 740024) and "Forum Tag Actions" (module 960015).
pub(super) fn menu_aria_label(target: Target) -> Option<&'static str> {
    match target {
        Target::Thread(_) => Some(dc_intl::t("1NBjqb")),
        Target::AttachmentLink => Some(dc_intl::t("5oIOLX")),
        Target::ForumTag(_) => Some(dc_intl::t("AzTwiu")),
        // Module 646938: `aria-label` "Actions".
        Target::RoleChip(_) => Some(dc_intl::t("ogxXGq")),
        Target::PermissionRow { role: false, .. } => Some(dc_intl::t("pJMipx")),
        Target::PermissionRow { role: true, .. } => Some(dc_intl::t("ltedOh")),
        Target::ProgressBar(_) => Some(dc_intl::t("m85s/h")),
        Target::RaidAlert(_) => Some(dc_intl::t("liqwPJ")),
        _ => None,
    }
}

/// Module 740024 (Link Options): Copy Media Link, Copy Text.
pub(super) fn attachment_link_items() -> Vec<Entry> {
    vec![
        Entry::Row(Row::new(Act::CopyLink, dc_intl::t("92CPQ+"))),
        Entry::Row(Row::new(Act::Copy, dc_intl::t("JrGD7E"))),
    ]
}

/// Module 495603 ("Channel Permissions Member Actions" / "Channel Permissions Role Actions"): Remove User or Remove
/// Role (danger, the trash can) unless it is @everyone, then Copy User ID or Copy Role ID in developer mode.
pub(super) fn permission_target_items(role: bool, everyone: bool, developer: bool) -> Vec<Entry> {
    let mut out = Vec::new();
    if !everyone {
        let label = if role {
            dc_intl::t("T+3Adg")
        } else {
            dc_intl::t("hXfRwD")
        };
        out.push(Entry::Row(
            Row::new(Act::RemovePermission, label)
                .icon("channel_settings_trash")
                .danger(),
        ));
    }
    if developer {
        let label = if role {
            dc_intl::t("sMsaLg")
        } else {
            dc_intl::t("/AXYnE")
        };
        group(&mut out, vec![Row::new(Act::CopyId, label).icon("menu_id")]);
    }
    tidy(out)
}

/// Module 495372 ("User Settings Actions", `moderation-raid-context`): Security Actions, then Server Settings and
/// Report Raid.
pub(super) fn raid_alert_items() -> Vec<Entry> {
    let mut out = vec![Entry::Row(Row::new(
        Act::SecurityActions,
        dc_intl::t("UgXhdn"),
    ))];
    group(
        &mut out,
        vec![
            Row::new(Act::ModerationSettings, dc_intl::t("154/bL")),
            Row::new(Act::ReportRaid, dc_intl::t("cswId3")),
        ],
    );
    tidy(out)
}

/// Module 709843 ("Progress Bar Actions"): the one checkbox, Enable Progress Bar.
pub(super) fn progress_bar_items(enabled: bool) -> Vec<Entry> {
    vec![Entry::Row(
        Row::new(Act::ToggleProgressBar, dc_intl::t("0CJWP2")).check(enabled),
    )]
}

/// Module 960015 (Forum Tag Actions): Copy Tag ID, the only row; the menu opens in developer mode only.
pub(super) fn forum_tag_items() -> Vec<Entry> {
    vec![Entry::Row(
        Row::new(Act::CopyId, dc_intl::t("8VG6IY")).icon("menu_id"),
    )]
}

/// Module 50268 for a role (`devmode-copy-id-{id}`, "Copy Role ID" with the id icon), the only item of module 646938.
pub(super) fn role_chip_items() -> Vec<Entry> {
    vec![Entry::Row(
        Row::new(Act::CopyId, dc_intl::t("sMsaLg")).icon("menu_id"),
    )]
}

/// `channel-context` of module 194195 ("Channel Actions") for a channel of an Audit Log entry: Edit Channel (Edit
/// Category) for whoever may see it and manage channels, roles or webhooks (module 317910), Jump to Channel for a
/// channel that is not a category; Copy Channel ID in its own group in Developer Mode.
pub(super) fn audit_channel_items(category: bool, can_edit: bool, developer: bool) -> Vec<Entry> {
    let mut out = Vec::new();
    if can_edit {
        out.push(Entry::Row(Row::new(
            Act::EditChannel,
            if category {
                dc_intl::t("zdPFs9")
            } else {
                dc_intl::t("3gUsJb")
            },
        )));
    }
    if !category {
        out.push(Entry::Row(Row::new(Act::GoToServer, dc_intl::t("bQIM40"))));
    }
    if developer {
        group(
            &mut out,
            vec![Row::new(Act::CopyId, dc_intl::t("gFHI3k")).icon("menu_id")],
        );
    }
    tidy(out)
}

pub(super) fn favorites_items() -> Vec<Entry> {
    let mut out = vec![Entry::Row(
        Row::new(Act::FavoritesSettings, dc_intl::tr("Thread Settings")).icon("menu_create_thread"),
    )];
    group(
        &mut out,
        vec![
            Row::new(Act::HideFavorites, dc_intl::tr("Hide Favorites"))
                .icon("eye_slash")
                .danger(),
        ],
    );
    tidy(out)
}

/// The menu of a channel inside the Favorites server (frame menus2/favorites-channel-menu): the unread mark, the way
/// back to its own server, the favourite's nickname, then the usual link, mute and notification rows, and the removal.
pub(super) fn favorite_channel_items(unread: bool, level: &str) -> Vec<Entry> {
    let mut out = vec![Entry::Row(
        Row::new(Act::MarkRead, dc_intl::tr("Mark As Read")).disabled(!unread),
    )];
    group(
        &mut out,
        vec![
            Row::new(Act::GoToServer, dc_intl::tr("Go to Server")),
            Row::new(Act::FavoritesNickname, dc_intl::tr("Change Nickname")),
        ],
    );
    group(
        &mut out,
        vec![Row::new(Act::CopyLink, dc_intl::tr("Copy Link"))],
    );
    group(
        &mut out,
        vec![
            Row::new(Act::Mute, dc_intl::tr("Mute Channel")).caret(),
            Row::new(Act::Notifications, dc_intl::tr("Notification Settings"))
                .sub(level)
                .caret(),
        ],
    );
    group(
        &mut out,
        vec![Row::new(Act::Favorite, dc_intl::tr("Remove from Favorites")).danger()],
    );
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
        Row::new(Act::MarkRead, dc_intl::tr("Mark As Read")).disabled(!unread),
    )];
    group(
        &mut out,
        vec![
            Row::new(Act::CollapseCategory, dc_intl::tr("Collapse Category")).check(collapsed),
            Row::new(Act::CollapseAll, dc_intl::tr("Collapse All Categories")),
        ],
    );
    group(
        &mut out,
        vec![
            Row::new(Act::Mute, dc_intl::tr("Mute Category")).caret(),
            Row::new(Act::Notifications, dc_intl::tr("Notification Settings"))
                .sub(level)
                .caret(),
        ],
    );
    if can_manage {
        group(
            &mut out,
            vec![Row::new(Act::MoveTo, dc_intl::tr("Move to...")).caret()],
        );
        group(
            &mut out,
            vec![
                Row::new(Act::EditChannel, dc_intl::tr("Edit Category")),
                Row::new(Act::DeleteChannel, dc_intl::tr("Delete Category")).danger(),
            ],
        );
    }
    if developer {
        group(
            &mut out,
            vec![Row::new(Act::CopyId, dc_intl::tr("Copy Category ID"))],
        );
    }
    tidy(out)
}

/// What the thread menu (`thread-context`, module 949488) depends on.
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct ThreadFacts {
    pub unread: bool,
    /// MANAGE_THREADS in the parent.
    pub manage: bool,
    pub owner: bool,
    pub joined: bool,
    pub developer: bool,
    pub archived: bool,
    pub locked: bool,
    /// The parent is a forum or media channel (`isForumPost`): the rows say "Post" instead of "Thread".
    pub forum_post: bool,
    /// The post carries the PINNED flag (`1 << 1`).
    pub pinned: bool,
    /// SEND_MESSAGES and SEND_MESSAGES_IN_THREADS in the thread and its parent (`L` of module 406704).
    pub can_send: bool,
    /// A post of a media channel (`isMediaThread`): it never offers Open.
    pub media: bool,
    /// A private thread (type 12) and its `thread_metadata.invitable`.
    pub private_thread: bool,
    pub invitable: bool,
}

/// `lK` of module 406704 (`R(thread, canSend, manage)`): an archived thread may be opened again by whoever can send in
/// it, a locked one only with MANAGE_THREADS; never a media post.
fn can_unarchive(facts: ThreadFacts) -> bool {
    !facts.media
        && if facts.locked {
            facts.manage
        } else {
            facts.can_send
        }
}

/// `label` for a thread, or `post` for a forum post (`isForumPost()` of the thread menu's rows).
fn post_or(facts: ThreadFacts, post: &str, thread: &str) -> String {
    if facts.forum_post { post } else { thread }.to_owned()
}

pub(super) fn thread_items(facts: ThreadFacts, level: &str) -> Vec<Entry> {
    let ThreadFacts {
        unread,
        manage,
        owner,
        joined,
        developer,
        ..
    } = facts;
    let mut out = vec![Entry::Row(
        Row::new(Act::MarkRead, dc_intl::tr("Mark As Read")).disabled(!unread),
    )];
    group(
        &mut out,
        vec![Row::new(Act::Favorite, dc_intl::tr("Add to Favorites")).icon("clips_star")],
    );
    let mut own = Vec::new();
    if !joined {
        // Module 660273: "Follow Post" for a forum post.
        own.push(Row::new(
            Act::JoinThread,
            post_or(
                facts,
                dc_intl::tr("Follow Post"),
                dc_intl::tr("Join Thread"),
            ),
        ));
    }
    // Module 182061: an archived thread opens again (`unarchiveThread`, gated by `lK`), an active one closes
    // (`archiveThread`, gated by `NI`: Manage Threads or the thread's owner).
    if facts.archived && can_unarchive(facts) {
        own.push(Row::new(
            Act::OpenThread,
            post_or(facts, dc_intl::tr("Open Post"), dc_intl::tr("Open Thread")),
        ));
    } else if !facts.archived && (manage || owner) {
        own.push(Row::new(
            Act::CloseThread,
            post_or(
                facts,
                dc_intl::tr("Close Post"),
                dc_intl::tr("Close Thread"),
            ),
        ));
    }
    if manage {
        // Module 707792: Lock or Unlock by `isLockedThread()`.
        own.push(if facts.locked {
            Row::new(
                Act::UnlockThread,
                post_or(
                    facts,
                    dc_intl::tr("Unlock post"),
                    dc_intl::tr("Unlock Thread"),
                ),
            )
        } else {
            Row::new(
                Act::LockThread,
                post_or(facts, dc_intl::tr("Lock Post"), dc_intl::tr("Lock Thread")),
            )
        });
    }
    if manage || owner {
        // Module 200273.
        own.push(Row::new(
            Act::EditChannel,
            post_or(facts, dc_intl::tr("Edit Post"), dc_intl::tr("Edit Thread")),
        ));
    }
    own.push(Row::new(Act::CopyLink, dc_intl::tr("Copy Link")));
    group(&mut out, own);
    let mute = post_or(facts, dc_intl::tr("Mute Post"), dc_intl::tr("Mute Thread"));
    group(
        &mut out,
        vec![
            // `mute-disabled` (module 57907) until the account has joined the thread.
            if joined {
                Row::new(Act::Mute, mute).caret()
            } else {
                Row::new(Act::Mute, mute).disabled(true)
            },
            Row::new(Act::Notifications, dc_intl::tr("Notification Settings"))
                .sub(level)
                .caret(),
        ],
    );
    // `admin-actions`: Anyone can invite (module 707653: a private thread, with MANAGE_THREADS), Pin Post (module
    // 41402: a post of a forum, with MANAGE_THREADS), then Delete (307623).
    let mut admin = Vec::new();
    if facts.private_thread && manage {
        admin.push(
            Row::new(Act::ToggleInvitable, dc_intl::tr("Anyone can invite")).check(facts.invitable),
        );
    }
    if facts.forum_post && manage {
        admin.push(if facts.pinned {
            Row::new(Act::UnpinPost, dc_intl::tr("Unpin Post"))
        } else {
            Row::new(Act::PinPost, dc_intl::tr("Pin Post"))
        });
    }
    if manage || owner {
        admin.push(
            Row::new(
                Act::DeleteChannel,
                post_or(
                    facts,
                    dc_intl::tr("Delete Post"),
                    dc_intl::tr("Delete Thread"),
                ),
            )
            .danger(),
        );
    }
    group(&mut out, admin);
    if joined {
        // Module 804006: "Unfollow Post" for a forum post.
        group(
            &mut out,
            vec![Row::new(
                Act::LeaveThread,
                post_or(
                    facts,
                    dc_intl::tr("Unfollow Post"),
                    dc_intl::tr("Leave Thread"),
                ),
            )],
        );
    }
    if developer {
        group(
            &mut out,
            vec![Row::new(Act::CopyId, dc_intl::tr("Copy Thread ID"))],
        );
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
    /// The `server-lockdown` item of module 664967: `Some(lockdown)` when shown.
    pub security_actions: Option<bool>,
}

pub(super) fn guild_items(facts: GuildFacts, level: &str) -> Vec<Entry> {
    let mut out = vec![Entry::Row(
        Row::new(Act::MarkRead, dc_intl::tr("Mark As Read")).disabled(!facts.unread),
    )];
    if facts.can_invite {
        group(
            &mut out,
            vec![Row::new(Act::Invite, dc_intl::tr("Invite to Server"))],
        );
    }
    group(
        &mut out,
        vec![
            Row::new(Act::Mute, dc_intl::tr("Mute Server")).caret(),
            Row::new(Act::Notifications, dc_intl::tr("Notification Settings"))
                .sub(level)
                .caret(),
            Row::new(Act::HideMuted, dc_intl::tr("Hide Muted Channels")).check(facts.hide_muted),
        ],
    );
    let mut settings = Vec::new();
    if facts.can_manage_guild {
        settings.push(Row::new(Act::ServerSettings, dc_intl::tr("Server Settings")).caret());
    }
    settings.push(Row::new(
        Act::PrivacySettings,
        dc_intl::tr("Privacy Settings"),
    ));
    settings.push(Row::new(
        Act::EditProfile,
        dc_intl::tr("Edit Per-server Profile"),
    ));
    group(&mut out, settings);
    if facts.can_manage_channels {
        group(
            &mut out,
            vec![
                Row::new(Act::CreateChannel, dc_intl::tr("Create Channel")),
                Row::new(Act::CreateCategory, dc_intl::tr("Create Category")),
                Row::new(Act::CreateEvent, dc_intl::tr("Create Event")),
            ],
        );
    }
    group(
        &mut out,
        vec![Row::new(Act::MoveTo, dc_intl::tr("Move to...")).caret()],
    );
    // Module 871638's last group: `server-lockdown` (module 664967, no icon here) and Leave Server.
    let mut last = Vec::new();
    if let Some(lockdown) = facts.security_actions {
        let label = if lockdown {
            dc_intl::t("+tSVi3")
        } else {
            dc_intl::t("EPlEdu")
        };
        last.push(Row::new(Act::SecurityActions, label).danger());
    }
    if !facts.owner {
        last.push(Row::new(Act::LeaveGuild, dc_intl::tr("Leave Server")).danger());
    }
    group(&mut out, last);
    if facts.developer {
        group(
            &mut out,
            vec![Row::new(Act::CopyId, dc_intl::tr("Copy Server ID"))],
        );
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
    /// "Open in Mod View" is offered (module 121127 `P`, module 985925 `q`): any moderation permission in the server.
    pub mod_view: bool,
    pub developer: bool,
    /// `Some(unread)` in a DM row, which adds the channel entries.
    pub dm: Option<bool>,
    pub invite_servers: bool,
    /// 313998: `Some(false)` offers Ring, `Some(true)` offers Stop Ringing, `None` neither.
    pub ring: Option<bool>,
    /// In the call we are in (never true unless a call is open): adds the voice rows.
    pub in_call: bool,
    /// Our own row with the media items (module 198229): `Some((muted, deafened))` offers Mute and Deafen.
    pub self_voice: Option<(bool, bool)>,
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
    pub video_disabled: bool,
    /// Module 949674: hide `Start a Call` when already in a 1-on-1 DM voice call with this user, or in a popout window.
    pub hide_call: bool,
    /// Module 385913 / 438653: the DM channel is already in Favorites (`Remove from Favorites` moves to a danger group near the end).
    pub dm_favorite: bool,
    /// Module 75139 / 107632: `showChatItems: false` omits `Mention`.
    pub omit_mention: bool,
    /// Module 107632 Group 1 (`106401` / `237749`): Stage speaker/audience actions before `Profile`.
    pub stage_acts: Vec<Act>,
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

/// The width of a 14px medium label.
fn label_width(window: &Window, text: &str) -> f32 {
    let style = window.text_style();
    let mut font = style.font();
    font.weight = FontWeight(500.);
    let run = gpui::TextRun {
        len: text.len(),
        font,
        color: style.color,
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    f32::from(
        window
            .text_system()
            .shape_line(SharedString::from(text.to_owned()), px(14.), &[run], None)
            .width,
    )
}

/// The menu of a group DM (module 4027 `f`): mark as read, pin and favourite, Edit Group, mute, Leave Group, and the
/// channel id for developers. Invites (owner only) and Apps are not offered yet.
pub(super) fn group_items(unread: bool, owner: bool, developer: bool) -> Vec<Entry> {
    let mut out = vec![Entry::Row(
        Row::new(Act::MarkRead, dc_intl::tr("Mark As Read")).disabled(!unread),
    )];
    group(
        &mut out,
        vec![
            Row::new(Act::PinDm, dc_intl::t!("uKw3A+")).icon("menu_pin_message"),
            Row::new(Act::Favorite, dc_intl::tr("Add to Favorites")).icon("clips_star"),
        ],
    );
    let mut manage = Vec::new();
    // The group's owner sees Invites above Edit Group (module 4027).
    if owner {
        manage.push(Row::new(Act::Invites, dc_intl::tr("Invites")));
    }
    manage.push(Row::new(Act::EditGroup, dc_intl::tr("Edit Group")));
    group(&mut out, manage);
    group(
        &mut out,
        vec![Row::new(Act::Mute, dc_intl::tr("Mute Conversation")).caret()],
    );
    group(
        &mut out,
        vec![Row::new(Act::LeaveGroup, dc_intl::tr("Leave Group")).danger()],
    );
    if developer {
        group(
            &mut out,
            vec![Row::new(Act::CopyId, dc_intl::tr("Copy Channel ID"))],
        );
    }
    tidy(out)
}

pub(super) fn user_items(facts: &UserFacts) -> Vec<Entry> {
    let mut out: Vec<Entry> = Vec::new();
    if let Some(unread) = facts.dm {
        out.push(Entry::Row(
            Row::new(Act::MarkRead, dc_intl::tr("Mark As Read")).disabled(!unread),
        ));
        let mut pins = vec![Row::new(Act::PinDm, dc_intl::t!("uKw3A+")).icon("menu_pin_message")];
        if !facts.me && !facts.dm_favorite {
            pins.push(Row::new(Act::Favorite, dc_intl::tr("Add to Favorites")).icon("clips_star"));
        }
        group(&mut out, pins);
    }
    if !facts.stage_acts.is_empty() {
        let stage_rows: Vec<Row> = facts
            .stage_acts
            .iter()
            .filter_map(|act| match act {
                Act::StageSpeak => Some(Row::new(Act::StageSpeak, dc_intl::tr("Speak on Stage"))),
                Act::StageInvite => {
                    Some(Row::new(Act::StageInvite, dc_intl::tr("Invite to Speak")))
                }
                Act::StageLeave => Some(Row::new(Act::StageLeave, dc_intl::tr("Leave the Stage"))),
                Act::StageMoveToAudience => Some(Row::new(
                    Act::StageMoveToAudience,
                    dc_intl::tr("Move to Audience"),
                )),
                _ => None,
            })
            .collect();
        group(&mut out, stage_rows);
    }
    let mut first = vec![Row::new(Act::Profile, dc_intl::tr("Profile"))];
    if facts.dm.is_none() {
        if !facts.omit_mention {
            first.push(Row::new(Act::Mention, dc_intl::tr("Mention")));
        }
        if !facts.me {
            first.push(Row::new(Act::Message, dc_intl::t!("OAJQlP")));
        }
    }
    if !facts.me {
        if !facts.bot && !facts.hide_call {
            first.push(Row::new(Act::Call, dc_intl::tr("Start a Call")).disabled(facts.blocked));
        }
        match facts.ring {
            Some(false) => first.push(Row::new(
                Act::RingUser,
                if facts.in_guild {
                    dc_intl::t!("3Hv9qQ")
                } else {
                    dc_intl::t!("RDheZm")
                },
            )),
            Some(true) => first.push(Row::new(Act::StopRinging, dc_intl::t!("ygslb0"))),
            None => {}
        }
        first.push(
            Row::new(Act::Note, dc_intl::tr("Add Note")).sub(dc_intl::tr("Only visible to you")),
        );
        if facts.friend {
            first.push(Row::new(
                Act::FriendNickname,
                if facts.nickname {
                    dc_intl::tr("Change Friend Nickname")
                } else {
                    dc_intl::tr("Add Friend Nickname")
                },
            ));
        }
    }
    if facts.dm.is_some() {
        first.push(Row::new(Act::CloseDm, dc_intl::tr("Close DM")));
    }
    group(&mut out, first);
    if let Some((muted, deafened)) = facts.self_voice {
        // Module 198229: our own row has the two self switches, their checks following the media engine.
        out.push(Entry::Separator);
        out.push(Entry::Row(
            Row::new(Act::VoiceMute, dc_intl::tr("Mute")).check(muted),
        ));
        out.push(Entry::Row(
            Row::new(Act::VoiceDeafen, dc_intl::tr("Deafen")).check(deafened),
        ));
    }
    if facts.in_call && !facts.me {
        // Module 198229: User Volume, Mute, Mute Soundboard (checkbox), Disable Video (checkbox).
        let mut rows = vec![
            Row::new(Act::Volume, dc_intl::t!("m7TNdF")),
            Row::new(Act::VoiceMute, dc_intl::tr("Mute")).check(facts.voice_muted),
            Row::new(Act::SoundboardMute, dc_intl::tr("Mute Soundboard"))
                .check(facts.soundboard_muted),
            Row::new(Act::DisableVideo, dc_intl::tr("Disable Video")).check(facts.video_disabled),
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
                dc_intl::tr("Edit Per-server Profile")
            } else {
                dc_intl::tr("Change Nickname")
            },
        ));
    }
    mid.push(Row::new(Act::Apps, dc_intl::tr("Apps")).caret());
    if facts.invite_servers && !facts.me {
        mid.push(Row::new(Act::InviteToServer, dc_intl::tr("Invite to Server")).caret());
    }
    if !facts.me {
        if !facts.bot {
            mid.push(if facts.friend {
                Row::new(Act::RemoveFriend, dc_intl::tr("Remove Friend"))
            } else {
                Row::new(Act::AddFriend, dc_intl::tr("Add Friend")).disabled(facts.blocked)
            });
        }
        mid.push(Row::new(
            Act::Ignore,
            if facts.ignored {
                dc_intl::t!("8wXU9B")
            } else {
                dc_intl::t!("ytCpKs")
            },
        ));
        // Module 239211: "Block" is critical/danger; "Unblock" is default color.
        mid.push(if facts.blocked {
            Row::new(Act::Block, dc_intl::tr("Unblock"))
        } else {
            Row::new(Act::Block, dc_intl::tr("Block")).danger()
        });
    }
    if facts.dm.is_some() {
        group(&mut out, mid);
    } else if facts.me {
        // The own menu has no separator before the nickname row only when nothing precedes it; the two self switches
        // of a voice row sit in the same group as the profile row (golden voice-user-menu/self-menu).
        if facts.self_voice.is_none() {
            out.push(Entry::Separator);
        }
        out.extend(mid.into_iter().map(Entry::Row));
    } else {
        out.push(Entry::Separator);
        out.extend(mid.into_iter().map(Entry::Row));
    }
    if facts.dm.is_none() && facts.in_guild {
        // Module 107632: Roles (180798) and Move to (252726) share a group before the moderation one.
        let mut place = Vec::new();
        if facts.roles {
            place.push(Row::new(Act::Roles, dc_intl::tr("Roles")).caret());
        }
        if facts.server_voice.is_some_and(|voice| voice.can_move) {
            place.push(Row::new(Act::MoveMember, dc_intl::tr("Move to")).caret());
        }
        group(&mut out, place);
    }
    if facts.dm.is_none() && facts.in_guild {
        let mut moderation = Vec::new();
        if facts.mod_view {
            moderation.push(Row::new(Act::ModView, dc_intl::tr("Open in Mod View")));
        }
        if !facts.me
            && let Some(verified) = facts.verify
        {
            moderation.push(Row::new(
                Act::Verify,
                if verified {
                    dc_intl::tr("Unverify Member")
                } else {
                    dc_intl::tr("Verify Member")
                },
            ));
        }
        // Module 291247 lists the voice rows between Verify and Timeout, in the same group; Server Mute and Server
        // Deafen are offered on our own row too (the voice-user capture), Disconnect never.
        {
            if let Some(voice) = facts.server_voice {
                if voice.can_mute {
                    moderation.push(
                        Row::new(Act::ServerMute, dc_intl::tr("Server Mute"))
                            .check(voice.mute)
                            .danger(),
                    );
                }
                if voice.can_deafen {
                    moderation.push(
                        Row::new(Act::ServerDeafen, dc_intl::tr("Server Deafen"))
                            .check(voice.deaf)
                            .danger(),
                    );
                }
                if voice.can_disconnect {
                    moderation.push(Row::new(Act::Disconnect, dc_intl::tr("Disconnect")).danger());
                }
            }
        }
        if !facts.me {
            if facts.can_moderate {
                moderation.push(if facts.in_timeout {
                    Row::new(
                        Act::RemoveTimeout,
                        dc_intl::t!("csKeta", user = &facts.name),
                    )
                    .danger()
                } else {
                    Row::new(Act::Timeout, dc_intl::t!("OhsOy0", user = &facts.name)).danger()
                });
            }
            if facts.can_kick {
                moderation
                    .push(Row::new(Act::Kick, dc_intl::t!("9l/iTS", user = &facts.name)).danger());
            }
            if facts.can_ban {
                moderation
                    .push(Row::new(Act::Ban, dc_intl::t!("WnpUBi", user = &facts.name)).danger());
            }
        }
        group(&mut out, moderation);
    }
    if let Some(_unread) = facts.dm {
        group(
            &mut out,
            // Module 995102: "Mute **{name}**" (`byjuJm`) with the name as `@name`.
            vec![
                Row::new(
                    Act::Mute,
                    dc_intl::t!("byjuJm", name = format!("@{}", facts.name)),
                )
                .caret(),
            ],
        );
        if !facts.me && facts.dm_favorite {
            group(
                &mut out,
                vec![Row::new(Act::Favorite, dc_intl::tr("Remove from Favorites")).danger()],
            );
        }
    }
    if facts.developer {
        group(
            &mut out,
            vec![Row::new(Act::CopyId, dc_intl::tr("Copy User ID"))],
        );
    }
    tidy(out)
}

pub(super) struct Open {
    pub target: Target,
    pub at: Point<Pixels>,
    pub items: Vec<Entry>,
    /// The open submenu: the row it hangs off, and where that row is.
    pub sub: Option<(Act, gpui::Bounds<Pixels>)>,
    /// The nested list open from a submenu row (a channel's Move to > Category): the row's index and where it is.
    pub nested: Option<(usize, gpui::Bounds<Pixels>)>,
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
    /// The next menu opens from a person's row in a voice channel's list: it shows the media items (module 340851
    /// `showMediaItems: true`), whether or not we are in that call.
    pub voice_row: bool,
    /// Module 107632 `showStageChannelItems: true` (Stage speaker or audience tile).
    pub stage_channel_items: bool,
    /// The next channel menu is Active Now's voice channel menu (module 113255).
    pub activity_voice: bool,
    /// Module 107632 `showChatItems: false` (Stage audience tile).
    pub omit_mention: bool,
    /// The game of the card whose "..." is open (`detected` of the Report Issue dialog).
    pub game_activity: Option<super::report_issue::Detected>,
    /// The picker's emoji whose menu is open.
    pub picker_emoji: Option<super::picker::Item>,
    /// The sound whose menu is open, and the call's server it was opened for (`activeCallGuildId`).
    pub picker_sound: Option<(super::soundboard::Sound, Option<Id>)>,
    /// The open menu was opened in the overlay window (its Chat widget's rail): it is drawn there, not in the app.
    pub in_overlay: bool,
    /// The address and file name of the attachment chip whose menu is open.
    pub attachment_link: Option<(String, String)>,
    /// The roles (name, colour) of the invite whose "+N" opened `invite-roles-menu`.
    pub invite_roles: Vec<(String, u32)>,
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
/// `EU` (module 37411): a thread's levels as thread member flags.
const THREAD_LEVELS: [(u64, &str); 3] = [
    (dc_state::THREAD_ALL_MESSAGES, "All Messages"),
    (dc_state::THREAD_ONLY_MENTIONS, "Only @mentions"),
    (dc_state::THREAD_NO_MESSAGES, "Nothing"),
];
/// `UserGuildSettingsChannelFlags.FAVORITED`: the channel is pinned to the top of the list.
const PINNED: u64 = 2048;
/// `OPT_IN_ENABLED`: set along with a pin.
const OPT_IN_ENABLED: u64 = 4096;
/// The store the official client keeps Hide Names in (`CollapsedVoiceChannelStore`, persisted as `collapsedChannels`).
pub(in crate::ui) const HIDDEN_NAMES: &str = "collapsedChannels";

/// The channel type number the API uses (`channel_type` of a favourite).
pub(super) fn channel_type_number(kind: ChannelKind) -> u64 {
    match kind {
        ChannelKind::Text => 0,
        ChannelKind::Voice => 2,
        ChannelKind::Category => 4,
        ChannelKind::Other(number) => number,
    }
}

/// Which list a channel is ordered in: categories, voice and stage channels, or the text kinds (`a` of module 44757).
fn order_group(kind: ChannelKind) -> u8 {
    match kind {
        ChannelKind::Category => 0,
        ChannelKind::Voice | ChannelKind::Other(13) => 1,
        _ => 2,
    }
}

/// The ids ordered with this channel: those in its category (or the categories themselves) of its kind, in order.
fn move_siblings(channels: &[Channel], me: &Channel) -> Vec<Id> {
    ordered_group(channels, me.kind, me.parent_id)
        .into_iter()
        .map(|known| known.id)
        .collect()
}

fn ordered_group(channels: &[Channel], kind: ChannelKind, parent: Option<Id>) -> Vec<&Channel> {
    let group = order_group(kind);
    let mut list: Vec<&Channel> = channels
        .iter()
        .filter(|known| {
            order_group(known.kind) == group && (group == 0 || known.parent_id == parent)
        })
        .collect();
    list.sort_by_key(|known| (known.position, known.id));
    list
}

/// Where a channel can go: Uncategorized (id 0, the null category of module 808728) and every category, the one it is in disabled.
fn move_destinations(channels: &[Channel], me: &Channel) -> Vec<(Id, String, bool)> {
    let mut list = vec![(
        dc_intl::tr("Uncategorized").to_owned(),
        0,
        me.parent_id.is_none(),
    )];
    list.extend(
        ordered_group(channels, ChannelKind::Category, None)
            .into_iter()
            .map(|known| (known.name.clone(), known.id, me.parent_id == Some(known.id))),
    );
    list.into_iter()
        .map(|(label, id, disabled)| (id, label, disabled))
        .collect()
}

/// The updates a Move to sends (`vX` of module 44757): the channel goes first or last among the channels of its
/// kind in its (new) category, and every channel whose position changes is listed with its new one. The mover also
/// carries its new category when that changed.
fn plan_channel_move(
    channels: &[Channel],
    id: Id,
    parent: Option<Option<Id>>,
    first: bool,
) -> Vec<(Id, Option<Option<Id>>, i64)> {
    let Some(me) = channels.iter().find(|known| known.id == id) else {
        return Vec::new();
    };
    let target = if me.kind == ChannelKind::Category {
        None
    } else {
        parent.unwrap_or(me.parent_id)
    };
    let mut order: Vec<&Channel> = ordered_group(channels, me.kind, target)
        .into_iter()
        .filter(|known| known.id != id)
        .collect();
    if first {
        order.insert(0, me);
    } else {
        order.push(me);
    }
    let moved = target != me.parent_id && me.kind != ChannelKind::Category;
    order
        .into_iter()
        .enumerate()
        .filter_map(|(index, known)| {
            let position = index as i64;
            let own = known.id == id;
            (known.position != position || (own && moved)).then_some((
                known.id,
                (own && moved).then_some(target),
                position,
            ))
        })
        .collect()
}

fn level_label(level: u64) -> &'static str {
    match level {
        0 => dc_intl::t!("n/bTaY"),
        1 => dc_intl::tr("Only @mentions"),
        2 => dc_intl::tr("Nothing"),
        _ => dc_intl::tr("Use Category Default"),
    }
}

impl Root {
    pub(super) fn developer_mode(&self) -> bool {
        self.pref_on("developer_mode", false)
    }

    /// A right press on a forum tag (`onContextMenu` of module 376310): Forum Tag Actions in developer mode only.
    /// True when the menu opened.
    pub(super) fn open_forum_tag_menu(
        &mut self,
        tag: Id,
        at: gpui::Point<gpui::Pixels>,
        cx: &mut Context<Self>,
    ) -> bool {
        let developer = self.developer_mode();
        if developer {
            self.open_context_menu(Target::ForumTag(tag), at, cx);
        }
        developer
    }

    /// A right press on a role chip of a profile (module 166005 `onContextMenu`): `hasDeveloperContextMenu` is
    /// `!readOnly && developerMode`, so outside developer mode the press does nothing. True when the menu opened.
    pub(super) fn open_role_chip_menu(
        &mut self,
        role: Id,
        at: gpui::Point<gpui::Pixels>,
        cx: &mut Context<Self>,
    ) -> bool {
        let developer = self.developer_mode();
        if developer {
            self.open_context_menu(Target::RoleChip(role), at, cx);
        }
        developer
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
    pub(super) fn is_in_timeout(&self, guild: Id, user: Id) -> bool {
        self.stores
            .members
            .get(guild, user)
            .and_then(|member| member.timeout_until.as_deref())
            .and_then(|until| chrono::DateTime::parse_from_rfc3339(until).ok())
            .is_some_and(|until| until > chrono::Utc::now())
    }

    /// Whether our roles outrank everything the person holds, and they do not own the server: what
    /// `canManageUser` asks (module 576705).
    pub(super) fn outranks(&self, guild: Id, user: Id) -> bool {
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
                    ..SubRow::default()
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
        // Module 291247: no voice rows without View Channel on the voice channel, and a suppressed person on a Stage
        // (channel type 13) cannot be server muted or deafened.
        let visible = self.my_permission(guild, Some(channel), permissions::VIEW_CHANNEL);
        let held_back = channel.kind == ChannelKind::Other(13) && state.suppress;
        Some(ServerVoice {
            mute: state.mute,
            deaf: state.deaf,
            can_mute: visible
                && !held_back
                && self.my_permission(guild, Some(channel), permissions::MUTE_MEMBERS),
            can_deafen: visible
                && !held_back
                && self.my_permission(guild, Some(channel), permissions::DEAFEN_MEMBERS),
            can_disconnect: visible
                && !me
                && self.my_permission(guild, Some(channel), permissions::MOVE_MEMBERS),
            can_move: !me
                && self.my_permission(guild, Some(channel), permissions::MOVE_MEMBERS)
                && !self.move_targets(guild, user).is_empty(),
        })
    }

    pub(super) fn my_permission(&self, guild: Id, channel: Option<&Channel>, bit: u64) -> bool {
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
    pub(super) fn guild_of_channel(&self, channel: Id) -> Option<Id> {
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

    /// 607508 `l` of a thread: `THREAD_*` flag the account's member settings (or the parent's level) give it.
    fn thread_level_of(&self, thread: Id) -> u64 {
        self.stores
            .guilds
            .iter()
            .find_map(|guild| {
                let known = guild.threads.iter().find(|known| known.id == thread)?;
                Some(dc_state::notifications::thread_level(
                    &self.stores,
                    guild,
                    known,
                    chrono::Utc::now(),
                ))
            })
            .unwrap_or(dc_state::THREAD_NO_MESSAGES)
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
        let at = if matches!(target, Target::User { .. }) {
            gpui::point(px(984.), px(88.))
        } else {
            gpui::point(px(520.), px(120.))
        };
        self.open_context_menu(target, at, cx);
        format!("menu open for {target:?}")
    }

    /// Isolated menu capture at the fixture reference's coordinates; card placement is tested through its button.
    #[cfg(feature = "capture")]
    pub(crate) fn script_spotify_context(
        &mut self,
        who: &str,
        surface: &str,
        cx: &mut Context<Self>,
    ) -> String {
        let me = self.stores.current_user.as_ref().map(|me| me.id);
        let user = match who {
            "self" => me,
            "other" => self
                .stores
                .private_channels
                .iter()
                .find(|channel| channel.id == 1_568_693_333_864_480_939)
                .and_then(|channel| {
                    channel
                        .recipients
                        .iter()
                        .copied()
                        .find(|id| Some(*id) != me)
                }),
            _ => None,
        };
        let Some(user) = user else {
            return "Spotify menu: fixture person unavailable".into();
        };
        let popout = match surface {
            "popout" => true,
            "sidebar" | "modal" => false,
            _ => return "Spotify menu: unknown surface".into(),
        };
        if spotify_activity_items(Some(user) == me, popout).is_empty() {
            self.close_context_menu(cx);
            return "Spotify menu: no entries on this surface".into();
        }
        self.open_context_menu(
            Target::SpotifyActivity { user, popout },
            gpui::point(px(986.), px(456.)),
            cx,
        );
        "Spotify menu: opened".into()
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

    /// Whether the target is muted now (a mute that has run out no longer counts), and until when.
    fn mute_state(&self, target: Target) -> (bool, Option<chrono::DateTime<chrono::Utc>>) {
        let (muted, end) = match target {
            Target::Guild(guild) => self
                .stores
                .guild_settings
                .guilds
                .get(&guild)
                .map_or((false, None), |settings| {
                    (settings.muted, settings.mute_end)
                }),
            // `isMuted` of the joined-threads store (module 152007): the account's own member of the thread.
            Target::Thread(id) => self
                .stores
                .guilds
                .iter()
                .flat_map(|guild| guild.threads.iter())
                .find(|thread| thread.id == id)
                .map_or((false, None), |thread| {
                    (thread.joined && thread.muted, thread.mute_end)
                }),
            Target::Channel(id) | Target::Category(id) => match self.guild_of_channel(id) {
                Some(guild) => {
                    let entry = self.stores.guild_settings.channel(guild, id);
                    (entry.muted, entry.mute_end)
                }
                None => (false, None),
            },
            Target::Dm(id) => {
                let entry = self.stores.guild_settings.channel(0, id);
                (entry.muted, entry.mute_end)
            }
            _ => (false, None),
        };
        let expired = end.is_some_and(|end| end <= chrono::Utc::now());
        (muted && !expired, end)
    }

    pub(super) fn names_hidden(&self, channel: Id) -> bool {
        crate::storage::get(HIDDEN_NAMES)
            .is_some_and(|stored| stored[channel.to_string()].as_bool() == Some(true))
    }

    /// The rows that depend on what is stored: Unmute for a muted target, Unpin for a pinned channel, the Hide Names
    /// tick, and no Move to when there is nowhere to move a server.
    fn adjust_menu_rows(&self, target: Target, items: Vec<Entry>) -> Vec<Entry> {
        let (muted, end) = self.mute_state(target);
        let pinned = matches!(target, Target::Channel(id)
        if self.guild_of_channel(id).is_some_and(|guild| {
            self.stores.guild_settings.channel(guild, id).flags & PINNED != 0
        }));
        let favorite = match target {
            Target::Channel(id) | Target::Thread(id) | Target::Dm(id) => {
                self.stores.favorites().is_favorite(id)
            }
            _ => false,
        };
        let dm_pinned = matches!(target, Target::Dm(id)
            if self.stores.guild_settings.channel(0, id).flags & PINNED != 0);
        let mut out = Vec::with_capacity(items.len());
        for entry in items {
            let Entry::Row(mut row) = entry else {
                out.push(entry);
                continue;
            };
            match row.act {
                Act::Mute if muted => {
                    // Each row's own Unmute message (module 995102), a DM's with its name (`eC+9rj`).
                    let unmute = [
                        ("Mute Channel", "Unmute Channel"),
                        ("Mute Category", "Unmute Category"),
                        ("Mute Thread", "Unmute Thread"),
                        ("Mute Server", "Unmute Server"),
                        ("Mute Conversation", "Unmute Conversation"),
                    ]
                    .into_iter()
                    .find(|(mute, _)| row.label == dc_intl::tr(mute))
                    .map(|(_, unmute)| dc_intl::tr(unmute).to_owned());
                    let dm_name = match target {
                        Target::Dm(id) => self
                            .stores
                            .private_channels
                            .iter()
                            .find(|known| known.id == id)
                            .map(|dm| format!("@{}", self.private_channel_name(dm))),
                        _ => None,
                    };
                    row.label = match (unmute, dm_name) {
                        (Some(unmute), _) => unmute,
                        (None, Some(name)) => dc_intl::t!("eC+9rj", name = name),
                        (None, None) => row.label.replacen("Mute", "Unmute", 1),
                    };
                    row.caret = false;
                    // "Muted until {endTime}" (`j7h4AJ`).
                    row.sub = end.map(|end| {
                        dc_intl::t!(
                            "j7h4AJ",
                            endTime = end
                                .with_timezone(&chrono::Local)
                                .format("%-m/%-d, %-I:%M %p")
                                .to_string()
                        )
                    });
                }
                Act::PinChannel if pinned => row.label = dc_intl::tr("Unpin Channel").to_owned(),
                Act::Favorite if favorite => {
                    row.label = dc_intl::tr("Remove from Favorites").to_owned();
                    if matches!(target, Target::Dm(_)) {
                        row.icon = None;
                        row.danger = true;
                    }
                }
                Act::PinDm if dm_pinned => row.label = dc_intl::t!("vDYmad").to_owned(),
                Act::HideNames => {
                    if let Target::Channel(id) = target {
                        row.check = Some(self.names_hidden(id));
                    }
                }
                Act::MoveTo if self.move_rows(target).is_empty() => continue,
                _ => {}
            }
            out.push(Entry::Row(row));
        }
        tidy(out)
    }

    /// Marks the next `open_context_menu` as opened from a person's row in a voice channel's list.
    pub(super) fn context_voice_row(&mut self) {
        self.context.voice_row = true;
    }

    /// Marks the next `open_context_menu` of a channel as Active Now's (`handleChannelContextMenu`, module 113255).
    pub(super) fn context_activity_voice(&mut self) {
        self.context.activity_voice = true;
    }

    /// Marks the next `open_context_menu` as opened for a Stage speaker tile (`showStageChannelItems: true`).
    pub(super) fn context_stage_tile(&mut self) {
        self.context.voice_row = true;
        self.context.stage_channel_items = true;
    }

    /// Marks the next `open_context_menu` as opened from a Stage audience tile (`75139.js`: `showMediaItems: true`,
    /// `showStageChannelItems: true`, `showChatItems: false`).
    pub(super) fn context_stage_audience(&mut self) {
        self.context.voice_row = true;
        self.context.stage_channel_items = true;
        self.context.omit_mention = true;
    }

    pub(super) fn open_context_menu(
        &mut self,
        target: Target,
        at: Point<Pixels>,
        cx: &mut Context<Self>,
    ) {
        self.context.in_overlay = false;
        let developer = self.developer_mode();
        let from_voice_row = std::mem::take(&mut self.context.voice_row);
        let stage_channel_items = std::mem::take(&mut self.context.stage_channel_items);
        let omit_mention = std::mem::take(&mut self.context.omit_mention);
        let activity_voice = std::mem::take(&mut self.context.activity_voice);
        let items = match target {
            Target::AccountPanel => self.account_panel_items(),
            Target::Role { role, guild } => self.better_role_items(role, guild),
            Target::PinCategory(index) => self.pin_category_items(usize::from(index)),
            Target::PluginBadge => self.global_badge_items(),
            Target::PluginPermissionRole { role, guild } => self.permission_role_items(role, guild),
            Target::PluginPermissionUser(_) => self.permission_user_items(),
            Target::Greet { .. } => self.greet_picker_items(),
            Target::SpotifyActivity { user, popout } => spotify_activity_items(
                self.stores
                    .current_user
                    .as_ref()
                    .is_some_and(|me| me.id == user),
                popout,
            ),
            Target::GameActivity {
                user,
                popout,
                embedded,
            } => game_activity_items(
                self.stores
                    .current_user
                    .as_ref()
                    .is_some_and(|me| me.id == user),
                popout,
                embedded,
                self.context
                    .game_activity
                    .as_ref()
                    .and_then(|game| game.application_id.as_deref())
                    .is_some_and(|app| self.game_profile_available(app)),
            ),
            Target::InventoryAuthor { .. } => {
                let mut items = vec![Entry::Row(Row::new(Act::Profile, dc_intl::tr("Profile")))];
                if developer {
                    items.push(Entry::Separator);
                    items.push(Entry::Row(Row::new(
                        Act::CopyId,
                        dc_intl::tr("Copy User ID"),
                    )));
                }
                items
            }
            Target::InventoryFeed | Target::InventoryFeedAtPointer => vec![
                Entry::Row(
                    Row::new(Act::InventoryAbout, dc_intl::tr("About Recent Activity"))
                        .icon("circle_info"),
                ),
                Entry::Row(
                    Row::new(Act::InventoryHide, dc_intl::tr("Hide Activity Cards"))
                        .icon("eye_slash")
                        .check(self.inventory_hidden()),
                ),
            ],
            Target::Composer => composer_items(&self.context.composer.clone().unwrap_or_default()),
            Target::AppDirectory(_) => app_directory_items(developer),
            Target::LauncherApp(_) => launcher_app_items(developer),
            Target::GameProfile(_) => game_profile_items(developer),
            Target::GameAddToProfile(game) => self.game_widget_items(game),
            Target::ProfileWidget { .. } => profile_widget_items(),
            Target::RtcChannel { guild, channel } => self.rtc_channel_items(guild, channel),
            Target::Favorites => favorites_items(),
            Target::ChannelMention(_) => channel_mention_items(developer),
            Target::AttachmentLink => attachment_link_items(),
            Target::ForumTag(_) => forum_tag_items(),
            Target::RoleChip(_) => role_chip_items(),
            Target::RaidAlert(_) => raid_alert_items(),
            Target::ProgressBar(guild) => progress_bar_items(
                self.stores
                    .guild(guild)
                    .is_some_and(|known| known.premium_progress_bar_enabled),
            ),
            Target::PermissionRow { role, everyone, .. } => {
                let items = permission_target_items(role, everyone, developer);
                if items.is_empty() {
                    return;
                }
                items
            }
            Target::AuditChannel(channel) => {
                let Some(guild) = self.guild_of_channel(channel) else {
                    return;
                };
                let known = self
                    .stores
                    .guild(guild)
                    .and_then(|known| known.channels.iter().find(|c| c.id == channel))
                    .cloned();
                let category = known
                    .as_ref()
                    .is_some_and(|c| c.kind == dc_state::ChannelKind::Category);
                let can_edit = [
                    dc_state::permissions::MANAGE_CHANNELS,
                    dc_state::permissions::MANAGE_ROLES,
                    dc_state::permissions::MANAGE_WEBHOOKS,
                ]
                .into_iter()
                .any(|bit| self.my_permission(guild, known.as_ref(), bit))
                    && self.my_permission(
                        guild,
                        known.as_ref(),
                        dc_state::permissions::VIEW_CHANNEL,
                    );
                audit_channel_items(category, can_edit, developer)
            }
            // Module 551309: one group of the roles, each with its circle; a pick only closes the menu.
            Target::InviteRoles => self
                .context
                .invite_roles
                .iter()
                .map(|(name, color)| Entry::Row(Row::new(Act::Noop, name.clone()).role_dot(*color)))
                .collect(),
            Target::PickerSticker(sticker) => {
                picker_sticker_items(self.sticker_favorited(sticker), developer)
            }
            Target::PickerEmoji => {
                let Some(item) = self.context.picker_emoji.clone() else {
                    return;
                };
                let key = super::picker::item_favorite_key(&item);
                picker_emoji_items(
                    self.emoji_favorited(&key),
                    matches!(item, super::picker::Item::Custom { .. }),
                    developer,
                )
            }
            Target::PickerSound => {
                let Some((sound, call_guild)) = self.context.picker_sound.clone() else {
                    return;
                };
                let items = sound_items(self.sound_facts(&sound, call_guild, developer));
                if items.is_empty() {
                    return;
                }
                items
            }
            Target::Guild(guild) => {
                let Some(known) = self.stores.guild(guild) else {
                    return;
                };
                let counts = self.guild_rail_counts(known);
                let facts = GuildFacts {
                    unread: counts.unread || counts.mentions > 0,
                    can_invite: self.my_permission(guild, None, CREATE_INSTANT_INVITE)
                        || known.vanity_url_code.is_some(),
                    can_manage_channels: self.my_permission(guild, None, MANAGE_CHANNELS),
                    can_manage_guild: self.my_permission(guild, None, permissions::MANAGE_GUILD),
                    hide_muted: self.hide_muted(guild),
                    owner: known.owner_id == self.stores.current_user.as_ref().map(|me| me.id),
                    developer,
                    security_actions: self.security_actions_item(guild),
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
                        let children: Vec<Id> = guild
                            .channels
                            .iter()
                            .filter(|child| child.parent_id == Some(id))
                            .map(|child| child.id)
                            .collect();
                        children
                            .iter()
                            .any(|child| self.has_unread_or_mentions(*child))
                            || guild.threads.iter().any(|thread| {
                                thread.joined
                                    && !thread.archived
                                    && children.contains(&thread.parent_id)
                                    && self.has_unread_or_mentions(thread.id)
                            })
                    });
                    category_items(unread, collapsed, can_manage, developer, level)
                } else if activity_voice {
                    activity_voice_items(
                        can_manage,
                        self.my_permission(guild, Some(&known), CREATE_INSTANT_INVITE),
                        developer,
                    )
                } else if self.in_favorites() {
                    favorite_channel_items(self.has_unread_or_mentions(id), level)
                } else {
                    let in_channel = self.call.as_ref().is_some_and(|call| call.channel == id);
                    let has_status = self
                        .stores
                        .channel_status
                        .get_channel_status(guild, id)
                        .is_some_and(|status| !status.is_empty());
                    let facts = ChannelFacts {
                        clear_status: known.kind == ChannelKind::Voice
                            && can_manage
                            && !in_channel
                            && has_status,
                        voice: matches!(known.kind, ChannelKind::Voice | ChannelKind::Other(13)),
                        unread: self.has_unread_or_mentions(id),
                        can_manage,
                        can_invite: self.my_permission(guild, Some(&known), CREATE_INSTANT_INVITE),
                        developer,
                        hide_names: false,
                        join_quiet: matches!(
                            known.kind,
                            ChannelKind::Voice | ChannelKind::Other(13)
                        ) && !in_channel
                            && self.my_permission(guild, Some(&known), permissions::CONNECT)
                            && self.stores.join_sound_set(guild),
                        report_stage: known.kind == ChannelKind::Other(13)
                            && self.stores.stage_instances.get_by_channel(id).is_some(),
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
                    ThreadFacts {
                        unread: self.has_unread_or_mentions(id),
                        manage,
                        owner,
                        joined: thread.joined,
                        developer,
                        archived: thread.archived,
                        locked: thread.locked,
                        forum_post: parent.as_ref().is_some_and(Channel::is_forum),
                        pinned: thread.pinned(),
                        can_send: parent.as_ref().is_some_and(|parent| {
                            self.my_permission(guild, Some(parent), permissions::SEND_MESSAGES)
                                && self.my_permission(
                                    guild,
                                    Some(parent),
                                    permissions::SEND_MESSAGES_IN_THREADS,
                                )
                        }),
                        media: parent.as_ref().is_some_and(Channel::is_media),
                        private_thread: thread.kind == 12,
                        invitable: thread.invitable,
                    },
                    level_label(
                        THREAD_LEVELS
                            .iter()
                            .position(|(flag, _)| *flag == self.thread_level_of(id))
                            .map_or(0, |index| index as u64),
                    ),
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
                if dm.kind == 3 {
                    group_items(
                        self.has_unread_or_mentions(channel),
                        dm.owner_id.is_some()
                            && dm.owner_id == self.stores.current_user.as_ref().map(|me| me.id),
                        developer,
                    )
                } else {
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
                        dm: Some(self.has_unread_or_mentions(channel)),
                        dm_favorite: self.stores.favorites().is_favorite(channel),
                        hide_call: user.bot
                            || self.vv.popout_render
                            || self.call.as_ref().is_some_and(|call| {
                                call.connected && call.channel == channel && dm.kind == 1
                            }),
                        invite_servers: !self.invite_servers(self.dm_user(channel)).is_empty(),
                        ring: self.ring_row_for_channel(channel),
                        ..UserFacts::default()
                    };
                    user_items(&facts)
                }
            }
            Target::User { user, guild } => {
                // The Apps submenu lists the user commands of the channel's index (module 173500).
                self.request_command_index();
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
                    mod_view: guild.is_some_and(|guild| self.can_open_mod_view(guild)),
                    developer,
                    dm: None,
                    invite_servers: !me && !self.invite_servers(Some(user)).is_empty(),
                    ring: if guild.is_none() {
                        self.dm_channel_for_user(user)
                            .and_then(|channel| self.ring_row_for_channel(channel))
                            .or_else(|| self.ring_row_for(user, guild))
                    } else {
                        self.ring_row_for(user, guild)
                    },
                    server_voice: guild.and_then(|guild| self.server_voice_of(guild, user, me)),
                    in_timeout: guild.is_some_and(|guild| self.is_in_timeout(guild, user))
                        && guild.is_some_and(|guild| {
                            self.my_permission(guild, None, permissions::MODERATE_MEMBERS)
                        }),
                    roles: guild.is_some_and(|guild| !self.role_sub_rows(guild, user).is_empty()),
                    verify: guild.and_then(|guild| self.verify_state(guild, user, me)),
                    in_call: from_voice_row
                        || self
                            .others_in_call()
                            .is_some_and(|others| others.contains(&user)),
                    // Our own switches follow the engine; the session's voice state (what the server holds for us)
                    // counts too, so a state fed without a call shows the same ticks as the golden.
                    self_voice: (me && from_voice_row).then(|| {
                        let held = guild
                            .and_then(|guild| self.stores.voice_states.get(&guild))
                            .and_then(|states| states.iter().find(|state| state.user_id == user));
                        let deaf = self.self_deaf || held.is_some_and(|state| state.self_deaf);
                        (
                            self.self_mute || deaf || held.is_some_and(|state| state.self_mute),
                            deaf,
                        )
                    }),
                    voice_muted: self.context.mix.get(&user).is_some_and(|mix| mix.1),
                    soundboard_muted: self.context.soundboard_muted.contains(&user),
                    video_disabled: super::call_menus::video_disabled(user),
                    hide_call: known.bot
                        || self.vv.popout_render
                        || self.call.as_ref().is_some_and(|call| {
                            call.connected
                                && self.stores.private_channels.iter().any(|dm| {
                                    dm.id == call.channel && dm.kind == 1 && dm.recipients == [user]
                                })
                        }),
                    dm_favorite: false,
                    omit_mention,
                    stage_acts: guild.map_or_else(Vec::new, |guild| {
                        self.stage_user_acts(guild, user, stage_channel_items)
                    }),
                };
                user_items(&facts)
            }
        };
        let items = self.adjust_menu_rows(target, items);
        // Not in the pinned client: CopyUserURLs' row (`plugin_hooks.rs`).
        let user_url = match target {
            Target::User { user, .. } => Some(user),
            Target::Dm(channel) => self.dm_user(channel),
            _ => None,
        };
        let items = super::plugin_hooks::with_user_url_row(items, user_url);
        // Not in the pinned client: MuteChannelMentions' check (`plugin_hooks/mute_channel_mentions.rs`).
        let hide_channel = match target {
            Target::Channel(channel) | Target::Thread(channel) => Some(channel),
            _ => None,
        };
        let items = self.with_hide_mention_row(items, hide_channel);
        // Not in the pinned client: ViewIcons' group (`plugin_hooks/view_icons.rs`).
        let items = self.with_view_icons_rows(items, target);
        // Not in the pinned client: ServerInfo's row (`plugin_hooks/server_info.rs`).
        let items = self.with_server_info_row(items, target);
        // Not in the pinned client: NewGuildSettings' row, before ServerInfo's (`plugin_hooks/new_guild_settings.rs`).
        let items = self.with_new_guild_settings_row(items, target);
        // Not in the pinned client: BiggerStreamPreview's row (`plugin_hooks/bigger_stream_preview.rs`).
        let items = self.with_stream_preview_row(items, target);
        // Not in the pinned client: PinDMs' "Pin DMs" row (`plugin_hooks/pin_dms.rs`).
        let items = self.with_pin_dms_row(items, target);
        // Not in the pinned client: BypassStatus' group before the last one (`plugin_hooks/bypass_status.rs`).
        let items = self.with_bypass_status_row(items, target);
        // Not in the pinned client: UserPFP's Set Avatar, last (`plugin_hooks/userpfp.rs`).
        let items = self.with_userpfp_row(items, target);
        // Not in the pinned client: PermissionsViewer's View Permissions (`plugin_hooks/permissions_viewer.rs`).
        let items = self.with_permissions_viewer_row(items, target);
        self.close_all_context_menus();
        self.menu_scroll.set_offset(gpui::point(px(0.), px(0.)));
        let at = flip_above(at, menu_height(&items), self.viewport.1);
        self.context.open = Some(Open {
            target,
            at,
            items,
            sub: None,
            nested: None,
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
            send_button: self.pref_on("enable_send_button_outside_experiment", false),
            ai: self.composer_ai_allowed() && !click.selected.trim().is_empty(),
            ai_busy: self.composer_ai.busy.clone(),
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

    /// Harness: runs the open right-click menu's row with this label, as a click on it would.
    #[cfg(feature = "capture")]
    pub(crate) fn script_context_run(
        &mut self,
        label: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> String {
        let Some(open) = self.context.open.as_ref() else {
            return "no menu".to_owned();
        };
        let target = open.target;
        let Some(row) = open.items.iter().find_map(|entry| match entry {
            Entry::Row(row) if row.label == label => Some(row.clone()),
            _ => None,
        }) else {
            return format!("no row {label}");
        };
        if row.disabled {
            return format!("row {label} is disabled");
        }
        let before = crate::inert::line_count();
        super::actions::take_effects();
        self.close_context_menu(cx);
        self.run_context(row.act, target, window, cx);
        let mut lines = vec![format!("ran {label}")];
        lines.extend(super::actions::take_effects());
        lines.extend(crate::inert::lines_since(before));
        lines.join(" | ")
    }

    pub(super) fn close_context_menu(&mut self, cx: &mut Context<Self>) {
        if self.context.open.take().is_some() {
            cx.notify();
        }
    }

    pub(super) fn mark_channel_read(&mut self, channel: Id) {
        if !self.has_unread_or_mentions(channel) {
            return;
        }
        let latest = self.ack_target_id(channel);
        if self.channel == Some(channel) {
            self.unread_from = None;
        }
        if let Some(state) = self.stores.read_states.states.get_mut(&channel) {
            state.manual = false;
        }
        self.stores.read_states.ack_local(channel, latest, false);
        self.data_revision += 1;
        if self.channel == Some(channel) {
            self.rebuild_rows();
        }
        let _ = self.requests.send(Request::Ack {
            channel,
            message: latest,
            manual: false,
            mention_count: 0,
            last_viewed: dc_state::read_state::days_since_epoch(super::picker::now_ms()),
        });
    }

    /// The live card's More button opens beside its measured bounds (285909).
    #[allow(clippy::too_many_arguments)]
    pub(super) fn open_live_activity_menu(
        &mut self,
        user: Id,
        spotify: bool,
        game: Option<(super::report_issue::Detected, bool)>,
        popout: bool,
        anchor: &SharedString,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let own = self
            .stores
            .current_user
            .as_ref()
            .is_some_and(|me| me.id == user);
        let embedded = game.as_ref().is_some_and(|(_, embedded)| *embedded);
        let items = if spotify {
            spotify_activity_items(own, popout)
        } else if let Some((detected, _)) = game.as_ref() {
            let profile = detected
                .application_id
                .as_deref()
                .is_some_and(|app| self.game_profile_available(app));
            game_activity_items(own, popout, embedded, profile)
        } else {
            // Another kind of activity: only View All Activity, in a popout.
            spotify_activity_items(false, popout)
        };
        if items.is_empty() {
            return;
        }
        let Some(bounds) = self.tip_anchors.borrow().get(anchor).copied() else {
            return;
        };
        let width = items
            .iter()
            .filter_map(|item| match item {
                Entry::Row(row) => Some(label_width(window, &row.label) + 34.),
                Entry::Heading(text) => Some(label_width(window, text) + 16.),
                Entry::Separator => None,
            })
            .fold(144., f32::max)
            .min(320.);
        let right = bounds.right() + px(8.);
        let x = if right + px(width) > window.viewport_size().width - px(8.) {
            bounds.left() - px(8. + width)
        } else {
            right
        };
        let target = match game {
            Some((detected, embedded)) if !spotify => {
                self.context.game_activity = Some(detected);
                Target::GameActivity {
                    user,
                    popout,
                    embedded,
                }
            }
            _ => Target::SpotifyActivity { user, popout },
        };
        self.open_context_menu(target, gpui::point(x, bounds.top()), cx);
    }

    /// Runs a row against its target.
    pub(super) fn run_context(
        &mut self,
        act: Act,
        target: Target,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match (act, target) {
            // Not in the pinned client: AccountPanelServerProfile (`plugin_hooks/account_panel.rs`).
            (Act::PluginAlternateProfile, Target::AccountPanel) => {
                self.account_panel_alternate(cx);
            }
            (Act::PluginPrioritizeServerProfile, Target::AccountPanel) => {
                self.account_panel_toggle_prioritize(cx);
            }
            // Not in the pinned client: ServerInfo (`plugin_hooks/server_info.rs`).
            // Not in the pinned client: NewGuildSettings (`plugin_hooks/new_guild_settings.rs`).
            (Act::PluginApplyNewGuildSettings, Target::Guild(guild)) => {
                self.apply_new_guild_settings(guild);
                cx.notify();
            }
            (Act::PluginServerInfo, Target::Guild(guild)) => {
                self.server_info_open(guild, cx);
            }
            // Not in the pinned client: BypassStatus (`plugin_hooks/bypass_status.rs`).
            (Act::PluginStatusBypass, target) => self.bypass_status_toggle(target, cx),
            // Not in the pinned client: UserPFP (`plugin_hooks/userpfp.rs`).
            (Act::PluginSetAvatar, Target::User { user, .. }) => self.userpfp_open(user, cx),
            // Not in the pinned client: GlobalBadges (`plugin_hooks/global_badges.rs`).
            (Act::PluginCopyBadgeName, Target::PluginBadge) => self.global_badge_copy(false, cx),
            // Not in the pinned client: PermissionsViewer (`plugin_hooks/permissions_viewer.rs`).
            (Act::PluginViewPermissions, target) => self.permissions_viewer_open(target, cx),
            (Act::PluginEditRole, Target::PluginPermissionRole { role, guild }) => {
                self.better_role_edit(role, guild, cx);
            }
            (Act::PluginCopyRoleColor, Target::PluginPermissionRole { role, guild }) => {
                self.better_role_copy_color(role, guild, cx);
            }
            (Act::PluginViewRoleIcon, Target::PluginPermissionRole { role, guild }) => {
                let format = super::plugin_hooks::role_icon_format_setting();
                self.role_icon_view(role, guild, &format, cx);
            }
            (Act::PluginViewRoleMembers, Target::PluginPermissionRole { role, .. }) => {
                let anchor = self
                    .tip_anchors
                    .borrow()
                    .get(&gpui::SharedString::from(format!(
                        "permissions-viewer-role-{role}"
                    )))
                    .copied();
                if let Some(anchor) = anchor {
                    self.better_role_members(role, anchor, cx);
                }
            }
            (Act::PluginCopyBadgeLink, Target::PluginBadge) => self.global_badge_copy(true, cx),
            // Not in the pinned client: GreetStickerPicker (`plugin_hooks/greet_sticker_picker.rs`).
            (
                Act::PluginGreetMode(_) | Act::PluginGreetSticker(_),
                Target::Greet { channel, message },
            ) => {
                self.greet_picker_run(act, channel, message, cx);
            }
            // Not in the pinned client: PinDMs' category menu (`plugin_hooks/pin_dms.rs`).
            (
                Act::PluginPinEditCategory
                | Act::PluginPinCategoryUp
                | Act::PluginPinCategoryDown
                | Act::PluginPinDeleteCategory,
                Target::PinCategory(index),
            ) => {
                self.pin_category_run(act, usize::from(index), cx);
            }
            // Not in the pinned client: BiggerStreamPreview (`plugin_hooks/bigger_stream_preview.rs`).
            (Act::PluginViewStreamPreview, Target::User { user, .. }) => {
                self.stream_preview_view(user, cx);
            }
            // Not in the pinned client: ViewIcons (`plugin_hooks/view_icons.rs`).
            (
                Act::PluginViewAvatar
                | Act::PluginViewServerAvatar
                | Act::PluginViewDecoration
                | Act::PluginViewIcon
                | Act::PluginViewBanner,
                _,
            ) => {
                self.view_icons_open(act, target, cx);
            }
            // Not in the pinned client: BetterRoleContext (`plugin_hooks/better_role_context.rs`).
            (Act::PluginEditRole, Target::Role { role, guild }) => {
                self.better_role_edit(role, guild, cx);
            }
            (Act::PluginCopyRoleColor, Target::Role { role, guild }) => {
                self.better_role_copy_color(role, guild, cx);
            }
            (Act::PluginViewRoleIcon, Target::Role { role, guild }) => {
                self.better_role_view_icon(role, guild, cx);
            }
            (Act::PluginViewRoleMembers, Target::Role { role, .. }) => {
                let anchor = self
                    .tip_anchors
                    .borrow()
                    .get(&gpui::SharedString::from(format!("member-group-{role}")))
                    .copied();
                if let Some(anchor) = anchor {
                    self.better_role_members(role, anchor, cx);
                }
            }
            (Act::PluginHideMentionBadge, Target::Channel(channel) | Target::Thread(channel)) => {
                self.toggle_hidden_mentions(channel);
                cx.notify();
            }
            (
                Act::ViewAllActivity,
                Target::SpotifyActivity { user, .. } | Target::GameActivity { user, .. },
            ) => {
                self.profile = None;
                self.open_full_profile_tab(user, super::full_profile::Tab::Activity, window, cx);
            }
            (Act::ManageSpotifyPrivacy, Target::SpotifyActivity { user, .. }) => {
                if self
                    .stores
                    .current_user
                    .as_ref()
                    .is_some_and(|me| me.id == user)
                {
                    self.profile = None;
                    self.close_full_profile(cx);
                    self.open_settings_category(
                        dc_intl::tr("Connected Apps"),
                        dc_intl::tr("Connections"),
                        cx,
                    );
                }
            }
            (Act::FavoriteExpression, Target::PickerEmoji) => {
                if let Some(item) = self.context.picker_emoji.take() {
                    self.toggle_favorite_emoji(&super::picker::item_favorite_key(&item));
                    self.refresh_open_picker(cx);
                }
            }
            (Act::FavoriteExpression, Target::PickerSticker(sticker)) => {
                self.toggle_favorite_sticker(sticker, cx);
            }
            (Act::CopyExpressionLink, Target::PickerSticker(sticker)) => {
                if let Some(known) = self.stores.stickers.sticker(sticker) {
                    let link = super::stickers::image_link(known, self.ui_scale.get());
                    cx.write_to_clipboard(ClipboardItem::new_string(link));
                }
            }
            (Act::CopyEmoji, Target::PickerEmoji) => {
                if let Some(item) = self.context.picker_emoji.take() {
                    cx.write_to_clipboard(ClipboardItem::new_string(item.message_text(0)));
                }
            }
            (Act::CopyExpressionLink, Target::PickerEmoji) => {
                if let Some(super::picker::Item::Custom { id, animated, .. }) =
                    self.context.picker_emoji.take()
                {
                    cx.write_to_clipboard(ClipboardItem::new_string(
                        super::picker::emoji_image_link(id, animated, self.ui_scale.get()),
                    ));
                }
            }
            (
                Act::EditSound
                | Act::EntranceServer
                | Act::EntranceGlobal
                | Act::DownloadSound
                | Act::CopyId,
                Target::PickerSound,
            ) => {
                let Some((sound, call_guild)) = self.context.picker_sound.take() else {
                    return;
                };
                match act {
                    Act::EditSound => {
                        if let Ok(guild) = sound.guild.parse::<Id>() {
                            let value = json!({
                                "sound_id": sound.id,
                                "name": sound.name,
                                "volume": sound.volume,
                                "emoji_id": sound.emoji_id,
                                "emoji_name": sound.emoji_name,
                            });
                            // The dialog comes over the app: the picker it was opened from goes, as Add Sound does.
                            self.close_panel_popout(cx);
                            self.edit_sound(guild, &value, window, cx);
                        }
                    }
                    Act::EntranceServer => {
                        if let Some(guild) = call_guild {
                            self.set_entrance_sound(guild, &sound);
                        }
                    }
                    Act::EntranceGlobal => self.set_entrance_sound(0, &sound),
                    Act::DownloadSound => self.download_sound(&sound, cx),
                    _ => cx.write_to_clipboard(ClipboardItem::new_string(sound.id.clone())),
                }
            }
            (Act::CopyId, Target::PickerEmoji) => {
                if let Some(super::picker::Item::Custom { id, .. }) =
                    self.context.picker_emoji.take()
                {
                    cx.write_to_clipboard(ClipboardItem::new_string(id.to_string()));
                }
            }
            (Act::ViewGameProfile, Target::GameActivity { .. }) => {
                if let Some(app) = self
                    .context
                    .game_activity
                    .take()
                    .and_then(|game| game.application_id)
                {
                    self.open_game_profile(app, cx);
                }
            }
            (Act::GameWidget(kind), Target::GameAddToProfile(game)) => {
                self.toggle_game_widget(game, kind, cx);
            }
            (Act::ReportGameIssue, Target::GameProfile(game)) => {
                let game = game.to_string();
                let name = self.game_profile_name(&game);
                self.game_profile = None;
                self.open_report_issue(
                    Some(super::report_issue::Detected {
                        name,
                        application_id: Some(game),
                        art: None,
                    }),
                    window,
                    cx,
                );
            }
            (Act::ReportGameIssue, Target::GameActivity { .. }) => {
                let detected = self.context.game_activity.take();
                self.open_report_issue(detected, window, cx);
            }
            (Act::ManageSpotifyPrivacy, Target::GameActivity { user, .. }) => {
                if self
                    .stores
                    .current_user
                    .as_ref()
                    .is_some_and(|me| me.id == user)
                {
                    self.profile = None;
                    self.close_full_profile(cx);
                    self.open_settings_page("activity_privacy", cx);
                }
            }
            (Act::ReportApp, Target::LauncherApp(app)) => {
                self.slash.launcher = None;
                self.open_report_app(app, "app_launcher", cx);
            }
            (Act::ReportApp, Target::AppDirectory(app)) => {
                self.open_report_app(app, "app_directory", cx);
            }
            (Act::ReportWidget, Target::ProfileWidget { user, widget }) => {
                self.open_report_widget(user, widget, cx);
            }
            (Act::InventoryAbout, Target::InventoryFeed | Target::InventoryFeedAtPointer) => {
                self.inventory.about = true
            }
            (Act::InventoryHide, Target::InventoryFeed | Target::InventoryFeedAtPointer) => {
                self.toggle_inventory_hidden()
            }
            // Module 871638: the server's Mark As Read is `BULK_ACK` of its channels and threads (module 567035).
            (Act::MarkRead, Target::Guild(guild)) => {
                self.mark_guilds_read(&[guild], cx);
            }
            (Act::MarkRead, Target::Category(category)) => {
                let targets: Vec<Id> = self
                    .guild_of_channel(category)
                    .and_then(|guild| self.stores.guild(guild))
                    .map(|guild| {
                        let children: Vec<Id> = guild
                            .channels
                            .iter()
                            .filter(|channel| channel.parent_id == Some(category))
                            .map(|channel| channel.id)
                            .collect();
                        let threads: Vec<Id> = guild
                            .threads
                            .iter()
                            .filter(|thread| {
                                thread.joined
                                    && !thread.archived
                                    && children.contains(&thread.parent_id)
                            })
                            .map(|thread| thread.id)
                            .collect();
                        children.into_iter().chain(threads).collect()
                    })
                    .unwrap_or_default();
                for channel in targets {
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
            (
                act @ (Act::Invite
                | Act::CopyLink
                | Act::RtcVoiceActivity
                | Act::RtcPushToTalk
                | Act::PopOutCall
                | Act::ShowVoiceUsers),
                Target::RtcChannel { channel, .. },
            ) => self.run_rtc_channel(act, channel, window, cx),
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
                self.open_settings_section(dc_intl::tr("Data & Privacy"), cx)
            }
            (Act::EditProfile, _) => self.open_settings_section(dc_intl::tr("Account"), cx),
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
            // Module 740024. GAP: an expired signed address is not refreshed first (`AN` of module 679164).
            (Act::CopyLink, Target::AttachmentLink) => {
                if let Some((url, _)) = self.context.attachment_link.take() {
                    cx.write_to_clipboard(ClipboardItem::new_string(url));
                }
            }
            (Act::Copy, Target::AttachmentLink) => {
                if let Some((_, name)) = self.context.attachment_link.take() {
                    cx.write_to_clipboard(ClipboardItem::new_string(name));
                }
            }
            (Act::CopyLink, Target::ChannelMention(channel)) => {
                let guild = self.guild_of_channel(channel);
                cx.write_to_clipboard(ClipboardItem::new_string(
                    super::chat_notices::channel_link(guild, channel, None),
                ));
            }
            // Not in the pinned client: CopyUserURLs' "Copy User URL" (`plugin_hooks.rs`).
            (Act::CopyLink, Target::User { user, .. }) => {
                super::plugin_hooks::copy_user_url(user, cx)
            }
            (Act::CopyLink, Target::Dm(channel)) => {
                if let Some(user) = self.dm_user(channel) {
                    super::plugin_hooks::copy_user_url(user, cx);
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
                    | Target::Dm(id)
                    | Target::ChannelMention(id)
                    | Target::AuditChannel(id) => id,
                    Target::User { user, .. }
                    | Target::InventoryAuthor { user }
                    | Target::SpotifyActivity { user, .. }
                    | Target::GameActivity { user, .. } => user,
                    Target::AppDirectory(id)
                    | Target::LauncherApp(id)
                    | Target::GameProfile(id)
                    | Target::GameAddToProfile(id)
                    | Target::PickerSticker(id)
                    | Target::ForumTag(id)
                    | Target::RoleChip(id) => id,
                    Target::PermissionRow { target, .. } => target,
                    Target::ProgressBar(guild) | Target::RaidAlert(guild) => guild,
                    Target::RtcChannel { channel, .. } => channel,
                    Target::Role { role, .. } | Target::PluginPermissionRole { role, .. } => role,
                    Target::PluginPermissionUser(user) => user,
                    Target::PinCategory(_) | Target::Greet { .. } | Target::PluginBadge => return,
                    Target::ProfileWidget { .. }
                    | Target::AttachmentLink
                    | Target::AccountPanel
                    | Target::InventoryFeed
                    | Target::InventoryFeedAtPointer
                    | Target::Composer
                    | Target::Favorites
                    | Target::PickerEmoji
                    | Target::PickerSound
                    | Target::InviteRoles => {
                        return;
                    }
                };
                cx.write_to_clipboard(ClipboardItem::new_string(id.to_string()));
            }
            (
                Act::EditChannel,
                Target::Channel(channel)
                | Target::Category(channel)
                | Target::Thread(channel)
                | Target::AuditChannel(channel),
            ) => {
                self.open_channel_settings(channel, window, cx);
            }
            // Jump to Channel of module 194195: `uh(guild, channel)`, then the settings close (`jH`).
            (Act::GoToServer, Target::AuditChannel(channel)) => {
                if let Some(guild) = self.guild_of_channel(channel) {
                    self.server_settings = None;
                    self.select(guild, Some(channel));
                }
            }
            (Act::DeleteChannel, Target::Channel(channel) | Target::Category(channel)) => {
                self.open_channel_settings(channel, window, cx);
                if let Some(open) = self.channel_settings.as_mut() {
                    open.confirm_delete = true;
                }
            }
            // Module 307623 opens the "Delete Thread" confirmation (894533); the request goes only from its button.
            (Act::DeleteChannel, Target::Thread(thread)) => {
                let name = self
                    .stores
                    .guilds
                    .iter()
                    .find_map(|guild| guild.threads.iter().find(|known| known.id == thread))
                    .map(|known| known.name.clone())
                    .unwrap_or_default();
                self.ask_destructive(
                    super::destructive::Pending::delete_thread(thread, &name),
                    cx,
                );
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
            // Module 217563: the entrance sound is skipped (`SOUNDBOARD_MUTE_JOIN_SOUND`) and the channel joined. This
            // client plays no entrance sounds, so the join is the plain one.
            (Act::JoinQuiet, Target::Channel(channel)) => {
                if let Some(guild) = self.guild_of_channel(channel) {
                    self.join_voice(guild, channel, cx);
                }
            }
            (Act::ReportStage, Target::Channel(channel)) => self.open_report_stage(channel, cx),
            (Act::OpenChat, Target::Channel(channel)) => {
                if let Some(guild) = self.guild_of_channel(channel) {
                    self.select(guild, Some(channel));
                }
            }
            // Modules 182061 (close / open), 707792 (lock / unlock) and 41402 (pin / unpin), as module 7909 sends them.
            (
                Act::CloseThread
                | Act::OpenThread
                | Act::LockThread
                | Act::UnlockThread
                | Act::UnpinPost,
                Target::Thread(thread),
            ) => {
                use super::chat_notices::ThreadStep;
                let step = match act {
                    Act::CloseThread => ThreadStep::Close,
                    Act::OpenThread => ThreadStep::Open,
                    Act::LockThread => ThreadStep::Lock,
                    Act::UnlockThread => ThreadStep::Unlock,
                    _ => ThreadStep::Unpin,
                };
                self.run_thread_step(thread, step);
            }
            (Act::PinPost, Target::Thread(thread)) => self.pin_post(thread),
            // Module 707653: `setInvitable(thread, !invitable)`, `PATCH /channels/:id {invitable}` (module 7909); the
            // gateway's THREAD_UPDATE brings the new value back.
            (Act::ToggleInvitable, Target::Thread(thread)) => {
                let invitable = self
                    .stores
                    .guilds
                    .iter()
                    .flat_map(|guild| guild.threads.iter())
                    .find(|known| known.id == thread)
                    .is_none_or(|known| known.invitable);
                let _ = self.requests.send(Request::Rest {
                    key: None,
                    method: reqwest::Method::PATCH,
                    route: format!("/channels/{thread}"),
                    body: Some(serde_json::json!({ "invitable": !invitable })),
                });
            }
            // `joinThread` (module 7909): `POST /channels/:id/thread-members/@me?location=Context Menu`; the gateway's
            // THREAD_MEMBER_UPDATE puts the account into the joined-threads store.
            (Act::JoinThread, Target::Thread(thread)) => {
                let _ = self.requests.send(Request::Rest {
                    key: None,
                    method: reqwest::Method::POST,
                    route: format!("/channels/{thread}/thread-members/@me?location=Context%20Menu"),
                    body: None,
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
            (Act::SecurityActions, Target::Guild(guild) | Target::RaidAlert(guild)) => {
                self.open_security_actions(guild, cx)
            }
            (Act::ModerationSettings, Target::RaidAlert(guild)) => {
                self.open_server_settings_at(guild, super::server_settings::Page::SafetySetup, cx)
            }
            (Act::ReportRaid, Target::RaidAlert(guild)) => self.open_report_raid(guild, cx),
            (Act::ToggleProgressBar, Target::ProgressBar(guild)) => {
                self.toggle_boost_progress_bar(guild, cx)
            }
            (
                Act::RemovePermission,
                Target::PermissionRow {
                    channel, target, ..
                },
            ) => self.confirm_remove_permission(channel, target, cx),
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
            // Both ask "Ready to start a call?" first (module 502054, `runVoiceCallAction` of module 949674).
            (Act::Call, Target::Dm(channel)) => {
                self.start_call_or_ask(super::voice_alerts::CallTarget::Dm(channel), cx)
            }
            // "Start a Call" of a person's menu: their DM opens and the voice call rings there.
            (Act::Call, Target::User { user, .. }) => {
                self.start_call_or_ask(super::voice_alerts::CallTarget::User(user), cx)
            }
            // 313998: `ring(channel, [user], "user_context_menu")` and `stopRinging(channel, [user])`.
            (Act::RingUser | Act::StopRinging, Target::Dm(channel)) => {
                self.toggle_ring_channel(channel, cx);
            }
            (Act::RingUser | Act::StopRinging, Target::User { user, guild }) => {
                if guild.is_none()
                    && let Some(channel) = self
                        .dm_channel_for_user(user)
                        .filter(|channel| self.ring_row_for_channel(*channel).is_some())
                {
                    self.toggle_ring_channel(channel, cx);
                } else if let Some(channel) = self.resolved_dm_call_channel() {
                    match act {
                        Act::RingUser => {
                            self.ring_call(channel, Some(vec![user]), Some("user_context_menu"))
                        }
                        Act::StopRinging => self.stop_ringing(channel, Some(&[user])),
                        _ => {}
                    }
                }
            }
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
            // Module 121127: the sidebar opens on the member's info (`modViewPanel: INFO`).
            (
                Act::ModView,
                Target::User {
                    user,
                    guild: Some(guild),
                },
            ) => self.open_mod_view(guild, user, cx),
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
            // Modules 399476 and 239211: Ignore and Block open their confirmation first; Unignore and Unblock are at once.
            (Act::Ignore, Target::User { user, .. }) => self.toggle_ignore(user, cx),
            (Act::Ignore, Target::Dm(channel)) => {
                if let Some(user) = self.dm_user(channel) {
                    self.toggle_ignore(user, cx);
                }
            }
            (Act::Block, Target::User { user, .. }) => self.toggle_block(user, cx),
            (Act::Block, Target::Dm(channel)) => {
                if let Some(user) = self.dm_user(channel) {
                    self.toggle_block(user, cx);
                }
            }
            (Act::CloseDm, Target::Dm(channel)) => self.close_dm(channel, cx),
            (Act::EditGroup, Target::Dm(channel)) => self.open_group_edit(channel, window, cx),
            // The group's invite list is not built; the nearest thing is the picker that adds people.
            (Act::Invites, Target::Dm(channel)) => {
                self.select_home(Some(channel));
                self.open_add_to_dm(window, cx);
            }
            (Act::LeaveGroup, Target::Dm(channel)) => self.open_group_leave(channel, false, cx),
            (Act::VoiceMute | Act::VoiceDeafen, Target::User { user, .. })
                if self
                    .stores
                    .current_user
                    .as_ref()
                    .is_some_and(|me| me.id == user) =>
            {
                if act == Act::VoiceDeafen {
                    self.toggle_self_deaf(cx);
                } else {
                    self.toggle_self_mute(cx);
                }
            }
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
            (Act::DisableVideo, Target::User { user, .. }) => {
                let disabled = !super::call_menus::video_disabled(user);
                super::call_menus::set_video_disabled(user, disabled);
            }
            (
                Act::StageSpeak | Act::StageInvite | Act::StageLeave | Act::StageMoveToAudience,
                Target::User {
                    user,
                    guild: Some(guild),
                },
            ) => self.run_stage_user_act(act, guild, user),
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
            (Act::SubmitButton, Target::Composer) => {
                self.toggle_pref("enable_send_button_outside_experiment", false);
            }
            (Act::Spellcheck, Target::Composer) => {
                spellcheck::toggle();
                self.refresh_spelling(cx);
            }
            (Act::SpellLanguages, Target::Composer) => cx.open_url(SPELLCHECK_HELP),
            (Act::AiFixGrammar, Target::Composer) => self.composer_fix_grammar(cx),
            (Act::AiTranslate, Target::Composer) => self.composer_translate(None, cx),
            (Act::AiTransforms, Target::Composer) => {}
            (Act::Cut, Target::Composer) => self
                .composer
                .update(cx, |composer, cx| composer.cut_selection(cx)),
            (Act::Copy, Target::Composer) => self
                .composer
                .update(cx, |composer, cx| composer.copy_selection(cx)),
            (Act::Paste, Target::Composer) => self
                .composer
                .update(cx, |composer, cx| composer.paste_clipboard(cx)),
            // The caret of a muted target's row is gone: the row itself turns the mute off.
            (
                Act::Mute,
                Target::Guild(_)
                | Target::Channel(_)
                | Target::Category(_)
                | Target::Thread(_)
                | Target::Dm(_),
            ) => match target {
                Target::Guild(guild) => {
                    self.patch_guild_setting(guild, "muted", json!(false));
                    self.patch_guild_setting(
                        guild,
                        "mute_config",
                        json!({"selected_time_window": null, "end_time": null}),
                    );
                }
                // Unmute Thread (module 57907): the account's thread member setting, not a channel override.
                Target::Thread(id) => self.patch_thread_settings(id, json!({"muted": false})),
                Target::Channel(id) | Target::Category(id) => {
                    if let Some(guild) = self.guild_of_channel(id) {
                        self.patch_channel_override(
                            guild,
                            id,
                            json!({"muted": false, "mute_config": null}),
                        );
                    }
                }
                Target::Dm(id) => {
                    self.patch_channel_override(
                        0,
                        id,
                        json!({"muted": false, "mute_config": null}),
                    );
                }
                _ => {}
            },
            // Pinning also opts the channel in (`jA` of module 816662); unpinning only clears the pin.
            (Act::PinChannel, Target::Channel(id)) => {
                if let Some(guild) = self.guild_of_channel(id) {
                    let flags = self.stores.guild_settings.channel(guild, id).flags;
                    let flags = if flags & PINNED != 0 {
                        flags & !PINNED
                    } else {
                        flags | PINNED | OPT_IN_ENABLED
                    };
                    self.patch_channel_override(guild, id, json!({"flags": flags}));
                }
            }
            (Act::Favorite, Target::Channel(id) | Target::Thread(id) | Target::Dm(id)) => {
                let kind = match target {
                    Target::Dm(_) => self
                        .stores
                        .private_channels
                        .iter()
                        .find(|known| known.id == id)
                        .map_or(1, |known| known.kind),
                    _ => self
                        .guild_of_channel(id)
                        .and_then(|guild| self.stores.guild(guild))
                        .and_then(|guild| guild.channels.iter().find(|known| known.id == id))
                        .map_or(0, |known| channel_type_number(known.kind)),
                };
                let mut favorites = self.stores.favorites();
                let changed = if favorites.is_favorite(id) {
                    favorites.remove_channel(id)
                } else {
                    favorites.add_channel(id, Some(kind))
                };
                if changed {
                    let patch = self.stores.write_favorites(&favorites);
                    super::actions::log_effect(
                        "Add to Favorites",
                        "PATCH /users/@me/settings-proto/1",
                        patch.len(),
                    );
                    if let Some(request) = super::rail_folders::guild_folders_request(&patch) {
                        let _ = self.requests.send(request);
                    }
                }
            }
            // "Go to Server" of a favourite: the channel in its own server, outside the Favorites view.
            (Act::GoToServer, Target::Channel(id)) => {
                if let Some(guild) = self.guild_of_channel(id) {
                    self.select(guild, Some(id));
                    self.request_images();
                    cx.notify();
                }
            }
            (Act::HideFavorites, Target::Favorites) => self.set_favorites_guild(false, cx),
            (Act::FavoritesSettings, Target::Favorites) => self.open_favorites_settings(cx),
            (Act::FavoritesNickname, Target::Channel(id)) => {
                self.open_favorite_nickname(id, window, cx)
            }
            // A direct message is pinned by its own override on the "@me" settings (`xl` of module 816662).
            (Act::PinDm, Target::Dm(id)) => {
                let flags = self.stores.guild_settings.channel(0, id).flags ^ PINNED;
                self.patch_channel_override(0, id, json!({"flags": flags}));
            }
            (Act::ClearStatus, Target::Channel(id)) => {
                self.clear_channel_status(id);
            }
            (Act::HideNames, Target::Channel(id)) => {
                let mut stored = crate::storage::get(HIDDEN_NAMES).unwrap_or_else(|| json!({}));
                if self.names_hidden(id) {
                    if let Some(map) = stored.as_object_mut() {
                        map.remove(&id.to_string());
                    }
                } else {
                    stored[id.to_string()] = json!(true);
                }
                crate::storage::set(HIDDEN_NAMES, stored);
            }
            // A pair that reaches here is a row built for a target it has no handler for; the debug assertion below
            // reports it.
            (act, target) => {
                debug_assert!(
                    status(act) != Status::Wired,
                    "context row {act:?} has no handler for {target:?}"
                );
            }
        }
    }

    /// Applies a person's volume and mute to the call's mixer, and remembers them for the menu.
    pub(super) fn set_user_mix(&mut self, user: Id, volume: f32, muted: bool) {
        // VolumeBooster on: up to its slider top. Off: as the pinned client, the voice engine stops at 2.
        let cap = super::plugin_hooks::user_volume_cap();
        let volume = cap.map_or(volume, |cap| volume.min(cap));
        if volume == 1. && !muted {
            self.context.mix.remove(&user);
        } else {
            self.context.mix.insert(user, (volume, muted));
        }
        if let Some(call) = &self.call {
            call.controls
                .set_user_mix(user, volume.min(cap.unwrap_or(2.)), muted);
        }
    }

    /// The store changed: the composer draws its underlines again (a learned word is gone from them at once).
    fn refresh_spelling(&mut self, cx: &mut Context<Self>) {
        self.composer.update(cx, |_, cx| cx.notify());
        cx.notify();
    }

    pub(super) fn dm_user(&self, channel: Id) -> Option<Id> {
        self.stores
            .private_channels
            .iter()
            .find(|known| known.id == channel)
            .and_then(|dm| self.stores.dm_recipient(dm))
            .map(|user| user.id)
    }

    fn dm_channel_for_user(&self, user: Id) -> Option<Id> {
        self.stores
            .private_channels
            .iter()
            .find(|dm| dm.kind == 1 && dm.recipients == [user])
            .map(|dm| dm.id)
    }

    fn toggle_block(&mut self, user: Id, cx: &mut Context<Self>) {
        if self.stores.relationships.get(&user) == Some(&2) {
            self.unblock_user(user, cx);
        } else {
            self.remediation = Some((user, super::remediation::Remedy::Block));
            cx.notify();
        }
    }

    fn toggle_ignore(&mut self, user: Id, cx: &mut Context<Self>) {
        if self.stores.is_ignored(user) {
            self.unignore_user(user, cx);
        } else {
            self.remediation = Some((user, super::remediation::Remedy::Ignore));
            cx.notify();
        }
    }

    /// The rows of a submenu: mute durations, notification levels, roles, voice channels or servers.
    pub(super) fn context_sub_rows(&self, act: Act, target: Target) -> Vec<SubRow> {
        let plain = |label: &str, on: bool| SubRow {
            label: label.to_owned(),
            on,
            ..SubRow::default()
        };
        match act {
            // `command-suggestions` of module 194400: Slash Commands, checked by the account's setting.
            // `translate-{locale}`: every language the client has, disabled while a request is out.
            Act::AiTranslate => super::composer_ai::languages()
                .into_iter()
                .map(|(_, name)| SubRow {
                    label: name,
                    disabled: self.composer_ai.busy.is_some(),
                    ..SubRow::default()
                })
                .collect(),
            // `text-transform-{id}` then Manage.
            Act::AiTransforms => {
                let busy = match &self.composer_ai.busy {
                    Some(super::composer_ai::Busy::Transform(name)) => Some(name.clone()),
                    _ => None,
                };
                let mut rows: Vec<SubRow> = super::composer_ai::transforms()
                    .into_iter()
                    .map(|t| SubRow {
                        label: if busy.as_deref() == Some(t.name.as_str()) {
                            format!("Applying {}", t.name)
                        } else {
                            t.name
                        },
                        disabled: self.composer_ai.busy.is_some(),
                        ..SubRow::default()
                    })
                    .collect();
                rows.push(plain(dc_intl::tr("Manage"), false));
                rows
            }
            Act::Suggestions => vec![plain(
                dc_intl::tr("Slash Commands"),
                self.pref_on("show_command_suggestions", true),
            )],
            Act::Roles => match target {
                Target::User {
                    user,
                    guild: Some(guild),
                } => self.role_sub_rows(guild, user),
                _ => Vec::new(),
            },
            // Module 173500 for a person: the user commands of the loaded index, else "No Commands Available".
            Act::Apps => {
                let commands = self.user_commands();
                if commands.is_empty() {
                    vec![SubRow {
                        label: dc_intl::tr("No Commands Available").to_owned(),
                        disabled: true,
                        ..SubRow::default()
                    }]
                } else {
                    commands
                        .into_iter()
                        .map(|(command, _)| SubRow {
                            label: command["name"].as_str().unwrap_or_default().to_owned(),
                            ..SubRow::default()
                        })
                        .collect()
                }
            }
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
                .invite_servers(match target {
                    Target::Dm(channel) => self.dm_user(channel),
                    Target::User { user, .. } => Some(user),
                    _ => None,
                })
                .into_iter()
                .map(|(id, name, _)| SubRow {
                    label: name,
                    id,
                    ..SubRow::default()
                })
                .collect(),
            Act::Mute => MUTE_FOR
                .iter()
                .map(|(_, label)| plain(dc_intl::tr(label), false))
                .collect(),
            // `EU` (module 37411) with the ticked one of 607508's `l`: a thread has three levels, no inherited one.
            Act::Notifications if matches!(target, Target::Thread(_)) => {
                let Target::Thread(id) = target else {
                    return Vec::new();
                };
                let current = self.thread_level_of(id);
                THREAD_LEVELS
                    .iter()
                    .map(|(flag, label)| SubRow {
                        radio: true,
                        ..plain(dc_intl::tr(label), current == *flag)
                    })
                    .collect()
            }
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
                    // What "Use Category Default" would give: the category's level, else the server's.
                    let inherited = match target {
                        Target::Channel(id) | Target::Thread(id) => self.inherited_level_label(id),
                        _ => dc_intl::t!("n/bTaY"),
                    };
                    rows.push(SubRow {
                        sub: Some(inherited.to_owned()),
                        ..plain(dc_intl::tr("Use Category Default"), current == INHERIT)
                    });
                }
                rows.extend(
                    LEVELS
                        .iter()
                        .map(|(value, label)| plain(dc_intl::tr(label), current == *value)),
                );
                for row in &mut rows {
                    row.radio = true;
                }
                rows
            }
            Act::MoveTo => self.move_rows(target),
            // Not in the pinned client: PinDMs' submenu (`plugin_hooks/pin_dms.rs`).
            Act::PluginPinDms => self.pin_dms_sub_rows(target),
            _ => Vec::new(),
        }
    }

    /// The level a channel gets from "Use Category Default": its category's own, else the server's.
    fn inherited_level_label(&self, channel: Id) -> &'static str {
        let Some(guild) = self.guild_of_channel(channel) else {
            return dc_intl::t!("n/bTaY");
        };
        let parent = self
            .stores
            .guild(guild)
            .and_then(|guild| guild.channels.iter().find(|known| known.id == channel))
            .and_then(|known| known.parent_id);
        match parent.map(|parent| self.channel_level(guild, parent)) {
            Some(own) if own != INHERIT => level_label(own),
            _ => self.guild_level_label(guild),
        }
    }

    /// Move to (module 906964): a server's First and Last in List; a channel's Category list and its First and Last in
    /// Category (First and Last in List for a category, or a channel outside any category).
    fn move_rows(&self, target: Target) -> Vec<SubRow> {
        let row = |label: &str, disabled: bool, caret: bool| SubRow {
            label: label.to_owned(),
            disabled,
            caret,
            ..SubRow::default()
        };
        match target {
            Target::Guild(guild) => match self.stores.folders.placement_disabled(guild) {
                Some((first, last)) => vec![
                    row(dc_intl::tr("First in List"), first, false),
                    row(dc_intl::tr("Last in List"), last, false),
                ],
                None => Vec::new(),
            },
            Target::Channel(id) | Target::Category(id) => {
                let Some(channels) = self
                    .guild_of_channel(id)
                    .and_then(|guild| self.stores.guild(guild))
                    .map(|guild| guild.channels.as_slice())
                else {
                    return Vec::new();
                };
                let Some(me) = channels.iter().find(|known| known.id == id) else {
                    return Vec::new();
                };
                let siblings = move_siblings(channels, me);
                let mut rows = Vec::new();
                if me.kind != ChannelKind::Category {
                    let list = move_destinations(channels, me);
                    if list.iter().any(|(_, _, disabled)| !disabled) && list.len() > 1 {
                        rows.push(row(dc_intl::tr("Category"), false, true));
                    }
                }
                if siblings.len() > 1 {
                    // "First in List" / "First in Category" (`IMqgs9`, `Q9TKt6`) and their Last.
                    let list = me.kind == ChannelKind::Category || me.parent_id.is_none();
                    rows.push(row(
                        dc_intl::t(if list { "IMqgs9" } else { "Q9TKt6" }),
                        siblings.first() == Some(&id),
                        false,
                    ));
                    rows.push(row(
                        dc_intl::t(if list { "8fQe3x" } else { "/Pkxmw" }),
                        siblings.last() == Some(&id),
                        false,
                    ));
                }
                rows
            }
            _ => Vec::new(),
        }
    }

    /// The list a channel's "Category" row opens: Uncategorized, then every category; the one it is in is disabled.
    fn category_rows(&self, target: Target) -> Vec<SubRow> {
        let Target::Channel(id) = target else {
            return Vec::new();
        };
        let Some(channels) = self
            .guild_of_channel(id)
            .and_then(|guild| self.stores.guild(guild))
            .map(|guild| guild.channels.as_slice())
        else {
            return Vec::new();
        };
        let Some(me) = channels.iter().find(|known| known.id == id) else {
            return Vec::new();
        };
        move_destinations(channels, me)
            .into_iter()
            .map(|(id, label, disabled)| SubRow {
                label,
                disabled,
                id,
                ..SubRow::default()
            })
            .collect()
    }

    /// Sends a channel's reorder: the changed positions (and the new category) to the server.
    fn send_channel_move(&mut self, channel: Id, parent: Option<Option<Id>>, first: bool) {
        let Some(guild) = self.guild_of_channel(channel) else {
            return;
        };
        let Some(channels) = self.stores.guild(guild).map(|guild| guild.channels.clone()) else {
            return;
        };
        let updates = plan_channel_move(&channels, channel, parent, first);
        if updates.is_empty() {
            return;
        }
        let body: Vec<serde_json::Value> = updates
            .into_iter()
            .map(|(id, parent, position)| match parent {
                Some(parent) => json!({"id": id.to_string(), "position": position,
                    "parent_id": parent.map(|parent| parent.to_string())}),
                None => json!({"id": id.to_string(), "position": position}),
            })
            .collect();
        super::actions::log_effect("Move to", "PATCH /guilds/{id}/channels", body.len());
        let _ = self.requests.send(Request::Rest {
            key: None,
            method: reqwest::Method::PATCH,
            route: format!("/guilds/{guild}/channels"),
            body: Some(json!(body)),
        });
    }

    /// Servers we may create an invite in, with the channel to invite to (module 777933's invite list): a text
    /// channel we can see, in a server where we hold Create Invite.
    pub(super) fn invite_servers(&self, person: Option<Id>) -> Vec<(Id, String, Id)> {
        self.stores
            .guilds
            .iter()
            // Someone who is already in the server needs no invite to it.
            .filter(|guild| {
                person.is_none_or(|user| self.stores.members.get(guild.id, user).is_none())
            })
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

    /// A destination of a channel's Move to > Category: the channel goes last in that category.
    fn run_category_move(&mut self, target: Target, index: usize) {
        let Target::Channel(id) = target else {
            return;
        };
        let Some(row) = self.category_rows(target).into_iter().nth(index) else {
            return;
        };
        if row.disabled {
            return;
        }
        let parent = (row.id != 0).then_some(row.id);
        self.send_channel_move(id, Some(parent), false);
    }

    pub(super) fn run_context_sub(
        &mut self,
        act: Act,
        target: Target,
        index: usize,
        cx: &mut Context<Self>,
    ) {
        let row = self.context_sub_rows(act, target).into_iter().nth(index);
        match act {
            // Not in the pinned client: PinDMs' submenu (`plugin_hooks/pin_dms.rs`).
            Act::PluginPinDms => {
                self.pin_dms_run_sub(target, index, cx);
            }
            Act::AiTranslate => {
                if let Some((code, name)) = super::composer_ai::languages().into_iter().nth(index) {
                    self.composer_translate(Some((code, name)), cx);
                }
            }
            Act::AiTransforms => {
                if index < super::composer_ai::transforms().len() {
                    self.composer_transform(index, cx);
                } else {
                    self.composer_ai.manage_open = true;
                    cx.notify();
                }
            }
            Act::Suggestions => {
                if row.is_some() {
                    self.toggle_pref("show_command_suggestions", true);
                }
            }
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
            Act::Apps => {
                if let (Some(_), Target::User { user, .. }) = (row, target)
                    && let Some((command, _)) = self.user_commands().into_iter().nth(index)
                {
                    self.run_user_command(&command, user);
                }
                return;
            }
            Act::MoveTo => {
                let Some(row) = row else {
                    return;
                };
                if row.disabled || row.caret {
                    return;
                }
                let first = ["IMqgs9", "Q9TKt6", "9qpRrY"]
                    .iter()
                    .any(|key| row.label == dc_intl::t(key));
                match target {
                    Target::Guild(guild) => self.move_guild(
                        guild,
                        if first {
                            dc_state::folders::Placement::First
                        } else {
                            dc_state::folders::Placement::Last
                        },
                        cx,
                    ),
                    Target::Channel(id) | Target::Category(id) => {
                        self.send_channel_move(id, None, first)
                    }
                    _ => {}
                }
                return;
            }
            Act::InviteToServer => {
                let user = match target {
                    Target::Dm(channel) => self.dm_user(channel),
                    Target::User { user, .. } => Some(user),
                    _ => None,
                };
                let (Some(row), Some(user)) = (row, user) else {
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
                    Target::Thread(id) => {
                        self.patch_thread_settings(id, Self::thread_mute_body(seconds));
                    }
                    Target::Channel(id) | Target::Category(id) => {
                        if let Some(guild) = self.guild_of_channel(id) {
                            self.patch_channel_override(
                                guild,
                                id,
                                json!({"muted": true, "mute_config": config}),
                            );
                        }
                    }
                    Target::Dm(id) => {
                        self.patch_channel_override(
                            0,
                            id,
                            json!({"muted": true, "mute_config": config}),
                        );
                    }
                    _ => {}
                }
            }
            Act::Notifications => match target {
                Target::Thread(id) => {
                    if let Some((flag, _)) = THREAD_LEVELS.get(index) {
                        self.patch_thread_settings(id, json!({"flags": flag}));
                    }
                }
                Target::Guild(guild) => {
                    let value = LEVELS[index].0;
                    self.patch_guild_setting(guild, "message_notifications", json!(value));
                }
                Target::Channel(id) | Target::Category(id) => {
                    let channel = matches!(target, Target::Channel(_));
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
                Entry::Heading(text) => rows.push(
                    div()
                        .h(px(HEADING_HEIGHT))
                        .flex_none()
                        .px(px(8.))
                        .flex()
                        .items_center()
                        .text_size(px(12.))
                        .line_height(px(16.))
                        .font_weight(FontWeight(600.))
                        .text_color(color("--text-muted"))
                        .child(*text)
                        .into_any_element(),
                ),
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
                // member-menu/user-guild, self-guild: a menu that would pass the right edge opens to the left of the
                // pointer (right edge at the pointer); vertically it keeps 8px from the window.
                .flip_horizontal_snap_with_margin(px(8.))
                .child(
                    div()
                        .parity_with_border("context-menu", 1.)
                        .id("context-menu-generic")
                        .when_some(menu_aria_label(target), |menu, label| {
                            menu.aria_label(label)
                        })
                        .relative()
                        .child(self.anchor_probe("context-menu-generic"))
                        .when(
                            matches!(
                                target,
                                Target::InventoryAuthor { .. } | Target::SpotifyActivity { .. }
                            ),
                            |d| d.min_w(px(144.)),
                        )
                        .when(
                            !matches!(
                                target,
                                Target::InventoryAuthor { .. }
                                    | Target::SpotifyActivity { .. }
                                    | Target::InventoryFeed
                                    | Target::AppDirectory(_)
                            ),
                            |d| d.min_w(px(188.)),
                        )
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
                                let in_sub = ["submenu", "submenu2"].iter().any(|key| {
                                    this.tip_anchors
                                        .borrow()
                                        .get(*key)
                                        .is_some_and(|bounds| bounds.contains(&event.position))
                                });
                                if !in_sub
                                    && this.press_outside(super::keyboard::Layer::ContextMenu, cx)
                                    && event.button != gpui::MouseButton::Right
                                {
                                    cx.stop_propagation();
                                }
                            },
                        ))
                        // `flexible_M3qk4j`: the menu grows with its labels up to 320 wide.
                        .max_w(px(320.))
                        // A menu taller than the window stops 96px short of its height and scrolls (user-guild: 624 of 720).
                        .max_h(px(
                            (f32::from(window.viewport_size().height) - 96.).max(120.)
                        ))
                        .flex()
                        .flex_col()
                        .child(
                            div()
                                .id("context-menu-scroll")
                                .track_scroll(&self.menu_scroll)
                                .min_h(px(0.))
                                .overflow_y_scroll()
                                .p(px(8.))
                                .flex()
                                .flex_col()
                                .children(rows.into_iter().map(|row| div().flex_none().child(row))),
                        )
                        .child(super::menu::menu_scrollbar(self.menu_scroll.clone())),
                ),
        )
        .with_priority(2)
        .into_any_element();
        let sub = open.sub.and_then(|(act, row)| {
            let labels = self.context_sub_rows(act, target);
            if labels.is_empty() {
                return None;
            }
            // A radio list is the text plus the 22px radio, 7px apart, with 17px and 15px of padding (the notification
            // submenus: 190 for 129 of text).
            let radio_list = labels.iter().any(|sub| sub.radio);
            let widest_label = labels
                .iter()
                .filter(|sub| sub.dot.is_none())
                .map(|sub| label_width(window, &sub.label))
                .fold(0., f32::max);
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
                        sub: second,
                        radio,
                        caret,
                        ..
                    } = sub;
                    let role = dot.is_some();
                    if radio || caret {
                        let height = if second.is_some() { 52. } else { 36. };
                        let mut item = div()
                            .id(SharedString::from(format!("context-sub-{index}")))
                            .relative()
                            .h(px(height))
                            .py(px(8.))
                            .pl(px(8.))
                            .pr(px(6.))
                            .rounded(px(2.))
                            .cursor_pointer()
                            .hover(|style| {
                                style
                                    .bg(color("--interactive-background-hover"))
                                    .rounded(px(4.))
                            })
                            .flex()
                            .items_center()
                            .opacity(if disabled { 0.5 } else { 1. })
                            .child(self.anchor_probe(format!("context-sub-{index}")))
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
                                            .truncate()
                                            .text_color(color("--text-strong"))
                                            .child(label),
                                    )
                                    .children(second.map(|text| {
                                        div()
                                            .mt(px(2.))
                                            .text_size(px(12.))
                                            .line_height(px(16.))
                                            .text_color(color("--text-muted"))
                                            .child(text)
                                    })),
                            );
                        if radio {
                            item = item.child(
                                div()
                                    .ml(px(7.))
                                    .flex_none()
                                    .size(px(22.))
                                    .rounded_full()
                                    .border_2()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .border_color(if on {
                                        color("--control-brand-foreground")
                                    } else {
                                        color("--checkbox-border-default")
                                    })
                                    .bg(if on {
                                        color("--control-brand-foreground")
                                    } else {
                                        color("--checkbox-background-default")
                                    })
                                    .when(on, |d| {
                                        d.child(div().size(px(8.)).rounded_full().bg(gpui::white()))
                                    }),
                            );
                        }
                        if caret {
                            item = item.child(div().ml(px(10.)).child(icon(
                                "menu_caret",
                                18.,
                                "--interactive-text-default",
                            )));
                            let probe = format!("context-sub-{index}");
                            item = item.on_hover(cx.listener(move |this, inside: &bool, _, cx| {
                                if !*inside {
                                    return;
                                }
                                let bounds = this.tip_anchors.borrow().get(probe.as_str()).copied();
                                if let Some(open) = this.context.open.as_mut() {
                                    open.nested = if disabled {
                                        None
                                    } else {
                                        bounds.map(|bounds| (index, bounds))
                                    };
                                }
                                cx.notify();
                            }));
                        }
                        if !plain && !disabled && !caret {
                            item = item.on_click(cx.listener(move |this, _, _, cx| {
                                this.run_context_sub(act, target, index, cx);
                                this.close_context_menu(cx);
                            }));
                        }
                        return (item.into_any_element(), height);
                    }
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
                    if act == Act::MoveTo {
                        item = item.on_hover(cx.listener(|this, inside: &bool, _, cx| {
                            if *inside {
                                if let Some(open) = this.context.open.as_mut() {
                                    open.nested = None;
                                }
                                cx.notify();
                            }
                        }));
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
            // 14px medium labels with 16px of padding either side and the 1px border (invite-sub: 212 for 179 of text).
            let widest = widest_label.max(0.);
            let width = if radio_list {
                widest + 61.
            } else {
                widest + 34.
            };
            // `submenu_M3qk4j`: 188 to 320 wide.
            Some(self.submenu_frame_wide(rows, row, window, width.clamp(188., 320.)))
        });
        let nested = open.nested.and_then(|(_, row)| {
            if open.sub.map(|(act, _)| act) != Some(Act::MoveTo) {
                return None;
            }
            let labels = self.category_rows(target);
            if labels.is_empty() {
                return None;
            }
            let widest = labels
                .iter()
                .map(|sub| label_width(window, &sub.label))
                .fold(0., f32::max);
            let width = (widest + 34.).clamp(188., 320.);
            let rows: Vec<AnyElement> = labels
                .into_iter()
                .enumerate()
                .map(|(index, sub)| {
                    let disabled = sub.disabled;
                    let mut item = self
                        .submenu_item(
                            SharedString::from(format!("context-nested-{index}")),
                            None,
                            sub.label,
                            34.,
                        )
                        .opacity(if disabled { 0.5 } else { 1. });
                    if !disabled {
                        item = item.on_click(cx.listener(move |this, _, _, cx| {
                            this.run_category_move(target, index);
                            this.close_context_menu(cx);
                        }));
                    }
                    item.into_any_element()
                })
                .collect();
            let height = 2. + 16. + 34. * rows.len() as f32;
            let viewport = window.viewport_size();
            let left = if f32::from(row.right()) + 4. + width <= f32::from(viewport.width) - 8. {
                f32::from(row.right()) + 4.
            } else {
                f32::from(row.left()) - 4. - width
            };
            let top = (f32::from(row.top()) - 9.)
                .min(f32::from(viewport.height) - 8. - height)
                .max(8.);
            Some(
                deferred(
                    div()
                        .id("submenu2")
                        .child(self.anchor_probe("submenu2"))
                        .occlude()
                        .absolute()
                        .left(px(left))
                        .top(px(top))
                        .w(px(width))
                        .rounded(px(8.))
                        .border_1()
                        .border_color(color("--border-subtle"))
                        .bg(color("--background-surface-higher"))
                        .shadow(vec![gpui::BoxShadow {
                            color: gpui::hsla(0., 0., 0., 0.24),
                            offset: gpui::point(px(0.), px(12.)),
                            blur_radius: px(24.),
                            spread_radius: px(0.),
                            inset: false,
                        }])
                        .p(px(8.))
                        .flex()
                        .flex_col()
                        .children(rows),
                )
                .with_priority(3)
                .into_any_element(),
            )
        });
        Some(
            div()
                .absolute()
                .top(px(0.))
                .left(px(0.))
                .child(self.menu_backdrop(cx))
                .child(menu)
                .children(sub)
                .children(nested)
                .into_any_element(),
        )
    }

    /// User Volume: shared `58px` menu slider (`0..=200%` perceptual curve) with live `%` thumb bubble.
    fn volume_row(&self, row: &Row, target: Target, cx: &mut Context<Self>) -> AnyElement {
        let user = match target {
            Target::User { user, .. } => user,
            Target::Dm(channel) => match self.dm_user(channel) {
                Some(user) => user,
                None => return div().into_any_element(),
            },
            _ => return div().into_any_element(),
        };
        let factor = self.context.mix.get(&user).map_or(1., |mix| mix.0);
        // 200, or more with the VolumeBooster plugin.
        let top = super::plugin_hooks::user_volume_max();
        let value = super::menu_slider::amplitude_to_perceptual(factor, top);
        self.menu_slider_row(
            super::menu_slider::MenuSliderConfig {
                id: format!("context-volume-{user}").into(),
                label: row.label.clone(),
                min: 0,
                max: top,
                value,
                palette: super::menu_slider::ThemePalette::Default,
                format: super::menu_slider::percent_format(),
                on_change: std::rc::Rc::new(move |this, pct, cx| {
                    let amp = super::menu_slider::perceptual_to_amplitude(pct, top);
                    let muted = this.context.mix.get(&user).is_some_and(|mix| mix.1);
                    this.set_user_mix(user, amp, muted);
                    cx.notify();
                }),
            },
            cx,
        )
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
        if let Some(side) = match row.act {
            Act::InputVolume => Some(super::audio_menu::Side::Input),
            Act::OutputVolume => Some(super::audio_menu::Side::Output),
            _ => None,
        } {
            return self.rtc_volume_row(row, side, cx);
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
        let is_check = row.check.is_some();
        let is_radio = row.radio;
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
        .when(act == Act::ViewAllActivity, |this| {
            this.parity("activity-menu-row")
        })
        .child(self.anchor_probe(probe))
        // `labelContainer_M3qk4j` has a min-height: a wrapped second line grows the row.
        .min_h(px(row.height()))
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
                let next = if has_sub {
                    bounds.map(|bounds| (act, bounds))
                } else {
                    None
                };
                if next.map(|(act, _)| act) != open.sub.map(|(act, _)| act) {
                    open.nested = None;
                }
                open.sub = next;
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
            if !is_check {
                this.close_context_menu(cx);
            } else if is_radio {
                // A radio item stays open and its group follows the pick.
                if let Some(open) = this.context.open.as_mut() {
                    for (at, entry) in open.items.iter_mut().enumerate() {
                        if let Entry::Row(entry) = entry
                            && entry.radio
                        {
                            entry.check = Some(at == index);
                        }
                    }
                }
            } else if let Some(open) = this.context.open.as_mut()
                && let Some(Entry::Row(entry)) = open.items.get_mut(index)
                && let Some(checked) = entry.check.as_mut()
            {
                *checked = !*checked;
            }
            this.run_context(act, target, window, cx);
            cx.notify();
        }))
        .children(
            row.status_dot
                .map(|on| div().mr(px(8.)).child(status_dot(on, tint.into()))),
        )
        .children(row.role_dot.map(|color| {
            // `roleDotAccessory_M3qk4j` around `roleCircle_7RsUnm` (12px, `margin: 0 4px`).
            div()
                .mr(px(8.))
                .size(px(20.))
                .flex_none()
                .flex()
                .items_center()
                .justify_center()
                .child(
                    div()
                        .size(px(12.))
                        .rounded_full()
                        .bg(gpui::rgb(if color == 0 { 0x99aab5 } else { color })),
                )
        }))
        .children(row.icon.map(|name| {
            div().mr(px(8.)).child(icon(
                name,
                // `iconContainerLeft_M3qk4j`: 20px in every captured menu (menus2/dm-row, channel-row,
                // favorites-rail-menu, guild-header-dropdown), then an 8px gap to the label.
                20.,
                // A danger row's icon takes the row's critical colour (frame menus2/favorites-rail-menu).
                if danger {
                    "--text-feedback-critical"
                } else {
                    "--interactive-text-default"
                },
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
                        .when(act == Act::ViewAllActivity, |this| {
                            this.parity("activity-menu-label")
                        })
                        .text_size(px(14.))
                        .line_height(px(18.))
                        .font_weight(FontWeight(500.))
                        .text_color(tint)
                        // `label_M3qk4j`: one line, cut with an ellipsis; `subtext_M3qk4j` wraps.
                        .truncate()
                        .child(label),
                )
                .children(row.sub.clone().map(|sub| {
                    div()
                        .mt(px(2.))
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
        .children(row.check.filter(|_| !row.radio).map(menu_check_box))
        .children(
            row.check
                .filter(|_| row.radio)
                .map(|on| div().ml(px(8.)).child(super::create_channel::radio(on))),
        )
        .children(
            row.status_dot
                .map(|on| div().ml(px(8.)).child(status_dot(on, tint.into()))),
        )
        .children(row.trailing.filter(|_| !row.caret).map(|name| {
            div()
                .ml(px(8.))
                .child(icon(name, 18., "--interactive-text-default"))
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

/// A menu row's check box. An 8px gap between the label and the box in every captured menu (menus2/guild-icon:
/// "Hide Muted Channels" ends 8px before the box, the menu is 192 wide).
pub(super) fn menu_check_box(on: bool) -> gpui::Div {
    div()
        .ml(px(8.))
        .size(px(20.))
        .flex_none()
        .rounded(px(4.))
        .border_1()
        // The same tokens as the role check box; the golden's ticked box is rgb(88, 101, 242) and its empty box
        // rgb(37, 37, 41) inside (voice-user-menu/self-menu).
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
        .flex()
        .items_center()
        .justify_center()
        .when(on, |this| {
            this.child(icon("checkbox_check", 16., "--white"))
        })
}

#[cfg(test)]
mod tests {

    use super::Act;

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

    use super::*;

    fn labels(entries: &[Entry]) -> Vec<String> {
        entries
            .iter()
            .map(|entry| match entry {
                Entry::Row(row) => row.label.clone(),
                Entry::Separator => "-".to_owned(),
                Entry::Heading(text) => format!("# {text}"),
            })
            .collect()
    }

    #[test]
    fn audit_channel_menu_follows_module_194195() {
        assert_eq!(
            labels(&audit_channel_items(false, true, true)),
            ["Edit Channel", "Jump to Channel", "-", "Copy Channel ID"]
        );
        assert_eq!(
            labels(&audit_channel_items(true, true, false)),
            ["Edit Category"]
        );
        assert_eq!(
            labels(&audit_channel_items(false, false, false)),
            ["Jump to Channel"]
        );
    }

    #[test]
    fn picker_sticker_menu_follows_module_233503() {
        assert_eq!(
            labels(&picker_sticker_items(Some(true), false)),
            ["Unfavorite Sticker"]
        );
        assert_eq!(
            labels(&picker_sticker_items(Some(false), true)),
            [
                "Favorite Sticker",
                "Copy Sticker ID",
                "Copy Sticker Image Link"
            ]
        );
        assert!(picker_sticker_items(None, false).is_empty());
    }

    /// Modules 740024 and 960015, and the names of 949488's, 740024's and 960015's menus.
    #[test]
    fn link_options_and_forum_tag_menus_and_their_names() {
        assert_eq!(
            labels(&attachment_link_items()),
            ["Copy Media Link", "Copy Text"]
        );
        assert_eq!(labels(&forum_tag_items()), ["Copy Tag ID"]);
        // Modules 646938 and 50268: a profile role chip's developer menu.
        assert_eq!(labels(&role_chip_items()), ["Copy Role ID"]);
        assert_eq!(menu_aria_label(Target::RoleChip(1)), Some("Actions"));
        assert_eq!(menu_aria_label(Target::Thread(1)), Some("Thread Actions"));
        assert_eq!(
            menu_aria_label(Target::AttachmentLink),
            Some("Link Options")
        );
        assert_eq!(
            menu_aria_label(Target::ForumTag(1)),
            Some("Forum Tag Actions")
        );
        assert_eq!(menu_aria_label(Target::Channel(1)), None);
    }

    #[test]
    fn picker_emoji_menu_follows_module_233503() {
        assert_eq!(
            labels(&picker_emoji_items(Some(false), false, false)),
            ["Favorite Emoji"]
        );
        assert_eq!(
            labels(&picker_emoji_items(Some(true), false, true)),
            ["Unfavorite Emoji", "Copy Emoji"]
        );
        assert_eq!(
            labels(&picker_emoji_items(Some(false), true, true)),
            ["Favorite Emoji", "Copy Emoji ID", "Copy Emoji Image Link"]
        );
        assert!(picker_emoji_items(None, true, false).is_empty());
        assert_eq!(
            crate::ui::picker::emoji_image_link(7, true, 1.),
            "https://cdn.discordapp.com/emojis/7.webp?size=32&animated=true"
        );
        assert_eq!(
            crate::ui::picker::emoji_image_link(7, false, 1.25),
            "https://cdn.discordapp.com/emojis/7.webp?size=40"
        );
    }

    #[test]
    fn live_spotify_menu_respects_profile_surface_and_ownership() {
        assert_eq!(
            labels(&spotify_activity_items(false, true)),
            ["View All Activity"]
        );
        assert_eq!(
            labels(&spotify_activity_items(true, true)),
            ["View All Activity", "Manage Privacy"]
        );
        assert_eq!(
            labels(&spotify_activity_items(true, false)),
            ["Manage Privacy"]
        );
        assert!(spotify_activity_items(false, false).is_empty());
        assert!(matches!(
            spotify_activity_items(true, true).as_slice(),
            [
                Entry::Row(Row {
                    act: Act::ViewAllActivity,
                    ..
                }),
                Entry::Row(Row {
                    act: Act::ManageSpotifyPrivacy,
                    ..
                })
            ]
        ));
    }

    #[test]
    fn a_game_card_menu_reports_an_issue_and_manages_our_own_privacy() {
        assert_eq!(
            labels(&game_activity_items(false, false, false, false)),
            ["Report Issue"]
        );
        assert_eq!(
            labels(&game_activity_items(false, true, false, true)),
            ["View All Activity", "View Game Profile", "Report Issue"]
        );
        assert_eq!(
            labels(&game_activity_items(true, false, false, false)),
            ["Report Issue", "Manage Privacy"]
        );
        assert_eq!(
            labels(&game_activity_items(true, false, true, false)),
            ["Report Issue"]
        );
    }

    #[test]
    fn a_voice_channel_with_a_status_offers_to_clear_it_between_open_chat_and_hide_names() {
        let with = labels(&channel_items(
            ChannelFacts {
                voice: true,
                clear_status: true,
                ..ChannelFacts::default()
            },
            "All Messages",
        ));
        let open = with.iter().position(|label| label == "Open Chat").unwrap();
        assert_eq!(with[open + 1], "Clear Channel Status");
        assert_eq!(with[open + 2], "Hide Names");
        let without = labels(&channel_items(
            ChannelFacts {
                voice: true,
                ..ChannelFacts::default()
            },
            "All Messages",
        ));
        assert!(!without.iter().any(|label| label == "Clear Channel Status"));
    }

    #[test]
    fn the_active_now_voice_menu_is_the_channel_actions_of_module_113255() {
        assert_eq!(
            labels(&activity_voice_items(true, true, true)),
            [
                "Edit Channel",
                "-",
                "Invite to Voice",
                "Duplicate Channel",
                "Create Voice Channel",
                "-",
                "Delete Channel",
                "-",
                "Copy Channel ID"
            ]
        );
        assert_eq!(
            labels(&activity_voice_items(false, true, false)),
            ["Invite to Voice"]
        );
    }

    #[test]
    fn join_without_entrance_sound_leads_and_report_stage_ends_the_voice_actions() {
        let all = labels(&channel_items(
            ChannelFacts {
                voice: true,
                join_quiet: true,
                report_stage: true,
                ..ChannelFacts::default()
            },
            "All Messages",
        ));
        let quiet = all
            .iter()
            .position(|label| label == "Join Without Entrance Sound")
            .unwrap();
        assert_eq!(all[quiet + 1], "Open Chat");
        assert_eq!(all[quiet + 2], "Hide Names");
        assert_eq!(all[quiet + 3], "Report Stage");
        let plain = labels(&channel_items(
            ChannelFacts {
                voice: true,
                ..ChannelFacts::default()
            },
            "All Messages",
        ));
        assert!(
            !plain
                .iter()
                .any(|label| label == "Join Without Entrance Sound")
        );
        assert!(!plain.iter().any(|label| label == "Report Stage"));
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
    fn a_voice_channel_menu_has_no_notification_settings() {
        let items = channel_items(
            ChannelFacts {
                voice: true,
                can_manage: true,
                ..ChannelFacts::default()
            },
            "All Messages",
        );
        let shown = labels(&items);
        assert!(!shown.contains(&"Notification Settings".to_owned()));
        assert!(shown.contains(&"Hide Names".to_owned()));
        assert!(shown.contains(&"Mute Channel".to_owned()));
    }

    fn place(
        id: u64,
        kind: dc_state::ChannelKind,
        parent: Option<u64>,
        position: i64,
    ) -> dc_state::Channel {
        dc_state::Channel {
            id,
            name: format!("c{id}"),
            kind,
            position,
            parent_id: parent,
            last_message_id: None,
            overwrites: Vec::new(),
            user_limit: 0,
            topic: None,
            rate_limit_per_user: 0,
            flags: 0,
            nsfw: false,
            default_auto_archive_duration: None,
            available_tags: Vec::new(),
            default_sort_order: None,
            default_forum_layout: 0,
        }
    }

    #[test]
    fn moving_a_channel_renumbers_its_siblings() {
        use dc_state::ChannelKind::{Category, Text, Voice};
        let channels = [
            place(1, Category, None, 0),
            place(2, Category, None, 1),
            place(10, Text, Some(1), 0),
            place(11, Text, Some(1), 1),
            place(12, Text, Some(1), 2),
            place(20, Voice, Some(1), 0),
        ];
        // Last in its category: the others keep their places, only it moves (voice channels are a list of their own).
        assert_eq!(
            super::plan_channel_move(&channels, 10, None, false),
            [(11, None, 0), (12, None, 1), (10, None, 2)]
        );
        // First: nothing to do when it already is.
        assert!(super::plan_channel_move(&channels, 10, None, true).is_empty());
        // Into the other category: it goes last there and names its new parent.
        assert_eq!(
            super::plan_channel_move(&channels, 11, Some(Some(2)), false),
            [(11, Some(Some(2)), 0)]
        );
        // Out of every category.
        assert_eq!(
            super::plan_channel_move(&channels, 12, Some(None), false),
            [(12, Some(None), 0)]
        );
        // Categories move among themselves, never into one.
        assert_eq!(
            super::plan_channel_move(&channels, 2, None, true),
            [(2, None, 0), (1, None, 1)]
        );
        let me = &channels[2];
        let list = super::move_destinations(&channels, me);
        assert_eq!(
            list.iter()
                .map(|(id, _, off)| (*id, *off))
                .collect::<Vec<_>>(),
            [(0, false), (1, true), (2, false)]
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
    fn a_menu_that_fits_above_the_pointer_opens_there() {
        let at = gpui::point(px(600.), px(682.));
        // The composer menu: 228px high, its bottom at the pointer.
        assert_eq!(flip_above(at, 228., 720.), gpui::point(px(600.), px(454.)));
        // Room below: unchanged.
        assert_eq!(
            flip_above(gpui::point(px(600.), px(100.)), 228., 720.),
            gpui::point(px(600.), px(100.))
        );
        // Too tall for either side: unchanged, the window snaps it.
        assert_eq!(
            flip_above(gpui::point(px(600.), px(300.)), 600., 720.),
            gpui::point(px(600.), px(300.))
        );
    }

    #[test]
    fn heights_follow_the_captures() {
        let items = channel_items(ChannelFacts::default(), "All Messages");
        let heights: Vec<f32> = items
            .iter()
            .filter_map(|entry| match entry {
                Entry::Row(row) => Some(row.height()),
                Entry::Separator | Entry::Heading(_) => None,
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
    fn a_thread_offers_join_or_leave_by_the_joined_store() {
        let joined = labels(&thread_items(
            ThreadFacts {
                joined: true,
                ..ThreadFacts::default()
            },
            "x",
        ));
        assert!(joined.contains(&"Leave Thread".to_owned()));
        assert!(!joined.contains(&"Join Thread".to_owned()));
        let apart = labels(&thread_items(ThreadFacts::default(), "x"));
        assert!(apart.contains(&"Join Thread".to_owned()));
        assert!(!apart.contains(&"Leave Thread".to_owned()));
    }

    #[test]
    fn anyone_can_invite_is_a_checkbox_of_private_threads_for_managers() {
        let row = |facts: ThreadFacts| {
            thread_items(facts, "x")
                .into_iter()
                .find_map(|entry| match entry {
                    Entry::Row(row) if row.act == Act::ToggleInvitable => Some(row),
                    _ => None,
                })
        };
        let private = ThreadFacts {
            manage: true,
            private_thread: true,
            invitable: true,
            ..ThreadFacts::default()
        };
        let shown = row(private).unwrap();
        assert_eq!(shown.label, "Anyone can invite");
        assert_eq!(shown.check, Some(true));
        assert_eq!(
            row(ThreadFacts {
                invitable: false,
                ..private
            })
            .unwrap()
            .check,
            Some(false)
        );
        // A public thread, or no Manage Threads: no row.
        assert!(
            row(ThreadFacts {
                private_thread: false,
                ..private
            })
            .is_none()
        );
        assert!(
            row(ThreadFacts {
                manage: false,
                ..private
            })
            .is_none()
        );
        // It leads the admin group, before Delete.
        let labels = labels(&thread_items(private, "x"));
        let at = |label: &str| labels.iter().position(|known| known == label).unwrap();
        assert!(at("Anyone can invite") < at("Delete Thread"));
    }

    #[test]
    fn mute_thread_waits_for_the_account_to_join() {
        // `mute-disabled` of module 57907: no durations until the thread is joined.
        let mute_row = |joined: bool| {
            thread_items(
                ThreadFacts {
                    joined,
                    ..ThreadFacts::default()
                },
                "x",
            )
            .into_iter()
            .find_map(|entry| match entry {
                Entry::Row(row) if row.act == Act::Mute => Some(row),
                _ => None,
            })
            .unwrap()
        };
        let apart = mute_row(false);
        assert!(apart.disabled && !apart.caret);
        let joined = mute_row(true);
        assert!(!joined.disabled && joined.caret);
    }

    #[test]
    fn a_thread_owner_without_manage_threads_can_still_close_and_edit_it() {
        let items = thread_items(
            ThreadFacts {
                owner: true,
                joined: true,
                ..ThreadFacts::default()
            },
            "x",
        );
        let shown = labels(&items);
        assert!(shown.contains(&"Close Thread".to_owned()));
        assert!(shown.contains(&"Edit Thread".to_owned()));
        assert!(!shown.contains(&"Lock Thread".to_owned()));
        assert!(shown.contains(&"Leave Thread".to_owned()));
        assert!(!shown.contains(&"Pin Post".to_owned()));
    }

    #[test]
    fn a_forum_post_says_post_and_can_be_pinned_by_a_moderator() {
        // Modules 182061, 200273, 41402, 707792, 307623, 804006, 57907 name a forum post a post.
        let facts = ThreadFacts {
            manage: true,
            joined: true,
            forum_post: true,
            ..ThreadFacts::default()
        };
        let shown = labels(&thread_items(facts, "x"));
        for label in [
            "Close Post",
            "Lock Post",
            "Edit Post",
            "Mute Post",
            "Pin Post",
            "Delete Post",
            "Unfollow Post",
        ] {
            assert!(shown.contains(&label.to_owned()), "{label} in {shown:?}");
        }
        // Pin Post sits with Delete in `admin-actions`, just before it.
        let pin = shown.iter().position(|label| label == "Pin Post").unwrap();
        assert_eq!(shown[pin + 1], "Delete Post");
        let shown = labels(&thread_items(
            ThreadFacts {
                archived: true,
                locked: true,
                pinned: true,
                ..facts
            },
            "x",
        ));
        // Locked: only Manage Threads opens it again.
        assert!(shown.contains(&"Open Post".to_owned()));
        assert!(shown.contains(&"Unlock post".to_owned()));
        assert!(shown.contains(&"Unpin Post".to_owned()));
        // An archived thread that is not locked opens for whoever can send in it, manager or not.
        let shown = labels(&thread_items(
            ThreadFacts {
                archived: true,
                can_send: true,
                ..ThreadFacts::default()
            },
            "x",
        ));
        assert!(shown.contains(&"Open Thread".to_owned()));
        let locked_member = labels(&thread_items(
            ThreadFacts {
                archived: true,
                locked: true,
                can_send: true,
                ..ThreadFacts::default()
            },
            "x",
        ));
        assert!(!locked_member.contains(&"Open Thread".to_owned()));
        let media = labels(&thread_items(
            ThreadFacts {
                archived: true,
                can_send: true,
                manage: true,
                media: true,
                forum_post: true,
                ..ThreadFacts::default()
            },
            "x",
        ));
        assert!(!media.contains(&"Open Post".to_owned()));
        let shown = labels(&thread_items(
            ThreadFacts {
                archived: true,
                manage: true,
                ..ThreadFacts::default()
            },
            "x",
        ));
        assert!(shown.contains(&"Join Thread".to_owned()));
        assert!(!shown.contains(&"Pin Post".to_owned()));
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
        assert!(!shown.contains(&"Security Actions".to_owned()));
        // Module 664967: Edit Security Actions while a pause runs, before Leave Server.
        let lockdown = labels(&guild_items(
            GuildFacts {
                security_actions: Some(true),
                ..GuildFacts::default()
            },
            "x",
        ));
        let at = lockdown
            .iter()
            .position(|label| label == "Edit Security Actions");
        let leave = lockdown.iter().position(|label| label == "Leave Server");
        assert!(at.is_some() && at < leave);
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
    fn the_server_voice_rows_follow_the_permissions_and_sit_in_the_moderation_group() {
        let voice = ServerVoice {
            mute: true,
            can_mute: true,
            can_deafen: true,
            can_disconnect: true,
            ..ServerVoice::default()
        };
        let other = UserFacts {
            name: "ann".to_owned(),
            in_guild: true,
            server_voice: Some(voice),
            mod_view: true,
            ..UserFacts::default()
        };
        let items = user_items(&other);
        let shown = labels(&items);
        let at = |label: &str| shown.iter().position(|shown| shown == label).unwrap();
        assert!(at("Open in Mod View") < at("Server Mute"));
        // Without a moderation permission there is no Mod View row (module 121127 `P`).
        assert!(
            !labels(&user_items(&UserFacts {
                mod_view: false,
                ..other.clone()
            }))
            .contains(&"Open in Mod View".to_owned())
        );
        assert!(at("Server Mute") < at("Server Deafen"));
        assert!(at("Server Deafen") < at("Disconnect"));
        assert!(items.iter().any(|entry| matches!(entry, Entry::Row(row) if row.act == Act::ServerMute && row.check == Some(true))));
        assert!(items.iter().any(|entry| matches!(entry, Entry::Row(row) if row.act == Act::ServerDeafen && row.check == Some(false))));
        // Our own row has no Disconnect (the facts already say so), and without the permission nothing shows.
        let own = UserFacts {
            me: true,
            server_voice: Some(ServerVoice {
                can_disconnect: false,
                ..voice
            }),
            ..other.clone()
        };
        let shown = labels(&user_items(&own));
        assert!(
            shown.contains(&"Server Mute".to_owned()) && !shown.contains(&"Disconnect".to_owned())
        );
        let denied = UserFacts {
            server_voice: Some(ServerVoice::default()),
            ..other
        };
        let shown = labels(&user_items(&denied));
        for gone in ["Server Mute", "Server Deafen", "Disconnect"] {
            assert!(!shown.contains(&gone.to_owned()), "{gone}");
        }
    }

    #[test]
    fn a_call_offers_ring_or_stop_ringing_by_who_is_in_it() {
        let ring = |in_guild, state| {
            labels(&user_items(&UserFacts {
                friend: true,
                in_guild,
                ring: state,
                ..Default::default()
            }))
        };
        assert!(ring(false, Some(false)).contains(&"Ring to Call".to_owned()));
        assert!(ring(true, Some(false)).contains(&"Ring to Voice".to_owned()));
        assert!(ring(false, Some(true)).contains(&"Stop Ringing".to_owned()));
        let none = ring(false, None);
        assert!(
            !none.contains(&"Ring to Call".to_owned())
                && !none.contains(&"Ring to Voice".to_owned())
                && !none.contains(&"Stop Ringing".to_owned())
        );
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
            ..UserFacts::default()
        };
        let items = user_items(&in_call);
        let shown = labels(&items);
        for want in ["User Volume", "Mute", "Mute Soundboard", "Disable Video"] {
            assert!(shown.contains(&want.to_owned()), "{want}");
        }
        let volume_row = items
            .iter()
            .find_map(|entry| match entry {
                Entry::Row(row) if row.act == Act::Volume => Some(row),
                _ => None,
            })
            .expect("User Volume row");
        assert_eq!(volume_row.sub, None);
        assert_eq!(volume_row.height(), 58.);
        let muted = UserFacts {
            soundboard_muted: true,
            voice_muted: true,
            video_disabled: true,
            ..in_call.clone()
        };
        let items = user_items(&muted);
        assert!(items.iter().any(|entry| {
            matches!(entry, Entry::Row(row) if row.label == "Mute Soundboard" && row.check == Some(true))
        }));
        assert!(items.iter().any(|entry| {
            matches!(entry, Entry::Row(row) if row.label == "Disable Video" && row.check == Some(true))
        }));
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
    fn call_and_block_and_favorite_and_stage_items_follow_official_gating() {
        // 949674: Start a Call is hidden for bots or when hide_call is set, and disabled when blocked.
        let base = UserFacts {
            name: "ann".to_owned(),
            ..UserFacts::default()
        };
        assert!(user_items(&base).iter().any(
            |entry| matches!(entry, Entry::Row(row) if row.act == Act::Call && !row.disabled)
        ));
        assert!(
            !user_items(&UserFacts {
                bot: true,
                ..base.clone()
            })
            .iter()
            .any(|entry| matches!(entry, Entry::Row(row) if row.act == Act::Call))
        );
        assert!(
            !user_items(&UserFacts {
                hide_call: true,
                ..base.clone()
            })
            .iter()
            .any(|entry| matches!(entry, Entry::Row(row) if row.act == Act::Call))
        );
        assert!(
            user_items(&UserFacts {
                blocked: true,
                ..base.clone()
            })
            .iter()
            .any(|entry| matches!(entry, Entry::Row(row) if row.act == Act::Call && row.disabled))
        );

        // 239211: Block is danger, Unblock is default color.
        assert!(user_items(&base).iter().any(|entry| matches!(entry, Entry::Row(row) if row.act == Act::Block && row.label == "Block" && row.danger)));
        assert!(user_items(&UserFacts { blocked: true, ..base.clone() }).iter().any(|entry| matches!(entry, Entry::Row(row) if row.act == Act::Block && row.label == "Unblock" && !row.danger)));

        // 385913 / 438653: Add to Favorites sits with Pin in Group 2; Remove from Favorites moves after Mute as danger.
        let dm_unfav = user_items(&UserFacts {
            dm: Some(false),
            developer: true,
            ..base.clone()
        });
        let fav_pos = dm_unfav.iter().position(|entry| matches!(entry, Entry::Row(row) if row.act == Act::Favorite && !row.danger && row.icon == Some("clips_star"))).unwrap();
        let profile_pos = dm_unfav
            .iter()
            .position(|entry| matches!(entry, Entry::Row(row) if row.act == Act::Profile))
            .unwrap();
        assert!(fav_pos < profile_pos);

        let dm_fav = user_items(&UserFacts {
            dm: Some(false),
            dm_favorite: true,
            developer: true,
            ..base.clone()
        });
        let mute_pos = dm_fav
            .iter()
            .position(|entry| matches!(entry, Entry::Row(row) if row.act == Act::Mute))
            .unwrap();
        let rem_pos = dm_fav.iter().position(|entry| matches!(entry, Entry::Row(row) if row.act == Act::Favorite && row.danger && row.icon.is_none())).unwrap();
        let copy_pos = dm_fav
            .iter()
            .position(|entry| matches!(entry, Entry::Row(row) if row.act == Act::CopyId))
            .unwrap();
        assert!(mute_pos < rem_pos && rem_pos < copy_pos);

        // 75139 / 107632: Stage audience items put Stage actions before Profile and omit Mention.
        let stage_items = user_items(&UserFacts {
            in_guild: true,
            omit_mention: true,
            stage_acts: vec![Act::StageInvite],
            ..base
        });
        let shown = labels(&stage_items);
        assert_eq!(shown[0], "Invite to Speak");
        assert_eq!(shown[1], "-");
        assert_eq!(shown[2], "Profile");
        assert!(!shown.contains(&"Mention".to_owned()));
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
            send_button: false,
            ai: false,
            ai_busy: None,
        };
        assert_eq!(
            labels(&composer_items(&menu)),
            [
                "Suggestions",
                "Send Message Button",
                "-",
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
            [
                "Suggestions",
                "Send Message Button",
                "-",
                "Remove from Dictionary",
                "Spellcheck",
                "-",
                "Paste"
            ]
        );
    }
}
