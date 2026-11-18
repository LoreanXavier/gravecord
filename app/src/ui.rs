//! First screen: rail, channel list, user panel, header, messages, composer.
//! Geometry and colors are from `design/dom/channel/text-darker-cozy.json`; every color is an official token.

use crate::editor::{Editor, EditorEvent, EditorStyle};
use crate::inline::{BoxStyle, Inline, Piece, SharedSelection, inline};
use crate::net::{Request, Requests, Update};
use crate::parity::Parity;
use chrono::Datelike;
use dc_markdown::highlight::highlight;
use dc_markdown::{Block, Kind, Span, parse};
use dc_state::grouping::starts_group;
use dc_state::member_list::Row as MemberRow;
use dc_state::{
    Channel, ChannelKind, Guild, Id, Message, Stores, TYPING_DURATION, User, fit_media,
};
use dc_tokens::Theme;
use gpui::{
    AnyElement, App, Bounds, ClipboardItem, Context, Entity, FocusHandle, Focusable, Font,
    FontFeatures, FontStyle, FontWeight, Hsla, Image, ImageFormat, IntoElement, KeyDownEvent,
    ListAlignment, ListState, MouseButton, MouseDownEvent, Pixels, Render, Rgba, SharedString,
    StyledText, TextRun, WeakEntity, Window, WindowControlArea, div, font, img, list, prelude::*,
    px, rgba, svg,
};
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::mpsc::UnboundedSender;

mod actions;
mod active_now;
mod activity_host;
mod activity_launch;
mod activity_shelf;
mod activity_sync;
mod add_server;
mod alt_text;
mod announcements;
mod app_dm;
mod apps;
mod attachment_modal;
mod audio_attachment;
mod audio_menu;
mod audit_spec;
mod autocomplete;
mod automod_alert;
mod autoscroll;
mod badge_modal;
mod badge_paths;
mod birthday;
mod blocked;
mod boost_apply;
mod call_background;
mod call_menus;
mod call_popout;
mod call_volume;
#[cfg(feature = "capture")]
pub(crate) mod callsim;
mod camera_failed;
mod carousel;
#[cfg(feature = "capture")]
mod ch_fixture;
mod channel_drag;
mod channel_icon;
pub(crate) mod channel_notice;
mod channel_settings;
mod channel_settings_pages;
mod channel_status;
mod chat_notices;
mod chat_skeleton;
mod clips;
mod code_copy;
mod command_feedback;
mod components;
pub(crate) mod composer_ai;
mod composer_plus;
#[cfg(feature = "capture")]
mod content_demo;
mod context_menu;
#[cfg(feature = "capture")]
mod crawl_reset;
mod create_channel;
mod dangerous_link;
mod deep_link_route;
mod destructive;
mod dm;
mod dm_profile;
mod embed;
#[cfg(feature = "capture")]
mod embed_demo;
mod embed_layout;
mod emoji_popout;
mod emoticons;
mod family_center;
mod file_actions;
mod files;
mod forward;
mod friend_nickname;
mod friend_request_notice;
mod friends;
mod full_profile;
mod game_add_to_profile;
mod game_announcements;
mod game_communities;
mod game_similar;
mod games_spec;
mod gateway_screens;
mod geo_restricted;
mod gifs;
mod go_live;
mod go_live_game;
mod graphic_modal;
mod group_dm;
mod group_manage;
mod guild_cap;
mod guild_menu;
mod guild_notifications;
mod history;
mod home;
mod inbox;
mod interaction_flow;
mod interaction_modal;
mod inventory_feed;
mod inventory_popout;
mod invite;
mod invite_embed;
mod keyboard;
mod last_channel;
mod library;
mod lightbox;
mod links;
mod live_activity;
pub mod login;
mod logout;
mod lottie_icon;
mod mark_read;
mod member_actions;
mod message_header;
mod message_kinds;
mod message_requests;
mod message_tips;
mod platform_presence;
mod premium_upsell;
mod presence;
mod provider_player;
mod role_dot;
pub(crate) mod role_gradient;
mod role_popout;
mod schedule_message;
mod sent_attachment;
mod spotify_activity;
mod stage;
mod stage_requests;
mod sticker_player;
mod sticker_popout;
mod stickers;
mod stream_previews;
mod stream_report;
mod stream_zoom;
mod timestamps;
mod top_emoji;
mod tray;
mod typing_avatar;
mod unread_badges;
mod unread_dms;
mod update_button;
mod vad_permission;
mod voice_message;
mod web_frames;
pub use logout::RootEvent;
mod account_switcher;
mod app_widget;
mod av_errors;
#[cfg(feature = "capture")]
mod avatar_demo;
mod avatar_play;
mod boost_page;
mod channel_browser;
mod discover;
mod favorite_nickname;
mod favorites;
mod feedback;
mod fetch_failures;
mod forums;
pub(crate) mod frame_clock;
mod game_genres;
mod game_profile;
pub(crate) mod gif_tiles;
mod graphic_dialogs;
mod guild_events;
mod guild_guide;
mod guild_navigation;
mod guild_stack;
mod hover_bar;
pub(crate) mod image_cache;
mod image_crop;
pub(crate) mod image_fit;
mod image_retry;
mod inline_video;
mod jump;
mod launcher_home;
mod media_image;
mod member_avatars;
mod member_onboarding;
pub(crate) mod memory_trim;
mod menu;
mod menu_slider;
mod mod_view;
mod modal;
mod mosaic;
pub(crate) mod motion;
mod name_styles;
mod nameplate_animation;
mod nameplates;
mod nitro;
mod nitro_purchase;
mod notice_bar;
mod notifications;
mod notify;
mod now_playing_menu;
mod oauth;
mod oauth_spec;
mod official_messages;
mod overlay;
mod overlay_chat;
mod overlay_panel;
mod overlay_survey;
mod overlay_video;
mod overlay_wheel;
mod overlay_widgets;
mod paste;
#[cfg(feature = "capture")]
mod perf;
mod permission_spec;
mod picker;
mod pins;
mod plaintext_extensions;
mod plugin_hooks;
mod poll;
mod popout_roles;
mod profile;
mod profile_art;
mod profile_board;
mod profile_bot;
mod profile_commands;
mod profile_editor;
pub(crate) mod profile_effects;
mod profile_frames;
mod profile_menu;
mod profile_other;
mod profile_theme;
mod profile_voice;
mod quests;
mod quickswitcher;
mod rail_folder_menu;
mod rail_folders;
mod reaction_tip;
mod reactions_modal;
pub(crate) mod regions;
mod remediation;
mod reminder_modal;
mod reorder_hint;
mod report;
mod report_issue;
mod rtc_channel;
mod running_game_panel;
mod running_games;
mod safety_notice;
mod saved;
mod screening;
mod search;
mod search_filters;
mod search_pager;
mod send_state;
mod server_access;
mod server_audit;
mod server_automod;
mod server_boost;
mod server_community;
mod server_discovery;
pub(crate) mod shop;
mod slash;
mod slowmode;
mod splash;
pub(crate) mod start_cache;
mod status_menu;
mod stream_failed;
mod video_pip;
#[cfg(feature = "capture")]
pub(crate) use plugin_hooks::more_commands_outgoing;
pub(crate) use plugin_hooks::spotify_premium_forced;
pub(crate) use server_discovery::FORCE_ELIGIBLE;
mod server_discovery_form;
mod server_emoji;
mod server_engagement;
mod server_games;
mod server_insights;
mod server_landing;
mod server_welcome;
pub(crate) mod server_welcome_new;
#[cfg(feature = "capture")]
pub(crate) use server_landing::FORCE_LANDING;
mod dm_call;
mod dm_call_view;
mod incoming_call;
mod member_safety;
mod member_window;
mod server_members;
mod server_onboarding;
mod server_role_editor;
mod server_roles;
mod server_safety;
mod server_safety_community;
mod server_settings;
mod server_settings_pages;
mod server_sounds;
mod server_stickers;
mod server_tag;
mod server_template;
mod server_webhooks;
mod settings;
mod soundboard;
mod stream_menus;
mod stream_volume;
mod tag_badges;
mod threads;
mod tooltip;
mod tutorial_tips;
mod unread;
mod unread_notice;
mod upload_area;
mod upload_error;
mod upload_progress;
mod uploads;
mod video_player;
mod video_stats;
mod video_tiles;
mod view_as_role;
mod voice;
mod voice_alerts;
mod voice_blocked;
mod voice_panel;
mod voice_sounds;
mod voice_video;
mod voice_view;

use motion::Transitions as _;
use tooltip::{Body, Side, Tip};

const SANS: &str = "gg sans";
/// The CSS family is `gg mono`; the font file calls itself `gg sans mono`.
const MONO: &str = "gg sans mono";

/// The page sets `text-rendering: optimizelegibility`. gpui hands DirectWrite an explicit feature list, so `kern` has to
/// be on that list or text comes out wider than the official client's.
fn kerning() -> FontFeatures {
    FontFeatures(Arc::new(vec![("kern".to_owned(), 1)]))
}

/// The palette every color comes from. "Dark" in the Appearance page is `Darker` here, "Ash" is `Dark` and "Onyx" is
/// `Midnight`, as the token sets are named.
static THEME: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(2);

/// The revision each per-render preparation step last ran at.
#[derive(Default)]
struct Prepared {
    avatars: Option<(u64, Option<Id>, Option<Id>)>,
    inventory: Option<(u64, Option<Id>, bool)>,
    badge: Option<u64>,
    /// When the unread badge was last refreshed, and whether a refresh is waiting for its turn.
    badge_at: Option<Instant>,
    badge_waiting: bool,
    /// The channel whose messages own the media playing now (`None` inside: no channel open).
    media_channel: Option<Option<Id>>,
    /// The favourite GIFs (key and picture address) whose pictures were last asked for.
    gif_favorites: Option<Vec<(String, String)>>,
    /// Whether the pointer was over the window at the last frame.
    window_hovered: Option<bool>,
}

/// Whether a step keyed by `now` has anything new to read since it last ran at `last`.
fn changed<T: PartialEq>(last: &mut Option<T>, now: T) -> bool {
    if last.as_ref() == Some(&now) {
        return false;
    }
    *last = Some(now);
    true
}

pub(crate) fn theme() -> Theme {
    match THEME.load(std::sync::atomic::Ordering::Relaxed) {
        0 => Theme::Dark,
        1 => Theme::Light,
        3 => Theme::Midnight,
        _ => Theme::Darker,
    }
}

pub(crate) fn set_theme(theme: Theme) {
    let value = match theme {
        Theme::Dark => 0,
        Theme::Light => 1,
        Theme::Darker => 2,
        Theme::Midnight => 3,
    };
    THEME.store(value, std::sync::atomic::Ordering::Relaxed);
}

/// `AccessibilityStore.saturation` (0 to 1), as the bits of an f32: the theme colours are resolved with it
/// (`ColorResolve({theme, saturation})`, module 413599). Set each frame from the Saturation slider.
static SATURATION: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0x3f80_0000);

pub(crate) fn set_saturation(factor: f32) {
    SATURATION.store(
        factor.clamp(0., 1.).to_bits(),
        std::sync::atomic::Ordering::Relaxed,
    );
}

fn saturation() -> f32 {
    f32::from_bits(SATURATION.load(std::sync::atomic::Ordering::Relaxed))
}

/// A colour with its saturation scaled.
fn desaturated(colour: Rgba, factor: f32) -> Rgba {
    let mut hsl: Hsla = colour.into();
    hsl.s *= factor;
    hsl.into()
}

pub(crate) fn color(name: &str) -> Rgba {
    let colour = settings::client_theme::overlay(name)
        .or_else(|| settings::client_theme::high_contrast(name, theme()))
        .unwrap_or_else(|| rgba(theme().color(name)));
    let factor = saturation();
    if factor < 1. {
        desaturated(colour, factor)
    } else {
        colour
    }
}

/// Selection highlight, measured on the official client. The bundle sets no `::selection`.
const SELECTION: u32 = 0x093da7ff;

fn hsla_from(value: u32) -> Hsla {
    rgba(value).into()
}

fn hsla(name: &str) -> Hsla {
    color(name).into()
}

fn icon(name: &str, size: f32, tint: &str) -> gpui::Svg {
    svg()
        .path(format!("icons/{name}.svg"))
        .size(px(size))
        .flex_none()
        .text_color(color(tint))
}

enum Row {
    /// The start of a DM's history.
    Welcome,
    Divider(String),
    /// The red line before the first unread message.
    NewDivider,
    /// The start of a forum post: its icon and title (`container_Wp3SYH`).
    PostIntro,
    /// Follow and Copy Link under a forum post's first message (`container_HeWz1C`).
    PostActions,
    Spacer,
    Message {
        index: usize,
        head: bool,
    },
    /// A run of messages from blocked, ignored or likely spammer people, folded; `first` is the first one's id.
    Blocked {
        first: Id,
        count: usize,
        kind: blocked::Fold,
    },
}

/// What the bar over a reply shows (module 448368 by the state of ReferencedMessageStore, module 9842).
enum Reference {
    Loaded(Box<Message>),
    NotLoaded { channel: Id, id: Id },
    Deleted,
}

pub struct Root {
    stores: Stores,
    requests: Requests,
    guild: Option<Id>,
    channel: Option<Id>,
    screening: Option<screening::Screening>,
    /// View Server As Role's popout state (`ui/view_as_role.rs`; the viewed roles are on the guild).
    view_as_role: view_as_role::State,
    /// Reapply of a pending member (module 267): the server whose verification form opens once its old request is gone.
    reapply_after_remove: Option<Id>,
    slowmode: dc_state::slowmode::SlowmodeStore,
    slowmode_ticking: bool,
    presence_ticking: bool,
    nameplates: Rc<RefCell<nameplates::Cache>>,
    profile_effects: Rc<RefCell<profile_effects::Cache>>,
    member_avatars: Rc<RefCell<member_avatars::Cache>>,
    member_window: member_window::State,
    inline_videos: Rc<RefCell<inline_video::Cache>>,
    gif_tiles: Rc<RefCell<gif_tiles::Cache>>,
    video_hub: Rc<RefCell<video_player::VideoHub>>,
    /// When each failed `Request::Fetch` key failed (`fetch_failures`).
    fetch_failures: HashMap<String, std::time::Instant>,
    /// Counts the changes the per-render preparation reads: store data (every `update`), the selected place and
    /// local read marks. `Prepared` remembers the count each step last ran at, so a render that follows no change
    /// does not walk the member list or the feed again.
    data_revision: u64,
    prepared: Prepared,
    hovered_media: Option<String>,
    guild_guide_open: bool,
    guild_guide_scroll: gpui::ScrollHandle,
    sidebar_scroll: gpui::ScrollHandle,
    /// The full-window Server Boost page (`boost_page.rs`).
    boost_page: Option<boost_page::BoostPage>,
    /// The "Boost This Server" dialog (`boost_apply.rs`).
    boost_apply: Option<boost_apply::BoostApply>,
    /// The scroll position of the open context menu (a menu taller than the window scrolls).
    pub(crate) menu_scroll: gpui::ScrollHandle,
    channel_browser_open: bool,
    channel_browser: Option<channel_browser::Browser>,
    hovered_nameplate: Option<Id>,
    /// The pointer is on a DM's welcome block (891522 animates the styled name then).
    hovered_welcome: bool,
    /// The pointer is over the group name in the channel header (its pencil is laid out only then).
    group_header_hover: bool,
    slowmode_emphasis: Option<Instant>,
    slowmode_rejected: HashMap<Id, std::collections::VecDeque<crate::net::RejectedDraft>>,
    /// The messages this account is sending and the failed ones (`send_state`).
    send_ui: send_state::State,
    /// Set while a failed message's content is built: its text, links and inline code take the critical colour.
    failed_tint: std::cell::Cell<bool>,
    /// Set while an official message's content is built (`official_messages`): its text colour.
    official_text: std::cell::Cell<Option<Rgba>>,
    /// Each server's official message colour and experiment.
    official_messages: official_messages::OfficialMessages,
    notification_runtime: crate::notifications::Runtime,
    /// Game detection and the Registered Games page (`running_games`).
    detect: running_games::Games,
    /// The game overlay window and what it follows (`overlay`).
    overlay: overlay::Overlay,
    tray_runtime: crate::tray::Runtime,
    #[cfg(feature = "capture")]
    capture_badge: Option<(u64, bool)>,
    #[cfg(feature = "capture")]
    capture_file_card: Option<Message>,
    #[cfg(feature = "capture")]
    capture_upload_card: Option<(u64, f32, bool)>,
    #[cfg(feature = "capture")]
    capture_requests: Option<u64>,
    #[cfg(feature = "capture")]
    capture_dm_rail: Option<(u64, bool)>,
    hovered_rail_dm: Option<Id>,
    rail_folders: rail_folders::State,
    account_switcher: account_switcher::State,
    #[cfg(feature = "capture")]
    capture_friend_nickname: bool,
    notification_focused: bool,
    /// Middle-button autoscroll (`autoscroll.rs`).
    autoscroll: autoscroll::Shared,
    notification_sounds: crate::notifications::SoundThrottle,
    rows: Vec<Row>,
    list: ListState,
    composer: Entity<Editor>,
    /// The composer's `@`, `#` and `:` suggestions.
    autocomplete: autocomplete::State,
    slash: slash::State,
    /// The right-click menu of a server, channel, DM or person.
    context: context_menu::State,
    channel_drag: channel_drag::State,
    /// A message over the length limit, held while the "too long" dialog asks what to do.
    too_long: Option<String>,
    /// Small chat dialogs and chained thread requests (`chat_notices`).
    chat_extra: chat_notices::State,
    focus: FocusHandle,
    /// Focus bookkeeping and keyboard modality (`keyboard`).
    keys: keyboard::State,
    images: HashMap<String, Arc<Image>>,
    requested: HashSet<String>,
    /// Retry state of the pictures asked for through `request_image`.
    image_retries: image_retry::Retries,
    /// The select image and image crop modals (`image_crop`).
    image_crop: image_crop::State,
    badge_modal: Option<badge_modal::BadgeModal>,
    spotify_actions: spotify_activity::Actions,
    /// Animated avatar files a frame found missing, asked for by `prepare_live_avatars`.
    avatar_wants: std::cell::RefCell<Vec<(String, String)>>,
    /// The window's pixel ratio as the last frame had it, for builders that have no window at hand.
    ui_scale: std::cell::Cell<f32>,
    /// The avatar surface the pointer is over (a row's or panel's own key), for `animate_on_hover` avatars.
    avatar_over: Option<String>,
    selection: SharedSelection,
    hovered: Option<Id>,
    /// The message being edited, with its own field. Enter saves, Escape cancels.
    editing: Option<(Id, Entity<Editor>)>,
    /// The message the next send answers.
    replying: Option<Message>,
    /// The reply bar's @ switch: whether the reply pings its author.
    reply_ping: bool,
    menu: Option<menu::OpenMenu>,
    /// What a right press landed on (a link, a picture) and where, set by that element before the message row's own
    /// handler opens the menu.
    menu_hit: Option<(gpui::Point<gpui::Pixels>, menu::Hit)>,
    menu_extra: menu::MenuExtra,
    /// The menu that ran the last action, for actions that open beside it.
    last_menu: Option<menu::OpenMenu>,
    confirm_delete: Option<Id>,
    show_members: bool,
    /// The server's Members view (`/member-safety`) in place of the chat.
    member_view: Option<server_members::MemberView>,
    /// The profile beside a DM (`Hide User Profile` in its header); shown until toggled off.
    show_dm_profile: bool,
    /// The member list beside a group DM: on by default.
    group_members: bool,
    friends_tab: friends::FriendsTab,
    skin_tone: u8,
    skin_menu: bool,
    search: Option<search::Search>,
    search_filters: Option<search_filters::SearchFilters>,
    add_server: Option<add_server::AddServer>,
    /// The account's date-of-birth state and the age gate dialogs (`birthday.rs`).
    birthday: birthday::State,
    file_menu: Option<file_actions::FileMenu>,
    file_wrap: HashSet<Id>,
    file_language: HashMap<Id, String>,
    file_expanded: Option<dc_state::Attachment>,
    view_reactions: Option<reactions_modal::ViewReactions>,
    polls: poll::PollUi,
    audio_menu: Option<audio_menu::AudioMenu>,
    /// The voice channel's call view: its idle timer, menus and docked chat.
    vv: voice_view::State,
    call_volume: call_volume::State,
    stream_previews: stream_previews::Previews,
    stream_zoom: stream_zoom::State,
    invite: Option<invite::Invite>,
    invite_codes: HashMap<Id, String>,
    pending_invite_link: Option<String>,
    guild_notifications: Option<Id>,
    raid_alerts_off: HashSet<(Id, u8)>,
    create_channel: Option<create_channel::CreateChannel>,
    channel_status: Option<channel_status::StatusModal>,
    follow: Option<announcements::FollowModal>,
    /// Module 505859 after a follow: the picture and the line it picked.
    follow_success: Option<(usize, usize)>,
    /// "Wanna join more servers?" (module 716480) is open.
    guild_cap_upsell: bool,
    /// "Introducing App Directory" (module 633605) is open for this server.
    app_directory_promo: Option<Id>,
    /// Module 455796 while it shows, and the servers whose completed guide it has shown for this session
    /// (`onboardingCompleteGuilds` of module 976639).
    guide_celebration: Option<guild_guide::GuideCelebration>,
    guide_complete_guilds: HashSet<Id>,
    start_stage: Option<stage::StartStage>,
    /// The Stage's age coachmark was closed with its X.
    stage_coach_closed: bool,
    /// The "Welcome to Stages!" modal is open for this Stage channel, and it was already shown once (assumed: not kept
    /// across runs).
    stage_welcome: Option<Id>,
    stage_welcome_seen: bool,
    /// The caret of the Stage's leave button is open (its "End Stage for Everyone" menu), and the confirm question
    /// for that Stage channel.
    stage_leave_menu: bool,
    stage_end_confirm: Option<Id>,
    /// The open voice, Stage or stream alert (`voice_alerts`), and the streams whose "Stream is Full" was shown.
    voice_alert: Option<voice_alerts::VoiceAlert>,
    stream_full_seen: std::collections::HashSet<String>,
    /// Blocked and ignored people in voice channels, and when warnings about them were dismissed (`voice_blocked`).
    blocked_in_voice: voice_blocked::BlockedInVoice,
    warning_dismissals: voice_blocked::WarningDismissals,
    /// `UnreadSettingNoticeStore2` and the channel showing its notice (`unread_notice`).
    unread_notice: unread_notice::State,
    /// The Stage's Requests to Speak sidebar is open, and the Stages whose full video notice was dismissed
    /// (`stage_requests`).
    stage_requests_open: bool,
    stage_video_upsell_dismissed: std::collections::HashSet<Id>,
    /// Videos waiting for their first picture, and the ones that never showed (`camera_failed`).
    video_ready: std::cell::RefCell<camera_failed::VideoReady>,
    /// The survey after the overlay was switched off, and its opening at the next frame (`overlay_survey`).
    overlay_survey: Option<overlay_survey::OverlaySurvey>,
    overlay_survey_pending: bool,
    /// The call, stream and report surveys (`feedback`).
    feedback: feedback::Feedback,
    /// Capture only: the next survey's 1% roll, set by `feedback upload`.
    feedback_roll: Option<f64>,
    /// The voice panel's strip of people shows them all (`rtc_channel`, Show All Users).
    voice_users_expanded: bool,
    /// The video picture-in-picture window (`video_pip`).
    video_pip: video_pip::VideoPip,
    /// The AV warnings of the call's streams and camera (`av_errors`).
    av_errors: av_errors::AvErrors,
    /// The menu over a speaker's or audience member's tile (module 106401 and 237749's rows), where it was opened.
    stage_tile_menu: Option<stage::TileMenu>,
    /// Stage instances whose Live Now card was closed with its X.
    live_now_closed: Vec<Id>,
    publish_confirm: Option<(Id, Id)>,
    follower_stats: announcements::StatsMap,
    /// Capture only (`ch blocker on`): the Follow bar as a member without Send Messages sees it.
    follow_blocker_forced: bool,
    guild_events: Option<guild_events::GuildEvents>,
    guild_menu: bool,
    /// The Favorites server is open (`/channels/@favorites/<channel>`): its sidebar replaces the server's.
    favorites_view: bool,
    /// "Dismiss suggested channels" of the Favorites sidebar (module 132598 `Q`), for this run.
    favorites_suggest_dismissed: bool,
    /// The Add Role picker over a profile popout (`popout_roles.rs`).
    role_picker: Option<popout_roles::Picker>,
    mod_view: Option<mod_view::ModView>,
    thread_browser: Option<threads::ThreadBrowser>,
    /// "All Active Threads" (`threads.rs`, from the server menu).
    thread_modal: bool,
    new_thread: Option<threads::NewThread>,
    pending_thread_message: Option<String>,
    inbox: Option<inbox::InboxTab>,
    inbox_read: HashSet<Id>,
    inbox_collapsed: HashSet<Id>,
    gifs: Option<gifs::Gifs>,
    sticker_ui: stickers::Stickers,
    /// The favourite GIFs once this session changed them; until then the settings fetch is read.
    favorite_gifs: Option<Vec<dc_proto::frecency::FavoriteGif>>,
    /// The favourite emoji (`favoriteEmojis.emojis`) once this session changed them; until then the settings fetch.
    emoji_favorites: Option<Vec<String>>,
    shown_blocked: HashSet<Id>,
    shown_blocked_profiles: HashSet<Id>,
    hovered_dm: Option<Id>,
    group_dm: Option<group_dm::GroupDm>,
    add_friend: Option<friends::AddFriend>,
    people_search: Option<friends::PeopleSearch>,
    friend_menu: Option<friends::FriendMenu>,
    confirm_remove_friend: Option<friends::RemoveFriend>,
    friend_nickname: Option<friend_nickname::Dialog>,
    /// Change Nickname of a favourite (module 250737).
    favorite_nickname: Option<favorite_nickname::Dialog>,
    group_edit: Option<group_manage::Edit>,
    group_leave: Option<group_manage::Leave>,
    /// Kick, Ban, Timeout and Remove Timeout of a member's menu.
    member_action: Option<member_actions::Dialog>,
    quick_switcher: Option<quickswitcher::QuickSwitcher>,
    activity_shelf: Option<activity_shelf::ActivityShelf>,
    activities: activity_launch::Activities,
    notify_open: bool,
    /// The Mute Channel row while its durations show.
    notify_submenu: Option<Bounds<Pixels>>,
    /// The open Pinned Messages popout, by the anchor key of the button that opened it.
    pins_open: Option<&'static str>,
    confirm_unpin: Option<Message>,
    forward: Option<forward::Forward>,
    profile_menu: Option<profile_menu::ProfileMenu>,
    profile_menu_which: profile_menu::Which,
    remediation: Option<(Id, remediation::Remedy)>,
    report: Option<report::Report>,
    full_profile: Option<full_profile::FullProfile>,
    profile_editor: Option<profile_editor::Open>,
    /// Invite codes already resolved or on their way.
    requested_invites: HashSet<String>,
    /// The command mentions of the loaded messages that are links here (module 990474), by id and name; worked out
    /// when the messages or the command index change, never while painting.
    command_mentions: Vec<(u64, String)>,
    /// Images this client draws itself (invite banners), by key.
    generated_images: std::cell::RefCell<HashMap<String, std::sync::Arc<gpui::Image>>>,
    /// The server icon stack's mask pictures being drawn (module 692617).
    guild_stack_pending: std::cell::RefCell<HashSet<String>>,
    /// The voice message players: loaded clips, each pill's position and bars, the shared speed and volume.
    voice_hub: std::rc::Rc<std::cell::RefCell<voice_message::Hub>>,
    voice_rec: std::rc::Rc<std::cell::RefCell<voice_message::RecState>>,
    /// Capture only: the key and length of the message `voice demo` added.
    #[cfg(feature = "capture")]
    voice_demo_key: Option<String>,
    #[cfg(feature = "capture")]
    voice_demo_seconds: f64,
    /// A server joined from an invite card, opened once its GUILD_CREATE arrives.
    pending_join: Option<Id>,
    /// A message sent from a popout to someone without a DM yet: open the DM once it exists.
    open_dm_after_send: bool,
    mention_filter: inbox::MentionFilter,
    channel_settings: Option<channel_settings::ChannelSettings>,
    server_settings: Option<server_settings::ServerSettings>,
    /// Game records by application id (Server Profile's Games).
    games: HashMap<String, server_games::Game>,
    security_actions: Option<channel_settings_pages::SecurityActions>,
    mentions_filter_open: bool,
    /// Channels this session opened, newest first: the forward dialog's first suggestions.
    channel_history: Vec<Id>,
    speech: crate::speech::Speech,
    /// The message Speak Message is reading.
    speaking_message: Option<Id>,
    toast: Option<saved::Toast>,
    app_toast: Option<saved::AppToast>,
    /// The open submenu of the message menu: the row it hangs off.
    submenu: Option<menu::Submenu>,
    custom_reminder: Option<reminder_modal::CustomReminder>,
    /// The media viewer over a picture of a message.
    lightbox: Option<lightbox::Lightbox>,
    /// The account popout of the user panel and the status menu hanging off it.
    status_popout: Option<status_menu::StatusPopout>,
    /// The "Set your status" modal.
    custom_status: Option<status_menu::CustomStatusModal>,
    plus_menu: bool,
    modify_attachment: Option<attachment_modal::ModifyAttachment>,
    /// Report Issue for a detected game (module 651930).
    report_issue: Option<report_issue::ReportIssue>,
    /// The game profile (module 581745).
    game_profile: Option<game_profile::GameProfile>,
    game_communities_layout: game_communities::Layout,
    game_announcements_scroll: f32,
    family_center: family_center::State,
    library: library::Library,
    provider_player: Option<provider_player::ProviderPlayer>,
    web_frames: web_frames::WebFrames,
    composer_ai: composer_ai::ComposerAi,
    game_similar_scroll: f32,
    game_communities_filter: game_communities::Filter,
    /// When the text launcher's collections last arrived (module 503406 asks again after ten minutes).
    launcher_collections_at: Option<i64>,
    /// The dialog of a refused upload (too large, too many, empty).
    upload_error: Option<upload_error::UploadError>,
    /// "Reimagine your profile with Nitro" (module 536015) is open.
    try_it_out_upsell: bool,
    /// The pencil or trash of a sent attachment's hover group (`sent_attachment.rs`).
    sent_edit: Option<sent_attachment::SentEdit>,
    snooze_menu: Option<(Id, Id, bool, gpui::Point<gpui::Pixels>)>,
    click_at: gpui::Point<gpui::Pixels>,
    leave_guild: Option<Id>,
    /// A destructive request waiting for its confirmation (`destructive.rs`).
    destructive: Option<destructive::Pending>,
    /// "Trying to reorder channels?" (module 354643): whether its "Don't show again" box is ticked.
    reorder_hint: Option<bool>,
    /// The tutorial tips (`tutorial_tips`, module 179913).
    tutorial_tips: tutorial_tips::TutorialTips,
    /// The Log Out confirmation of the settings sidebar.
    logout_confirm: bool,
    poll: Option<composer_plus::PollForm>,
    /// The select list open under a bot message, and the presses in flight (`components`, `interaction_flow`).
    components: components::State,
    interaction_ui: interaction_flow::State,
    /// The Schedule Message modal, the "+" menu's submenu and the create request in flight.
    schedule: schedule_message::State,
    /// The window's size as of the last frame, for the viewer's room.
    viewport: (f32, f32),
    /// The open channel and the ack its unread messages started after, for the NEW divider.
    unread_from: Option<(Id, Id)>,
    /// A jump to a message in flight, and the flash it leaves (`jump.rs`).
    jump: jump::JumpUi,
    /// The expression picker, open from the composer or for a reaction.
    /// The text field the picker was opened from when its target is `Target::Field`.
    picker_field: Option<Entity<Editor>>,
    picker: Option<picker::Picker>,
    /// Emoji picked this session, for the frecency list until the stored settings catch up.
    emoji_used: Vec<(String, u64)>,
    /// The message waiting in the "Pin It. Pin It Good." modal.
    confirm_pin: Option<Message>,
    /// Reactions given this session, for the frecency list until the stored settings catch up.
    reactions_used: Vec<(String, u64)>,
    history: history::History,
    /// The Leaving Discord modal, open while a link waits for confirmation.
    leaving: Option<links::Leaving>,
    last_typing_sent: Option<Instant>,
    tip: Option<tooltip::Active>,
    /// The target whose delayed tooltip is waiting for the pointer to rest.
    tip_pending: Option<SharedString>,
    pub(super) active_menu_slider_drag: Option<menu_slider::ActiveSliderDrag>,
    /// Channels whose first message has been loaded: the welcome goes above it.
    /// Width of a message's content column, set each frame: media never grows past it (`max-width: 100%`).
    content_width: f32,
    /// The open profile popout.
    profile: Option<profile::Popout>,
    /// The voice call, from joining until leaving.
    call: Option<voice::Call>,
    /// A call asked for with a person whose DM did not exist yet: (video), started when the DM opens.
    pending_call: Option<bool>,
    dmc: dm_call_view::State,
    ring_audio: incoming_call::RingAudio,
    video: voice_video::Video,
    /// The Go Live picker, while open.
    go_live: Option<go_live::State>,
    go_live_generation: u64,
    /// Why our camera did not start (`handleCameraUnavailable`), while its dialog is open.
    camera_error: Option<String>,
    panel: voice_panel::PanelState,
    soundboard: soundboard::Board,
    self_mute: bool,
    self_deaf: bool,
    /// PermissionVADStore (`vad_permission.rs`).
    vad: vad_permission::VadState,
    /// Lets the system-wide push to talk hook (another thread) reach the window.
    updates: UnboundedSender<Update>,
    /// The system-wide push to talk hook, while connected to voice in push to talk mode.
    ptt_hook: Option<dc_hotkey::Hook>,
    /// Push to talk is down, as far as the sounds know.
    ptt_down: bool,
    /// Users the voice connection reports as speaking.
    speaking: HashSet<Id>,
    /// Text of attachments shown as plaintext previews, by `text:` key.
    texts: HashMap<String, String>,
    /// Files picked or dropped, waiting for the next send.
    pending_files: Vec<uploads::Pending>,
    /// The upload row's horizontal scroll (`channelAttachmentArea_iH8bsl`).
    upload_scroll: gpui::ScrollHandle,
    /// This view, for handlers built while rendering that have no context of their own.
    weak: WeakEntity<Root>,
    /// Spoilers opened by a click: message, block and the first span of the spoiler.
    revealed: HashSet<(Id, u32, usize)>,
    settings: Option<settings::Open>,
    /// Settings the user changed in the settings pages, by key.
    setting_choices: HashMap<&'static str, settings::Choice>,
    /// Dialogs and fetched state of the settings pages' buttons and account-route switches.
    settings_extra: settings::ActionsState,
    /// Voice & Video's stored choices, microphone preview, Mic Test, keybind recorder and slider drags.
    voice: settings::VoiceState,
    /// The Clips page's stored settings, keybind recorder, select and slider.
    clips: settings::ClipsState,
    /// Bodies of `Request::Fetch`, by key.
    fetched: HashMap<String, serde_json::Value>,
    active_now: active_now::State,
    /// The Nitro page: its plans and scroll, and whether it is the Home content.
    nitro: nitro::NitroState,
    nitro_open: bool,
    discover: discover::State,
    shop: shop::State,
    quests: quests::State,
    /// The Message Requests page (`/message-requests`).
    message_requests: message_requests::State,
    /// "Upload a Sound" / "Edit Sound" (module 122998) while it is open, from Server Settings or a sound's menu.
    sound_dialog: Option<server_sounds::SoundDialog>,
    /// A stream's "Report a problem" (module 956215) while it is open.
    stream_report: Option<stream_report::Form>,
    tip_anchors: Rc<RefCell<HashMap<SharedString, Bounds<Pixels>>>>,
    user_panel_height: Option<Pixels>,
    inventory: inventory_feed::State,
    inventory_popout: inventory_popout::State,
    /// The popout a sticker in a message opens.
    sticker_popout: Option<sticker_popout::StickerPopout>,
    emoji_popout: Option<emoji_popout::EmojiPopout>,
    role_popout: Option<role_popout::RolePopout>,
    /// The popout of a header chip: the role icon's card or a server tag's profile (`message_header`).
    header_popout: Option<message_header::HeaderPopout>,
    /// Code blocks whose copy button was pressed (`code_copy`).
    copied_code: HashSet<(u64, u32)>,
    pub ready: bool,
    hydrated_from_cache: bool,
    /// Members the start cache brought (server, user): asked for again once the gateway is up, so none stays as the
    /// last run knew it.
    start_members: Vec<(Id, Id)>,
    /// The member list the start cache drew (server and its `GUILD_MEMBER_LIST_UPDATE`), until the live one replaces it.
    start_member_list: Option<(Id, serde_json::Value)>,
    /// When the open server's member list was last kept for the next start.
    member_list_saved: Option<(Id, Instant)>,
    /// Pictures of the screen kept for the next start that are not in yet: each is written to the start's folder
    /// when it comes in (`start_cache::kept_for_the_start`).
    start_keep: HashSet<String>,
    /// The connecting screen, up until the first content is in.
    splash: splash::Splash,
    forums: HashMap<Id, forums::Forum>,
    /// The areas of the window that are views of their own (`regions.rs`).
    regions: regions::Regions,
    /// Combined rail (72px) + sidebar width in `[264.0, 432.0]` (default `375.0`).
    sidebar_width_combined: f32,
    /// Active drag state for the sidebar resize handle: `(start_x, start_combined_width)`.
    sidebar_resizing: Option<(f32, f32)>,
    /// Trimmed READY snapshot used to keep persisted private channels in sync for instant cold starts.
    cached_ready: Option<serde_json::Value>,
    /// Last time `cached_ready` was flushed to disk during live DM updates.
    last_ready_cache_save: Option<Instant>,
    /// Cached first wishlist IDs keyed by user id, persisted across launches.
    wishlist_ids_cache: HashMap<u64, String>,
    /// Wishlist SKU ids whose Shop preview art should prefetch as soon as products load.
    wishlist_prefetch_skus: HashSet<String>,
}

impl Root {
    pub fn new(
        requests: Requests,
        updates: UnboundedSender<Update>,
        cx: &mut Context<Self>,
    ) -> Self {
        cx.on_release(|this, cx| {
            this.release_nameplates(cx);
            this.release_member_avatars(cx);
            this.release_inline_videos(cx);
            this.release_videos(cx);
            this.release_gif_tiles(cx);
            this.release_call_background(cx);
        })
        .detach();
        audio_menu::restore_audio_prefs();
        // The background updater reaches this session's window through the same channel as the tray.
        crate::updater::attach(updates.clone());
        let voice = settings::VoiceState::restore();
        let mut setting_choices = voice.choices();
        setting_choices.extend(settings::prefs::restore());
        let composer = cx.new(|cx| {
            let mut editor = Editor::new(editor_style(), cx);
            editor.set_spellcheck(true);
            editor.set_markdown_preview(true);
            editor.set_ellipsis_placeholder(true);
            editor
        });
        cx.subscribe(&composer, |this, composer, event, cx| match event {
            EditorEvent::Submit(_) if this.screening_pending() => {}
            EditorEvent::Submit(_) if this.slash.pending.is_some() => {
                this.submit_pending(cx);
            }
            EditorEvent::Submit(content) => {
                if this.hold_too_long(content, cx) || this.hold_slowmode(content, cx) {
                    return;
                }
                this.last_typing_sent = None;
                this.send(content.clone());
            }
            EditorEvent::Edited => {
                if this.pref_on("chat_emoji_convert_emoticons", true)
                    && let Some(converted) = emoticons::convert(composer.read(cx).text())
                {
                    composer.update(cx, |editor, _| editor.set_text(&converted));
                }
                this.notify_typing()
            }
            EditorEvent::Cancel => {}
        })
        .detach();
        // The editor notifies on every edit and caret move; the suggestion list follows the word at the caret.
        cx.observe(&composer, |this, _, cx| {
            this.refresh_autocomplete(false, cx)
        })
        .detach();
        Self {
            stores: Stores::default(),
            requests,
            guild: None,
            channel: None,
            screening: None,
            view_as_role: Default::default(),
            reapply_after_remove: None,
            slowmode: Default::default(),
            slowmode_ticking: false,
            presence_ticking: false,
            nameplates: Default::default(),
            profile_effects: Default::default(),
            member_avatars: Default::default(),
            member_window: member_window::State::default(),
            inline_videos: Default::default(),
            gif_tiles: Default::default(),
            video_hub: Rc::new(RefCell::new(video_player::VideoHub::new())),
            fetch_failures: HashMap::new(),
            data_revision: 0,
            prepared: Prepared::default(),
            hovered_media: None,
            guild_guide_open: false,
            guild_guide_scroll: gpui::ScrollHandle::new(),
            sidebar_scroll: gpui::ScrollHandle::new(),
            boost_page: None,
            boost_apply: None,
            menu_scroll: gpui::ScrollHandle::new(),
            channel_browser_open: false,
            channel_browser: None,
            hovered_nameplate: None,
            hovered_welcome: false,
            group_header_hover: false,
            slowmode_emphasis: None,
            slowmode_rejected: HashMap::new(),
            send_ui: send_state::State::default(),
            failed_tint: std::cell::Cell::new(false),
            official_text: std::cell::Cell::new(None),
            official_messages: official_messages::OfficialMessages::default(),
            notification_runtime: crate::notifications::Runtime::new(updates.clone()),
            detect: running_games::Games::new(updates.clone()),
            overlay: overlay::Overlay::default(),
            tray_runtime: crate::tray::Runtime::new(updates.clone()),
            #[cfg(feature = "capture")]
            capture_badge: None,
            #[cfg(feature = "capture")]
            capture_file_card: None,
            #[cfg(feature = "capture")]
            capture_upload_card: None,
            #[cfg(feature = "capture")]
            capture_requests: None,
            #[cfg(feature = "capture")]
            capture_dm_rail: None,
            hovered_rail_dm: None,
            rail_folders: rail_folders::State::default(),
            account_switcher: account_switcher::State::default(),
            #[cfg(feature = "capture")]
            capture_friend_nickname: false,
            notification_focused: false,
            autoscroll: autoscroll::Shared::default(),
            notification_sounds: Default::default(),
            rows: Vec::new(),
            list: ListState::new(0, ListAlignment::Bottom, px(1000.)),
            composer,
            autocomplete: autocomplete::State::default(),
            slash: slash::State::default(),
            context: context_menu::State::default(),
            channel_drag: channel_drag::State::default(),
            too_long: None,
            chat_extra: chat_notices::State::default(),
            focus: cx.focus_handle(),
            keys: Default::default(),
            images: HashMap::new(),
            requested: HashSet::new(),
            image_retries: image_retry::Retries::default(),
            image_crop: Default::default(),
            badge_modal: None,
            spotify_actions: spotify_activity::Actions::default(),
            avatar_wants: std::cell::RefCell::new(Vec::new()),
            ui_scale: std::cell::Cell::new(1.),
            avatar_over: None,
            selection: SharedSelection::default(),
            hovered: None,
            editing: None,
            replying: None,
            reply_ping: true,
            menu: None,
            menu_hit: None,
            menu_extra: menu::MenuExtra::default(),
            last_menu: None,
            confirm_delete: None,
            // Capture builds: `GRAVECORD_SHOW_MEMBERS=1` starts with the member list open, so a start's first frames show
            // it (the panel is opened by its header button otherwise; ChannelSectionStore is not ported, F-0081).
            show_members: cfg!(feature = "capture")
                && std::env::var("GRAVECORD_SHOW_MEMBERS").as_deref() == Ok("1"),
            member_view: None,
            show_dm_profile: true,
            group_members: true,
            friends_tab: friends::FriendsTab::default(),
            skin_tone: picker::stored_tone(),
            skin_menu: false,
            search: None,
            search_filters: None,
            add_server: None,
            birthday: birthday::State::default(),
            file_menu: None,
            file_wrap: HashSet::new(),
            file_language: HashMap::new(),
            file_expanded: None,
            view_reactions: None,
            polls: poll::PollUi::default(),
            audio_menu: None,
            vv: voice_view::State::default(),
            call_volume: call_volume::State::default(),
            stream_previews: stream_previews::Previews::default(),
            stream_zoom: stream_zoom::State::default(),
            invite: None,
            invite_codes: HashMap::new(),
            pending_invite_link: None,
            guild_notifications: None,
            raid_alerts_off: HashSet::new(),
            create_channel: None,
            channel_status: None,
            follow: None,
            follow_success: None,
            guild_cap_upsell: false,
            app_directory_promo: None,
            guide_celebration: None,
            guide_complete_guilds: Default::default(),
            start_stage: None,
            stage_coach_closed: false,
            stage_welcome: None,
            stage_welcome_seen: false,
            stage_leave_menu: false,
            stage_end_confirm: None,
            voice_alert: None,
            stream_full_seen: Default::default(),
            blocked_in_voice: Default::default(),
            warning_dismissals: voice_blocked::WarningDismissals::load(),
            unread_notice: unread_notice::State::load(),
            stage_requests_open: false,
            stage_video_upsell_dismissed: Default::default(),
            video_ready: Default::default(),
            overlay_survey: None,
            overlay_survey_pending: false,
            feedback: Default::default(),
            feedback_roll: None,
            voice_users_expanded: false,
            video_pip: Default::default(),
            av_errors: Default::default(),
            stage_tile_menu: None,
            live_now_closed: Vec::new(),
            publish_confirm: None,
            follower_stats: Default::default(),
            follow_blocker_forced: false,
            guild_events: None,
            guild_menu: false,
            favorites_view: false,
            favorites_suggest_dismissed: false,
            role_picker: None,
            mod_view: None,
            thread_browser: None,
            thread_modal: false,
            new_thread: None,
            pending_thread_message: None,
            inbox: None,
            inbox_read: HashSet::new(),
            inbox_collapsed: HashSet::new(),
            gifs: None,
            sticker_ui: stickers::Stickers::default(),
            favorite_gifs: None,
            emoji_favorites: None,
            shown_blocked: HashSet::new(),
            shown_blocked_profiles: HashSet::new(),
            hovered_dm: None,
            group_dm: None,
            add_friend: None,
            people_search: None,
            friend_menu: None,
            confirm_remove_friend: None,
            friend_nickname: None,
            favorite_nickname: None,
            group_edit: None,
            group_leave: None,
            member_action: None,
            quick_switcher: None,
            activity_shelf: None,
            activities: Default::default(),
            notify_open: false,
            notify_submenu: None,
            pins_open: None,
            confirm_unpin: None,
            forward: None,
            profile_menu: None,
            profile_menu_which: profile_menu::Which::Overflow,
            remediation: None,
            report: None,
            full_profile: None,
            profile_editor: None,
            requested_invites: HashSet::new(),
            command_mentions: Vec::new(),
            generated_images: std::cell::RefCell::new(HashMap::new()),
            guild_stack_pending: std::cell::RefCell::new(HashSet::new()),
            voice_hub: std::rc::Rc::new(std::cell::RefCell::new(voice_message::Hub::new())),
            voice_rec: std::rc::Rc::new(std::cell::RefCell::new(voice_message::RecState::new())),
            #[cfg(feature = "capture")]
            voice_demo_key: None,
            #[cfg(feature = "capture")]
            voice_demo_seconds: 0.,
            pending_join: None,
            open_dm_after_send: false,
            mention_filter: inbox::MentionFilter::stored(),
            channel_settings: None,
            server_settings: None,
            games: HashMap::new(),
            security_actions: None,
            mentions_filter_open: false,
            channel_history: Vec::new(),
            speech: crate::speech::Speech::new(),
            speaking_message: None,
            toast: None,
            app_toast: None,
            submenu: None,
            custom_reminder: None,
            lightbox: None,
            status_popout: None,
            custom_status: None,
            plus_menu: false,
            schedule: Default::default(),
            modify_attachment: None,
            report_issue: None,
            game_profile: None,
            game_communities_layout: Default::default(),
            game_announcements_scroll: 0.,
            family_center: Default::default(),
            library: Default::default(),
            provider_player: None,
            web_frames: web_frames::WebFrames::default(),
            composer_ai: Default::default(),
            game_similar_scroll: 0.,
            game_communities_filter: Default::default(),
            launcher_collections_at: None,
            upload_error: None,
            try_it_out_upsell: false,
            sent_edit: None,
            snooze_menu: None,
            click_at: gpui::Point::default(),
            leave_guild: None,
            destructive: None,
            reorder_hint: None,
            tutorial_tips: tutorial_tips::TutorialTips::default(),
            logout_confirm: false,
            poll: None,
            components: components::State::default(),
            interaction_ui: interaction_flow::State::default(),
            viewport: (1280., 720.),
            unread_from: None,
            jump: jump::JumpUi::default(),
            picker_field: None,
            picker: None,
            emoji_used: Vec::new(),
            confirm_pin: None,
            reactions_used: Vec::new(),
            history: history::History::default(),
            leaving: None,
            last_typing_sent: None,
            tip: None,
            tip_pending: None,
            active_menu_slider_drag: None,
            content_width: 793.,
            profile: None,
            call: None,
            pending_call: None,
            dmc: Default::default(),
            ring_audio: Default::default(),
            video: Default::default(),
            go_live: None,
            go_live_generation: 0,
            camera_error: None,
            panel: voice_panel::PanelState::default(),
            soundboard: soundboard::Board::new(),
            updates,
            ptt_hook: None,
            ptt_down: false,
            self_mute: false,
            self_deaf: false,
            vad: vad_permission::VadState::default(),
            speaking: HashSet::new(),
            texts: HashMap::new(),
            pending_files: Vec::new(),
            upload_scroll: gpui::ScrollHandle::new(),
            revealed: HashSet::new(),
            weak: cx.weak_entity(),
            settings: None,
            setting_choices,
            settings_extra: settings::ActionsState::default(),
            voice,
            clips: settings::ClipsState::restore(),
            fetched: HashMap::new(),
            active_now: active_now::State::default(),
            nitro: nitro::NitroState::default(),
            nitro_open: false,
            discover: discover::State::default(),
            shop: shop::State::default(),
            quests: quests::State::default(),
            message_requests: message_requests::State::default(),
            sound_dialog: None,
            stream_report: None,
            tip_anchors: Rc::default(),
            user_panel_height: None,
            inventory: inventory_feed::State::restore(),
            inventory_popout: inventory_popout::State::default(),
            sticker_popout: None,
            emoji_popout: None,
            role_popout: None,
            header_popout: None,
            copied_code: HashSet::new(),
            ready: false,
            hydrated_from_cache: false,
            start_members: Vec::new(),
            start_member_list: None,
            member_list_saved: None,
            start_keep: HashSet::new(),
            splash: splash::Splash::new(),
            forums: HashMap::new(),
            regions: regions::Regions::default(),
            sidebar_width_combined: if crate::storage::is_capture_run() {
                375.
            } else {
                crate::storage::get("resizable-sidebar-width-2")
                    .and_then(|v| v.as_f64())
                    .map(|w| (w as f32).clamp(264., 432.))
                    .unwrap_or(375.)
            },
            sidebar_resizing: None,
            cached_ready: None,
            last_ready_cache_save: None,
            wishlist_ids_cache: HashMap::new(),
            wishlist_prefetch_skus: HashSet::new(),
        }
    }

    pub(crate) fn combined_sidebar_width(&self) -> f32 {
        self.sidebar_width_combined.clamp(264., 432.)
    }

    pub(crate) fn sidebar_width(&self) -> f32 {
        self.combined_sidebar_width() - 72.
    }

    pub(crate) fn sidebar_row_width(&self) -> f32 {
        self.sidebar_width() - 17.
    }

    fn sidebar_resize_move(&mut self, event: &gpui::MouseMoveEvent, cx: &mut Context<Self>) {
        let Some((start_x, start_width)) = self.sidebar_resizing else {
            return;
        };
        let next = (start_width + (f32::from(event.position.x) - start_x)).clamp(264., 432.);
        if (next - self.sidebar_width_combined).abs() > 0.25 {
            self.sidebar_width_combined = next;
            cx.notify();
        }
    }

    fn sidebar_resize_end(&mut self, cx: &mut Context<Self>) {
        if self.sidebar_resizing.take().is_some() {
            if !crate::storage::is_capture_run() {
                crate::storage::set(
                    "resizable-sidebar-width-2",
                    serde_json::Value::from(self.combined_sidebar_width() as f64),
                );
            }
            cx.notify();
        }
    }

    fn sidebar_resize_handle(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let dragging = self.sidebar_resizing.is_some();
        div()
            .id("sidebar-resize-handle")
            .occlude()
            .group("sidebar-resize")
            .absolute()
            .left(px(self.combined_sidebar_width() - 4.))
            .top(px(32.))
            .bottom(px(0.))
            .w(px(8.))
            .cursor(gpui::CursorStyle::ResizeLeftRight)
            .flex()
            .justify_center()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, event: &gpui::MouseDownEvent, _window, cx| {
                    cx.stop_propagation();
                    if event.click_count >= 2 {
                        this.sidebar_resizing = None;
                        this.sidebar_width_combined = 375.;
                        if !crate::storage::is_capture_run() {
                            crate::storage::set(
                                "resizable-sidebar-width-2",
                                serde_json::Value::from(375.0_f64),
                            );
                        }
                    } else {
                        this.sidebar_resizing =
                            Some((f32::from(event.position.x), this.combined_sidebar_width()));
                    }
                    cx.notify();
                }),
            )
            .child(
                div()
                    .w(px(2.))
                    .h_full()
                    .when(dragging, |bar| {
                        bar.bg(color("--control-primary-background-default"))
                    })
                    .when(!dragging, |bar| {
                        bar.group_hover("sidebar-resize", |s| s.bg(color("--border-strong")))
                    }),
            )
    }

    pub fn update(&mut self, update: Update, cx: &mut Context<Self>) {
        let stage = match &update {
            Update::Dispatch { name, .. } if name == "READY" => Some("ready_applied"),
            Update::Dispatch { name, .. } if name == "READY_SUPPLEMENTAL" => Some("supp_applied"),
            Update::Dispatch { name, .. } if name == "GUILD_MEMBER_LIST_UPDATE" => {
                Some("members_applied")
            }
            Update::Messages { channel, .. } if Some(*channel) == self.channel => {
                Some("msgs_applied")
            }
            Update::Image { .. } => Some("img_first_recv"),
            _ => None,
        };
        let applying = std::time::Instant::now();
        self.apply_update(update, cx);
        if let Some(stage) = stage {
            crate::start_trace::mark(stage);
            match stage {
                "ready_applied" => crate::start_trace::took("dur_ready_total", applying),
                "msgs_applied" => crate::start_trace::took("dur_msgs_apply", applying),
                _ => {}
            }
        }
        self.data_revision += 1;
        self.splash_check();
        // Native indicators must update even while the window is hidden and not rendering.
        self.refresh_notification_badge_soon(cx);
    }

    fn apply_update(&mut self, update: Update, cx: &mut Context<Self>) {
        match update {
            Update::GatewayConnected(connected) => {
                self.tutorial_connection(connected);
                self.inventory_connection_changed(connected, cx);
                self.detect_connection_changed(connected, cx);
            }
            Update::Games(scan) => {
                self.detect_scanned(*scan, cx);
                self.overlay_sync(cx);
            }
            Update::DetectablesCdn {
                key,
                status,
                etag,
                games,
            } => {
                if self.detect_cdn_fetched(&key, status, etag, games) {
                    cx.notify();
                }
            }
            Update::OverlayFocus(foreground) => self.overlay_focus_changed(foreground, cx),
            Update::Rpc(event) => self.rpc_event(event, cx),
            // Not in the pinned client: a plugin's desktop notification (`plugin_hooks/plugin_notifications.rs`).
            Update::NotificationClicked(channel) if self.plugin_notification_clicked(channel, cx) => {}
            Update::NotificationClicked(channel) => self.open_notification_channel(channel, cx),
            Update::Tray(action) => self.tray_action(action, cx),
            Update::Updater => cx.notify(),
            Update::SpotifySocket { account, connected } => {
                self.stores
                    .spotify
                    .set_socket_connected(&account, connected);
                self.spotify_devices_changed(cx);
                cx.notify();
            }
            Update::SpotifyReply { request, reply } => self.spotify_reply(request, reply, cx),
            Update::Slowmode {
                channel,
                kind,
                millis,
            } => self.set_slowmode(channel, kind, millis, cx),
            Update::SlowmodeRejected {
                channel,
                draft,
                reason,
            } => {
                self.slowmode_rejected
                    .entry(channel)
                    .or_default()
                    .push_back(draft);
                if let Some(reason) = reason {
                    self.put_app_toast(reason, false, cx);
                }
            }
            Update::SendResult {
                channel,
                nonce,
                outcome,
            } => self.send_finished(channel, &nonce, outcome, cx),
            Update::SendProgress {
                nonce,
                index,
                sent,
                total,
            } => self.send_progress(&nonce, index, sent, total, cx),
            Update::Dispatch { name, data } => {
                if self.expand_voice_batch(&name, &data, cx) {
                    return;
                }
                // The server's answer to an older settings write of this app, arriving while a newer one is on its
                // way: applying it put the older choice back on screen (`net::Requests`). The last write's answer
                // brings the server's settings in (`Update::SettingsWritten`).
                if name == "USER_SETTINGS_PROTO_UPDATE"
                    && data["settings"]["type"].as_u64() == Some(1)
                    && self.requests.settings_writes_pending()
                {
                    return;
                }
                let voice_before = (name == "VOICE_STATE_UPDATE")
                    .then(|| (self.others_in_call(), self.streamers_in_call()));
                // The person's channel before this state (`oldChannelId`), for `voice_blocked`.
                let voice_mover = (name == "VOICE_STATE_UPDATE")
                    .then(|| {
                        let user = data["user_id"].as_str()?.parse::<Id>().ok()?;
                        let guild = data["guild_id"]
                            .as_str()
                            .and_then(|id| id.parse::<Id>().ok())
                            .unwrap_or(0);
                        Some((guild, user, self.voice_channel_of(guild, user)))
                    })
                    .flatten();
                let ready_prev_place = (name == "READY"
                    && (self.hydrated_from_cache || self.stores.current_user.is_some()))
                .then_some((self.guild, self.channel));
                let rebuild_messages = matches!(
                    name.as_str(),
                    "MESSAGE_CREATE"
                        | "MESSAGE_DELETE"
                        | "MESSAGE_UPDATE"
                        | "MESSAGE_ACK"
                        | "AUTO_MODERATION_CONTENT_DELETED"
                );
                // Capture before dispatch changes the message indices that the current rows refer to.
                let message_anchor = rebuild_messages.then(|| self.scroll_anchor()).flatten();
                if name == "MESSAGE_CREATE" {
                    self.send_echo(&data);
                }
                if name == "GUILD_MEMBER_LIST_UPDATE" {
                    self.member_list_arriving(&data);
                }
                let dispatching = std::time::Instant::now();
                self.stores.dispatch(&name, &data);
                self.unread_notice_event(&name, &data);
                match name.as_str() {
                    "READY" => crate::start_trace::took("dur_ready_dispatch", dispatching),
                    "READY_SUPPLEMENTAL" => {
                        crate::start_trace::took("dur_supp_dispatch", dispatching)
                    }
                    _ => {}
                }
                self.urgent_message_event(&name);
                // PermissionVADStore's events (module 596339 `wf`): the check runs again while in a call.
                if self.call.is_some()
                    && matches!(
                        name.as_str(),
                        "CHANNEL_UPDATE"
                            | "THREAD_UPDATE"
                            | "GUILD_ROLE_UPDATE"
                            | "GUILD_MEMBER_UPDATE"
                            | "VOICE_STATE_UPDATE"
                    )
                    && self.sync_vad_permission()
                {
                    cx.notify();
                }
                self.spotify_devices_changed(cx);
                self.spotify_loading_changed();
                if name == "READY" {
                    self.blocked_in_voice.reset();
                    self.keep_start_member_presences();
                }
                if let Some((guild, user, old)) = voice_mover {
                    let new = self.voice_channel_of(guild, user);
                    self.blocked_voice_state(user, old, new, cx);
                }
                if name.starts_with("RELATIONSHIP_") {
                    self.blocked_relationship_changed(&data, cx);
                }
                self.friend_notice_event(&name, &data);
                if name == "VOICE_STATE_UPDATE" {
                    self.ended_stream_voice_state();
                    self.stage_invite_check(&data, cx);
                    self.voice_state_of_other_session(&data, cx);
                    // Not in the pinned client: the VoiceStats plugin (`plugin_hooks/voice_stats.rs`).
                    self.voice_stats_observe(cx);
                }
                // Not in the pinned client: CallTimer's join times (`plugin_hooks/call_timer.rs`).
                if matches!(
                    name.as_str(),
                    "VOICE_STATE_UPDATE" | "PASSIVE_UPDATE_V1" | "READY" | "GUILD_CREATE"
                ) {
                    self.call_timer_observe(cx);
                }
                self.official_messages.dispatch(&name, &data);
                self.birthday_dispatch(&name, &data);
                self.tutorial_tips.dispatch(&name, &data);
                self.gateway_screens_dispatch(&name, &data, cx);
                self.channel_info_check();
                // Another session changed the status or the custom status: the account's own presence follows.
                if name == "USER_SETTINGS_PROTO_UPDATE" || name == "SESSIONS_REPLACE" {
                    self.refresh_own_presence();
                    self.schedule_presence_retry(cx);
                }
                // The Spotify activity of the account's own presence (modules 655116, 491009 and 894768).
                if matches!(
                    name.as_str(),
                    "SPOTIFY_PLAYER_STATE" | "SPOTIFY_PLAYER_PLAY" | "USER_CONNECTIONS_UPDATE"
                ) {
                    self.spotify_presence_changed(cx);
                }
                self.calls_after_dispatch(&name, cx);
                self.interaction_dispatched(&name, &data, cx);
                if name == "READY" {
                    self.voice_after_dispatch(&name, cx);
                    self.inventory_connection_open();
                    self.detect_ready();
                    self.clips_connection_open(cx);
                }
                self.sync_dm_call_idle_timer(cx);
                if matches!(name.as_str(), "READY" | "GUILD_DELETE") {
                    self.request_geo_restricted_icons();
                }
                if name == "GUILD_MEMBER_UPDATE" {
                    self.guide_member_updated(&data, cx);
                }
                // The server in view is gone (left, removed from it, or deleted): back to Home, as the official
                // client goes. An outage keeps the server and the view.
                // A geo-restricted server (module 543897) goes from the store too, so the view leaves it.
                if name == "GUILD_DELETE"
                    && (data["unavailable"] != true || data["geo_restricted"] == true)
                    && let Some(guild) = data["id"].as_str().and_then(|id| id.parse::<Id>().ok())
                {
                    if self.guild == Some(guild) {
                        self.select_home(None);
                    } else {
                        self.rebuild_rows();
                    }
                }
                if matches!(
                    name.as_str(),
                    "RECENT_MENTION_DELETE" | "MESSAGE_DELETE" | "MESSAGE_DELETE_BULK"
                ) {
                    self.mentions_deleted(&name, &data, cx);
                }
                if name == "QUESTS_USER_STATUS_UPDATE" {
                    self.quests.store.apply_gateway_status(&data);
                }
                // Capture only: `gw QUESTS_CAPTURE_CURRENT <body>` stands for the answer of the current-quests request.
                #[cfg(feature = "capture")]
                if name == "QUESTS_CAPTURE_CURRENT" {
                    self.quests.store.apply_current(&data);
                }
                // Capture only: `gw LIBRARY_CAPTURE [entries]` stands for the library answer.
                #[cfg(feature = "capture")]
                if name == "LIBRARY_CAPTURE" {
                    let answer = serde_json::json!({"ok": true, "status": 200, "body": data, "capture": true});
                    self.library_fetched("library:applications", &answer);
                }
                // ... and `gw QUESTS_CAPTURE_CLAIMED <body>` for the claimed list's.
                #[cfg(feature = "capture")]
                if name == "QUESTS_CAPTURE_CLAIMED" {
                    quests::CLAIMED_INJECTED.store(true, std::sync::atomic::Ordering::Relaxed);
                    let mut data = data.clone();
                    data["capture"] = serde_json::Value::Bool(true);
                    self.fetched.insert("quests:claimed".to_owned(), data);
                    self.quests_fetched("quests:claimed", cx);
                }
                if name == "QUESTS_USER_COMPLETION_UPDATE" {
                    self.quests.store.apply_completion_update(&data);
                }
                if name == "CONTENT_INVENTORY_INBOX_STALE" {
                    self.inventory_stale(&data, cx);
                }
                self.sidebar_event_update(&name, &data);
                // Not in the pinned client: NewGuildSettings on a server just joined
                // (`plugin_hooks/new_guild_settings.rs`).
                self.new_guild_settings_dispatch(&name, &data);
                // Not in the pinned client: RelationshipNotifier's flux handlers
                // (`plugin_hooks/relationship_notifier.rs`).
                self.relationship_notifier_dispatch(&name, &data, cx);
                // Not in the pinned client: BypassStatus' MESSAGE_CREATE (`plugin_hooks/bypass_status.rs`).
                self.bypass_status_dispatch(&name, &data, cx);
                if name == "CHANNEL_PINS_UPDATE" {
                    self.ack_open_pins();
                }
                if name == "PRESENCE_UPDATE" {
                    self.request_presence_image_of(&data);
                    self.listen_along_presence(&data, cx);
                }
                if matches!(
                    name.as_str(),
                    "READY"
                        | "READY_SUPPLEMENTAL"
                        | "GUILD_MEMBER_LIST_UPDATE"
                        | "GUILD_MEMBERS_CHUNK"
                        | "GUILD_CREATE"
                ) {
                    self.request_presence_images();
                }
                if name == "MESSAGE_CREATE" {
                    self.notify_message(&data, cx);
                }
                if name == "CHANNEL_UPDATE"
                    && let Some(channel) = data["id"].as_str().and_then(|id| id.parse().ok())
                    && let Some((rate, bypass)) = self.slowmode_channel(channel)
                {
                    self.slowmode
                        .channel_updated(channel, rate, bypass, Instant::now());
                }
                if name == "GUILD_MEMBERS_CHUNK" {
                    self.refresh_autocomplete(true, cx);
                    self.request_role_popout_images();
                }
                self.soundboard_dispatch(&name, &data, cx);
                match name.as_str() {
                    "EMBEDDED_ACTIVITY_UPDATE_V2" => self.activity_instance_update(&data, cx),
                    "READY_SUPPLEMENTAL" => {
                        for guild in data["guilds"].as_array().into_iter().flatten() {
                            for instance in
                                guild["activity_instances"].as_array().into_iter().flatten()
                            {
                                let mut instance = instance.clone();
                                instance["guild_id"] = guild["id"].clone();
                                self.activity_instance_update(&instance, cx);
                            }
                        }
                    }
                    _ => {}
                }
                self.stream_dispatch(&name, &data, cx);
                if name == "VOICE_STATE_UPDATE" {
                    let (others, streamers) = voice_before.unwrap_or_default();
                    self.after_voice_state(others, streamers, &data);
                    self.dm_auto_watch(cx);
                }
                if name.starts_with("RELATIONSHIP_") {
                    self.rebuild_rows();
                    self.request_images();
                }
                if matches!(
                    name.as_str(),
                    "CHANNEL_CREATE"
                        | "CHANNEL_UPDATE"
                        | "CHANNEL_UPDATES"
                        | "CHANNEL_RECIPIENT_ADD"
                ) {
                    self.request_images();
                    self.request_group_images();
                }
                if matches!(name.as_str(), "USER_UPDATE" | "READY_SUPPLEMENTAL")
                    || (name == "PRESENCE_UPDATE" && data["user"].get("avatar").is_some())
                {
                    self.request_images();
                }
                if name == "READY" {
                    // The account's saved theme is read again by the theme rules at the next frame.
                    self.reset_theme_sync();
                    crate::sounds::refresh_soundpack(
                        crate::sounds::now_ms(),
                        &self.stores.experiments,
                    );
                    self.hydrated_from_cache = false;
                    self.refresh_start_members();
                    if let Some((prev_guild, prev_channel)) = ready_prev_place {
                        self.reconcile_after_ready(prev_guild, prev_channel);
                    } else {
                        self.after_ready();
                    }
                    let trimming = std::time::Instant::now();
                    let trimmed = start_cache::trim_ready(&data, self.cache_guild().as_deref());
                    crate::start_trace::took("dur_ready_trim", trimming);
                    if let Some(mut trimmed) = trimmed {
                        if !self.stores.private_channels.is_empty() {
                            start_cache::sync_private_channels(&mut trimmed, &self.stores);
                        }
                        self.cached_ready = Some(trimmed.clone());
                        self.last_ready_cache_save = Some(Instant::now());
                        start_cache::save_trimmed_ready(trimmed);
                    }
                }
                if name == "READY_SUPPLEMENTAL" {
                    self.request_group_images();
                    self.sync_ready_dm_cache(true);
                    self.warm_top_dm_profiles();
                    self.warm_gifs();
                    self.warm_nitro_art();
                    // As the Nitro art: the settings pictures are on disk before settings first open.
                    if start_cache::enabled() {
                        self.request_settings_art();
                    }
                    if let Some(ch) = self.channel {
                        self.warm_channel_pins(ch);
                    }
                }
                if matches!(
                    name.as_str(),
                    "CHANNEL_CREATE" | "CHANNEL_DELETE" | "CHANNEL_RECIPIENT_ADD"
                ) {
                    self.sync_ready_dm_cache(false);
                }
                if name == "MESSAGE_CREATE"
                    && let Some(cid) = data["channel_id"]
                        .as_str()
                        .and_then(|id| id.parse::<Id>().ok())
                    && self.stores.private_channels.iter().any(|dm| dm.id == cid)
                {
                    self.sync_ready_dm_cache(false);
                }
                if name == "MESSAGE_CREATE"
                    && self.notification_focused
                    && data["channel_id"].as_str()
                        == self.channel.map(|id| id.to_string()).as_deref()
                {
                    // A message in the open channel is read as it arrives.
                    self.read_current_channel_live();
                }
                if rebuild_messages {
                    self.rebuild_rows();
                    self.restore_scroll_anchor(message_anchor);
                }
                if matches!(name.as_str(), "VOICE_STATE_UPDATE" | "VOICE_SERVER_UPDATE") {
                    self.voice_after_dispatch(&name, cx);
                    self.sync_dm_call_idle_timer(cx);
                    self.maybe_connect_voice();
                }
                if matches!(
                    name.as_str(),
                    "GUILD_MEMBER_LIST_UPDATE" | "GUILD_MEMBER_UPDATE" | "GUILD_MEMBERS_CHUNK"
                ) {
                    self.request_member_images();
                }
                if name == "GUILD_MEMBER_LIST_UPDATE" {
                    self.member_list_applied(&data);
                }
                if name == "GUILD_CREATE"
                    && let Some(joined) = self.pending_join
                    && self.stores.guilds.iter().any(|guild| guild.id == joined)
                {
                    self.pending_join = None;
                    self.select(joined, None);
                }
                if name == "TYPING_START" {
                    // Redraw when the notice runs out.
                    cx.spawn(async move |this, cx| {
                        cx.background_executor().timer(TYPING_DURATION).await;
                        let _ = this.update(cx, |_, cx| cx.notify());
                    })
                    .detach();
                }
            }
            Update::Messages {
                channel,
                page,
                started_ms,
            } => {
                let anchor = self.scroll_anchor();
                self.stores.load_messages_since(channel, &page, started_ms);
                self.reinsert_local_sends(channel);
                if Some(channel) == self.channel {
                    self.read_current_channel();
                }
                self.rebuild_rows();
                self.restore_scroll_anchor(anchor);
                self.request_images();
                if Some(channel) == self.channel {
                    let pictures = self.start_pictures();
                    self.keep_start_pictures(&pictures);
                    start_cache::save_messages(
                        self.stores.current_user.as_ref().map(|me| me.id),
                        channel,
                        &page,
                        &pictures,
                        self.start_members_to_keep(),
                    );
                }
                // A page asked for at the start can land before READY, with nothing else to draw it with yet.
                if Some(channel) == self.channel || self.stores.current_user.is_some() {
                    self.ready = true;
                }
                self.kb_sync_hooks();
            }
            Update::MessagesLoaded { spec, page } => self.window_loaded(spec, page, cx),
            Update::MessagesFailed { channel } => {
                self.stores.messages_failed(channel);
                // The open channel's first page failed: the first content is as in as it will get, so the
                // connecting screen ends (`splash_check` runs after every update).
                if Some(channel) == self.channel {
                    self.ready = true;
                }
                cx.notify();
            }
            Update::Voice(generation, event) => self.on_voice_event(generation, event, cx),
            Update::Stream {
                generation,
                key,
                event,
            } => self.on_stream_event(generation, key, event, cx),
            Update::PushToTalk(held) => self.ptt_held_changed(held),
            Update::ClipSaveKey => self.clips_save_key(cx),
            Update::Keybind { id, down } => self.kb_fire(&id, down, cx),
            // The shell, which owns the login, takes this one before it reaches the view.
            Update::AuthFailed | Update::TokenRenewed(_) => {}
            Update::LimitedAccess => self.limited_access_alert(cx),
            Update::SettingsWritten(answer) => {
                // The last write was answered: what the server holds now, the changes of other sessions that were
                // left out in between included. Merged, since everything this app wrote is in it already.
                if let Some(proto) = answer
                    && !self.requests.settings_writes_pending()
                {
                    self.apply_update(
                        Update::Dispatch {
                            name: "USER_SETTINGS_PROTO_UPDATE".to_owned(),
                            data: serde_json::json!({"settings": {"type": 1, "proto": proto}, "partial": true}),
                        },
                        cx,
                    );
                }
            }
            Update::Fetched { key, body } => {
                if let Some(rest) = key.strip_prefix(crate::net::EDIT_KEY) {
                    self.edit_answered(rest, &body, cx);
                    return;
                }
                if let Some(rest) = key.strip_prefix(deep_link_route::CHANNEL_CHECK_KEY) {
                    self.link_channel_checked(rest, &body, cx);
                    return;
                }
                if self.spotify_metadata_fetched(&key, &body, cx) {
                    return;
                }
                // Not in the pinned client: WhoReacted's reactors join the users (`plugin_hooks/who_reacted.rs`).
                self.who_reacted_fetched(&key, &body);
                if self.image_crop_fetched(&key, &body, cx) {
                    return;
                }
                // Not in the pinned client: OpenInApp's resolved short link (`plugin_hooks/open_in_app.rs`).
                if self.open_in_app_resolved(&key, &body, cx) {
                    return;
                }
                // Not in the pinned client: FriendInvites' answers (`plugin_hooks/plugin_commands.rs`).
                if self.friend_invites_answered(&key, &body) {
                    return;
                }
                // Not in the pinned client: RelationshipNotifier's person lookups (`plugin_hooks/relationship_notifier.rs`).
                if self.relationship_notifier_fetched(&key, &body, cx) {
                    return;
                }
                // Not in the pinned client: GlobalBadges' list (`plugin_hooks/global_badges.rs`).
                if self.global_badges_fetched(&key, &body, cx) {
                    return;
                }
                // Not in the pinned client: UserPFP's list (`plugin_hooks/userpfp.rs`).
                if self.userpfp_fetched(&key, &body, cx) {
                    return;
                }
                // Not in the pinned client: USRBG's list (`plugin_hooks/usrbg.rs`).
                if self.usrbg_fetched(&key, &body, cx) {
                    return;
                }
                // Not in the pinned client: ZipPreview's archive arrived (`plugin_hooks/zip_preview.rs`).
                if self.zip_preview_fetched(&key, cx) {
                    return;
                }
                // Not in the pinned client: MoreCommands' fetched answers (`plugin_hooks/more_commands.rs`).
                if self.more_commands_fetched(&key, &body, cx) {
                    return;
                }
                // Not in the pinned client: ValidUser's fetched user (`plugin_hooks/valid_user.rs`).
                if self.valid_user_fetched(&key, &body, cx) {
                    return;
                }
                // Not in the pinned client: DownloadAllAttachments' outcome (`plugin_hooks/download_all.rs`).
                if self.download_all_answered(&key, &body, cx) {
                    return;
                }
                self.active_now_fetched(&key, &body);
                if key.starts_with(chat_notices::KEY_PREFIX) {
                    self.thread_step_fetched(&key, &body);
                    return;
                }
                if key.starts_with(message_requests::KEY_PREFIX) {
                    self.message_requests_fetched(&key, &body, cx);
                    return;
                }
                if key.starts_with(mod_view::KEY_PREFIX) {
                    self.mod_view_fetched(&key, body, cx);
                    return;
                }
                if key.starts_with(stream_previews::KEY_PREFIX) {
                    self.stream_preview_fetched(&key, &body, cx);
                    return;
                }
                if self.top_emoji_fetched(&key, &body, cx) {
                    return;
                }
                if self.library_fetched(&key, &body) {
                    cx.notify();
                    return;
                }
                if key.starts_with(composer_ai::KEY_PREFIX) {
                    self.composer_ai_answer(&body, cx);
                    return;
                }
                if key.starts_with(game_similar::KEY_PREFIX) {
                    self.game_similar_fetched(&key, body);
                    cx.notify();
                    return;
                }
                if key.starts_with(game_announcements::KEY_PREFIX) {
                    self.game_announcements_fetched(&key, body);
                    cx.notify();
                    return;
                }
                if key.starts_with(game_communities::KEY_PREFIX) {
                    self.game_communities_fetched(&key, body);
                    cx.notify();
                    return;
                }
                if key.starts_with(boost_apply::KEY_PREFIX) {
                    self.boost_apply_fetched(&key, &body, cx);
                    return;
                }
                if key == channel_status::STATUS_KEY {
                    self.voice_status_answered(&body, cx);
                    return;
                }
                if key == announcements::FOLLOW_KEY {
                    self.follow_answered(&body, cx);
                    return;
                }
                if key == announcements::CROSSPOST_KEY {
                    return;
                }
                if key == stage::STAGE_KEY {
                    self.stage_answered(&body, cx);
                    return;
                }
                if let Some(channel) = key
                    .strip_prefix(announcements::STATS_KEY)
                    .and_then(|rest| rest.parse::<Id>().ok())
                {
                    self.follower_stats_answered(channel, &body, cx);
                    return;
                }
                self.thread_members_fetched(&key, &body);
                if key.starts_with(voice_message::VOICE_SEND_KEY) {
                    self.voice_sent(&body, cx);
                    return;
                }
                if key.starts_with(role_popout::IDS_PREFIX)
                    || key.starts_with(role_popout::COUNTS_PREFIX)
                {
                    self.role_popout_fetched(&key, body, cx);
                    return;
                }
                if self.profile_effect_fetched(&key, &body, cx) {
                    return;
                }
                if self.video_background_fetched(&key, &body, cx) {
                    return;
                }
                #[cfg(feature = "capture")]
                if key == "webhook-demo" {
                    self.webhook_demo_done(&body);
                    return;
                }
                if key.starts_with("dm-call-ringable:") {
                    self.dm_call_ringable(&key, &body, cx);
                    return;
                }
                if let Some(nonce) = key.strip_prefix("interaction:") {
                    self.interaction_answered(nonce, &body, cx);
                    return;
                }
                if key == settings::SESSIONS_LOGOUT_KEY {
                    self.sessions_logout_answered(&body);
                    return;
                }
                if key == settings::MFA_KEY || key == settings::MFA_CREDENTIALS_KEY {
                    self.mfa_answered(&key, &body, cx);
                    return;
                }
                if key == settings::ACCOUNT_USERNAME_KEY {
                    self.username_check_answered(&body, cx);
                    return;
                }
                if key == settings::ACCOUNT_FORM_KEY {
                    self.account_form_fetched(&body, cx);
                    return;
                }
                if self.connection_authorize_answered(&key, &body, cx) {
                    return;
                }
                if self.settings_fetched(&key, &body) {
                    cx.notify();
                    return;
                }
                if self.detect_lists_fetched(&key, &body) {
                    return;
                }
                if self.fetch_failed(&key, &body, cx) {
                    // Retrying is driven by each key's own request guard (`retry_failed_fetch`).
                    cx.notify();
                    return;
                }
                if self.guild_events_fetched(&key, &body, cx) {
                    return;
                }
                if self.schedule_fetched(&key, &body, cx) {
                    return;
                }
                if self.inventory_games_fetched(&key, &body) {
                    cx.notify();
                    return;
                }
                if self.inventory_reply_fetched(&key, &body, cx) {
                    return;
                }
                if self.inventory_fetched(&key, &body) {
                    self.schedule_inventory(cx);
                    cx.notify();
                    return;
                }
                if self.friend_nickname_fetched(&key, &body, cx) {
                    return;
                }
                if self.member_action_fetched(&key, &body, cx) {
                    return;
                }
                if key.starts_with("presence-button:") {
                    self.live_button_fetched(&key, body, cx);
                    return;
                }
                if key.starts_with("oauth-token:") {
                    self.live_tokens_fetched(key, body);
                    cx.notify();
                    return;
                }
                if key.starts_with("presence-app:") {
                    self.live_application_fetched(&body);
                    self.public_application_answered(&key, &body);
                    self.fetched.insert(key, body);
                    cx.notify();
                    return;
                }
                if self.screening_fetched(&key, &body, cx) {
                    return;
                }
                if self.game_request_answered(&key, &body, cx) {
                    return;
                }
                if self.join_request_answered(&key, &body, cx) {
                    return;
                }
                if self.invite_accept_answered(&key, &body, cx) {
                    return;
                }
                if self.birthday_answered(&key, &body, cx) {
                    return;
                }
                if self.thread_browser_fetched(&key, &body, cx) {
                    return;
                }
                if self.forum_fetched(&key, &body, cx) {
                    return;
                }
                if key.starts_with(activity_launch::ACTIVITY_PREFIX) {
                    self.activity_fetched(&key, &body, cx);
                    return;
                }
                if key.starts_with("game-profile:") {
                    self.game_profile_fetched(&key, body, cx);
                    return;
                }
                if key.starts_with(app_widget::CONFIG_PREFIX)
                    || key.starts_with(app_widget::IDENTITY_PREFIX)
                {
                    self.app_widget_fetched(key, body, cx);
                    return;
                }
                if key.starts_with(profile_frames::PRODUCT_PREFIX) {
                    self.profile_frame_fetched(key, body, cx);
                    return;
                }
                if key.starts_with(launcher_home::APP_INDEX_PREFIX) {
                    self.fetched.insert(key, body);
                    cx.notify();
                    return;
                }
                if key == launcher_home::COLLECTIONS_KEY {
                    self.launcher_collections_fetched(body, cx);
                    return;
                }
                if key.starts_with(activity_shelf::SHELF_PREFIX) {
                    self.fetched.insert(key.clone(), body);
                    self.activity_shelf_fetched(&key, cx);
                    return;
                }
                if key.starts_with("soundboard:") {
                    self.soundboard_fetched(&key, &body, cx);
                    return;
                }
                if key == lightbox::SAVED_KEY {
                    self.lightbox_saved(&body, cx);
                    return;
                }
                if key == "voice-logs-upload" {
                    self.voice_logs_uploaded(&body, cx);
                    return;
                }
                if self.friends_fetched(&key, &body, cx) || self.slash_fetched(&key, &body, cx) {
                    return;
                }
                if self.full_profile.is_some()
                    && [
                        "outbox:",
                        "apps:",
                        "note:",
                        "profile:",
                        "profile-games:",
                        "profile-wishlist:",
                    ]
                    .iter()
                    .any(|prefix| key.starts_with(prefix))
                {
                    self.fetched.insert(key.clone(), body.clone());
                    self.full_profile_fetched(&key, cx);
                }
                if key.starts_with(server_settings_pages::WEBHOOKS_PREFIX) {
                    self.fetched.insert(key.clone(), body.clone());
                    self.webhooks_fetched(&key, cx);
                    return;
                }
                if key.starts_with(server_audit::AUDIT_PREFIX)
                    || key.starts_with("guild-audit-next:")
                {
                    self.fetched.insert(key.clone(), body.clone());
                    self.audit_fetched(&key, cx);
                    return;
                }
                if key.starts_with(server_tag::TAG_PREFIX) {
                    self.fetched.insert(key.clone(), body.clone());
                    self.tag_fetched(&key, cx);
                    return;
                }
                if key.starts_with(discover::FETCH_PREFIX) {
                    self.fetched.insert(key.clone(), body.clone());
                    self.discover_fetched(&key, cx);
                    return;
                }
                if key.starts_with(quests::FETCH_PREFIX) {
                    self.fetched.insert(key.clone(), body.clone());
                    self.quests_fetched(&key, cx);
                    return;
                }
                if key.starts_with(profile_editor::FETCH_PREFIX) {
                    self.fetched.insert(key.clone(), body.clone());
                    self.profile_editor_fetched(&key, cx);
                    return;
                }
                if key.starts_with(shop::FETCH_PREFIX) {
                    self.fetched.insert(key.clone(), body.clone());
                    self.shop_fetched(&key, cx);
                    return;
                }
                if key.starts_with(nitro::NITRO_PREFIX) {
                    self.fetched.insert(key.clone(), body.clone());
                    self.nitro_fetched(&key, cx);
                    return;
                }
                if key.starts_with(server_boost::BOOST_PREFIX) {
                    self.fetched.insert(key.clone(), body.clone());
                    self.boost_fetched(&key, cx);
                    return;
                }
                if key.starts_with(server_community::OVERVIEW_PREFIX) {
                    self.fetched.insert(key.clone(), body.clone());
                    self.overview_fetched(&key, cx);
                    return;
                }
                if key.starts_with(server_community::COMMUNITY_PREFIX) {
                    self.fetched.insert(key.clone(), body.clone());
                    self.community_fetched(&key, cx);
                    return;
                }
                if key.starts_with(server_safety::SAFETY_PREFIX) {
                    self.fetched.insert(key.clone(), body.clone());
                    self.safety_fetched(&key, cx);
                    return;
                }
                if key.starts_with(server_discovery::DISCOVERY_PREFIX) {
                    self.fetched.insert(key.clone(), body.clone());
                    self.discovery_fetched(&key, cx);
                    return;
                }
                if key.starts_with(server_welcome::WELCOME_PREFIX) {
                    self.fetched.insert(key.clone(), body.clone());
                    self.welcome_fetched(&key, cx);
                    return;
                }
                if key.starts_with(server_insights::INSIGHTS_PREFIX) {
                    self.fetched.insert(key.clone(), body.clone());
                    self.insights_fetched(&key, cx);
                    return;
                }
                if key == dangerous_link::BAD_DOMAINS_KEY {
                    self.blocked_domains_fetched(&body);
                    return;
                }
                if key.starts_with(server_automod::AUTOMOD_PREFIX) {
                    self.fetched.insert(key.clone(), body.clone());
                    self.automod_fetched(&key, cx);
                    return;
                }
                if key.starts_with(server_onboarding::ONBOARDING_PREFIX) {
                    self.fetched.insert(key.clone(), body.clone());
                    self.onboarding_fetched(&key, cx);
                    return;
                }
                if key.starts_with(server_access::ACCESS_PREFIX) {
                    self.fetched.insert(key.clone(), body.clone());
                    self.access_fetched(&key, cx);
                    return;
                }
                if key.starts_with(server_stickers::STICKERS_PREFIX) {
                    self.fetched.insert(key.clone(), body.clone());
                    self.stickers_fetched(&key, cx);
                    return;
                }
                if key.starts_with(server_sounds::SOUNDS_PREFIX) {
                    self.fetched.insert(key.clone(), body.clone());
                    self.sounds_fetched(&key, cx);
                    return;
                }
                if key.starts_with(server_engagement::ENGAGEMENT_PREFIX) {
                    self.fetched.insert(key.clone(), body.clone());
                    self.engagement_fetched(&key, cx);
                    return;
                }
                if key.starts_with(server_members::MEMBERS_PREFIX) {
                    self.fetched.insert(key.clone(), body.clone());
                    self.members_fetched(&key, cx);
                    return;
                }
                if key.starts_with(server_template::TEMPLATES_PREFIX) {
                    self.fetched.insert(key.clone(), body.clone());
                    self.templates_fetched(&key, cx);
                    return;
                }
                if key.starts_with(server_role_editor::ROLES_PREFIX) {
                    self.fetched.insert(key.clone(), body.clone());
                    self.roles_fetched(&key, cx);
                    return;
                }
                if key.starts_with(server_roles::ROLE_CREATED_PREFIX) {
                    self.fetched.insert(key.clone(), body.clone());
                    self.role_created(&key, cx);
                    return;
                }
                if key.starts_with(server_emoji::EMOJIS_PREFIX) {
                    self.fetched.insert(key.clone(), body.clone());
                    self.emojis_fetched(&key, cx);
                    return;
                }
                if key.starts_with(server_settings_pages::INVITES_PREFIX)
                    || key.starts_with(server_settings_pages::BANS_PREFIX)
                {
                    self.fetched.insert(key.clone(), body.clone());
                    self.server_page_fetched(&key, cx);
                    return;
                }
                if key.starts_with(poll::VOTE_KEY) {
                    self.poll_vote_answered(&key, &body, cx);
                    return;
                }
                if key.starts_with(server_games::GAMES_PREFIX) {
                    self.fetched.insert(key.clone(), body.clone());
                    self.games_fetched(&key, cx);
                    return;
                }
                if key.starts_with(server_settings::PROFILE_KEY) {
                    self.fetched.insert(key.clone(), body.clone());
                    self.server_profile_fetched(&key, cx);
                    return;
                }
                if key == channel_settings::SAVE_KEY {
                    self.channel_settings_saved(&body, cx);
                    return;
                }
                if let Some(channel) = key
                    .strip_prefix("channel-webhook-created:")
                    .and_then(|channel| channel.parse::<Id>().ok())
                {
                    self.request_channel_settings_page(channel, false);
                    return;
                }
                if self.report_fetched(&key, &body, cx) {
                    return;
                }
                if key.starts_with(profile_menu::INVITE_KEY) {
                    self.profile_invite_created(&key, &body);
                    return;
                }
                if key == invite::CREATE_KEY {
                    self.invite_created(&body, cx);
                    return;
                }
                if key.starts_with(invite::DM_KEY) {
                    self.invite_dm_opened(&body);
                    return;
                }
                if key == threads::CREATE_KEY {
                    self.thread_created(&body, cx);
                    return;
                }
                let gif_answer = key.starts_with("gifs:");
                let profile = key
                    .strip_prefix("profile:")
                    .and_then(|user| user.parse::<Id>().ok());
                let frecency = key == actions::FRECENCY_KEY;
                if key == saved::SAVED_KEY {
                    self.stores.load_saved(&body);
                }
                self.fetched.insert(key.clone(), body);
                if key.starts_with(badge_modal::CATALOG_PREFIX) {
                    self.badge_catalog_fetched(&key);
                }
                if key.starts_with("invite:") {
                    self.invite_fetched(&key);
                }
                if key.starts_with("command-index:") {
                    self.refresh_command_mentions();
                }
                if key.starts_with(poll::VOTERS_PREFIX) {
                    self.poll_voters_fetched(&key);
                }
                if gif_answer {
                    self.request_gif_images(&key);
                }
                if key.starts_with("pins:") {
                    self.pins_fetched(&key);
                }
                if key == stickers::PACKS_KEY {
                    self.sticker_packs_fetched(cx);
                }
                // Search results: their authors' avatars.
                if key == search::RESULTS_KEY {
                    let users: Vec<User> = self.fetched[&key]["messages"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(|group| group.as_array().and_then(|group| group.first()))
                        .filter_map(|message| User::from_value(&message["author"]))
                        .collect();
                    for user in users {
                        let (image, url) = avatar_source(&user);
                        self.request_image(image, url);
                    }
                }
                // Who reacted: their avatars.
                if key.starts_with(reactions_modal::REACTORS_PREFIX) {
                    let users: Vec<User> = self.fetched[&key]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(User::from_value)
                        .collect();
                    for user in users {
                        let (image, url) = avatar_source(&user);
                        self.request_image(image, url);
                    }
                }
                // Messages shown in the Inbox: their authors' avatars.
                if key.starts_with("inbox-") || key == saved::SAVED_KEY {
                    let body = &self.fetched[&key];
                    let list = body.as_array().cloned().unwrap_or_else(|| {
                        body["results"]
                            .as_array()
                            .into_iter()
                            .flatten()
                            .map(|entry| entry["message"].clone())
                            .collect()
                    });
                    let authors: Vec<User> = list
                        .iter()
                        .filter_map(|message| User::from_value(&message["author"]))
                        .collect();
                    for user in authors {
                        let (image, url) = avatar_source(&user);
                        self.request_image(image, url);
                    }
                }
                if key == "affinities" {
                    let users: Vec<User> = group_dm::affinities(&self.fetched[&key])
                        .into_iter()
                        .filter_map(|(id, _)| self.stores.users.get(&id).cloned())
                        .collect();
                    for user in users {
                        let (image, url) = avatar_source(&user);
                        self.request_image(image, url);
                    }
                }
                if let Some(user) = profile {
                    let payload = self.fetched[&key]["user"].clone();
                    self.stores.merge_user(&payload);
                    self.request_profile_images(user);
                    let wid_opt = shop::data::wishlist_ids(&self.fetched[&key])
                        .into_iter()
                        .next();
                    if let Some(wid) = wid_opt {
                        let changed = self.wishlist_ids_cache.insert(user, wid.clone()).as_deref()
                            != Some(wid.as_str());
                        if changed {
                            self.persist_wishlist_cache();
                        }
                        let wl_key = format!("profile-wishlist:{user}");
                        if self.requested.insert(wl_key.clone()) {
                            let _ = self.requests.send(Request::Fetch {
                                key: wl_key,
                                route: shop::data::wishlist_route(&wid),
                            });
                        }
                    }
                }
                if let Some(uid) = key
                    .strip_prefix("profile-wishlist:")
                    .and_then(|s| s.parse::<Id>().ok())
                {
                    let list = self.fetched[&key].clone();
                    self.shop_seed_effects(&list);
                    for product in shop::data::parse_wishlist_products(&list) {
                        self.shop
                            .products
                            .insert(product.sku_id.clone(), product.clone());
                        self.shop_prefetch_product_art(&product, 78.);
                    }
                    let all_skus: Vec<String> = dm_profile::wishlist_entries(&list)
                        .into_iter()
                        .map(|(sku, _)| sku)
                        .collect();
                    for sku in &all_skus {
                        self.wishlist_prefetch_skus.insert(sku.clone());
                        if let Some(product) = self.shop.products.get(sku).cloned() {
                            self.shop_prefetch_product_art(&product, 78.);
                        }
                    }
                    self.shop_need_products(&all_skus);
                    self.editor_wishlist_need(&all_skus);
                    if let Some(wid) = list["id"].as_str() {
                        self.wishlist_ids_cache.insert(uid, wid.to_owned());
                    }
                    self.persist_wishlist_cache();
                }
                if frecency {
                    let mut wanted: Vec<&'static str> = self
                        .quick_reactions()
                        .into_iter()
                        .chain(self.hover_bar_reactions())
                        .map(|emoji| emoji.surrogates)
                        .collect();
                    wanted.dedup();
                    for surrogates in wanted {
                        self.request_emoji_image(surrogates);
                    }
                    self.warm_gifs();
                }
                cx.notify();
            }
            Update::ImageFailed { key } => {
                if key.starts_with(profile_effects::ART_PREFIX) {
                    self.profile_effect_art_failed(&key, cx);
                } else if key.starts_with(sticker_player::PREFIX) {
                    self.sticker_art_failed(&key);
                } else if key.starts_with(inline_video::PREFIX) {
                    self.inline_video_failed(&key);
                    cx.notify();
                } else if self.image_crop_full_failed(&key, cx) {
                } else {
                    // Every other picture is asked for again.
                    self.image_retry_after_failure(key, cx);
                }
            }
            Update::Image { key, bytes } if key.starts_with(image_crop::FULL_PREFIX) => {
                self.image_crop_full_fetched(&key, bytes, cx);
            }
            Update::Image { key, bytes } if key.starts_with(profile_effects::ART_PREFIX) => {
                self.profile_effect_art_fetched(&key, bytes, cx);
            }
            Update::Image { key, bytes } if key.starts_with(nameplates::ANIMATION_PREFIX) => {
                self.nameplate_animation_fetched(&key, bytes, cx);
            }
            Update::Image { key, bytes } if key.starts_with(sticker_player::PREFIX) => {
                self.sticker_art_fetched(&key, bytes, cx);
            }
            Update::Image { key, bytes } if key.starts_with(inline_video::PREFIX) => {
                self.inline_video_fetched(&key, bytes);
            }
            Update::Image { key, bytes } if key == nitro::home::BACKGROUND_KEY => {
                self.image_retries.arrived(&key);
                self.nitro_background_fetched(bytes, cx);
            }
            Update::Image { key, bytes } if key == quests::HERO_VIDEO_KEY => {
                self.quests_hero_video_fetched(bytes, cx);
            }
            Update::Image { key, bytes } if key.starts_with("text:") => {
                self.texts
                    .insert(key, String::from_utf8_lossy(&bytes).into_owned());
                cx.notify();
            }
            Update::Image { key, bytes } => {
                let format = image_format(&key, &bytes);
                self.image_retries.arrived(&key);
                let image = Arc::new(Image::from_bytes(format, bytes));
                self.start_picture_arrived(&key, &image);
                self.images.insert(key, image);
            }
            Update::Spinner(clip) => self.splash_clip(clip),
            Update::Images(images) => {
                for (key, bytes) in images {
                    self.images
                        .insert(key, Arc::new(Image::from_bytes(ImageFormat::Png, bytes)));
                }
            }
            Update::SaveFailed {
                channel,
                message,
                save,
            } => {
                if save {
                    self.stores.saved.remove(&(channel, message));
                } else {
                    self.stores.saved.entry((channel, message)).or_insert(None);
                }
                self.put_app_toast("Unable to save message.".to_owned(), false, cx);
            }
            Update::ReactFailed {
                channel,
                message,
                emoji,
                add,
            } => {
                let (name, emoji_id) = menu::parse_reaction_route(&emoji);
                self.stores
                    .apply_own_reaction(channel, message, name, emoji_id, !add);
            }
        }
        cx.notify();
    }

    fn after_ready(&mut self) {
        self.request_blocked_domains();
        // The app starts where it was closed: the server last shown on the channel it was left on, or Home. A first
        // start, and a server that is gone, open Home as the official client does: opening a server marks its
        // channel read, and nobody chose that server.
        let saved = last_channel::last();
        let guild = saved
            .flatten()
            .filter(|guild| self.stores.guild(*guild).is_some());
        // Harness scripts written before this rule expect a profile with no saved place to open the first server.
        #[cfg(feature = "capture")]
        let guild = guild.or_else(|| {
            saved
                .is_none()
                .then(|| self.stores.guilds.first().map(|guild| guild.id))
                .flatten()
        });
        match guild {
            Some(guild_id) => {
                let channel = self.last_channel_of(guild_id);
                self.select(guild_id, channel);
            }
            None => self.select_home(None),
        }
        // Opening on the last place is not a step back can return from.
        self.history = history::History::default();
        self.request_images();
        self.request_emoji_button();
        self.request_frecency();
        self.request_saved();
    }

    fn reconcile_after_ready(&mut self, prev_guild: Option<Id>, prev_channel: Option<Id>) {
        match prev_guild {
            Some(guild_id) => {
                if let Some(guild) = self.stores.guild(guild_id) {
                    if let Some(chan) = prev_channel
                        && guild.channels.iter().any(|ch| ch.id == chan)
                    {
                        self.guild = Some(guild_id);
                        self.channel = Some(chan);
                        let _ = self.requests.send(Request::Subscribe {
                            guild: guild_id,
                            channel: chan,
                        });
                        if !self.current_channel().is_some_and(Channel::is_forum) {
                            let _ = self.requests.send(Request::LoadMessages { channel: chan });
                        }
                        self.rebuild_rows();
                    } else {
                        let channel = self.last_channel_of(guild_id);
                        self.select(guild_id, channel);
                    }
                } else {
                    self.select_home(None);
                }
            }
            None => {
                if let Some(dm_id) = prev_channel {
                    if self.stores.private_channels.iter().any(|dm| dm.id == dm_id) {
                        self.guild = None;
                        self.channel = Some(dm_id);
                        let _ = self.requests.send(Request::LoadMessages { channel: dm_id });
                        self.stores.calls.connect_channel(dm_id, true, false);
                        self.calls_after_dispatch_effects();
                        self.rebuild_rows();
                    } else {
                        self.select_home(None);
                    }
                } else {
                    self.guild = None;
                    self.channel = None;
                    self.rebuild_rows();
                }
            }
        }
        self.request_images();
        self.request_emoji_button();
        self.request_frecency();
        self.request_saved();
    }

    /// The composer's emoji button is a sprite cell tinted with the theme's icon color, so it follows the theme.
    pub(crate) fn request_emoji_button(&mut self) {
        let tint = theme().color("--interactive-text-default");
        let _ = self.requests.send(Request::Sprite {
            key: "emoji_button".to_owned(),
            tint,
        });
        let _ = self.requests.send(Request::EmojiButtonSprites);
    }

    fn sync_ready_dm_cache(&mut self, force: bool) {
        if !start_cache::enabled() {
            return;
        }
        if !force
            && self
                .last_ready_cache_save
                .is_some_and(|at| at.elapsed() < std::time::Duration::from_secs(2))
        {
            return;
        }
        let Some(mut trimmed) = self.cached_ready.clone() else {
            return;
        };
        start_cache::sync_private_channels(&mut trimmed, &self.stores);
        self.cached_ready = Some(trimmed.clone());
        self.last_ready_cache_save = Some(Instant::now());
        start_cache::save_trimmed_ready(trimmed);
    }

    pub(super) fn persist_wishlist_cache(&self) {
        if !start_cache::enabled() {
            return;
        }
        let wishlists: HashMap<u64, serde_json::Value> = self
            .fetched
            .iter()
            .filter_map(|(k, v)| {
                let uid = k.strip_prefix("profile-wishlist:")?.parse::<u64>().ok()?;
                Some((uid, v.clone()))
            })
            .collect();
        let mut wishlist_ids = self.wishlist_ids_cache.clone();
        if let Some(me) = self.stores.current_user.as_ref().map(|u| u.id)
            && let Some(wid) = &self.shop.wishlist_id
        {
            wishlist_ids.entry(me).or_insert_with(|| wid.clone());
        }
        let mut skus = HashSet::new();
        for list in wishlists.values() {
            for (sku, _) in dm_profile::wishlist_entries(list) {
                skus.insert(sku);
            }
        }
        let products = skus
            .into_iter()
            .filter_map(|sku| self.shop.products.get(&sku).cloned())
            .collect();
        start_cache::save_wishlists(start_cache::CachedWishlists {
            wishlist_ids,
            wishlists,
            products,
        });
    }

    pub(super) fn prefetch_known_wishlist(&mut self, user: Id) {
        if let Some(wid) = self.wishlist_ids_cache.get(&user).cloned() {
            let key = format!("profile-wishlist:{user}");
            if self.requested.insert(key.clone()) {
                let _ = self.requests.send(Request::Fetch {
                    key,
                    route: shop::data::wishlist_route(&wid),
                });
            }
        }
        if let Some(list) = self
            .fetched
            .get(&format!("profile-wishlist:{user}"))
            .cloned()
        {
            self.shop_seed_effects(&list);
            let all_skus: Vec<String> = dm_profile::wishlist_entries(&list)
                .into_iter()
                .map(|(sku, _)| sku)
                .collect();
            self.wishlist_prefetch_skus.extend(all_skus.iter().cloned());
            for sku in &all_skus {
                if let Some(product) = self.shop.products.get(sku).cloned() {
                    self.shop_prefetch_product_art(&product, 78.);
                }
            }
            self.shop_need_products(&all_skus);
            self.editor_wishlist_need(&all_skus);
        }
    }

    fn warm_top_dm_profiles(&mut self) {
        if !start_cache::enabled() {
            return;
        }
        let mut targets: Vec<Id> = Vec::new();
        if let Some(dm) = self.current_dm()
            && let Some(recipient) = self.stores.dm_recipient(dm)
        {
            targets.push(recipient.id);
        }
        let mut dms: Vec<(u64, Id)> = self
            .stores
            .private_channels
            .iter()
            .filter_map(|dm| {
                let uid = self.stores.dm_recipient(dm)?.id;
                Some((dm.last_message_id.max(dm.id), uid))
            })
            .collect();
        dms.sort_by_key(|b| std::cmp::Reverse(b.0));
        for (_, uid) in dms {
            if !targets.contains(&uid) {
                targets.push(uid);
            }
            if targets.len() >= 8 {
                break;
            }
        }
        if let Some(me) = self.stores.current_user.as_ref().map(|u| u.id)
            && !targets.contains(&me)
        {
            targets.push(me);
        }
        for (idx, uid) in targets.into_iter().enumerate() {
            self.prefetch_known_wishlist(uid);
            if idx < 4 && !self.fetched.contains_key(&format!("profile:{uid}")) {
                let key = format!("profile:{uid}");
                if self.requested.insert(key.clone()) {
                    let _ = self.requests.send(Request::Fetch {
                        key,
                        route: self.dm_profile_route(uid),
                    });
                }
            }
        }
    }

    fn select(&mut self, guild: Id, channel: Option<Id>) {
        let from = self.route();
        self.history.visit(
            from,
            history::Route {
                guild: Some(guild),
                channel,
                favorites: false,
                nitro: false,
                guild_guide: false,
                channel_browser: false,
                discover: None,
                quests: false,
                shop: false,
                message_requests: false,
                library: false,
            },
        );
        self.favorites_view = false;
        self.discover.open = false;
        self.nitro_open = false;
        self.guild_guide_open = false;
        self.channel_browser_open = false;
        self.voice_record_stop_leaving(channel);
        self.guild = Some(guild);
        self.channel = channel;
        self.channel_notice_wanted();
        // A message menu and the delete question act on the channel that was open when they were asked.
        self.menu = None;
        self.submenu = None;
        self.confirm_delete = None;
        self.selection.borrow_mut().clear_all();
        // `CHANNEL_SELECT` is activity to `IdleStore` (module 885576).
        self.note_activity();
        last_channel::remember(guild, channel);
        self.member_view = None;
        self.unread_from = None;
        self.data_revision += 1;
        if let Some(channel) = channel {
            self.remember_channel(channel);
            self.forum_reopened(channel);
        }
        if let Some(channel) = channel {
            let _ = self.requests.send(Request::Subscribe { guild, channel });
            if self.current_channel().is_some_and(Channel::is_forum) {
                self.ready = true;
            } else {
                self.open_channel_messages(channel);
            }
        }
        self.stop_lurking_elsewhere();
        self.rebuild_rows();
    }

    fn request_image(&mut self, key: String, url: String) {
        if self.requested.insert(key.clone()) {
            self.image_retries.sent(&key, &url);
            let _ = self.requests.send(Request::Image { key, url });
        } else if !self.images.contains_key(&key)
            && self.image_retries.again(&key, &url, image_retry::now_ms())
        {
            // It failed every attempt a while ago and is wanted again.
            let _ = self.requests.send(Request::Image { key, url });
        }
    }

    /// Failed downloads of `key` since it last arrived, for the harness's state lines.
    #[cfg(feature = "capture")]
    pub(crate) fn image_failures(&self, key: &str) -> u32 {
        self.image_retries.failures(key)
    }

    /// A picture requested through `request_image` failed to download: ask again after a doubling delay.
    fn image_retry_after_failure(&mut self, key: String, cx: &mut Context<Self>) {
        if self.images.contains_key(&key) {
            return;
        }
        let Some(retry) = self.image_retries.failed(&key, image_retry::now_ms()) else {
            return;
        };
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(std::time::Duration::from_millis(retry.after_ms))
                .await;
            let _ = this.update(cx, |this, _| {
                if this.images.contains_key(&key) {
                    this.image_retries.arrived(&key);
                } else if let Some(url) = this.image_retries.resend(&key) {
                    let _ = this.requests.send(Request::Image { key, url });
                }
            });
        })
        .detach();
    }

    fn request_images(&mut self) {
        self.request_stored_app_icon();
        self.request_role_icon_images();
        self.request_stage_art();
        let guilds: Vec<(Id, String)> = self
            .stores
            .guilds
            .iter()
            .filter_map(|guild| Some((guild.id, guild.icon.clone()?)))
            .collect();
        for (id, icon) in guilds {
            self.request_image(
                format!("guild:{id}"),
                format!("https://cdn.discordapp.com/icons/{id}/{icon}.webp?size=96"),
            );
        }
        let mut users: Vec<User> = self.stores.current_user.iter().cloned().collect();
        users.extend(
            self.stores
                .relationship_and_channel_users()
                .into_iter()
                .cloned(),
        );
        users.extend(self.stage_people());
        let mut media = Vec::new();
        // The messages thread starters stand for are drawn in their place, so their people and media are needed too.
        let starters: Vec<Message> = self
            .channel
            .and_then(|channel| self.stores.messages.get(&channel))
            .into_iter()
            .flatten()
            .filter(|message| message.kind == dc_state::message_kinds::THREAD_STARTER_MESSAGE)
            .filter_map(|message| self.starter_target(message))
            .collect();
        if let Some(messages) = self
            .channel
            .and_then(|channel| self.stores.messages.get(&channel))
        {
            users.extend(messages.iter().map(|message| message.author.clone()));
            users.extend(starters.iter().map(|message| message.author.clone()));
            users.extend(
                messages
                    .iter()
                    .filter_map(|message| Some(message.extra.interaction.as_ref()?.1.clone())),
            );
            media.extend(message_kinds::wanted(messages));
            if messages.iter().any(|message| message.kind == dm::USER_JOIN) {
                media.push(("inbox:join".to_owned(), dm::JOIN_ARROW.to_owned()));
            }
            // The sticker of each join message's "Wave to say hi!" button.
            let me = self.stores.current_user.as_ref().map(|me| me.id);
            for message in messages
                .iter()
                .filter(|message| message.kind == dm::USER_JOIN)
            {
                let (id, format_type) = dm::welcome_sticker(me, message.id);
                let sticker = dc_state::stickers::Sticker {
                    id,
                    name: String::new(),
                    description: None,
                    tags: None,
                    kind: 0,
                    format_type,
                    guild_id: None,
                    pack_id: None,
                    available: true,
                    user_id: None,
                };
                media.push((stickers::art_key(id), stickers::art_url(&sticker)));
            }
            // A thread's card shows its latest message: load each started thread once.
            let threads: Vec<Id> = messages
                .iter()
                .filter(|message| message.kind == dm::THREAD_CREATED)
                .filter_map(|message| message.reference_channel_id)
                .collect();
            for thread in threads {
                if self.requested.insert(format!("thread-load:{thread}")) {
                    let _ = self
                        .requests
                        .send(Request::LoadMessages { channel: thread });
                }
            }
            for message in messages.iter().chain(&starters) {
                media.extend(media_image::wanted(
                    message,
                    self.pref_on("animate_gifs", true) && !motion::reduced(),
                    self.pref_on("animate_emojis", true),
                ));
            }
        }
        for (key, url) in media {
            self.request_image(key, url);
        }
        for user in users {
            let (key, url) = avatar_source(&user);
            self.request_image(key, url);
            let (key, url) = self.guild_avatar_source(&user);
            self.request_image(key, url);
            if let Some(url) = user.tag.as_ref().and_then(dc_state::ServerTag::badge_url) {
                self.request_image(url.clone(), url);
            }
        }
    }

    fn request_member_images(&mut self) {
        let Some(list) = self
            .guild
            .and_then(|guild| self.stores.member_lists.get(&guild))
        else {
            return;
        };
        let users: Vec<User> = list
            .rows
            .iter()
            .filter_map(|row| match row {
                MemberRow::Member { user, .. } => Some(user.clone()),
                _ => None,
            })
            .collect();
        for user in users {
            if let Some(plate) = &user.nameplate {
                self.request_image(format!("nameplate:{}", plate.sku_id), plate.static_url());
            }
            let (key, url) = self.guild_avatar_source(&user);
            self.request_image(key, url);
            if let Some(url) = user.tag.as_ref().and_then(dc_state::ServerTag::badge_url) {
                self.request_image(url.clone(), url);
            }
        }
    }

    fn guild_avatar_source(&self, user: &User) -> (String, String) {
        // Not in the pinned client: UserPFP's `overrideServerAvatars` (`plugin_hooks/userpfp.rs`).
        let server = self
            .guild
            .and_then(|guild| self.stores.members.get(guild, user.id))
            .is_some_and(|member| member.avatar.is_some());
        if let Some(source) = plugin_hooks::userpfp_server_avatar(user, server) {
            return source;
        }
        self.guild
            .and_then(|guild| self.stores.members.get(guild, user.id)?.avatar_url(guild))
            .map(|url| (url.clone(), url))
            .unwrap_or_else(|| avatar_source(user))
    }

    /// "Show spoiler content": Always reveals every spoiler, "On servers I moderate" those of a server where the user
    /// manages messages, On click (the default) leaves them hidden until clicked (`render_spoilers`).
    fn spoilers_shown(&self) -> bool {
        match self.pref_index("chat_spoilers_show_spoilers", 0) {
            1 => true,
            2 => match (self.current_guild(), self.stores.me()) {
                (Some(guild), Some(me)) => dc_state::permissions::can_in_guild(
                    guild,
                    me,
                    dc_state::permissions::MANAGE_MESSAGES,
                ),
                _ => false,
            },
            _ => false,
        }
    }

    /// The role colour of a name, in the way "Role Colors" asks for it: in the name (the default), as a dot beside it,
    /// or not at all.
    fn role_color_of(&self, user: Id) -> Option<u32> {
        self.current_guild().and_then(|guild| {
            let member = self.stores.members.get(guild.id, user)?;
            guild.role_color(&member.roles)
        })
    }

    fn guild_author_color(&self, user: Id, fallback: &str) -> Rgba {
        // Not in the pinned client: BetterRoleDot's `bothStyles` (`plugin_hooks/better_role_dot.rs`).
        if !self.role_names_colored() {
            return color(fallback);
        }
        self.current_guild()
            .and_then(|guild| {
                let member = self.stores.members.get(guild.id, user)?;
                guild.role_color(&member.roles)
            })
            .map_or_else(
                || color(fallback),
                |role| {
                    let colour = gpui::rgb(role);
                    // "Apply saturation setting to custom colors" (`desaturateUserColors`).
                    if self.pref_on("desaturate_custom_colors", false) && saturation() < 1. {
                        desaturated(colour, saturation())
                    } else {
                        colour
                    }
                },
            )
    }

    /// A message author's name with their display name style (315934): in a server the font alone; in a DM or group
    /// DM the effect too when the pointer is on the name.
    fn author_name(&self, key: String, user: &User) -> AnyElement {
        if let Some(colors) = self.role_gradient_of(user)
            && self.name_styles(user, self.guild).is_none()
        {
            // Not in the pinned client: AlwaysAnimate slides it (`plugin_hooks/always_animate.rs`).
            return role_gradient::name(self.guild_display_name(user), colors)
                .animate(plugin_hooks::always_animate(
                    plugin_hooks::AnimatePart::ChatAndMemberGradients,
                ))
                .into_any_element();
        }
        let in_dm = self.current_guild().is_none() && self.current_dm().is_some();
        name_styles::name(
            key,
            self.guild_display_name(user),
            self.name_styles(user, self.guild).as_ref(),
            if in_dm {
                name_styles::Display::FontOnHover
            } else {
                name_styles::Display::Font
            },
        )
        .into_any_element()
    }

    /// The gradient pair of the user's colour role in the open server, when names show role colours in the name
    /// ("Role Colors" default) and the server has the enhanced styles (modules 967144, 463930).
    fn role_gradient_of(&self, user: &User) -> Option<dc_state::RoleColors> {
        if !self.role_names_colored() {
            return None;
        }
        self.role_gradient_by_id(user.id)
    }

    fn role_gradient_by_id(&self, user: Id) -> Option<dc_state::RoleColors> {
        let guild = self.current_guild()?;
        let member = self.stores.members.get(guild.id, user)?;
        guild.role_gradient(&member.roles)
    }

    /// The role dot of "Next to names": the gradient dot when the colour role has a gradient, else the solid one.
    fn role_dot_of(&self, user: Id, solid: u32) -> gpui::Div {
        match self.role_gradient_by_id(user) {
            Some(colors) => role_dot::dot_gradient(&colors),
            None => role_dot::dot(solid),
        }
    }

    /// `text` as a name in the user's role colours: the gradient when the role has one, else the plain text (which
    /// takes the colour the caller set).
    fn role_named(&self, user: &User, text: String) -> AnyElement {
        match self.role_gradient_of(user) {
            Some(colors) => role_gradient::name(text, colors).into_any_element(),
            None => text.into_any_element(),
        }
    }

    fn guild_display_name(&self, user: &User) -> String {
        if self.current_guild().is_none() && self.current_dm().is_some() {
            return self.dm_display_name(user);
        }
        self.current_guild()
            .and_then(|guild| self.stores.members.get(guild.id, user.id))
            .and_then(|member| member.nick.clone())
            .unwrap_or_else(|| user.display_name().to_owned())
    }

    /// The official panel: 264px wide, 42px rows, section headers with their counts.
    fn member_panel(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let list = self
            .guild
            .and_then(|guild| self.stores.member_lists.get(&guild));
        let owner = self.current_guild().and_then(|guild| guild.owner_id);
        let roles = self.current_guild().map(|guild| &guild.roles);
        let first_activity = list.and_then(|list| {
            list.rows.iter().find_map(|row| match row {
                MemberRow::Member { user, .. } if self.has_presence_line(user, self.guild) => {
                    Some(user.id)
                }
                _ => None,
            })
        });
        let first_nameplate = list.and_then(|list| {
            list.rows.iter().find_map(|row| match row {
                MemberRow::Member { user, .. } if user.nameplate.is_some() => Some(user.id),
                _ => None,
            })
        });
        let first_styled = list.and_then(|list| {
            list.rows.iter().find_map(|row| match row {
                MemberRow::Member { user, .. } if self.name_styles(user, self.guild).is_some() => {
                    Some(user.id)
                }
                _ => None,
            })
        });
        let first_decoration = list.and_then(|list| {
            list.rows.iter().find_map(|row| match row {
                MemberRow::Member { user, .. } if self.equipped_decoration(user).is_some() => {
                    Some(user.id)
                }
                _ => None,
            })
        });
        let all_rows = list.map(|list| list.rows.as_slice()).unwrap_or_default();
        // Only the rows near the visible part are built; spacers of the exact height stand in for the rest.
        let heights: Vec<f32> = all_rows
            .iter()
            .map(|row| match row {
                MemberRow::Member { user, .. } if self.has_presence_line(user, self.guild) => 44.,
                _ => 42.,
            })
            .collect();
        let scroll = &self.member_window.scroll;
        let view = match f32::from(scroll.bounds().size.height) {
            known if known > 0. => known,
            _ => self.viewport.1,
        };
        let rows_total: f32 = heights.iter().sum();
        // What sits above the first row (the inventory section): the content's height less the rows'.
        let before = if scroll.bounds().size.height > px(0.) {
            (f32::from(scroll.max_offset().y) + view - rows_total).max(0.)
        } else {
            0.
        };
        #[allow(unused_mut)]
        let mut built =
            member_window::visible(&heights, -f32::from(scroll.offset().y), view, before);
        #[cfg(feature = "capture")]
        if crate::perf::legacy("members") {
            built = 0..heights.len();
        }
        let above: f32 = heights[..built.start].iter().sum();
        let below: f32 = heights[built.end..].iter().sum();
        let rows: Vec<AnyElement> = all_rows
            .iter()
            .enumerate()
            .skip(built.start)
            .take(built.len())
            .map(|(_, row)| match row {
                MemberRow::Group { id, count } => div()
                    .flex_none()
                    .h(px(42.))
                    .pt(px(20.))
                    .pl(px(16.))
                    .flex()
                    .flex_row()
                    .text_size(px(14.))
                    .line_height(px(18.))
                    .font_weight(FontWeight(500.))
                    .text_color(color("--channels-default"))
                    // 282006: the role's icon, 16px with 4px after it (`roleIcon_86es0b`), before the group name.
                    .children(
                        id.parse::<Id>()
                            .ok()
                            .zip(self.current_guild())
                            .and_then(|(role, guild)| self.role_icon_art(guild, role, 16.))
                            .map(|icon| div().flex_none().mr(px(4.)).child(icon)),
                    )
                    // Not in the pinned client: RoleColorEverywhere's `memberList`
                    // (`plugin_hooks/role_color_everywhere.rs`).
                    .child(plugin_hooks::role_colored_label(
                        self.role_color_everywhere_role(id),
                        format!("{} \u{2014} {count}", group_label(id, roles)),
                    ))
                    // Not in the pinned client: BetterRoleContext's role menu on a right click
                    // (`plugin_hooks/better_role_context.rs`).
                    .when_some(
                        id.parse::<Id>()
                            .ok()
                            .zip(self.current_guild().map(|guild| guild.id))
                            .filter(|_| self.better_role_menu_on()),
                        |this, (role, guild)| {
                            let anchors = self.tip_anchors.clone();
                            let key = SharedString::from(format!("member-group-{role}"));
                            this.relative()
                                .child(
                                    gpui::canvas(
                                        move |bounds, _, _| {
                                            anchors.borrow_mut().insert(key.clone(), bounds);
                                        },
                                        |_, _, _, _| {},
                                    )
                                    .absolute()
                                    .inset_0(),
                                )
                                .on_mouse_up(
                                    gpui::MouseButton::Right,
                                    cx.listener(move |this, event: &gpui::MouseUpEvent, _, cx| {
                                        this.open_context_menu(
                                            context_menu::Target::Role { role, guild },
                                            event.position,
                                            cx,
                                        );
                                        cx.stop_propagation();
                                    }),
                                )
                        },
                    )
                    .into_any_element(),
                MemberRow::Member {
                    user, nick, status, ..
                } => {
                    let status = self
                        .stores
                        .presence
                        .get(user.id, self.guild)
                        .map_or(status.as_str(), |p| p.status.as_str());
                    let name = nick
                        .clone()
                        .unwrap_or_else(|| user.display_name().to_owned());
                    // Module 110574 draws module 342312's `ActivityStatus` here (BetterActivities patches it).
                    let activity = self.activity_status_line(
                        user,
                        self.guild,
                        16.,
                        "--channels-default",
                        "member",
                        cx,
                    );
                    let height = if activity.is_some() { 42. } else { 40. };
                    let has_nameplate = user.nameplate.is_some();
                    let text_width = Rc::new(std::cell::Cell::new(0.));
                    let measured_width = text_width.clone();
                    let nameplate =
                        self.member_nameplate(user, text_width, first_nameplate == Some(user.id));
                    let member_user = user.id;
                    let item = div()
                        .id(SharedString::from(format!("member-item-{member_user}")))
                        .on_hover(cx.listener(move |this, inside: &bool, _, cx| {
                            this.nameplate_hover(member_user, *inside, cx)
                        }))
                        .relative()
                        .h(px(height))
                        .w(px(248.))
                        .pl(px(8.))
                        .pr(px(if has_nameplate { 32. } else { 8. }))
                        .rounded(px(8.))
                        .flex()
                        .items_center()
                        .hover(|style| style.bg(color("--interactive-background-hover")))
                        // `.clickable_J1Adu_:active .childContainer_J1Adu_` (56606): the pressed row.
                        .active(|style| style.bg(color("--interactive-background-active")))
                        .when(
                            self.profile
                                .as_ref()
                                .is_some_and(|profile| profile.user == user.id),
                            |this| this.bg(color("--interactive-background-selected")),
                        )
                        .children(nameplate)
                        .child(self.member_avatar(
                            user,
                            status,
                            first_decoration == Some(user.id),
                            self.hovered_nameplate == Some(user.id),
                            cx,
                        ))
                        .child(
                            div()
                                .relative()
                                .ml(px(12.))
                                .min_w(px(0.))
                                .max_w(px(if has_nameplate { 164. } else { 188. }))
                                .flex()
                                .flex_col()
                                .items_start()
                                .child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .min_w(px(0.))
                                        .h(px(20.))
                                        // "Next to names" (`roleStyle` "dot"): the role's dot before the name, 4px
                                        // clear of it (`.roleDot` 790340).
                                        .children(
                                            self.role_dots_shown()
                                                .then(|| self.role_color_of(user.id))
                                                .flatten()
                                                .map(|rgb| {
                                                    div()
                                                        .flex_none()
                                                        .size(px(20.))
                                                        .mr(px(4.))
                                                        .rounded(px(4.))
                                                        .bg(color("--background-mod-normal"))
                                                        .flex()
                                                        .items_center()
                                                        .justify_center()
                                                        // Not in the pinned client: BetterRoleDot copies
                                                        // the colour (`plugin_hooks/better_role_dot.rs`).
                                                        .child(self.role_dot_copy(
                                                            format!("member-{}", user.id),
                                                            rgb,
                                                            self.role_dot_of(user.id, rgb),
                                                            cx,
                                                        ))
                                                }),
                                        )
                                        .child(
                                            div()
                                                .min_w(px(0.))
                                                .when(first_styled == Some(user.id), |this| {
                                                    this.parity("member-name-styled")
                                                })
                                                .text_ellipsis()
                                                .line_height(px(20.))
                                                .font_weight(FontWeight(500.))
                                                .text_color(self.guild_author_color(
                                                    user.id,
                                                    "--channels-default",
                                                ))
                                                .child(
                                                    match self.role_gradient_of(user).filter(|_| {
                                                        self.name_styles(user, self.guild).is_none()
                                                    }) {
                                                        // 463930: the gradient slides while the
                                                        // pointer is on the row.
                                                        Some(colors) => role_gradient::name(
                                                            name.clone(),
                                                            colors,
                                                        )
                                                        .animate(
                                                            self.hovered_nameplate == Some(user.id)
                                                                // Not in the pinned client: AlwaysAnimate.
                                                                || plugin_hooks::always_animate(
                                                                    plugin_hooks::AnimatePart::ChatAndMemberGradients,
                                                                ),
                                                        )
                                                        .into_any_element(),
                                                        None => name_styles::name(
                                                            format!("member-{}", user.id),
                                                            name,
                                                            self.name_styles(user, self.guild)
                                                                .as_ref(),
                                                            // 110574: without a guild (a group DM's
                                                            // list) the full effect shows, animated
                                                            // under the row's pointer.
                                                            if self.guild.is_some() {
                                                                name_styles::Display::Font
                                                            } else if self.hovered_nameplate
                                                                == Some(user.id)
                                                            {
                                                                name_styles::Display::Animated
                                                            } else {
                                                                name_styles::Display::Static
                                                            },
                                                        )
                                                        .into_any_element(),
                                                    },
                                                ),
                                        )
                                        .children(user.tag.as_ref().map(|tag| {
                                            self.server_tag_chip(tag).ml(px(4.)).flex_none()
                                        }))
                                        .when(owner == Some(user.id), |this| {
                                            this.child(div().ml(px(4.)).child(icon(
                                                "owner_crown",
                                                14.,
                                                "--status-warning",
                                            )))
                                        })
                                        // Not in the pinned client: PlatformIndicators
                                        // (`plugin_hooks/platform_indicators.rs`).
                                        .children(self.platform_icons(
                                            user.id,
                                            plugin_hooks::PlatformPlace::MemberList,
                                            cx,
                                        )),
                                )
                                .children(activity.map(|line| {
                                    div()
                                        .mt(px(-2.))
                                        .when(first_activity == Some(user.id), |this| {
                                            this.parity("presence-line")
                                        })
                                        .child(line)
                                }))
                                .child(
                                    gpui::canvas(
                                        move |bounds, _, _| {
                                            measured_width.set(f32::from(bounds.size.width))
                                        },
                                        |_, _, _, _| {},
                                    )
                                    .absolute()
                                    .top_0()
                                    .left_0()
                                    .size_full(),
                                ),
                        );
                    div()
                        .flex_none()
                        .h(px(height + 2.))
                        .pt(px(1.))
                        .pl(px(8.))
                        .child(self.profile_trigger_side(
                            format!("member-{}", user.id),
                            user.id,
                            item,
                            true,
                            cx,
                        ))
                        .into_any_element()
                }
                MemberRow::Unknown => div().flex_none().h(px(42.)).into_any_element(),
            })
            .collect();
        div()
            .parity("members")
            .flex_none()
            .w(px(self.member_list_width()))
            .h_full()
            // A colour theme's chat layer behind it is the member list's own (the official repaints one layer).
            .when(settings::client_theme::active().is_none(), |panel| {
                panel.bg(color("--background-base-lower"))
            })
            .flex()
            .flex_col()
            .child(
                div()
                    .id("member-rows")
                    .flex_1()
                    .min_h(px(0.))
                    .flex()
                    .flex_col()
                    .overflow_y_scroll()
                    .track_scroll(&self.member_window.scroll)
                    // Not in the pinned client: MemberCount's counts (`plugin_hooks/member_count.rs`).
                    .children(self.member_count_list(cx))
                    .children(self.inventory_section(cx))
                    .when(above > 0., |this| {
                        this.child(div().flex_none().h(px(above)))
                    })
                    .children(rows)
                    .when(below > 0., |this| {
                        this.child(div().flex_none().h(px(below)))
                    }),
            )
    }

    fn rebuild_rows(&mut self) {
        // Other rows: a link the pointer was last found over may be gone (`inline::middle_press_on_link`).
        crate::inline::forget_link_under_pointer();
        self.request_invites();
        // A command mention needs the channel's command index to know whether it is a link (module 990474).
        if self
            .channel
            .and_then(|channel| self.stores.messages.get(&channel))
            .is_some_and(|messages| {
                messages
                    .iter()
                    .any(|message| message.content.contains("</"))
            })
        {
            self.request_command_index();
        }
        self.refresh_command_mentions();
        self.rows.clear();
        let empty = Vec::new();
        let messages = self
            .channel
            .and_then(|channel| self.stores.messages.get(&channel))
            .unwrap_or(&empty);
        let mut last_day = None;
        // `!hasMoreBefore`: the window reaches the first message of the channel.
        let complete = self.channel.is_some_and(|channel| {
            self.stores
                .windows
                .get(&channel)
                .is_some_and(|window| window.has_fetched && !window.has_more_before)
        });
        let in_post = self.open_post_forum().is_some();
        let text_start = !in_post
            && self.current_channel().is_some_and(|channel| {
                matches!(channel.kind, ChannelKind::Text | ChannelKind::Other(5))
            });
        if complete && (self.current_dm().is_some() || self.voice_view_open() || text_start) {
            self.rows.push(Row::Welcome);
        }
        if complete && in_post {
            self.rows.push(Row::PostIntro);
        }
        let divider = self.divider_after();
        let mut divided = false;
        let blocked: HashSet<Id> = self
            .stores
            .relationships
            .iter()
            .filter(|(_, kind)| **kind == blocked::RELATIONSHIP_BLOCKED)
            .map(|(user, _)| *user)
            .collect();
        // `isIgnoredForMessage`: an ignored author folds the same way, under its own line.
        let ignored: HashSet<Id> = self
            .stores
            .ignored
            .iter()
            .filter(|user| !blocked.contains(user))
            .copied()
            .collect();
        // Module 891522 `eA` with `treatSpam`: a likely spammer's messages fold too, outside one-to-one DMs, for an
        // account in the spam redaction treatment.
        // `iJ` of module 694318: any channel but a one-to-one DM (servers, threads, group DMs).
        let spam = self.treat_spam()
            && self.channel.is_some()
            && !self.current_dm().is_some_and(|channel| channel.kind == 1);
        // The run being folded: its first id, whether it is open, and its kind.
        let mut run: Option<(Id, bool, blocked::Fold)> = None;
        // Not in the pinned client: BetterBlockedUsers folds every blocked (and every ignored) message of a hidden
        // stretch into one row (`mergeHiddenMessages`, `plugin_hooks.rs`): each kind's first row of the stretch, by
        // `Fold` (the spam fold is never merged).
        let merge = plugin_hooks::merge_hidden_messages();
        let mut stretch: [Option<Id>; blocked::FOLDS] = [None; blocked::FOLDS];
        for (index, message) in messages.iter().enumerate() {
            let local = message.local_time();
            let day = (local.year(), local.ordinal());
            let new_day = last_day != Some(day);
            if new_day {
                self.rows.push(Row::Divider(timestamps::long_date(local)));
                last_day = Some(day);
                stretch = [None; blocked::FOLDS];
            }
            let fold = if blocked.contains(&message.author.id) {
                Some(blocked::Fold::Blocked)
            } else if ignored.contains(&message.author.id) {
                Some(blocked::Fold::Ignored)
            } else if spam && self.likely_spammer_message(message) {
                Some(blocked::Fold::Spammer)
            } else {
                None
            };
            if let Some(kind) = fold {
                let merged = stretch[kind as usize]
                    .filter(|_| merge && !new_day && matches!(kind, blocked::Fold::Blocked | blocked::Fold::Ignored));
                let open = match run {
                    Some((first, open, run_kind)) if !new_day && run_kind == kind => {
                        if let Some(Row::Blocked { count, .. }) = self
                            .rows
                            .iter_mut()
                            .rev()
                            .find(|row| matches!(row, Row::Blocked { first: known, .. } if *known == first))
                        {
                            *count += 1;
                        }
                        open
                    }
                    // Not in the pinned client: the kind's row of this stretch takes the message (BetterBlockedUsers).
                    _ if merged.is_some() => {
                        let first = merged.unwrap_or(message.id);
                        if let Some(Row::Blocked { count, .. }) = self
                            .rows
                            .iter_mut()
                            .rev()
                            .find(|row| matches!(row, Row::Blocked { first: known, .. } if *known == first))
                        {
                            *count += 1;
                        }
                        let open = self.shown_blocked.contains(&first);
                        run = Some((first, open, kind));
                        open
                    }
                    _ => {
                        let open = self.shown_blocked.contains(&message.id);
                        stretch[kind as usize] = Some(message.id);
                        self.rows.push(Row::Blocked {
                            first: message.id,
                            count: 1,
                            kind,
                        });
                        run = Some((message.id, open, kind));
                        open
                    }
                };
                if open {
                    self.rows.push(Row::Message { index, head: true });
                }
                continue;
            }
            run = None;
            stretch = [None; blocked::FOLDS];
            let mut head = new_day || index == 0 || starts_group(&messages[index - 1], message);
            if !divided && divider.is_some_and(|acked| message.id > acked) {
                // The divider starts a new group, as the official list breaks one there.
                self.rows.push(Row::NewDivider);
                divided = true;
                head = true;
            }
            self.rows.push(Row::Message { index, head });
            if in_post && Some(message.id) == self.channel {
                self.rows.push(Row::PostActions);
            }
        }
        self.rows.push(Row::Spacer);
        self.list.reset(self.rows.len());
    }

    /// The open channel when it is one of the guild's threads.
    fn current_thread(&self) -> Option<dc_state::Thread> {
        let channel = self.channel?;
        self.current_guild()?
            .threads
            .iter()
            .find(|thread| thread.id == channel)
            .cloned()
    }

    /// Whether the message mentions the signed-in account (module 451919 with the guild's suppress settings).
    fn mentions_me(&self, message: &Message) -> bool {
        let Some(me) = self.stores.current_user.as_ref().map(|user| user.id) else {
            return false;
        };
        let guild = self
            .current_guild()
            .or_else(|| self.stores.guild_of_channel(message.channel_id));
        let settings = guild.and_then(|guild| self.stores.guild_settings.guilds.get(&guild.id));
        message.mentions_me(
            me,
            guild.map_or(&[][..], |guild| guild.my_roles.as_slice()),
            settings.is_some_and(|settings| settings.suppress_everyone),
            settings.is_some_and(|settings| settings.suppress_roles),
        )
    }

    fn current_guild(&self) -> Option<&Guild> {
        self.guild.and_then(|guild| self.stores.guild(guild))
    }

    /// The current channel's name, or the other person's in a DM.
    fn channel_title(&self) -> String {
        if let Some(channel) = self.current_channel() {
            return channel.name.clone();
        }
        if let Some(thread) = self.current_thread() {
            return thread.name;
        }
        self.current_dm()
            .map(|channel| self.private_channel_name(channel))
            .unwrap_or_default()
    }

    /// Harness only: the open DM's recipient, for `gw` fixtures that stand for someone else.
    #[cfg(feature = "capture")]
    pub(crate) fn script_dm_user(&self) -> Option<String> {
        self.current_dm()
            .and_then(|dm| self.stores.dm_recipient(dm))
            .map(|user| user.id.to_string())
    }

    fn current_dm(&self) -> Option<&dc_state::PrivateChannel> {
        let channel = self.channel?;
        self.stores
            .private_channels
            .iter()
            .find(|known| known.id == channel)
    }

    fn current_channel(&self) -> Option<&Channel> {
        let channel = self.channel?;
        self.current_guild()?
            .channels
            .iter()
            .find(|known| known.id == channel)
    }

    fn rendered_block_text(&self, block: &Block) -> String {
        match block {
            Block::Paragraph(spans)
            | Block::Quote(spans)
            | Block::Subtext(spans)
            | Block::Heading { spans, .. }
            | Block::ListItem { spans, .. } => {
                spans.iter().map(|span| self.span_label(span)).collect()
            }
            Block::Code { text, .. } => text.lines().collect::<Vec<_>>().join("\n"),
        }
    }

    /// The selected text across messages, one block per line.
    pub fn selected_text(&self) -> String {
        let selection = self.selection.borrow();
        let empty = Vec::new();
        let messages = self
            .channel
            .and_then(|channel| self.stores.messages.get(&channel))
            .unwrap_or(&empty);
        let mut lines = Vec::new();
        for message in messages {
            for (index, block) in parse(&message.content).iter().enumerate() {
                let text = self.rendered_block_text(block);
                if let Some((from, to)) = selection.range_in((message.id, index as u32), text.len())
                {
                    let from = clamp_char_boundary(&text, from);
                    let to = clamp_char_boundary(&text, to);
                    if from < to {
                        lines.push(text[from..to].to_owned());
                    }
                }
            }
        }
        lines.join("\n")
    }

    /// For test scripts: where the app is now.
    #[cfg(feature = "capture")]
    pub fn describe(&self, cx: &App) -> String {
        let composer = self.composer.read(cx).text();
        let add_friend = self
            .add_friend
            .as_ref()
            .map(|form| (form.name.read(cx).text().to_owned(), form.result.clone()));
        let notification_events = crate::notifications::events();
        let nameplates = self.nameplate_status();
        let gifv = self.inline_video_status();
        let profile = self.profile.as_ref().map(|profile| {
            let body = self.fetched.get(&format!("profile:{}", profile.user));
            (
                body.is_some(),
                body.and_then(|body| body["private"].as_bool()),
            )
        });
        format!(
            "notifications={notification_events:?} nameplates=[{nameplates}] gifv=[{gifv}] profile_loaded_private={profile:?} status={} expires={} custom_status={:?} profile_menu={:?} add_friend={add_friend:?} friends_tab={:?} guild={} channel={} composer={composer:?} rows={} selected={:?} hovered={:?} editing={} embeds={} theme={} voice={}",
            self.stores.status,
            self.stores
                .status_state()
                .map_or(0, |state| state.expires_at_ms),
            self.stores
                .status_state()
                .and_then(|state| state.custom)
                .map(|custom| custom.text),
            self.profile_menu
                .as_ref()
                .map(|menu| (menu.user, menu.invite, menu.sent.len())),
            self.friends_tab,
            self.current_guild()
                .map(|guild| guild.name.as_str())
                .unwrap_or("-"),
            self.current_channel()
                .map(|channel| channel.name.as_str())
                .unwrap_or("-"),
            self.rows.len(),
            self.selected_text(),
            self.hovered,
            self.editing.is_some(),
            self.channel
                .and_then(|channel| self.stores.messages.get(&channel))
                .map_or(0, |messages| messages
                    .iter()
                    .map(|message| message.embeds.len())
                    .sum::<usize>()),
            self.stores.theme,
            match &self.call {
                Some(call) if call.connected => "connected",
                Some(_) => "connecting",
                None => "none",
            },
        ) + &format!(" voice_settings=[{}]", self.voice.describe())
            + &format!(" video=[{}]", self.video.describe())
            + &format!(
                " sounds={:?} ptt_hook={}",
                crate::sounds::played(),
                self.ptt_hook.is_some()
            )
    }

    /// Keys the focused composer answers itself: Escape drops the reply, Up in an empty field edits the last message
    /// of yours in the channel. True when the key was used.
    fn composer_key(
        &mut self,
        keys: &gpui::Keystroke,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if !self.composer.read(cx).focus_handle(cx).is_focused(window) {
            return false;
        }
        let plain = !(keys.modifiers.control
            || keys.modifiers.alt
            || keys.modifiers.shift
            || keys.modifiers.platform);
        if self.slash_key(keys, window, cx) {
            return true;
        }
        if self.autocomplete_key(keys, window, cx) {
            return true;
        }
        // Ctrl+V with a picture or files on the clipboard attaches them; text is the field's own paste.
        if keys.key == "v"
            && keys.modifiers.control
            && !keys.modifiers.alt
            && !keys.modifiers.shift
            && let Some(item) = cx.read_from_clipboard()
            && self.paste_clipboard_item(&item, cx)
        {
            return true;
        }
        if keys.key == "up" && plain && self.composer.read(cx).text().is_empty() {
            let me = self.stores.current_user.as_ref().map(|user| user.id);
            let last = self
                .channel
                .and_then(|channel| self.stores.messages.get(&channel))
                .and_then(|messages| {
                    messages
                        .iter()
                        .rev()
                        .find(|message| Some(message.author.id) == me && message.kind == 0)
                })
                .map(|message| (message.id, message.content.clone()));
            if let Some((id, content)) = last {
                self.begin_edit(id, &content, window, cx);
                return true;
            }
        }
        false
    }

    fn begin_edit(&mut self, message: Id, text: &str, window: &mut Window, cx: &mut Context<Self>) {
        let editor = cx.new(|cx| {
            let mut editor = Editor::new(editor_style(), cx);
            // The edit box is the same rich input as the composer: typed markdown and links are styled.
            editor.set_markdown_preview(true);
            editor.set_text(text);
            editor
        });
        cx.subscribe_in(&editor, window, move |this, _, event, window, cx| {
            match event {
                // Saving an empty edit asks to delete the message instead.
                EditorEvent::Submit(content) if content.is_empty() => {
                    this.confirm_delete = Some(message);
                    this.editing = None;
                }
                // Saving or cancelling an edit hands focus back to the composer (module 699344).
                EditorEvent::Submit(content) => {
                    if let Some(channel) = this.channel {
                        let _ = this.requests.send(Request::Edit {
                            channel,
                            message,
                            // Built-in plugins that rewrite outgoing text (ClearURLs).
                            content: crate::plugins::outgoing(content),
                        });
                    }
                    this.end_edit(window, cx);
                }
                EditorEvent::Cancel => this.end_edit(window, cx),
                EditorEvent::Edited => {}
            }
            cx.notify();
        })
        .detach();
        window.focus(&editor.read(cx).focus_handle(cx), cx);
        self.editing = Some((message, editor));
        cx.notify();
    }

    /// `TYPING_START_LOCAL`: at most one notice per 80% of the notice's length.
    fn notify_typing(&mut self) {
        if self.screening_pending() {
            return;
        }
        let due = self
            .last_typing_sent
            .is_none_or(|sent| sent.elapsed() >= TYPING_DURATION.mul_f32(0.8));
        if let (true, Some(channel)) = (due, self.channel) {
            self.last_typing_sent = Some(Instant::now());
            let _ = self.requests.send(Request::Typing { channel });
        }
    }

    /// A reaction's count, which rolls when it changes (`NumberTicker`, module 114166): the old number slides out
    /// and the new one in over a clipped, fixed-height box, as wide as the wider of the two.
    fn rolling_count(&self, message: Id, index: usize, count: u64) -> AnyElement {
        let roll = motion::number_roll(motion::key(&("reaction-count", message, index)), count);
        let Some((previous, previous_y)) = roll.previous else {
            return div().child(count.to_string()).into_any_element();
        };
        let widest = previous.max(roll.current).to_string();
        let line = |text: String, y: f32| {
            div()
                .absolute()
                .left(px(0.))
                .right(px(0.))
                .top(px(16. * y))
                .h(px(16.))
                .flex()
                .justify_center()
                .child(text)
        };
        div()
            .relative()
            .overflow_hidden()
            .child(div().opacity(0.).child(widest))
            .child(line(previous.to_string(), previous_y))
            .child(line(roll.current.to_string(), roll.current_y))
            .into_any_element()
    }

    fn typing_bar(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let channel = self.channel?;
        // Not in the pinned client: TypingTweaks' list and line (`plugin_hooks/typing_tweaks.rs`).
        let tweaks = self.typing_tweaks_users(channel);
        let users = tweaks
            .clone()
            .unwrap_or_else(|| self.stores.typing_users(channel, Instant::now()));
        let names: Vec<String> = users
            .iter()
            .filter_map(|user| self.stores.users.get(user))
            .map(|user| user.display_name().to_owned())
            .collect();
        // Module 620014 `X`: one, two and three names (`lJ9sZX`, `rB0CUa`, `StKThj`), else "Several people"
        // (`Q8lUnE`); the names in bold.
        let runs = match names.as_slice() {
            [] => return None,
            [a] => dc_intl::format_runs("lJ9sZX", &[("a", a.into())]),
            [a, b] => dc_intl::format_runs("rB0CUa", &[("a", a.into()), ("b", b.into())]),
            [a, b, c] => dc_intl::format_runs(
                "StKThj",
                &[("a", a.into()), ("b", b.into()), ("c", c.into())],
            ),
            _ => dc_intl::format_runs("Q8lUnE", &[]),
        };
        // `.inTextChannel_U9Tvvz.base_U9Tvvz`: `--space-md` (16) in, `--text-subtle`; `typingDots_U9Tvvz` 8px in; the
        // dots are module 128539's CSS dots under the experiment, else module 863610's (`typing_avatar`); the text
        // `--space-md + --space-8` after them (the capture `pt-activities` computes it to 24px).
        Some(
            div()
                .absolute()
                .left(px(16.))
                .top(px(-24.))
                .h(px(24.))
                .flex()
                .items_center()
                .text_size(px(12.))
                .line_height(px(16.))
                .font_weight(FontWeight(500.))
                .text_color(color("--text-subtle"))
                .child(
                    div()
                        .ml(px(8.))
                        .child(typing_avatar::chat_dots(typing_avatar::css_dots(
                            &self.stores.experiments,
                        ))),
                )
                .child(
                    div()
                        .ml(px(24.))
                        .flex()
                        .whitespace_nowrap()
                        .map(|this| match tweaks {
                            Some(users) => this.children(self.typing_tweaks_line(&users, runs, cx)),
                            None => this.children(runs.into_iter().map(|(text, tag)| {
                                div()
                                    .when(tag.as_deref() == Some("$b"), |this| {
                                        this.font_weight(FontWeight(600.))
                                            .text_color(color("--text-default"))
                                    })
                                    .child(text)
                            })),
                        }),
                )
                .into_any_element(),
        )
    }

    fn send(&mut self, content: String) {
        if self.screening_pending() {
            return;
        }
        // Built-in plugins that rewrite outgoing text (ClearURLs).
        let content = crate::plugins::outgoing(&content);
        let staged = self.sticker_ui.take_staged();
        if content.is_empty() && self.pending_files.is_empty() && staged.is_none() {
            return;
        }
        // Not in the pinned client: SilentMessageToggle's pre-send listener, for a message that goes out
        // (`plugin_hooks/silent_message_toggle.rs`).
        let content = self.silent_message_presend(content);
        let ping = self.reply_ping;
        let reply = self
            .replying
            .take()
            .map(|message| (self.guild, message.id, ping));
        let files = self
            .pending_files
            .drain(..)
            .map(|pending| pending.upload())
            .collect();
        if let Some(channel) = self.channel {
            // With a time set for the channel the message is scheduled instead of sent.
            if let Some(timestamp) = self.scheduled_timestamp(channel) {
                self.send_scheduled(
                    channel,
                    timestamp,
                    content,
                    files,
                    reply,
                    staged.into_iter().collect(),
                );
                return;
            }
            if let Some(sticker) = staged {
                // `STICKER_TRACK_USAGE` follows the send.
                self.stores.stickers.track(&[sticker], picker::now_ms());
            }
            // The row shows at once and the message waits in the send queue (`send_state`).
            self.start_local_send(channel, content, files, reply, staged.into_iter().collect());
        }
    }

    pub(crate) fn begin_reply(
        &mut self,
        message: Message,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.replying = Some(message);
        self.reply_ping = true;
        window.focus(&self.composer.read(cx).focus_handle(cx), cx);
        cx.notify();
    }

    /// What a reply's bar shows (ReferencedMessageStore, module 9842, read by module 448368): the message, or
    /// NOT_LOADED / DELETED. `referenced_message` null in the payload is DELETED; absent, the loaded messages are
    /// looked up by the reference, else NOT_LOADED. Only replies (type 19) take the bar here: a thread starter has
    /// its own line (`starter_missing_line`), and a context menu command keeps its command bar.
    fn reply_reference(&self, message: &Message) -> Option<Reference> {
        if let Some(referenced) = &message.referenced {
            return Some(Reference::Loaded(referenced.clone()));
        }
        if message.kind != 19 {
            return None;
        }
        let id = message.reference_id?;
        if message.extra.reference_deleted {
            return Some(Reference::Deleted);
        }
        let channel = message.reference_channel_id.unwrap_or(message.channel_id);
        Some(
            match self
                .stores
                .messages
                .get(&channel)
                .and_then(|list| list.iter().rev().find(|known| known.id == id))
            {
                Some(found) => Reference::Loaded(Box::new(found.clone())),
                None => Reference::NotLoaded { channel, id },
            },
        )
    }

    /// Module 448368 for NOT_LOADED and DELETED: the spine, the arrow badge in place of the avatar
    /// (`replyBadge_2i8Bfm`: 16px, `--background-base-lowest`, the 10.8 x 7.2 arrow `replyIcon` in `--text-default`),
    /// no author, and `repliedTextPlaceholder_2i8Bfm` (italic, 2px at the end): "Message could not be loaded"
    /// (`1i+hMi`, clickable: it turns `--text-strong` under the pointer) or "Original message was deleted"
    /// (`mE3KJN`, not clickable).
    fn reply_missing(&self, reply: Id, deleted: bool) -> AnyElement {
        let spine_key = motion::key(&("reply-spine", reply, "missing"));
        let text_key = motion::key(&("reply-text", reply, "missing"));
        let over = !deleted && (motion::hovered(spine_key) || motion::hovered(text_key));
        let spine = motion::color(
            motion::key(&("reply-spine-color", reply, "missing")),
            color(if over {
                "--interactive-text-default"
            } else {
                "--spine-default"
            }),
            motion::css(50., motion::Ease::Ease),
        );
        div()
            .relative()
            .h(px(18.))
            .flex()
            .items_center()
            .text_size(px(14.))
            .line_height(px(18.))
            .child(
                div()
                    .absolute()
                    .left(px(-37.))
                    .top(px(8.))
                    .w(px(33.))
                    .h(px(12.))
                    .border_l_2()
                    .border_t_2()
                    .rounded_tl(px(6.))
                    .border_color(spine)
                    .when(!deleted, |spine| {
                        spine.child(motion::hover_probe(spine_key))
                    }),
            )
            .child(
                div()
                    .size(px(16.))
                    .flex_none()
                    .mr(px(4.))
                    .rounded_full()
                    .bg(color("--background-base-lowest"))
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(
                        icon("reply_badge", 10.8, "--text-default")
                            .w(px(10.8))
                            .h(px(7.2)),
                    ),
            )
            .child(
                div()
                    .min_w(px(0.))
                    .h(px(18.))
                    .pr(px(2.))
                    .italic()
                    .whitespace_nowrap()
                    .overflow_hidden()
                    .text_color(color(if over {
                        "--text-strong"
                    } else {
                        "--text-default"
                    }))
                    .child(if deleted {
                        dc_intl::t!("mE3KJN")
                    } else {
                        dc_intl::t!("1i+hMi")
                    })
                    .when(!deleted, |text| text.child(motion::hover_probe(text_key))),
            )
            .into_any_element()
    }

    /// The official preview: a spine into the avatar, 16px avatar, name, and one line of the text.
    fn reply_preview(&self, referenced: &Message) -> impl IntoElement {
        let avatar = self
            .images
            .get(&self.guild_avatar_source(&referenced.author).0)
            .cloned();
        let text = first_spans(&referenced.content);
        // `.repliedMessageClickableSpine:hover` and `.repliedTextPreview.clickable:hover` (738015): over either one
        // the spine turns `--interactive-text-default` (`transition: border 50ms`) and the text `--text-strong`.
        let spine_key = motion::key(&("reply-spine", referenced.id));
        let text_key = motion::key(&("reply-text", referenced.id));
        let over = motion::hovered(spine_key) || motion::hovered(text_key);
        let spine = motion::color(
            motion::key(&("reply-spine-color", referenced.id)),
            color(if over {
                "--interactive-text-default"
            } else {
                "--spine-default"
            }),
            motion::css(50., motion::Ease::Ease),
        );
        div()
            .relative()
            .h(px(18.))
            .flex()
            .items_center()
            .text_size(px(14.))
            .line_height(px(18.))
            .child(
                div()
                    .absolute()
                    .left(px(-37.))
                    .top(px(8.))
                    .w(px(33.))
                    .h(px(12.))
                    .border_l_2()
                    .border_t_2()
                    .rounded_tl(px(6.))
                    .border_color(spine)
                    .child(motion::hover_probe(spine_key)),
            )
            .child(
                div()
                    .size(px(16.))
                    .flex_none()
                    .children(avatar.map(|image| img(image).size(px(16.)).rounded_full())),
            )
            .child(
                div()
                    .ml(px(4.))
                    .flex_none()
                    .opacity(0.64)
                    .font_weight(FontWeight(500.))
                    .text_color(self.guild_author_color(referenced.author.id, "--text-strong"))
                    .child(
                        self.author_name(format!("reply-{}", referenced.id), &referenced.author),
                    ),
            )
            .child(
                div()
                    .ml(px(4.))
                    .flex_1()
                    .min_w(px(0.))
                    .h(px(18.))
                    .child(if text.is_empty() && referenced.is_voice_message() {
                        self.voice_reply_placeholder(over)
                    } else {
                        self.preview_text(&text, over).into_any_element()
                    })
                    .child(motion::hover_probe(text_key)),
            )
    }

    fn preview_text(&self, spans: &[Span], strong: bool) -> Inline {
        let color_value = hsla(if strong {
            "--text-strong"
        } else {
            "--text-default"
        });
        let pieces = spans
            .iter()
            .map(|span| {
                let style = span.style;
                let mut run_font: Font = font(if style.code { MONO } else { SANS });
                run_font.features = kerning();
                run_font.weight = if style.bold {
                    FontWeight(700.)
                } else {
                    FontWeight(400.)
                };
                run_font.style = if style.italic {
                    FontStyle::Italic
                } else {
                    FontStyle::Normal
                };
                Piece {
                    // The reply preview shows a mention as its resolved `@name`, never the raw `<@id>`.
                    text: match span.kind {
                        Kind::User(_) | Kind::Role(_) | Kind::Channel(_) | Kind::Command { .. } => {
                            self.span_label(span)
                        }
                        _ => span.text.clone(),
                    },
                    font: run_font,
                    size: if style.code { px(11.9) } else { px(14.) },
                    color: color_value,
                    underline: style.underline,
                    strike: style.strike,
                    boxed: style.code.then(preview_code_box),
                    image: None,
                    hover: None,
                    hover_underline: false,
                    gradient: None,
                }
            })
            .collect();
        inline(pieces, px(18.)).truncate()
    }

    /// Every text field that is open now, for the test harness to find the focused one.
    #[cfg(feature = "capture")]
    pub fn open_editors(&self) -> Vec<Entity<Editor>> {
        let mut list: Vec<Entity<Editor>> = Vec::new();
        list.extend(
            self.go_live
                .as_ref()
                .and_then(|state| state.game.as_ref())
                .filter(|game| game.choose_channel)
                .map(|game| game.search.clone()),
        );
        list.extend(self.inventory_reply_editor());
        list.extend(settings::client_theme::editor_fields());
        if self.channel_browser_open {
            list.extend(
                self.channel_browser
                    .as_ref()
                    .map(|browser| browser.search.clone()),
            );
        }
        list.extend(self.forum_editors());
        list.extend(self.group_dm.as_ref().map(|picker| picker.search.clone()));
        list.extend(self.discover_editors());
        list.extend(
            self.quick_switcher
                .as_ref()
                .map(|switcher| switcher.search.clone()),
        );
        list.extend(self.full_profile.as_ref().map(|open| open.note.clone()));
        list.extend(self.custom_status.as_ref().map(|open| open.input.clone()));
        list.extend(self.friend_nickname.as_ref().map(|open| open.input.clone()));
        list.extend(
            self.favorite_nickname
                .as_ref()
                .map(|open| open.input.clone()),
        );
        list.extend(self.chat_extra.lockdown_other.clone());
        list.extend(self.group_edit.as_ref().map(|open| open.input.clone()));
        list.extend(self.member_action_input());
        list.extend(self.modify_attachment_editors());
        list.extend(self.report_issue_editors());
        list.extend(self.sound_dialog.as_ref().map(|dialog| dialog.name.clone()));
        if let Some(form) = self.poll.as_ref() {
            list.push(form.question.clone());
            list.extend(form.answers.iter().cloned());
        }
        list.extend(
            self.member_view
                .as_ref()
                .map(|view| view.state.search.clone()),
        );
        if let Some(settings) = self.server_settings.as_ref() {
            list.push(settings.name.clone());
            list.push(settings.description.clone());
            list.extend(settings.traits.iter().cloned());
            list.push(settings.games_query.clone());
            list.extend(
                settings
                    .members
                    .as_ref()
                    .map(|members| members.search.clone()),
            );
            if let Some(dialog) = settings.sticker_dialog.as_ref() {
                list.push(dialog.name.clone());
                list.push(dialog.description.clone());
            }
            if let Some(access) = settings.access.as_ref() {
                list.extend(access.editors());
            }
            list.extend(settings.discovery.description.clone());
            list.extend(settings.discovery.tag_input.clone());
            list.extend(settings.landing.editors());
            if let Some(automod) = settings.automod.as_ref() {
                list.extend(automod.editors());
            }
            if let Some(onboarding) = settings.onboarding.as_ref() {
                list.extend(onboarding.editors());
            }
            list.extend(
                settings
                    .webhooks
                    .open
                    .as_ref()
                    .map(|open| open.name.clone()),
            );
            if let Some(template) = settings.template.as_ref() {
                list.push(template.title.clone());
                list.push(template.description.clone());
            }
            list.extend(settings.bans.as_ref().map(|bans| bans.search.clone()));
            list.extend(
                settings
                    .emoji
                    .as_ref()
                    .map(|emoji| emoji.fields())
                    .unwrap_or_default(),
            );
            list.extend(
                settings
                    .roles
                    .as_ref()
                    .map(|roles| roles.fields())
                    .unwrap_or_default(),
            );
            list.extend(
                settings
                    .delete_name
                    .as_ref()
                    .map(|(field, _)| field.clone()),
            );
        }
        if let Some(settings) = self.channel_settings.as_ref() {
            list.push(settings.name.clone());
            list.push(settings.topic.clone());
            list.extend(settings.add_target.as_ref().map(|(field, _)| field.clone()));
        }
        list.extend(
            self.profile
                .as_ref()
                .and_then(|popout| popout.message.as_ref())
                .map(|message| message.editor.clone()),
        );
        if let Some(forward) = self.forward.as_ref() {
            list.push(forward.search.clone());
            list.push(forward.note.clone());
        }
        if let Some(form) = self.add_friend.as_ref() {
            list.push(form.name.clone());
            list.push(form.note.clone());
        }
        list.extend(
            self.people_search
                .as_ref()
                .map(|search| search.field.clone()),
        );
        if let Some(form) = self.new_thread.as_ref() {
            list.push(form.name.clone());
            list.push(form.message.clone());
        }
        list.extend(
            self.thread_browser
                .as_ref()
                .map(|browser| browser.search.clone()),
        );
        list.extend(self.gifs.as_ref().map(|gifs| gifs.search.clone()));
        list.extend(self.sticker_ui.search());
        list.extend(self.create_channel.as_ref().map(|form| form.name.clone()));
        list.extend(self.channel_status_fields());
        list.extend(self.guild_events_editors());
        list.extend(self.schedule_editors());
        list.extend(self.search.as_ref().map(|search| search.field.clone()));
        list.extend(
            self.search
                .as_ref()
                .and_then(|search| search.jump.as_ref())
                .map(|jump| jump.field.clone()),
        );
        list.extend(self.search_editors());
        list.extend(self.shop_search_editors());
        list.push(self.active_editor());
        list
    }

    /// The field with focus: the inline editor while editing, else the composer.
    #[cfg(feature = "capture")]
    pub fn active_editor(&self) -> Entity<Editor> {
        if let Some(search) = self.picker.as_ref().map(picker::Picker::search) {
            return search;
        }
        self.editing
            .as_ref()
            .map_or_else(|| self.composer.clone(), |(_, editor)| editor.clone())
    }

    fn on_key(&mut self, event: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        let keystroke = &event.keystroke;
        if keystroke.key == "c" && keystroke.modifiers.control {
            let text = self.selected_text();
            if !text.is_empty() {
                cx.write_to_clipboard(ClipboardItem::new_string(text));
            }
        }
    }

    fn titlebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let title = match (self.current_guild(), self.channel) {
            _ if self.in_favorites() => dc_intl::tr("Favorites").to_owned(),
            (Some(guild), _) => guild.name.clone(),
            (None, None) if self.discover_active() => dc_intl::tr("Discover").to_owned(),
            (None, None) if self.nitro_open => dc_intl::tr("Nitro").to_owned(),
            (None, None) if self.quests_active() => dc_intl::tr("Quests").to_owned(),
            (None, None) if self.shop_active() => dc_intl::tr("Shop").to_owned(),
            // The title bar of /message-requests carries no title (golden message-requests/requests-empty).
            (None, None) if self.message_requests_active() => String::new(),
            // As /library's title bar in the capture (library/empty): no title.
            (None, None) if self.library_active() => String::new(),
            (None, None) => dc_intl::tr("Friends").to_owned(),
            // A DM's window title is the section, not the person (`title_LnjVAX` in the DM capture).
            (None, Some(_)) => dc_intl::tr("Direct Messages").to_owned(),
        };
        let in_dm = self.guild.is_none() && self.channel.is_some();
        let guild_icon = self.current_guild().map(|guild| {
            // The Favorites view's title carries the star (18px, frame menus2/favorites-view), not the server's icon.
            if self.in_favorites() {
                return icon("clips_star", 18., "--interactive-icon-default").into_any_element();
            }
            match self.images.get(&format!("guild:{}", guild.id)).cloned() {
                Some(image) => img(image).size(px(16.)).rounded(px(4.)).into_any_element(),
                None => div()
                    .size(px(16.))
                    .rounded(px(4.))
                    .bg(color("--background-base-lower"))
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_size(px(8.))
                    .text_color(color("--text-default"))
                    .child(guild.acronym())
                    .into_any_element(),
            }
        });
        let update_button = self.update_button(cx);
        div()
            .parity("titlebar")
            .relative()
            .flex_none()
            .h(px(32.))
            .w_full()
            .window_control_area(WindowControlArea::Drag)
            .child(self.history_button(true, cx))
            .child(self.history_button(false, cx))
            .child(
                div()
                    .absolute()
                    .inset_0()
                    .flex()
                    .items_center()
                    .justify_center()
                    .gap(px(8.))
                    .child(if let Some(guild_icon) = guild_icon {
                        guild_icon
                    } else if in_dm {
                        icon("title_direct_messages", 18., "--text-default").into_any_element()
                    } else if self.discover_active() {
                        icon("discover", 18., "--text-default").into_any_element()
                    } else if self.nitro_open {
                        icon("home_nitro", 16., "--text-default").into_any_element()
                    } else if self.quests_active() {
                        icon("home_quests", 18., "--text-default").into_any_element()
                    } else if self.shop_active() {
                        icon("home_shop", 18., "--text-default").into_any_element()
                    } else if self.message_requests_active() || self.library_active() {
                        div().into_any_element()
                    } else {
                        icon("home_friends", 16., "--text-default").into_any_element()
                    })
                    .child(
                        div()
                            .text_size(px(14.))
                            .line_height(px(18.))
                            .font_weight(FontWeight(500.))
                            .text_color(color("--text-default"))
                            .child(title),
                    ),
            )
            .child(
                div()
                    .occlude()
                    .absolute()
                    .right(px(0.))
                    .top(px(0.))
                    .flex()
                    .items_center()
                    .child({
                        let inbox_open = self.inbox.is_some();
                        let overdue = self.has_overdue_reminder();
                        let inbox = self
                            .title_stop(bar_button("inbox", "inbox", inbox_open), "inbox", 6., cx)
                            .relative()
                            .on_click(cx.listener(|this, _, _, cx| this.toggle_inbox(cx)))
                            .when(overdue, |button| {
                                // `189252` + `573435` `HEADER_BAR_BADGE_BOTTOM` + `687828` `.badge_fErCLM`: a 6px
                                // `--background-feedback-notification` dot in a 9px cutout at the icon's bottom right.
                                button.child(
                                    div()
                                        .absolute()
                                        .right(px(3.))
                                        .bottom(px(3.))
                                        .size(px(9.))
                                        .rounded_full()
                                        .bg(color("--background-base-lowest"))
                                        .group_hover(SharedString::from("bar-inbox"), |style| {
                                            style.bg(color("--background-mod-normal"))
                                        })
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .child(
                                            div()
                                                .size(px(6.))
                                                .rounded_full()
                                                .bg(color("--background-feedback-notification")),
                                        ),
                                )
                            })
                            .child(self.anchor_probe("bar-inbox"));
                        if inbox_open {
                            inbox.occlude().mr(px(12.)).into_any_element()
                        } else {
                            self.with_tip(
                                "title-inbox",
                                Tip::text(dc_intl::tr("Inbox"), Side::Bottom),
                                inbox,
                                cx,
                            )
                            .occlude()
                            .mr(px(12.))
                            .into_any_element()
                        }
                    })
                    .child({
                        // `ar` in module 596339: an anchor to the Help Center (`tP.C`, https:// + support host).
                        let help = self
                            .title_stop(bar_button("help", "help", false), "help", 6., cx)
                            .relative()
                            .on_click(|_, _, cx| cx.open_url("https://support.discord.com"));
                        // The bar's items sit 12 apart (`trailing_3zSH5d`); the update button, when up, follows.
                        self.with_tip(
                            "title-help",
                            Tip::text(dc_intl::tr("Help"), Side::Bottom),
                            help,
                            cx,
                        )
                        .occlude()
                        .mr(px(if update_button.is_some() {
                            12.
                        } else {
                            16.
                        }))
                    })
                    .children(update_button)
                    .child(
                        div()
                            .w(px(1.))
                            .h(px(20.))
                            .mr(px(8.))
                            .bg(color("--background-mod-subtle")),
                    )
                    .child(window_controls()),
            )
    }

    fn rail_item(&self, guild: &Guild, cx: &mut Context<Self>) -> impl IntoElement {
        let id = guild.id;
        let selected = self.guild == Some(id) && !self.in_favorites();
        let counts = self.guild_rail_counts(guild);
        let image = self.images.get(&format!("guild:{id}")).cloned();
        let row_key = format!("guild-{id}");
        // An animated icon plays while the pointer is over its rail item (`live_guild_icon`).
        let hover_key = format!("rail-guild:{id}");
        let live = self.live_guild_icon(guild, self.avatar_hovered(&hover_key), cx);
        let tile = div()
            .id(SharedString::from(format!("guild-{id}")))
            .on_hover(self.avatar_hover(hover_key, cx))
            .on_mouse_up(
                MouseButton::Right,
                cx.listener(move |this, event: &gpui::MouseUpEvent, _, cx| {
                    this.open_context_menu(context_menu::Target::Guild(id), event.position, cx);
                    cx.stop_propagation();
                }),
            )
            .size(px(40.))
            .rounded(px(12.))
            .overflow_hidden()
            .flex()
            .items_center()
            .justify_center()
            .cursor_pointer()
            // Where Shift+F10 puts the server's menu (`keyboard/context_key.rs`).
            .child(self.anchor_probe(format!("stop-guild-{id}")))
            .on_click(cx.listener(move |this, _, _, cx| this.go_guild(id, cx)));
        let tile = self.group_stop(tile, "rail", keyboard::Nav::List, &row_key, selected, cx);
        let tile = match (live, image) {
            (Some(live), _) => tile.child(live),
            (None, Some(image)) => tile.child(img(image).size(px(40.)).rounded(px(12.))),
            (None, None) => tile
                // `childWrapper_JJ-hgR`: brand coloured while hovered or selected, over .15s.
                .bg(motion::rail::tile_background(
                    &row_key,
                    selected,
                    "--background-mod-subtle",
                ))
                .text_size(px(18.))
                .line_height(px(21.6))
                .font_weight(FontWeight(500.))
                .text_color(motion::rail::tile_foreground(&row_key, selected))
                .child(guild.acronym()),
        };
        div()
            .relative()
            .w(px(72.))
            .h(px(40.))
            .flex_none()
            .flex()
            .justify_center()
            // `item__o8_Xq`: one pill that moves between hidden, the 8px unread dot, 20px hovered and 40px selected.
            .child(motion::rail::pill(&row_key, selected, counts.unread))
            .child(motion::hover_probe(motion::rail::hover_key(&row_key)))
            .child(self.with_tip(format!("guild-{id}"), Tip::rail(Body::Guild(id)), tile, cx))
            .when(counts.mentions > 0, |row| {
                row.child(unread_badges::rail_number(
                    counts.mentions,
                    counts.high_mentions == 0,
                    selected.then_some("rail-mention-badge"),
                ))
            })
            // The server this client is in a call in carries a green speaker at the icon's top right.
            .when(
                self.call.as_ref().is_some_and(|call| call.guild == id),
                |this| {
                    this.child(
                        div()
                            .absolute()
                            .left(px(41.))
                            .top(px(1.))
                            .size(px(14.))
                            .rounded_full()
                            .bg(color("--status-positive"))
                            .shadow(vec![gpui::BoxShadow {
                                color: color("--background-base-lowest").into(),
                                offset: gpui::point(px(0.), px(0.)),
                                blur_radius: px(0.),
                                spread_radius: px(2.),
                                inset: false,
                            }])
                            .flex()
                            .items_center()
                            .justify_center()
                            // `getCurrentUserActiveStream` (module 396655): the stream this client sends or
                            // the one it watches puts the screen on the badge.
                            .child(if self.in_a_stream() {
                                icon("rail_stream_badge", 10., "--white").into_any_element()
                            } else if self.call.as_ref().is_some_and(|call| call.camera) {
                                icon("rail_camera_badge", 10., "--white").into_any_element()
                            } else if self.call.as_ref().is_some_and(|call| {
                                self.stores.guild(call.guild).is_some_and(|guild| {
                                    guild.channels.iter().any(|channel| {
                                        channel.id == call.channel
                                            && channel.kind == ChannelKind::Other(13)
                                    })
                                })
                            }) {
                                // A Stage's badge carries its own glyph (stage-ann/stage-live).
                                icon("stage_channel", 10., "--white").into_any_element()
                            } else {
                                icon("voice_channel", 10., "--white").into_any_element()
                            }),
                    )
                },
            )
    }

    fn rail(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let tool = |name: &str, size: f32| {
            div()
                .id(SharedString::from(format!("rail-tool-{name}")))
                .flex_none()
                .size(px(40.))
                .rounded(px(12.))
                .bg(color("--background-mod-subtle"))
                // `circleIconButton_g8YUxD`: brand coloured on hover, over .15s ease-out.
                .hover(|style| style.bg(color("--background-brand")))
                .rule(motion::Rule::CircleIconButton)
                .flex()
                .items_center()
                .justify_center()
                .child(icon(name, size, "--text-default"))
        };
        let list =
            div()
                .parity("rail")
                .id("guild-rail-scroll")
                .overflow_y_scroll()
                .flex_1()
                .min_h(px(0.))
                .w(px(72.))
                .flex()
                .flex_col()
                .items_center()
                .gap(px(8.))
                .child(self.home_tile(cx))
                // Not in the pinned client: ReadAllNotificationsButton (`plugin_hooks/read_all.rs`).
                .children(self.read_all_button(cx))
                // Not in the pinned client: ServerListIndicators (`plugin_hooks/server_list_indicators.rs`).
                .children(self.server_list_indicators(cx))
                .children(self.favorites_tile(cx))
                .children(self.unread_dm_rail(cx))
                .child(
                    div()
                        .flex_none()
                        .w(px(32.))
                        .h(px(1.))
                        .bg(color("--background-mod-subtle")),
                )
                .on_drag_move::<rail_folders::RailDrag>(cx.listener(
                    |this, event: &gpui::DragMoveEvent<rail_folders::RailDrag>, _, cx| {
                        this.rail_drag_moved(event, cx)
                    },
                ))
                .on_drop(cx.listener(|this, drag: &rail_folders::RailDrag, _, cx| {
                    this.rail_dropped(drag, cx)
                }))
                .children(self.rail_entries(cx))
                // Module 396655 `iX`: the geo-restricted servers after the tree (module 543897).
                .children(self.geo_restricted_tiles(cx))
                .child(
                    self.with_tip(
                        "add-server",
                        Tip::rail(Body::Heading(dc_intl::tr("Add a Server").into())),
                        tool("add_server", 20.),
                        cx,
                    )
                    .cursor_pointer()
                    // `tX` of module 396655: at the server limit the upsell of module 716480 opens instead.
                    .on_click(cx.listener(|this, _, window, cx| {
                        if this.at_guild_cap() {
                            this.open_guild_cap_upsell(cx);
                        } else {
                            this.open_add_server(window, cx);
                        }
                    }))
                    // The Create Your Own Server tip (`tutorial_tips`).
                    .children(self.tutorial_rail_tip(cx)),
                )
                .child(self.discover_tile(cx));
        // wrapper_5dqHTG reserves the observed panel border-box height + 8px - 16px.
        let reserved = self.user_panel_height.unwrap_or(px(58.)) - px(8.);
        div()
            .flex_none()
            .w(px(72.))
            .h_full()
            .flex()
            .flex_col()
            .child(list)
            .child(div().flex_none().h(reserved))
    }

    /// What the row's icon is chosen from (`ChannelItemIcon`, 166444).
    fn channel_icon_facts(&self, channel: &Channel, rules: bool) -> channel_icon::Facts {
        use dc_state::permissions;
        let guild = self.current_guild();
        let me = self.stores.current_user.as_ref().map(|user| user.id);
        let vocal = matches!(channel.kind, ChannelKind::Voice | ChannelKind::Other(13));
        channel_icon::Facts {
            kind: channel.kind,
            rules,
            nsfw: channel.nsfw,
            private: guild.is_some_and(|guild| permissions::is_private(guild, channel)),
            pending: guild.is_some_and(permissions::pending_gate),
            locked: vocal && !self.can_in_channel(channel, permissions::CONNECT),
            active_threads: guild.is_some_and(|guild| {
                guild.threads.iter().any(|thread| {
                    thread.parent_id == channel.id
                        && !thread.archived
                        && (thread.joined || (thread.owner_id.is_some() && thread.owner_id == me))
                })
            }),
        }
    }

    /// "UI Density" (`uiDensity`, the `.density-*` classes of the root): the member list is 256px wide at Compact, 264px
    /// by default and 268px at Spacious (`--custom-member-list-width`, stylesheet 419070).
    fn member_list_width(&self) -> f32 {
        [256., 264., 268.][self.pref_index("appearance_ui_density", 1).min(2)]
    }

    /// `--custom-category-channel-space-before-category`: 16px, `--space-8` at Compact.
    fn category_space_before(&self) -> f32 {
        if self.pref_index("appearance_ui_density", 1) == 0 {
            8.
        } else {
            16.
        }
    }

    /// `--channels-name-line-height` is 24px, 28px at Spacious (stylesheet 952007); a channel row is that line plus the
    /// link's 4px padding above and below. Compact's 20px stays under the 24px icon, so its rows keep their height.
    fn name_line_extra(&self) -> f32 {
        if self.pref_index("appearance_ui_density", 1) == 2 {
            4.
        } else {
            0.
        }
    }

    fn channel_row(&self, channel: &Channel, cx: &mut Context<Self>) -> AnyElement {
        if channel.kind == ChannelKind::Category {
            let collapsed = self.guild.is_some_and(|guild| {
                self.stores
                    .guild_settings
                    .channel(guild, channel.id)
                    .collapsed
            });
            let (guild, category) = (self.guild, channel.id);
            return self
                .group_stop_hollow(
                    div().id(SharedString::from(format!("category-{}", channel.id))),
                    "channels",
                    keyboard::Nav::List,
                    &format!("category-{}", channel.id),
                    false,
                    4.,
                    cx,
                )
                .relative()
                .child(self.anchor_probe(format!("stop-category-{category}")))
                .on_mouse_up(
                    MouseButton::Right,
                    cx.listener(move |this, event: &gpui::MouseUpEvent, _, cx| {
                        this.open_context_menu(
                            context_menu::Target::Category(category),
                            event.position,
                            cx,
                        );
                        cx.stop_propagation();
                    }),
                )
                .flex_none()
                .group(SharedString::from(format!("category-group-{category}")))
                // `--custom-category-channel-space-before-category`: 16px, 8px at Compact (stylesheet 419070).
                .pt(px(self.category_space_before()))
                .h(px(self.category_space_before() + 24.))
                .pl(px(16.))
                .pr(px(17.))
                .cursor_pointer()
                .on_click(cx.listener(move |this, _, _, cx| {
                    if let Some(guild) = guild {
                        this.toggle_category(guild, category);
                    }
                    cx.notify();
                }))
                .child(
                    div()
                        .h(px(24.))
                        .flex()
                        .items_center()
                        .gap(px(4.))
                        .text_size(px(14.))
                        .line_height(px(18.))
                        .font_weight(FontWeight(500.))
                        .text_color(color("--channels-default"))
                        // `.wrapper_yOdhm_:hover .icon_yOdhm_, .wrapper_yOdhm_:hover .name_yOdhm_` (ed1ab6525c3040).
                        .group_hover(
                            SharedString::from(format!("category-group-{category}")),
                            |style| style.text_color(color("--interactive-text-hover")),
                        )
                        .child(channel.name.clone())
                        .child(
                            svg()
                                .path("icons/category_chevron.svg")
                                .size(px(12.))
                                .flex_none()
                                // `.icon_yOdhm_`: `transform .1s linear`, turned -90deg when collapsed.
                                .with_transformation(motion::rotation(
                                    motion::key(&("category-chevron", category)),
                                    if collapsed {
                                        -std::f32::consts::FRAC_PI_2
                                    } else {
                                        0.
                                    },
                                    motion::css(100., motion::Ease::Linear),
                                )),
                        )
                        .child(div().flex_1())
                        // Create Channel inside this category, for those who may manage channels.
                        .when(self.can_manage_channels(), |this| {
                            this.child(
                                div()
                                    .id(SharedString::from(format!("category-add-{category}")))
                                    .cursor_pointer()
                                    // `.addButtonIcon_yOdhm_:hover` text-hover; `:active` text-active, 0.5px down.
                                    .text_color(color("--channels-default"))
                                    .hover(|style| {
                                        style.text_color(color("--interactive-text-hover"))
                                    })
                                    .active(|style| {
                                        style
                                            .text_color(color("--interactive-text-active"))
                                            .relative()
                                            .top(px(0.5))
                                    })
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        cx.stop_propagation();
                                        this.open_create_channel(false, window, cx);
                                        if let Some(form) = this.create_channel.as_mut() {
                                            form.parent = Some(category);
                                        }
                                    }))
                                    .child(
                                        svg()
                                            .path("icons/plus_small.svg")
                                            .size(px(16.))
                                            .flex_none(),
                                    ),
                            )
                        }),
                )
                // These are your channels! (`tutorial_tips`).
                .children(self.tutorial_channel_tip(channel, cx))
                .into_any_element();
        }
        let selected = self.channel == Some(channel.id)
            && self.member_view.is_none()
            && !self.channel_browser_open;
        // The voice channel this client is in reads like a selected one, with a green speaker and a status line.
        let in_call = channel.kind == ChannelKind::Voice
            && self
                .call
                .as_ref()
                .is_some_and(|call| call.channel == channel.id);
        // `modeUnreadImportant`: an unread channel reads in `--text-strong`, with a pill at the sidebar's edge.
        // `modeMuted`: a muted channel never reads unread, and dims to half in `--text-muted`.
        let muted = self.guild.is_some_and(|guild| {
            self.stores
                .guild_settings
                .channel_muted(guild, channel.id, chrono::Utc::now())
        });
        let unread = !muted
            && matches!(channel.kind, ChannelKind::Text | ChannelKind::Other(5))
            && self.stores.is_unread(channel.id);
        let mentions = self.stores.read_states.mentions(channel.id);
        let dim = muted && !selected && !in_call && mentions == 0;
        // A live Stage this client is connected to: green glyph and the three row actions, like a voice row
        // (stage-ann/stage-live).
        let in_stage = channel.kind == ChannelKind::Other(13)
            && self.stores.stage_instances.is_live(channel.id)
            && self
                .call
                .as_ref()
                .is_some_and(|call| call.channel == channel.id);
        let tint = if selected || in_call || unread || mentions > 0 {
            "--text-strong"
        } else if dim {
            "--text-muted"
        } else {
            "--channels-default"
        };
        let rules = self
            .current_guild()
            .and_then(|guild| guild.rules_channel_id)
            == Some(channel.id);
        let facts = self.channel_icon_facts(channel, rules);
        let name = channel_icon::name(&facts);
        let group = SharedString::from(format!("channel-{}", channel.id));
        let voice = channel.kind == ChannelKind::Voice;
        // The status line under the name (`linkBottom_TNyK-N`) makes the row 50 tall.
        let two_line = self.voice_row_has_line(channel);
        let actions: &[&str] = if voice || in_stage {
            &["voice_action_1", "voice_action_2", "voice_action_3"]
        } else {
            &["invite_small", "settings_small"]
        };
        let action_tint = if selected || in_call {
            "--text-strong"
        } else {
            "--interactive-icon-default"
        };
        let action_group = group.clone();
        let name_group = group.clone();
        let pill_group = group.clone();
        // `channelInfo_Wx6SnQ`: the call's timer takes the action icons' place until the row is hovered.
        let timer = in_call.then(|| self.call_timer()).flatten().map(|text| {
            div()
                .absolute()
                .right(px(0.))
                .text_size(px(12.))
                .line_height(px(16.))
                .font_family(MONO)
                .text_color(color("--text-feedback-positive"))
                .when(self.vv.row_focus, |this| this.opacity(0.))
                .group_hover(group.clone(), |style| style.opacity(0.))
                .child(text)
        });
        let target = channel.id;
        let glyph = div().when(dim, |this| this.opacity(0.5)).child(icon(
            name,
            20.,
            if in_call || in_stage {
                "--icon-voice-connected"
            } else if dim {
                "--icon-muted"
            } else {
                tint
            },
        ));
        let glyph = match channel_icon::label(&facts) {
            Some(label) => self
                .with_tip(
                    if rules {
                        format!("rules-channel-icon-{target}")
                    } else {
                        format!("channel-icon-{target}")
                    },
                    Tip::text(label, Side::Top),
                    glyph,
                    cx,
                )
                .into_any_element(),
            None => glyph.into_any_element(),
        };
        div()
            .flex_none()
            .relative()
            .h(px(if two_line {
                50.
            } else {
                34. + self.name_line_extra()
            }))
            .pt(px(1.))
            .pl(px(8.))
            .when(unread, |this| {
                this.child(
                    div()
                        .absolute()
                        .left(px(1.))
                        .top(px(13.))
                        .w(px(4.))
                        .h(px(8.))
                        .rounded_r(px(4.))
                        .bg(color("--interactive-text-active")),
                )
            })
            .child(
                self.group_stop_hollow(
                    div().id(SharedString::from(format!("channel-row-{}", channel.id))),
                    "channels",
                    keyboard::Nav::List,
                    &format!("channel-row-{}", channel.id),
                    selected,
                    8.,
                    cx,
                )
                // The harness reads this box, and Shift+F10 puts the channel's menu at its centre.
                .relative()
                .child(self.anchor_probe(format!("capture-channel-{}", channel.id)))
                .cursor_pointer()
                // `.wrapper_TNyK-N.modeSelected_TNyK-N{cursor:default}` (f4b2f93f8f92b7).
                .when(selected, |this| this.cursor_default())
                .on_mouse_up(
                    MouseButton::Right,
                    cx.listener(move |this, event: &gpui::MouseUpEvent, _, cx| {
                        this.open_context_menu(
                            context_menu::Target::Channel(target),
                            event.position,
                            cx,
                        );
                        cx.stop_propagation();
                    }),
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    if voice {
                        // The clicked row keeps its action icons until a press lands elsewhere (`:focus-within`).
                        this.vv.row_focus = true;
                        if let Some(guild) = this.guild {
                            this.request_join_voice(guild, target, cx);
                        }
                    } else {
                        let guild = this.guild;
                        if let Some(guild) = guild {
                            this.select(guild, Some(target));
                            this.request_images();
                        }
                        cx.notify();
                    }
                }))
                .group(group)
                .h(px(if two_line {
                    48.
                } else {
                    32. + self.name_line_extra()
                }))
                .w(px(self.sidebar_row_width()))
                .pl(px(8.))
                .pr(px(8.))
                .rounded(px(8.))
                .when(two_line, |this| this.pt(px(4.)))
                .flex()
                .flex_col()
                // `.modeSelected .link`, `.wrapper:hover .link` and `.wrapper:active .link`: the pressed look wins over
                // the selected one (f4b2f93f8f92b7).
                .when(selected, |this| {
                    this.parity("channel-selected")
                        .bg(color("--interactive-background-selected"))
                })
                .when(!selected, |this| {
                    this.hover(|style| style.bg(color("--interactive-background-hover")))
                })
                .active(|style| style.bg(color("--interactive-background-active")))
                .child(
                    div()
                        .w_full()
                        .h(px(if two_line {
                            24.
                        } else {
                            32. + self.name_line_extra()
                        }))
                        .flex_none()
                        .relative()
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .child(glyph)
                        .child(
                            div()
                                .flex_1()
                                .when(dim, |this| this.opacity(0.5))
                                .text_size(px(16.))
                                .line_height(px(20.))
                                .font_weight(FontWeight(500.))
                                .text_color(color(tint))
                                .group_hover(name_group, |style| {
                                    style.text_color(color("--text-strong"))
                                })
                                .child(channel.name.clone()),
                        )
                        // Not in the pinned client: TypingIndicator (`plugin_hooks/typing_indicator.rs`).
                        .children(self.typing_indicator(channel.id, self.guild, cx))
                        // `channelInfo_Wx6SnQ` with the primary badge (module 616740 `isNewChannel` or mention count): right edge of the
                        // row's content, hidden while the row is hovered for the action icons (stage-ann/ann-view).
                        .when(mentions > 0, |row| {
                            row.child(
                                div()
                                    .absolute()
                                    .right(px(0.))
                                    .when(!selected, |badge| {
                                        badge.group_hover(pill_group.clone(), |style| {
                                            style.opacity(0.)
                                        })
                                    })
                                    .when(selected, |badge| badge.right(px(40.)))
                                    .child(unread_badges::number(
                                        mentions,
                                        "--background-feedback-notification",
                                        None,
                                    )),
                            )
                        })
                        .when(mentions == 0 && self.is_new_channel(channel), |row| {
                            row.child(
                                div()
                                    .absolute()
                                    .right(px(0.))
                                    .h(px(16.))
                                    .px(px(8.))
                                    .rounded(px(8.))
                                    .bg(color("--brand-500"))
                                    .text_size(px(12.))
                                    .line_height(px(16.))
                                    .font_weight(FontWeight(600.))
                                    .text_color(color("--white"))
                                    .group_hover(pill_group, |style| style.opacity(0.))
                                    .child(dc_intl::tr("NEW")),
                            )
                        })
                        .children(timer)
                        .child(
                            div()
                                .flex()
                                .gap(px(4.))
                                .when(!selected || in_call || voice || in_stage, |this| {
                                    if (in_call && self.vv.row_focus) || in_stage {
                                        this
                                    } else {
                                        this.opacity(0.)
                                            .group_hover(action_group, |style| style.opacity(1.))
                                    }
                                })
                                .children(
                                    actions
                                        .iter()
                                        .filter(|name| {
                                            // The Favorites list shows the invite icon alone (frame menus2/favorites-view).
                                            if **name == "settings_small" && self.in_favorites() {
                                                return false;
                                            }
                                            // The gear needs Manage Channels, the invite Create Invite (`can` in module 336541).
                                            let needed = match **name {
                                                "settings_small" | "voice_action_3" => {
                                                    dc_state::permissions::MANAGE_CHANNELS
                                                }
                                                "invite_small" | "voice_action_2" => {
                                                    dc_state::permissions::CREATE_INSTANT_INVITE
                                                }
                                                _ => 0,
                                            };
                                            needed == 0 || self.can_in_channel(channel, needed)
                                        })
                                        .map(|name| {
                                            // "Create Invite" opens the invite dialog for this channel; the gear opens its
                                            // settings.
                                            let invite = *name == "invite_small";
                                            // A voice row's third icon is the same gear (`voice_action_3`).
                                            let gear = matches!(
                                                *name,
                                                "settings_small" | "voice_action_3"
                                            );
                                            div()
                                                .id(SharedString::from(format!(
                                                    "channel-action-{target}-{name}"
                                                )))
                                                .when(gear, |this| {
                                                    this.cursor_pointer().on_click(cx.listener(
                                                        move |this, _, window, cx| {
                                                            cx.stop_propagation();
                                                            this.open_channel_settings(
                                                                target, window, cx,
                                                            );
                                                        },
                                                    ))
                                                })
                                                .when(invite, |this| {
                                                    this.cursor_pointer().on_click(cx.listener(
                                                        move |this, _, _, cx| {
                                                            cx.stop_propagation();
                                                            if let Some(guild) = this.guild {
                                                                this.select(guild, Some(target));
                                                            }
                                                            this.open_invite(cx);
                                                        },
                                                    ))
                                                })
                                                .child(icon(name, 16., action_tint))
                                                .into_any_element()
                                        }),
                                ),
                        ),
                )
                .children(
                    two_line
                        .then(|| self.voice_status_line(channel, cx))
                        .flatten(),
                ),
            )
            // The voice channels tip (`tutorial_tips`).
            .children(self.tutorial_channel_tip(channel, cx))
            .into_any_element()
    }

    fn sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let guild = self.current_guild();
        let name = guild.map(|guild| guild.name.clone()).unwrap_or_default();
        // A channel-like row: `.wrapper:hover .link`, `:active .link` and the hovered name colour (f4b2f93f8f92b7).
        let plain_row = |icon_name: &str, label: &str| {
            let group = SharedString::from(format!("plain-row-{icon_name}"));
            div().flex_none().h(px(34.)).pt(px(1.)).pl(px(8.)).child(
                div()
                    .id(group.clone())
                    .group(group.clone())
                    .h(px(32.))
                    .w(px(self.sidebar_row_width()))
                    .pl(px(8.))
                    .rounded(px(8.))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .hover(|style| style.bg(color("--interactive-background-hover")))
                    .active(|style| style.bg(color("--interactive-background-active")))
                    .child(icon(icon_name, 20., "--channels-default"))
                    .child(
                        div()
                            .text_size(px(16.))
                            .line_height(px(24.))
                            .font_weight(FontWeight(500.))
                            .text_color(color("--channels-default"))
                            .group_hover(group, |style| {
                                style.text_color(color("--interactive-text-hover"))
                            })
                            .child(label.to_owned()),
                    ),
            )
        };
        self.channel_drag_begin();
        let rows_pt = if self.has_channel_browser()
            || self.current_guild().is_some_and(|guild| {
                guild
                    .sidebar_rows()
                    .first()
                    .is_some_and(|first| first.kind != ChannelKind::Category)
            }) {
            14.
        } else {
            2.
        };
        let mut sidebar_items: Vec<(f32, f32, bool, bool)> = Vec::new();
        self.tutorial_sidebar_begin();
        let rows: Vec<AnyElement> = guild
            .map(|guild| {
                let collapsed = |category: Option<Id>| {
                    category.is_some_and(|category| {
                        self.stores
                            .guild_settings
                            .channel(guild.id, category)
                            .collapsed
                    })
                };
                // Pinned channels (`FAVORITED`) leave their category for the favourites section above the rest
                // (`favoritesCategory` of module 791513; `eg` hides them in their own category).
                let mut ordered = guild.sidebar_rows();
                let pinned: Vec<&Channel> = ordered
                    .iter()
                    .copied()
                    .filter(|row| {
                        row.kind != ChannelKind::Category
                            && self.stores.guild_settings.channel(guild.id, row.id).flags & 2048
                                != 0
                    })
                    .collect();
                ordered.retain(|row| !pinned.iter().any(|known| known.id == row.id));
                ordered.splice(0..0, pinned.iter().copied());
                let collapsed = |row: &Channel| {
                    !pinned.iter().any(|known| known.id == row.id) && collapsed(row.parent_id)
                };
                let now = chrono::Utc::now();
                let mut y = rows_pt;
                let mut rows = Vec::new();
                for row in ordered
                    .into_iter()
                    .filter(|row| self.followed_sidebar_row(guild, row))
                    .filter(|row| self.view_as_role_shows(guild, row))
                    // Selected channels and mentions precede mute/collapse filtering (791513.eA).
                    .filter(|row| {
                        !self.hide_muted(guild.id)
                            || row.kind == ChannelKind::Category
                            || self.sidebar_channel_attention(guild, row)
                            || !self
                                .stores
                                .guild_settings
                                .channel_muted(guild.id, row.id, now)
                    })
                    .filter(|row| {
                        row.kind == ChannelKind::Category
                            || !collapsed(row)
                            || self.sidebar_channel_attention(guild, row)
                    })
                {
                    let line = self.channel_row(row, cx);
                    rows.push(self.channel_drag_wrap(row, line, cx));
                    if row.kind == ChannelKind::Category {
                        y += self.category_space_before() + 24.;
                    } else {
                        let h = if self.voice_row_has_line(row) {
                            50.
                        } else {
                            34. + self.name_line_extra()
                        };
                        let muted = self
                            .stores
                            .guild_settings
                            .channel_muted(guild.id, row.id, now);
                        let mentions = self.stores.read_states.mentions(row.id);
                        let unread = !muted
                            && matches!(row.kind, ChannelKind::Text | ChannelKind::Other(5))
                            && self.stores.is_unread(row.id);
                        if unread || mentions > 0 {
                            sidebar_items.push((y, y + h, unread, mentions > 0));
                        }
                        y += h;
                    }
                    if row.kind == ChannelKind::Text || row.is_forum() {
                        let thread_meta = self.sidebar_threads(row.id);
                        let rendered_threads = self.thread_rows(row.id, cx);
                        for thread in thread_meta {
                            let mentions = self.stores.read_states.mentions(thread.id);
                            let unread = !thread.is_muted(now) && self.stores.is_unread(thread.id);
                            if unread || mentions > 0 {
                                sidebar_items.push((y, y + 34., unread, mentions > 0));
                            }
                            y += 34.;
                        }
                        rows.extend(rendered_threads);
                    }
                    if row.kind == ChannelKind::Voice {
                        // Hide Names collapses the channel (`CollapsedVoiceChannelStore`): no participant rows.
                        let before = rows.len();
                        if !self.names_hidden(row.id) {
                            rows.extend(self.voice_members(guild.id, row.id, cx));
                        }
                        rows.extend(self.voice_activity_rows(row.id, cx));
                        rows.extend(self.voice_invite_row(guild.id, row.id, cx));
                        y += 32. * (rows.len().saturating_sub(before) as f32);
                    }
                }
                rows
            })
            .unwrap_or_default();
        let view_h = f32::from(self.sidebar_scroll.bounds().size.height);
        let max_scroll = f32::from(self.sidebar_scroll.max_offset().y);
        let (top_bar, bottom_bar) = if view_h > 1. && max_scroll > 0.5 {
            let scroll_y = (-f32::from(self.sidebar_scroll.offset().y)).clamp(0., max_scroll);
            let mut top_mentions = false;
            let mut top_unread = false;
            let mut top_target: Option<f32> = None;
            let mut bottom_mentions = false;
            let mut bottom_unread = false;
            let mut bottom_target: Option<f32> = None;
            for &(row_top, row_bottom, unread, mentioned) in &sidebar_items {
                if row_bottom <= scroll_y + 2. {
                    if mentioned && !top_mentions {
                        top_mentions = true;
                        top_target = Some((row_top - 8.).max(0.));
                    } else if unread && !top_mentions && top_target.is_none() {
                        top_unread = true;
                        top_target = Some((row_top - 8.).max(0.));
                    }
                } else if row_top >= scroll_y + view_h - 2. {
                    let target = (row_bottom - view_h + 16.).clamp(0., max_scroll);
                    if mentioned && !bottom_mentions {
                        bottom_mentions = true;
                        bottom_target = Some(target);
                    } else if unread && !bottom_mentions {
                        bottom_unread = true;
                        bottom_target = Some(target);
                    }
                }
            }
            (
                (top_mentions || top_unread)
                    .then(|| (top_mentions, top_target.unwrap_or_default())),
                (bottom_mentions || bottom_unread)
                    .then(|| (bottom_mentions, bottom_target.unwrap_or(max_scroll))),
            )
        } else {
            (None, None)
        };
        div()
            .parity("sidebar")
            .relative()
            .flex_none()
            .w(px(self.sidebar_width()))
            .h_full()
            .flex()
            .flex_col()
            .child(
                div()
                    .absolute()
                    .inset_0()
                    .border_l_1()
                    .border_t_1()
                    .rounded_tl(px(12.))
                    .border_color(color("--background-mod-subtle")),
            )
            .child(div().flex_none().h(px(1.)))
            .child(
                div()
                    .parity_below("guild-header", 1.)
                    .flex_none()
                    .h(px(49.))
                    .border_b_1()
                    .border_color(color("--background-mod-subtle"))
                    .px(px(8.))
                    .pt(px(8.))
                    .child(
                        div()
                            .h(px(32.))
                            .flex()
                            .items_center()
                            .child(
                                // `.guildDropdown_XH4OpX`: 32px tall, 8px radius and inline padding, `gap: 2px`, and
                                // `--background-mod-subtle` while hovered, with no transition.
                                div()
                                    .id("guild-name")
                                    // `.headerContent_XH4OpX` `gap: 4px` between the dropdown and the invite button, which
                                    // a long name closes up to (boost/boosted-guild-dropdown: chevron right edge 323.7).
                                    .mr(px(4.))
                                    .flex()
                                    .items_center()
                                    // A long name is cut with an ellipsis before the chevron; the invite button stays whole.
                                    .min_w(px(0.))
                                    .h(px(32.))
                                    .px(px(8.))
                                    .rounded(px(8.))
                                    .hover(|style| style.bg(color("--background-mod-subtle")))
                                    .cursor_pointer()
                                    .on_click(
                                        cx.listener(|this, _, _, cx| this.toggle_guild_menu(cx)),
                                    )
                                    .children(guild.and_then(tooltip::badge_image).map(|badge| {
                                        img(badge).ml(px(1.)).mr(px(9.)).size(px(18.)).flex_none()
                                    }))
                                    .child(
                                        div()
                                            .min_w(px(0.))
                                            .truncate()
                                            .text_size(px(16.))
                                            .line_height(px(20.))
                                            .font_weight(FontWeight(600.))
                                            .text_color(color("--text-strong"))
                                            .child(name),
                                    )
                                    // Open, the chevron points up (`chevron_up` in the capture's header).
                                    .child(
                                        div().flex_none().ml(px(2.)).child(
                                            gpui::svg()
                                                .path("icons/chevron_down.svg")
                                                .size(px(16.))
                                                .text_color(color("--text-strong"))
                                                .when(self.guild_menu, |this| {
                                                    this.with_transformation(
                                                        gpui::Transformation::rotate(
                                                            gpui::radians(std::f32::consts::PI),
                                                        ),
                                                    )
                                                }),
                                        ),
                                    ),
                            )
                            .child(div().flex_1())
                            .child(
                                div()
                                    .id("guild-header-invite")
                                    .flex_none()
                                    .size(px(32.))
                                    .rounded(px(8.))
                                    // `.inviteButton_XH4OpX:hover` (fdfe0d4e5e8803).
                                    .hover(|style| style.bg(color("--background-mod-subtle")))
                                    .cursor_pointer()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .on_click(cx.listener(|this, _, _, cx| this.header_invite(cx)))
                                    .child(icon("invite", 20., "--text-strong")),
                            ),
                    ),
            )
            .children(self.channel_notice(cx))
            .children(self.live_now_card(cx))
            .child(div().flex_none().h(px(12.)))
            .children(self.guild_guide_rows(cx))
            .children(self.boost_progress_row(cx))
            .children(self.channel_browser_row(cx))
            .when(self.sidebar_has_events(), |sidebar| {
                sidebar.child(self.guild_events_row(cx))
            })
            .children(self.member_safety_row(cx))
            .when(self.sidebar_has_boosts(), |sidebar| {
                sidebar.child(
                    plain_row("boosts", dc_intl::tr("Server Boosts"))
                        .id("sidebar-boosts")
                        .cursor_pointer()
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.open_settings_section(dc_intl::tr("Server Boost"), cx);
                        })),
                )
            })
            .child(
                div()
                    .flex_none()
                    .mt(px(11.))
                    .ml(px(16.))
                    .w(px(271.))
                    .h(px(1.))
                    .bg(color("--background-mod-subtle")),
            )
            // The rows scroll under the fixed header, as the official channel list does.
            // `container_H5ozaj` pads its bottom by the panel's height + 8px - 16px and `scroller_ysO7Gy` keeps 4px of margin
            // and 16px of padding, so the list ends 54px above the window's bottom: nothing paints in the strip below
            // the panel or beside its rounded corners (`design/dom/channel/text-darker-cozy.json`).
            .child(
                div()
                    .flex_1()
                    .min_h(px(0.))
                    .relative()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .id("sidebar-rows")
                            .track_scroll(&self.sidebar_scroll)
                            // 2px of room above the list for the drop line of its first row (`top: -2px`), taken back
                            // from the margin so nothing moves.
                            .mt(px(-2.))
                            // 12px more above uncategorized channels (stage-ann/ann-view: divider at 207, spacer 208 to 220,
                            // first row at 220).
                            .pt(px(rows_pt))
                            .flex_1()
                            .min_h(px(0.))
                            .mb(px(4.))
                            .flex()
                            .flex_col()
                            .overflow_y_scroll()
                            .on_drag_move::<channel_drag::ChannelDrag>(cx.listener(
                                |this,
                                 event: &gpui::DragMoveEvent<channel_drag::ChannelDrag>,
                                 _,
                                 cx| {
                                    this.channel_drag_moved(event, cx)
                                },
                            ))
                            .on_drop(cx.listener(
                                |this, drag: &channel_drag::ChannelDrag, _, cx| {
                                    this.channel_dropped(drag, cx)
                                },
                            ))
                            .children(rows)
                            .child(div().flex_none().h(px(16.))),
                    )
                    .children(top_bar.map(|(mentions, target)| {
                        self.sidebar_unread_pill("sidebar-unread-top", mentions, target, true, cx)
                    }))
                    .children(bottom_bar.map(|(mentions, target)| {
                        self.sidebar_unread_pill(
                            "sidebar-unread-bottom",
                            mentions,
                            target,
                            false,
                            cx,
                        )
                    })),
            )
            .child(
                div()
                    .flex_none()
                    .h(self.user_panel_height.unwrap_or(px(58.)) - px(8.)),
            )
            // `sidebar_pog62O::after`: 8px of `--background-base-lowest` along the bottom, from the left edge to 1px
            // short of the right, above the list's border; `.custom-theme-background .sidebar_pog62O:after{display:none}`.
            .children(settings::client_theme::active().is_none().then(|| {
                div()
                    .absolute()
                    .left_0()
                    .right(px(1.))
                    .bottom_0()
                    .h(px(8.))
                    .bg(color("--background-base-lowest"))
            }))
    }

    fn sidebar_unread_pill(
        &self,
        id: &'static str,
        mentions: bool,
        target: f32,
        top: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        div()
            .absolute()
            .when(top, |d| d.top(px(8.)))
            .when(!top, |d| d.bottom(px(8.)))
            .left_0()
            .right_0()
            .flex()
            .justify_center()
            .child(
                div()
                    .id(id)
                    .h(px(24.))
                    .px(px(12.))
                    .rounded_full()
                    .bg(color(if mentions {
                        "--background-feedback-notification"
                    } else {
                        "--background-surface-highest"
                    }))
                    .when(!mentions, |pill| {
                        pill.border_1().border_color(color("--border-subtle"))
                    })
                    .cursor_pointer()
                    .flex()
                    .items_center()
                    .gap(px(4.))
                    .text_size(px(12.))
                    .line_height(px(16.))
                    .font_weight(FontWeight(600.))
                    .text_color(color(if mentions { "--white" } else { "--text-strong" }))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.sidebar_scroll
                            .set_offset(gpui::point(px(0.), px(-target)));
                        cx.notify();
                    }))
                    .child(if mentions {
                        dc_intl::tr("New Mentions")
                    } else {
                        dc_intl::tr("New Unreads")
                    }),
            )
            .into_any_element()
    }

    fn user_panel(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let user = self.stores.current_user.clone().unwrap_or_default();
        let avatar = self.images.get(&avatar_source(&user).0).cloned();
        let in_voice = self.call.as_ref().is_some_and(|call| call.connected);
        // `9FaEzi` "In a call" with the phone glyph while the call is a DM's.
        let dm_call = self.call.as_ref().is_some_and(|call| call.guild == 0);
        // `QygGCN` "In a stage" while the call is a Stage's (module 342312).
        let stage_call = self.call.as_ref().is_some_and(|call| {
            self.stores
                .guild(call.guild)
                .and_then(|guild| {
                    guild
                        .channels
                        .iter()
                        .find(|channel| channel.id == call.channel)
                })
                .is_some_and(|channel| channel.kind == ChannelKind::Other(13))
        });
        // Muted and deafened buttons turn red with their chevron: `redGlow_Y-8oZW`.
        let tint = |muted: bool| {
            if muted {
                "--icon-voice-muted"
            } else {
                "--interactive-text-default"
            }
        };
        // `button_Y-8oZW.enabled:hover`: `--control-secondary-background-hover` and the hovered text colour, over
        // `background-color .2s ease` (718878).
        let button = |name: &str| {
            div()
                .id("settings-button")
                .group("settings-button")
                .size(px(32.))
                .rounded(px(8.))
                .flex()
                .items_center()
                .justify_center()
                .hover(|style| style.bg(color("--control-secondary-background-hover")))
                .transition(
                    motion::Props::BACKGROUND,
                    motion::css(200., motion::Ease::Ease),
                )
                .child(
                    icon(name, 20., "--interactive-text-default")
                        .group_hover("settings-button", |style| {
                            style.text_color(color("--interactive-text-hover"))
                        }),
                )
        };
        // The mic and headphones buttons sit in an `audioButtonParent_gKpIZP`: while the pointer is anywhere over the
        // pair both take `--background-mod-muted`, the one under it `--interactive-background-selected` and the hovered
        // text colour; when muted (`redGlow_Y-8oZW`) the hovered one is `--opacity-red-24` instead.
        let audio_look = |this: gpui::Stateful<gpui::Div>, muted: bool, parent: &'static str| {
            if muted {
                this.bg(color("--background-voice-muted"))
                    .hover(|style| style.bg(color("--opacity-red-24")))
            } else {
                this.group_hover(parent, |style| style.bg(color("--background-mod-muted")))
                    .hover(|style| style.bg(color("--interactive-background-selected")))
            }
            .transition(
                motion::Props::BACKGROUND,
                motion::css(200., motion::Ease::Ease),
            )
        };
        // Module 993382 draws `useMicrophoneLottie(muted ? "unmute" : "mute")` and
        // `useDeafenLottie(deafened ? "undeafen" : "deafen")` (348858), the hover events on the button.
        let ptt_indicator = self.call.is_some() && self.kb_push_to_talk();
        let self_speaking = self
            .stores
            .current_user
            .as_ref()
            .is_some_and(|me| self.speaking.contains(&me.id));
        let toggle =
            |anim: lottie_icon::Anim, state: &'static str, muted: bool, parent: &'static str| {
                let lottie = format!("user-panel-{parent}");
                let hovered = lottie_icon::hovered(&lottie);
                audio_look(
                    div().id(SharedString::from(format!("{parent}-button"))),
                    muted,
                    parent,
                )
                .size(px(32.))
                .relative()
                .rounded_l(px(8.))
                .flex()
                .items_center()
                .justify_center()
                .child(lottie_icon::icon_with_panel_mask(
                    &lottie,
                    anim,
                    Some(state),
                    20.,
                    color(match (muted, hovered) {
                        (true, _) => "--icon-voice-muted",
                        (false, true) => "--interactive-text-hover",
                        (false, false) => "--interactive-text-default",
                    }),
                    anim == lottie_icon::Anim::Microphone && ptt_indicator,
                ))
                .when(
                    anim == lottie_icon::Anim::Microphone && ptt_indicator,
                    |button| {
                        button.child(
                            div()
                                .parity("user-panel-ptt-indicator")
                                .absolute()
                                .bottom(px(7.))
                                .right(px(7.))
                                .size(px(8.))
                                .rounded_full()
                                .border_2()
                                .border_color(color(if self_speaking {
                                    "--status-positive"
                                } else {
                                    "--status-danger"
                                })),
                        )
                    },
                )
            };
        let chevron = |muted: bool, side: audio_menu::Side| {
            let (probe, parent) = match side {
                audio_menu::Side::Input => ("mic-chevron", "mic-pair"),
                audio_menu::Side::Output => ("headphones-chevron", "headphones-pair"),
            };
            audio_look(div().id(probe), muted, parent)
                .relative()
                .cursor_pointer()
                .child(self.anchor_probe(probe))
                .on_click(cx.listener(move |this, _, _, cx| this.toggle_audio_menu(side, cx)))
                .w(px(16.))
                .h(px(32.))
                .ml(px(1.))
                .rounded_r(px(8.))
                .flex()
                .items_center()
                .justify_center()
                .child(icon("audio_chevron", 16., tint(muted)))
        };
        let mic_muted = self.self_mute || self.self_deaf;
        let mic_chevron = chevron(mic_muted, audio_menu::Side::Input);
        let headphones_chevron = chevron(self.self_deaf, audio_menu::Side::Output);
        // Not in the pinned client: GameActivityToggle's button before the microphone
        // (`plugin_hooks/game_activity_toggle.rs`).
        let game_toggle = self.game_activity_toggle(cx);
        let extra = if game_toggle.is_some() { 40. } else { 0. };
        let row = div()
            .relative()
            .h(px(56.))
            .flex()
            .items_center()
            .pl(px(11.))
            .pr(px(12.))
            // `.accountPopoutButtonWrapper_g6VwXp:hover`: the avatar and the name tag share one 184x42 hover box with
            // `--background-mod-normal` and 4px corners (718878; the avatar's own `:hover` opacity is cancelled there).
            .child(
                div()
                    .id("panel-who-hover")
                    .absolute()
                    .left(px(7.))
                    .right(px(166. + extra))
                    .top(px(6.))
                    .h(px(42.))
                    .rounded(px(4.))
                    .hover(|style| style.bg(color("--background-mod-normal"))),
            )
            .child({
                let in_call = self.call.is_some();
                let soundboard_ring = in_call && self.soundboard.user_playing(user.id);
                let voice_ring = in_call
                    && !self.self_mute
                    && !self.self_deaf
                    && self.speaking.contains(&user.id);
                div()
                    .id("panel-avatar")
                    .relative()
                    .flex_none()
                    .size(px(32.))
                    .cursor_pointer()
                    // Not in the pinned client: AccountPanelServerProfile can open the server profile here, and
                    // the panel takes a right click (`plugin_hooks/account_panel.rs`).
                    .on_click(cx.listener(|this, _, _, cx| this.account_panel_clicked(cx)))
                    .when(self.account_panel_menu_on(), |this| {
                        this.child(self.anchor_probe(plugin_hooks::ACCOUNT_PANEL_ANCHOR))
                            .on_mouse_down(
                                gpui::MouseButton::Right,
                                cx.listener(|this, event: &gpui::MouseDownEvent, _, cx| {
                                    this.open_context_menu(
                                        context_menu::Target::AccountPanel,
                                        event.position,
                                        cx,
                                    );
                                }),
                            )
                    })
                    .on_hover(self.avatar_hover("panel-avatar".to_owned(), cx))
                    .children(
                        self.live_avatar(&user, 32., self.avatar_hovered("panel-avatar"), cx)
                            .or_else(|| {
                                avatar.map(|image| {
                                    img(image).size(px(32.)).rounded_full().into_any_element()
                                })
                            }),
                    )
                    .when(soundboard_ring || voice_ring, |this| {
                        this.child(
                            div()
                                .absolute()
                                .inset_0()
                                .child(
                                    div()
                                        .absolute()
                                        .inset_0()
                                        .rounded_full()
                                        .border(px(3.))
                                        .border_color(color("--background-base-lower")),
                                )
                                .child(
                                    div()
                                        .absolute()
                                        .inset_0()
                                        .rounded_full()
                                        .border(px(2.))
                                        .border_color(color(if soundboard_ring {
                                            "--brand-500"
                                        } else {
                                            "--status-speaking"
                                        })),
                                ),
                        )
                    })
                    .child(status_dot(&self.stores.status, "--background-base-low"))
            })
            .child(
                div()
                    .id("panel-names")
                    .ml(px(8.))
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .cursor_pointer()
                    .on_click(cx.listener(|this, _, _, cx| this.account_panel_clicked(cx)))
                    .when(self.account_panel_menu_on(), |this| {
                        this.on_mouse_down(
                            gpui::MouseButton::Right,
                            cx.listener(|this, event: &gpui::MouseDownEvent, _, cx| {
                                this.open_context_menu(
                                    context_menu::Target::AccountPanel,
                                    event.position,
                                    cx,
                                );
                            }),
                        )
                    })
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .min_w_0()
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .text_size(px(16.))
                            .line_height(px(18.))
                            .font_weight(FontWeight(500.))
                            .text_color(color("--text-strong"))
                            .child(name_styles::name(
                                "panel-name".to_owned(),
                                user.display_name().to_owned(),
                                self.name_styles(&user, None).as_ref(),
                                name_styles::Display::StaticOnHover,
                            )),
                    )
                    .child(
                        div()
                            .min_w_0()
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .mt(px(2.))
                            .text_size(px(12.))
                            .line_height(px(13.))
                            .font_weight(FontWeight(500.))
                            .text_color(color("--interactive-text-default"))
                            // `activityStatus`: in voice, this replaces the status line.
                            // Out of voice it is the account's activity or custom status, or else its status.
                            .when(!in_voice, |this| {
                                match self.presence_line_with(
                                    &user,
                                    None,
                                    true,
                                    13.,
                                    "--interactive-text-default",
                                ) {
                                    Some(line) => this.child(line),
                                    None => this.child(status_label(&self.stores.status)),
                                }
                            })
                            .when(in_voice, |this| {
                                let sharing = self.own_stream().is_some();
                                this.flex()
                                    .items_center()
                                    .gap(px(4.))
                                    .child(icon(
                                        if sharing {
                                            "voice_status_sharing"
                                        } else if dm_call {
                                            "status_in_call"
                                        } else if stage_call {
                                            "stage_channel"
                                        } else {
                                            "voice_status_in_voice"
                                        },
                                        12.,
                                        "--text-feedback-positive",
                                    ))
                                    .child(dc_intl::tr(if sharing {
                                        "Sharing their screen"
                                    } else if dm_call {
                                        "In a call"
                                    } else if stage_call {
                                        "In a stage"
                                    } else {
                                        "In voice"
                                    }))
                            }),
                    ),
            )
            .children(game_toggle)
            .child(
                div()
                    .id("mic-pair")
                    .group("mic-pair")
                    .ml(px(if extra > 0. { 8. } else { 12. }))
                    .flex_none()
                    .flex()
                    .items_center()
                    .child(
                        div()
                            .id("toggle-mute")
                            .cursor_pointer()
                            .on_click(cx.listener(|this, _, _, cx| this.toggle_self_mute(cx)))
                            .child(
                                self.with_tip(
                                    "mic",
                                    Tip::text(
                                        dc_intl::tr(if mic_muted {
                                            dc_intl::tr("Unmute")
                                        } else {
                                            dc_intl::tr("Mute")
                                        }),
                                        Side::Top,
                                    ),
                                    toggle(
                                        lottie_icon::Anim::Microphone,
                                        if mic_muted { "unmute" } else { "mute" },
                                        mic_muted,
                                        "mic-pair",
                                    )
                                    .on_hover(
                                        voice_view::lottie_hover(
                                            "user-panel-mic-pair".to_owned(),
                                            cx,
                                        ),
                                    ),
                                    cx,
                                ),
                            ),
                    )
                    .child(mic_chevron),
            )
            .child(
                div()
                    .id("headphones-pair")
                    .group("headphones-pair")
                    .ml(px(8.))
                    .flex_none()
                    .flex()
                    .items_center()
                    .child(
                        div()
                            .id("toggle-deafen")
                            .cursor_pointer()
                            .on_click(cx.listener(|this, _, _, cx| this.toggle_self_deaf(cx)))
                            .child(
                                self.with_tip(
                                    "deafen",
                                    Tip::text(
                                        dc_intl::tr(if self.self_deaf {
                                            "Undeafen"
                                        } else {
                                            "Deafen"
                                        }),
                                        Side::Top,
                                    ),
                                    toggle(
                                        lottie_icon::Anim::Deafen,
                                        if self.self_deaf { "undeafen" } else { "deafen" },
                                        self.self_deaf,
                                        "headphones-pair",
                                    )
                                    .on_hover(
                                        voice_view::lottie_hover(
                                            "user-panel-headphones-pair".to_owned(),
                                            cx,
                                        ),
                                    ),
                                    cx,
                                ),
                            ),
                    )
                    .child(headphones_chevron),
            )
            .child(
                div()
                    .id("open-settings")
                    .ml(px(8.))
                    .flex_none()
                    .cursor_pointer()
                    .on_click(cx.listener(|this, _, _, cx| this.open_settings(cx)))
                    .child(self.with_tip(
                        "user-settings",
                        Tip::text(dc_intl::tr("User Settings"), Side::Top),
                        button("settings"),
                        cx,
                    )),
            );
        let measured = self.user_panel_height;
        let root = self.weak.clone();
        let measure = gpui::canvas(
            move |bounds, _, cx| {
                let height = bounds.size.height;
                if measured != Some(height) {
                    cx.defer(move |cx| {
                        let _ = root.update(cx, |this, cx| {
                            if this.user_panel_height != Some(height) {
                                this.user_panel_height = Some(height);
                                cx.notify();
                            }
                        });
                    });
                }
            },
            |_, (), _, _| {},
        )
        .absolute()
        .top(px(-1.))
        .right(px(-1.))
        .bottom(px(-1.))
        .left(px(-1.));
        // `panels_pog62O`: one bordered box; while in voice the call panel sits above the user row.
        div()
            .parity_with_border("user-panel", 1.)
            .absolute()
            // The channel list runs behind this box: presses here must not reach its rows.
            .occlude()
            .left(px(8.))
            .bottom(px(8.))
            .w(px(self.combined_sidebar_width() - 16.))
            .rounded(px(8.))
            .bg(color("--background-base-low"))
            .border_1()
            .border_color(color(settings::client_theme::panels_border()))
            .flex()
            .flex_col()
            .children(settings::client_theme::panels_cover())
            .child(measure)
            .children(self.activity_panel(cx))
            .children(self.clips_panels(cx))
            .children(self.voice_panel(cx))
            .child(row)
    }

    fn header(&self, cx: &mut Context<Self>) -> impl IntoElement {
        self.header_with(cx, false)
    }

    /// The channel header. `in_call_area` draws it inside a DM's call area (module 540462 `useExternalHeader`, theme
    /// dark): no fill and no bottom border (`transparent_ybUK7Z`), the profile button dimmed.
    pub(super) fn header_with(
        &self,
        cx: &mut Context<Self>,
        in_call_area: bool,
    ) -> impl IntoElement {
        let name = self.channel_title();
        // The forum of an open post: its name and icon for the breadcrumb.
        let post_forum: Option<(String, &'static str)> = self
            .open_post_forum()
            .and_then(|forum| self.forum_channel(forum))
            .map(|forum| {
                let facts = channel_icon::Facts {
                    locked: false,
                    ..self.channel_icon_facts(forum, false)
                };
                (forum.name.clone(), channel_icon::name(&facts))
            });
        let dm_channel = self.current_dm().map(|channel| channel.id);
        let dm_user = self
            .current_dm()
            .and_then(|channel| self.stores.dm_recipient(channel))
            .cloned();
        let group_channel = self
            .current_dm()
            .filter(|channel| channel.kind == 3)
            .cloned();
        // 849205: a system DM's title has no avatar; 289287 `renderHeaderToolbar` gives it no toolbar at all.
        let system_dm = self
            .current_dm()
            .is_some_and(|channel| self.stores.is_system_dm(channel));
        // Module 187360: the bell is slashed while the channel is muted or its resolved level is not All Messages.
        let muted = self.channel_bell_slashed();
        // Module 586944: "Search in DMs" (`m7OrlR`) in the every-DM context, else "Search {name}" (`LDZtFO`) with the
        // server's or the conversation's name (the key is the same for both; its Turkish reads "{name} sunucusunu ara").
        let search = if self.search_all_dms() {
            dc_intl::t!("m7OrlR").to_owned()
        } else {
            dc_intl::t!(
                "LDZtFO",
                name = match &dm_user {
                    Some(user) => user.username.as_str(),
                    None => self
                        .current_guild()
                        .map(|guild| guild.name.as_str())
                        .unwrap_or_default(),
                }
            )
        };
        // Header icons brighten to `--interactive-text-default` under the pointer.
        // A button whose popout is open shows in `--interactive-text-active`.
        let open_popout = |name: &str| match name {
            "bell" | "bell_muted" => self.notify_open,
            "pin" => self.pins_open.is_some(),
            "threads" => self.thread_browser.is_some(),
            _ => false,
        };
        // `hasUnreadPins`: the pin button carries the `unread` badge at the bottom (module 287074 -> 58736): an 8px
        // `--background-brand` dot 2px past the corner of the 20px icon, which is cut away in a 5px-radius circle
        // around the dot's centre (`svg-mask-header-bar-badge-bottom`). The cut shows the header's own fill.
        let pins_unread = self
            .channel
            .is_some_and(|channel| self.stores.has_unread_pins(channel));
        let header_fill = color("--background-base-lower");
        let tool = |name: &str| {
            let group = SharedString::from(format!("header-{name}"));
            let active = open_popout(name);
            let badge = name == "pin" && pins_unread;
            div()
                .group(group.clone())
                .relative()
                .size(px(32.))
                .flex()
                .items_center()
                .justify_center()
                .child(
                    svg()
                        .path(format!("icons/{name}.svg"))
                        .size(px(20.))
                        .flex_none()
                        .text_color(if active {
                            color("--interactive-text-active")
                        } else {
                            color("--text-muted")
                        })
                        .group_hover(group, |style| {
                            style.text_color(color("--interactive-text-default"))
                        }),
                )
                .when(badge, |this| {
                    this.child(
                        div()
                            .absolute()
                            .left(px(16.))
                            .top(px(16.))
                            .size(px(10.))
                            .rounded_full()
                            .bg(header_fill),
                    )
                    .child(
                        div()
                            .parity("pins-unread-dot")
                            .absolute()
                            .left(px(17.))
                            .top(px(17.))
                            .size(px(8.))
                            .rounded_full()
                            .bg(color("--background-brand")),
                    )
                })
        };
        // `children_ybUK7Z`: a server channel's icon and title are one clipped box (chat-200 dump: 10 device px wide
        // after the search box has shrunk to its 64px minimum, the icon cut to a sliver and the title gone).
        let one_box = self.guild.is_some()
            && post_forum.is_none()
            && dm_user.is_none()
            && group_channel.is_none();
        div()
            .parity_below("chat-header", 1.)
            .flex_none()
            .h(px(49.))
            .when(!in_call_area, |this| {
                this.border_b_1()
                    .border_color(color("--background-mod-subtle"))
                    .bg(color("--background-base-lower"))
            })
            .pl(px(if in_call_area { 16. } else { 15. }))
            .pr(px(8.))
            .flex()
            .items_center()
            .children(self.favorites_breadcrumb())
            .when(self.guild.is_some(), |this| {
                // 353428: the title's icon is the channel list's own (`gU(channel, guild)`), never `locked`.
                let name = match self.current_channel() {
                    Some(channel) => {
                        let rules = self
                            .current_guild()
                            .and_then(|guild| guild.rules_channel_id)
                            == Some(channel.id);
                        channel_icon::name(&channel_icon::Facts {
                            locked: false,
                            ..self.channel_icon_facts(channel, rules)
                        })
                    }
                    // A forum post: the forum's own icon, its name and the post's title follow it.
                    None if post_forum.is_some() => {
                        post_forum.as_ref().map_or("header_hash", |f| f.1)
                    }
                    // A thread: its own title row draws the rest.
                    None => "header_hash",
                };
                // The icon sits inside the title wrapper of the official header, so it clips with the title.
                let mut lead = div();
                lead.style().flex_shrink = Some(0.001);
                let lead = lead.min_w(px(0.)).overflow_hidden().child(icon(
                    name,
                    20.,
                    "--channels-default",
                ));
                if one_box {
                    // Icon, 9px, then the nowrap title, all inside the box that clips (`children_ybUK7Z`).
                    let mut boxed = div();
                    boxed.style().flex_shrink = Some(0.001);
                    return this.child(
                        boxed
                            // The toolbar's own 8px of padding stays when this box has shrunk (chat-200 dump).
                            .mr(px(8.))
                            .min_w(px(0.))
                            .overflow_hidden()
                            .flex()
                            .items_center()
                            .child(
                                div()
                                    .flex_none()
                                    .child(icon(name, 20., "--channels-default")),
                            )
                            .child(
                                div()
                                    .flex_none()
                                    .ml(px(9.))
                                    .whitespace_nowrap()
                                    .text_size(px(16.))
                                    .line_height(px(20.))
                                    .font_weight(FontWeight(500.))
                                    .text_color(color("--text-strong"))
                                    .child(
                                        name_styles::name(
                                            "dm-title",
                                            self.channel_title(),
                                            None,
                                            name_styles::Display::FontOnHover,
                                        )
                                        .bold_opacity(0.9),
                                    ),
                            ),
                    );
                }
                this.child(lead)
            })
            // `.breadcrumb_z_gUBg` of a forum post (`forum-fixture-posted`): the forum's name, then a chevron.
            .when_some(post_forum.clone(), |this, (forum_name, _)| {
                this.child(
                    div()
                        .ml(px(9.))
                        .text_size(px(16.))
                        .line_height(px(20.))
                        .font_weight(FontWeight(500.))
                        .text_color(color("--text-strong"))
                        .child(forum_name),
                )
                .child(
                    div().ml(px(7.)).child(
                        icon("chevron_down", 12., "--text-muted").with_transformation(
                            gpui::Transformation::rotate(gpui::percentage(0.75)),
                        ),
                    ),
                )
            })
            .when_some(dm_user.clone().filter(|_| !system_dm), |this, user| {
                // `avatar_z_gUBg`: 20px with the status cut into its corner (`svg-mask-avatar-status-round-20`).
                let presence = self.stores.status(user.id).to_owned();
                let picture = self.images.get(&avatar_source(&user).0).cloned();
                this.child(
                    div()
                        .ml(px(1.))
                        .relative()
                        .size(px(20.))
                        .when_some(dm_channel, |d, channel| {
                            d.on_mouse_up(
                                MouseButton::Right,
                                cx.listener(move |this, event: &gpui::MouseUpEvent, _, cx| {
                                    cx.stop_propagation();
                                    this.open_context_menu(
                                        context_menu::Target::Dm(channel),
                                        event.position,
                                        cx,
                                    );
                                }),
                            )
                        })
                        .children(picture.map(|picture| img(picture).size(px(20.)).rounded_full()))
                        .child(status_dot_small(&presence, "--background-base-lower")),
                )
            })
            .when_some(group_channel.clone(), |this, channel| {
                let id = channel.id;
                this.child(
                    div()
                        .on_mouse_up(
                            MouseButton::Right,
                            cx.listener(move |this, event: &gpui::MouseUpEvent, _, cx| {
                                cx.stop_propagation();
                                this.open_context_menu(
                                    context_menu::Target::Dm(id),
                                    event.position,
                                    cx,
                                );
                            }),
                        )
                        .child(self.group_header_name(&channel, cx)),
                )
            })
            .when(group_channel.is_none() && !one_box, |this| {
                // At a high zoom the search box gives its width up first (down to 64px, chat-200 and chat-150
                // dumps), and only then the nowrap title clips: its shrink is near zero, so the search takes
                // the deficit until its minimum freezes it.
                let mut title = div();
                title.style().flex_shrink = Some(0.001);
                this.child(
                    title
                        // Without the avatar a system DM's title takes its place.
                        .ml(px(if system_dm {
                            1.
                        } else if dm_user.is_some() {
                            8.
                        } else {
                            9.
                        }))
                        .min_w(px(0.))
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .text_size(px(16.))
                        .line_height(px(20.))
                        .font_weight(FontWeight(500.))
                        .text_color(color("--text-strong"))
                        .when_some(dm_channel, |d, channel| {
                            d.on_mouse_up(
                                MouseButton::Right,
                                cx.listener(move |this, event: &gpui::MouseUpEvent, _, cx| {
                                    cx.stop_propagation();
                                    this.open_context_menu(
                                        context_menu::Target::Dm(channel),
                                        event.position,
                                        cx,
                                    );
                                }),
                            )
                        })
                        // 849205: a DM's header title applies only the other person's display-name font class (`73392`),
                        // never the colour/animation effect.
                        .child(name_styles::name(
                            "dm-title",
                            name,
                            dm_user
                                .as_ref()
                                .and_then(|user| self.name_styles(user, None))
                                .as_ref(),
                            name_styles::Display::Font,
                        )),
                )
            })
            // `renderFollowButton` (module 353428 `zF`): right after the title of an announcement channel.
            .children(self.follow_button(cx))
            .children(self.favorites_jump_button(cx))
            // `dmHeaderActivityStatus_z_gUBg`: 8px after the title, 4px gap, the clickable part 2px 4px with an 8px
            // radius holding the activity's glyph and text (14px / 18px medium).
            .children(
                dm_user
                    .as_ref()
                    .and_then(|user| {
                        self.presence_line_with(user, None, false, 18., "--text-default")
                    })
                    .map(|line| {
                        div().ml(px(8.)).min_w(px(0.)).flex().items_center().child(
                            div()
                                .min_w(px(0.))
                                .px(px(4.))
                                .py(px(2.))
                                .rounded(px(8.))
                                .child(line),
                        )
                    }),
            )
            .child(div().flex_1().h_full().when_some(dm_channel, |d, channel| {
                d.on_mouse_up(
                    MouseButton::Right,
                    cx.listener(move |this, event: &gpui::MouseUpEvent, _, cx| {
                        cx.stop_propagation();
                        this.open_context_menu(
                            context_menu::Target::Dm(channel),
                            event.position,
                            cx,
                        );
                    }),
                )
            }))
            .when(dm_user.is_some() && !system_dm, |this| {
                let shown = self.show_dm_profile;
                // `l2` of module 289287: the button is disabled while the call panel shows, joined or not, and in a
                // window narrower than 1132px, and then says so (`iconDisabled_ybUK7Z`, opacity .6).
                let profile_disabled = self.dm_call_shown() || self.viewport.0 < 1132.;
                this.child(
                    div()
                        .flex()
                        .gap(px(8.))
                        .children(self.dm_call_header_items(cx))
                        .children(
                            self.channel
                                .map(|channel| {
                                    self.header_call_buttons(
                                        channel,
                                        dm_user.as_ref().map(|user| user.id),
                                        &tool,
                                        cx,
                                    )
                                })
                                .into_iter()
                                .flatten(),
                        )
                        .child(
                            self.header_stop(
                                self.with_tip(
                                    "dm-pins",
                                    Tip::text(dc_intl::tr("Pinned Messages"), Side::Bottom),
                                    tool("pin"),
                                    cx,
                                ),
                                "dm-pins",
                                cx,
                            )
                            .cursor_pointer()
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.tip = None;
                                this.toggle_pins("dm-pins", cx);
                            })),
                        )
                        .child(
                            self.header_stop(
                                self.with_tip(
                                    "dm-add",
                                    Tip::text(dc_intl::tr("Add to DM"), Side::Bottom),
                                    tool("dm_add"),
                                    cx,
                                ),
                                "dm-add",
                                cx,
                            )
                            .cursor_pointer()
                            .on_click(cx.listener(
                                |this, _, window, cx| {
                                    this.tip = None;
                                    this.open_add_to_dm(window, cx);
                                },
                            )),
                        )
                        .child(
                            self.header_stop(
                                self.with_tip(
                                    "dm-profile",
                                    Tip::text(
                                        if profile_disabled {
                                            dc_intl::tr("Show User Profile (Unavailable)")
                                        } else if shown {
                                            dc_intl::tr("Hide User Profile")
                                        } else {
                                            dc_intl::tr("Show User Profile")
                                        },
                                        Side::Bottom,
                                    ),
                                    div()
                                        .id("toggle-dm-profile")
                                        .when(!profile_disabled, |this| {
                                            this.cursor_pointer().on_click(cx.listener(
                                                |this, _, _, cx| {
                                                    this.show_dm_profile = !this.show_dm_profile;
                                                    cx.notify();
                                                },
                                            ))
                                        })
                                        .when(profile_disabled, |this| this.opacity(0.6))
                                        .size(px(32.))
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .child(icon(
                                            "dm_profile",
                                            20.,
                                            if shown {
                                                "--text-strong"
                                            } else {
                                                "--text-muted"
                                            },
                                        )),
                                    cx,
                                ),
                                "dm-profile",
                                cx,
                            ),
                        ),
                )
            })
            .when(dm_user.is_none() && self.current_dm().is_some(), |this| {
                // 289287 `GROUP_DM`: calls, pins, "Invite to Group DM" (unless managed) and the members toggle.
                this.child(
                    div()
                        .flex()
                        .gap(px(8.))
                        .children(
                            self.channel
                                .map(|channel| self.header_call_buttons(channel, None, &tool, cx))
                                .into_iter()
                                .flatten(),
                        )
                        .child(
                            self.header_stop(
                                self.with_tip(
                                    "dm-pins",
                                    Tip::text(dc_intl::tr("Pinned Messages"), Side::Bottom),
                                    tool("pin"),
                                    cx,
                                ),
                                "dm-pins",
                                cx,
                            )
                            .cursor_pointer()
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.tip = None;
                                this.toggle_pins("dm-pins", cx);
                            })),
                        )
                        .child(
                            self.header_stop(
                                self.with_tip(
                                    "dm-add",
                                    Tip::text(dc_intl::tr("Invite to Group DM"), Side::Bottom),
                                    tool("dm_add"),
                                    cx,
                                ),
                                "dm-add",
                                cx,
                            )
                            .cursor_pointer()
                            .on_click(cx.listener(
                                |this, _, window, cx| {
                                    this.tip = None;
                                    this.open_add_to_dm(window, cx);
                                },
                            )),
                        )
                        .child(
                            self.header_stop(div().id("toggle-members"), "toggle-members", cx)
                                .cursor_pointer()
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.group_members = !this.group_members;
                                    cx.notify();
                                }))
                                .child(
                                    div()
                                        .size(px(32.))
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .child(icon(
                                            "members",
                                            20.,
                                            if self.group_members {
                                                "--text-strong"
                                            } else {
                                                "--text-muted"
                                            },
                                        )),
                                ),
                        ),
                )
            })
            .when(dm_user.is_none() && self.current_dm().is_none(), |this| {
                this.child(
                    div()
                        .flex()
                        .gap(px(8.))
                        .child(
                            self.header_stop(
                                self.with_tip(
                                    "threads",
                                    Tip::text(dc_intl::tr("Threads"), Side::Bottom),
                                    tool("threads")
                                        .relative()
                                        .child(self.anchor_probe("header-threads")),
                                    cx,
                                ),
                                "threads",
                                cx,
                            )
                            .when(
                                self.current_channel().is_some_and(Channel::is_forum)
                                    || self.open_post_forum().is_some(),
                                |this| this.hidden(),
                            )
                            .cursor_pointer()
                            .on_click(cx.listener(
                                |this, _, window, cx| {
                                    this.tip = None;
                                    this.toggle_thread_browser(window, cx);
                                },
                            )),
                        )
                        .child(
                            self.header_stop(
                                self.with_tip(
                                    "notifications",
                                    Tip::text(dc_intl::tr("Notification Settings"), Side::Bottom),
                                    tool(if muted { "bell_muted" } else { "bell" }),
                                    cx,
                                ),
                                "notifications",
                                cx,
                            )
                            .cursor_pointer()
                            .on_click(cx.listener(
                                |this, event: &gpui::ClickEvent, _, cx| {
                                    this.tip = None;
                                    // Module 187360: a shift-click mutes or unmutes at once, without the menu.
                                    if event.modifiers().shift {
                                        this.toggle_channel_mute_now();
                                    } else {
                                        this.notify_open = !this.notify_open;
                                        this.notify_submenu = None;
                                    }
                                    cx.notify();
                                },
                            )),
                        )
                        .child(
                            self.header_stop(
                                self.with_tip(
                                    "pins",
                                    Tip::text(dc_intl::tr("Pinned Messages"), Side::Bottom),
                                    tool("pin"),
                                    cx,
                                ),
                                "pins",
                                cx,
                            )
                            .when(
                                self.current_channel().is_some_and(Channel::is_forum),
                                |this| this.hidden(),
                            )
                            .cursor_pointer()
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.tip = None;
                                this.toggle_pins("pins", cx);
                            })),
                        )
                        .child(
                            self.header_stop(div().id("toggle-members"), "toggle-members", cx)
                                .cursor_pointer()
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.show_members = !this.show_members;
                                    cx.notify();
                                }))
                                .child(
                                    div()
                                        .size(px(32.))
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .child(icon(
                                            "members",
                                            20.,
                                            if self.show_members {
                                                "--text-strong"
                                            } else {
                                                "--text-muted"
                                            },
                                        )),
                                ),
                        )
                        // A forum post's header ends its tools with More (`forum-fixture-posted`, aria "More"):
                        // the thread's own menu.
                        .children(self.open_post_forum().and(self.channel).map(|thread| {
                            self.header_stop(
                                div().id("forum-post-more-header"),
                                "forum-post-more",
                                cx,
                            )
                            .cursor_pointer()
                            .on_click(cx.listener(move |this, event: &gpui::ClickEvent, _, cx| {
                                this.tip = None;
                                this.open_context_menu(
                                    context_menu::Target::Thread(thread),
                                    event.position(),
                                    cx,
                                );
                            }))
                            .child(
                                div()
                                    .size(px(32.))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .child(icon("more_horizontal", 20., "--text-muted")),
                            )
                        })),
                )
            })
            .child(
                div()
                    .ml(px(12.))
                    .parity_with_border("search", 1.)
                    // Narrowed by the New Thread panel, the search keeps 72px (`Se…` in the capture).
                    .w(px(if self.new_thread.is_some() {
                        72.
                    } else if self.mod_view.is_some() {
                        // The Mod View panel leaves 80px (menus2/mod-view-self: the box at x 711 to 791).
                        80.
                    } else {
                        // `.search_bWCaDq`: `clamp(64px, 100cqi - 320px, 244px)`, the container being the header
                        // (chat-150 dump: 134.7px of a 478.7px header with its 24px of padding).
                        (self.viewport.0 - self.combined_sidebar_width() - 24. - 320.)
                            .clamp(64., 244.)
                    }))
                    .min_w(px(64.))
                    .h(px(32.))
                    .rounded(px(8.))
                    .bg(color("--input-background-default"))
                    .border_1()
                    .border_color(color("--background-mod-subtle"))
                    .flex()
                    .items_center()
                    .overflow_hidden()
                    .text_size(px(14.))
                    .line_height(px(18.))
                    .font_weight(FontWeight(500.))
                    .text_color(color("--input-placeholder-text-default"))
                    .relative()
                    .child(self.anchor_probe("header-search"))
                    .id("header-search-box")
                    .cursor_text()
                    .on_click(cx.listener(|this, _, window, cx| this.focus_search(window, cx)))
                    .children(self.search.as_ref().map(|search| {
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .pl(px(8.))
                            .pr(px(6.))
                            .overflow_hidden()
                            .child(search.field.clone())
                    }))
                    .when(self.search.is_none(), |this| {
                        this.child(search).child(div().flex_1())
                    })
                    .child({
                        // With text in the field the magnifier becomes Clear (`clear_v966Ys`).
                        let typed = self.search.as_ref().is_some_and(|search| {
                            !search.field.read(cx).text().is_empty() || search.shown.is_some()
                        });
                        div()
                            .id("header-search-icon")
                            .w(px(24.))
                            .h(px(24.))
                            .flex_none()
                            .flex()
                            .items_center()
                            .justify_center()
                            .when(typed, |this| {
                                this.cursor_pointer()
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        cx.stop_propagation();
                                        this.clear_search(cx);
                                    }))
                            })
                            .child(icon(
                                if typed { "search_clear" } else { "search" },
                                16.,
                                "--interactive-text-default",
                            ))
                    }),
            )
    }

    /// What a mention or stamp shows, resolved against the stores.
    fn span_label(&self, span: &Span) -> String {
        match &span.kind {
            Kind::User(id) => {
                let user = self.stores.users.get(id).or(self
                    .stores
                    .current_user
                    .as_ref()
                    .filter(|user| user.id == *id));
                format!("@{}", user.map_or("unknown-user", User::display_name))
            }
            Kind::Role(id) => {
                let role = self.current_guild().and_then(|guild| guild.roles.get(id));
                // `YV4F/n`: a role the client does not know is plain "@unknown-role" (`roleMention.parse` of the rules).
                format!(
                    "@{}",
                    role.map_or_else(|| dc_intl::tr("unknown-role").to_string(), String::clone)
                )
            }
            Kind::Channel(id) => self
                .current_guild()
                .and_then(|guild| guild.channels.iter().find(|channel| channel.id == *id))
                .map_or_else(|| "unknown".to_owned(), |channel| channel.name.clone()),
            Kind::Emoji { name, .. } => format!(":{name}:"),
            Kind::Timestamp { unix, format } => format_timestamp(*unix, *format),
            // Module 990474 `E`: "/" and the command's name, as a link or as plain text.
            Kind::Command { name, .. } => format!("/{name}"),
            Kind::Text
            | Kind::Link(_)
            | Kind::AttachmentLink(_)
            | Kind::Everyone
            | Kind::Unicode => span.text.clone(),
        }
    }

    fn piece(&self, span: &Span, base: Hsla, size: f32, weight: f32, emoji_size: f32) -> Piece {
        let style = span.style;
        let deleted_role = matches!(
            span.kind,
            Kind::Role(id) if self.current_guild().is_none_or(|guild| !guild.roles.contains_key(&id))
        );
        let mention = matches!(span.kind, Kind::User(_) | Kind::Channel(_) | Kind::Everyone)
            || (matches!(span.kind, Kind::Role(_)) && !deleted_role)
            || matches!(&span.kind, Kind::Command { name, id } if self.command_mention_usable(name, *id));
        let mut run_font: Font = font(if style.code { MONO } else { SANS });
        run_font.features = kerning();
        run_font.weight = match (style.bold, mention) {
            (true, _) => FontWeight(700.),
            (false, true) => FontWeight(500.),
            (false, false) => FontWeight(weight),
        };
        run_font.style = if style.italic {
            FontStyle::Italic
        } else {
            FontStyle::Normal
        };
        // 447215: a user mention takes the mentioned user's display name font only outside a guild (direct and
        // group messages); with a guild id the mention keeps the plain role-coloured look.
        if let Kind::User(id) = &span.kind
            && self.current_guild().is_none()
            && let Some(user) = self.stores.users.get(id)
            && let Some(styles) = self.name_styles(user, None)
            && let Some((family, weight)) = name_styles::run_font(&styles)
        {
            run_font.family = family;
            run_font.weight = FontWeight(weight);
        }
        // 787564 `er`: a role mention of a coloured role, with "Role Colors" in the name, takes the role colour for
        // its text and the colour at .1 (.3 on hover) behind it (332173); the enhanced styles draw the text in the
        // gradient (`roleColors` of the same module, gated by `X_` of 967144).
        let role_look = match span.kind {
            Kind::Role(id) if self.role_names_colored() => self.current_guild().and_then(|guild| {
                let info = guild.role_info.get(&id).filter(|info| info.color != 0)?;
                Some((info.color, guild.role_colors_of(id)))
            }),
            // Not in the pinned client: RoleColorEverywhere's `chatMentions` hands the mention the person's colour
            // (`plugin_hooks/role_color_everywhere.rs`).
            Kind::User(id) => self
                .role_color_everywhere(plugin_hooks::RolePlace::ChatMentions, id)
                .map(|look| (look.color, look.gradient)),
            _ => None,
        };
        let tint = match span.kind {
            Kind::Link(_) | Kind::AttachmentLink(_) if self.failed_tint.get() => {
                hsla("--text-feedback-critical")
            }
            Kind::Link(_) | Kind::AttachmentLink(_) => hsla("--text-link"),
            Kind::Role(_) | Kind::User(_) if role_look.is_some() => {
                Hsla::from(gpui::rgb(role_look.map_or(0, |(color, _)| color)))
            }
            _ if mention => hsla("--mention-foreground"),
            // A hidden spoiler paints its text in the box's own color.
            _ if style.spoiler => hsla("--spoiler-hidden-background"),
            _ => base,
        };
        let boxed = if style.code {
            Some(inline_code_box())
        } else if style.spoiler {
            Some(pill(0., 4., "--spoiler-hidden-background"))
        } else if matches!(span.kind, Kind::Channel(_)) {
            let mut channel = pill(2., 3., "--mention-background");
            channel.icon = Some(("icons/text_channel.svg".into(), px(16.), px(20.)));
            Some(channel)
        } else if mention {
            let mut mention_box = pill(2., 3., "--mention-background");
            if let Some((color, _)) = role_look {
                mention_box.background = role_tint(color, 0.1);
            }
            Some(mention_box)
        } else if matches!(span.kind, Kind::AttachmentLink(_)) {
            // Module 862316's attachmentLink: AttachmentIcon at 1em in the link's colour (`.icon__RYHZ5`), then the
            // file name, with no box of its own.
            let mut chip = pill(0., 0., "--text-link");
            chip.background = Hsla::transparent_black();
            chip.icon = Some(("icons/attachment_link.svg".into(), px(size), px(size + 4.)));
            Some(chip)
        } else if matches!(span.kind, Kind::Timestamp { .. }) {
            Some(pill(2., 3., "--interactive-background-active"))
        } else {
            None
        };
        let emoji_key = match &span.kind {
            Kind::Unicode => Some(format!("emoji:{}", span.text)),
            Kind::Emoji { id, animated, .. } => Some(if *animated {
                format!("custom-animated:{id}")
            } else {
                format!("custom:{id}")
            }),
            _ => None,
        };
        let image = emoji_key
            .and_then(|key| {
                self.images.get(&key).cloned().or_else(|| {
                    // The still picture until the animated one is in, or when the setting is off.
                    let still = key.replace("custom-animated:", "custom:");
                    self.images.get(&still).cloned()
                })
            })
            .map(|image| (image, px(emoji_size)));
        Piece {
            text: self.span_label(span),
            font: run_font,
            size: if style.code {
                px(size * 0.85)
            } else {
                px(size)
            },
            color: tint,
            // `underline_links`: links carry their underline at rest.
            underline: style.underline
                || (matches!(span.kind, Kind::Link(_) | Kind::AttachmentLink(_))
                    && self.underline_links()),
            strike: style.strike,
            boxed,
            image,
            // `.interactive:hover { background-color: var(--brand-500); color: var(--white) }` on the mention
            // (stylesheet 17256's markup rules); `@everyone` and `@here` are not interactive.
            hover: (mention && !matches!(span.kind, Kind::Everyone)).then(|| match role_look {
                // The inline `color` outranks the stylesheet's white; only the background deepens.
                Some((color, _)) => (role_tint(color, 0.3), tint),
                None => (hsla("--brand-500"), hsla("--white")),
            }),
            // `.anchor:hover` underlines a link.
            hover_underline: matches!(span.kind, Kind::Link(_) | Kind::AttachmentLink(_)),
            gradient: role_look.and_then(|(_, colors)| colors),
        }
    }

    fn spans(&self, spans: &[Span], text_color: Hsla, key: (u64, u32)) -> Inline {
        self.spans_edited(spans, text_color, key, &[])
    }

    /// A message's last paragraph ends in `(edited)` when the message was edited: `edited_2i8Bfm`, 10px in
    /// `--chat-text-muted`, after a space in the message's own size. A blocked edit adds `(reverted)` the same way
    /// (module 469759: the edited timestamp of the message's own time). Each mark is its text and its tooltip's time.
    fn spans_edited(
        &self,
        spans: &[Span],
        text_color: Hsla,
        key: (u64, u32),
        marks: &[(String, Option<chrono::DateTime<chrono::Utc>>)],
    ) -> Inline {
        let jumbo = dc_markdown::is_jumbo(spans);
        // `AccessibilityStore.fontSize`: the text, its 1.375 line and the 22px emoji scale with the chat font.
        let font_size = self.chat_font_size();
        let emoji_size = if jumbo {
            48.
        } else {
            (22. * font_size / 16.).round()
        };
        let spoilers = Self::spoiler_run_starts(spans);
        let mut pieces: Vec<Piece> = spans
            .iter()
            .zip(&spoilers)
            .map(|(span, start)| {
                self.piece_with_spoiler(
                    span,
                    *start,
                    key,
                    text_color,
                    (font_size, 400., emoji_size),
                )
            })
            .collect();
        let mut extra_hovers = Vec::new();
        for (text, at) in marks {
            let plain = Span {
                text: " ".to_owned(),
                style: dc_markdown::Style::default(),
                kind: Kind::Text,
            };
            pieces.push(self.piece(&plain, text_color, font_size, 400., emoji_size));
            let mut label = self.piece(&plain, hsla("--chat-text-muted"), 10., 400., emoji_size);
            label.text = text.clone();
            pieces.push(label);
            extra_hovers.push(None);
            extra_hovers.push(at.and_then(message_tips::edited_tip));
        }
        let line = if jumbo {
            px(48.)
        } else {
            px((font_size * 1.375).round())
        };
        let element =
            inline(pieces, line).selectable(key, self.selection.clone(), hsla_from(SELECTION));
        self.wire_inline_spans(element, spans, &spoilers, key, &extra_hovers)
    }

    fn spoiler_run_starts(spans: &[Span]) -> Vec<Option<usize>> {
        // A spoiler is a run of spoiler spans; clicking any of them opens the run, keyed by its first span.
        let mut run_start = vec![0; spans.len()];
        for index in 0..spans.len() {
            run_start[index] =
                if index > 0 && spans[index].style.spoiler && spans[index - 1].style.spoiler {
                    run_start[index - 1]
                } else {
                    index
                };
        }
        spans
            .iter()
            .enumerate()
            .map(|(index, span)| span.style.spoiler.then_some(run_start[index]))
            .collect()
    }

    fn piece_with_spoiler(
        &self,
        span: &Span,
        spoiler_start: Option<usize>,
        key: (u64, u32),
        base: Hsla,
        (size, weight, emoji_size): (f32, f32, f32),
    ) -> Piece {
        let open = spoiler_start.is_some_and(|start| {
            self.spoilers_shown()
                || self.revealed.contains(&(key.0, key.1, start))
                || plugin_hooks::all_spoilers_revealed(&self.revealed, key.0)
        });
        let mut piece = self.piece(span, base, size, weight, emoji_size);
        if open {
            piece.color = base;
            piece.boxed = Some(pill(0., 4., "--__spoiler-background-color--revealed"));
        }
        piece
    }

    /// Attaches hover tooltips, emoji/mention/role popouts, link context menu and click handlers to an inline block.
    fn wire_inline_spans(
        &self,
        mut element: Inline,
        spans: &[Span],
        spoilers: &[Option<usize>],
        key: (u64, u32),
        extra_hovers: &[Option<Tip>],
    ) -> Inline {
        // What the pointer finds on a piece: the "(edited)" mark and a stamp carry a tooltip, an emoji opens its popout.
        let mut hovers: Vec<Option<Tip>> = spans
            .iter()
            .map(|span| match &span.kind {
                Kind::Timestamp { unix, .. } => message_tips::stamp_tip(*unix),
                _ => None,
            })
            .collect();
        hovers.extend(extra_hovers.iter().cloned());
        // Not in the pinned client: ValidUser fetches an unknown mentioned user under the pointer
        // (`plugin_hooks/valid_user.rs`); `None` while it is off.
        let unknown = self.valid_user_unknown(spans);
        let tips = hovers.iter().any(Option::is_some);
        if tips || unknown.is_some() {
            let root = self.weak.clone();
            let hovers = std::rc::Rc::new(hovers);
            element = element.on_piece_hover(move |over, _, cx| {
                let _ = root.update(cx, |this, cx| {
                    if let (Some(unknown), Some((piece, _))) = (&unknown, &over)
                        && let Some(Some(user)) = unknown.get(*piece)
                    {
                        this.valid_user_hovered(*user, cx);
                    }
                    if !tips {
                        return;
                    }
                    let tip = over.and_then(|(piece, bounds)| {
                        hovers
                            .get(piece)
                            .cloned()
                            .flatten()
                            .map(|tip| (piece, tip, bounds))
                    });
                    this.message_tip(format!("message-piece-{}-{}", key.0, key.1), tip, cx);
                });
            });
        }
        let emoji: Vec<Option<emoji_popout::Kind>> = spans
            .iter()
            .map(|span| match &span.kind {
                Kind::Unicode => Some(emoji_popout::Kind::Standard(span.text.clone())),
                Kind::Emoji { name, id, .. } => Some(emoji_popout::Kind::Custom {
                    id: *id,
                    name: name.clone(),
                }),
                _ => None,
            })
            .collect();
        // A right press on a link or an emoji tells the message menu what it was on (modules 777933 and 253925).
        let emoji_keys: Vec<Option<String>> = emoji
            .iter()
            .map(|kind| kind.as_ref().and_then(picker::favorite_key))
            .collect();
        // A user mention opens the user popout beside itself, as the author name does (module 288539 wraps
        // the mention in the popout of module 342296); a channel mention navigates.
        let user_mentions: Vec<Option<u64>> = spans
            .iter()
            .map(|span| match span.kind {
                Kind::User(id) => Some(id),
                _ => None,
            })
            .collect();
        // A role mention opens the list of its members (module 593284); `@everyone` does too, `@here` does not.
        let role_mentions: Vec<Option<Option<u64>>> = spans
            .iter()
            .map(|span| match &span.kind {
                Kind::Role(id) => Some(Some(*id)),
                Kind::Everyone if span.text == "@everyone" => Some(None),
                _ => None,
            })
            .collect();
        if emoji.iter().any(Option::is_some)
            || user_mentions.iter().any(Option::is_some)
            || role_mentions.iter().any(Option::is_some)
        {
            let root = self.weak.clone();
            element = element.on_piece_click_at(move |piece, bounds, _, cx| {
                if let Some(Some(kind)) = emoji.get(piece).cloned() {
                    let _ = root.update(cx, |this, cx| this.open_emoji_popout(kind, bounds, cx));
                } else if let Some(Some(user)) = user_mentions.get(piece).copied() {
                    let _ = root.update(cx, |this, cx| this.open_profile(user, bounds, false, cx));
                } else if let Some(Some(role)) = role_mentions.get(piece).copied() {
                    let _ = root.update(cx, |this, cx| {
                        this.close_emoji_popout(cx);
                        this.open_role_popout(role, bounds, cx)
                    });
                }
            });
        }
        let links = link_runs(spans);
        let channels: Vec<Option<u64>> = spans
            .iter()
            .map(|span| match span.kind {
                Kind::Channel(id) => Some(id),
                _ => None,
            })
            .collect();
        if links.iter().any(Option::is_some)
            || emoji_keys.iter().any(Option::is_some)
            || channels.iter().any(Option::is_some)
        {
            let root = self.weak.clone();
            let runs = links.clone();
            let mentions = channels.clone();
            let chips: Vec<bool> = spans
                .iter()
                .map(|span| matches!(span.kind, Kind::AttachmentLink(_)))
                .collect();
            element = element.on_piece_context(move |piece, at, _, cx| {
                // A channel mention opens its own menu (module 223998) instead of the message's.
                if let Some(Some(channel)) = mentions.get(piece).copied() {
                    let _ = root.update(cx, |this, _| {
                        this.chat_extra.mention_hit = Some((at, channel))
                    });
                    return;
                }
                // So does an attachment chip: Link Options (module 740024).
                if chips.get(piece).copied().unwrap_or(false)
                    && let Some(Some((url, name))) = runs.get(piece).cloned()
                {
                    let _ = root.update(cx, |this, _| {
                        this.chat_extra.attachment_hit = Some((at, url, name))
                    });
                    return;
                }
                let hit = if let Some(Some((url, label))) = runs.get(piece).cloned() {
                    Some(menu::Hit::Link { url, label })
                } else if let Some(Some(key)) = emoji_keys.get(piece).cloned() {
                    Some(menu::Hit::Emoji { key })
                } else {
                    None
                };
                if let Some(hit) = hit {
                    let _ = root.update(cx, |this, _| this.menu_hit = Some((at, hit)));
                }
            });
        }
        // A usable command mention puts its command in the composer (module 990474).
        let commands: Vec<Option<(String, u64)>> = spans
            .iter()
            .map(|span| match &span.kind {
                Kind::Command { name, id } if self.command_mention_usable(name, *id) => {
                    Some((name.clone(), *id))
                }
                _ => None,
            })
            .collect();
        // A middle click on a link follows it as a left click does (module 9578 `onAuxClick`); a hidden spoiler
        // still takes the first (left) click.
        if links.iter().any(Option::is_some) {
            let links = links.clone();
            let spoilers = spoilers.to_vec();
            let root = self.weak.clone();
            let marks = links
                .iter()
                .zip(spoilers.iter().chain(std::iter::repeat(&None)))
                .map(|(link, spoiler)| {
                    link.is_some()
                        && spoiler.is_none_or(|start| {
                            self.spoilers_shown()
                                || self.revealed.contains(&(key.0, key.1, start))
                                || plugin_hooks::all_spoilers_revealed(&self.revealed, key.0)
                        })
                })
                .collect();
            element = element.on_link_aux_click(marks, move |piece, _, cx| {
                if let Some(Some((url, label))) = links.get(piece).cloned() {
                    let _ = root.update(cx, |this, cx| this.follow_link(&url, &label, cx));
                }
            });
        }
        if spoilers.iter().any(Option::is_some)
            || links.iter().any(Option::is_some)
            || channels.iter().any(Option::is_some)
            || commands.iter().any(Option::is_some)
        {
            let spoilers = spoilers.to_vec();
            let root = self.weak.clone();
            element = element.on_piece_click(move |piece, window, cx| {
                let modifiers = window.modifiers();
                let _ = root.update(cx, |this, cx| {
                    // A hidden spoiler takes the first click; a link inside it follows on the next.
                    if let Some(Some(start)) = spoilers.get(piece).copied()
                        && this.revealed.insert((key.0, key.1, start))
                    {
                        // Not in the pinned client: RevealAllSpoilers opens the others too (`plugin_hooks.rs`).
                        let loaded: Vec<Id> = this
                            .channel
                            .and_then(|channel| this.stores.messages.get(&channel))
                            .map(|messages| messages.iter().map(|known| known.id).collect())
                            .unwrap_or_default();
                        plugin_hooks::reveal_all_spoilers(
                            &mut this.revealed,
                            key.0,
                            &loaded,
                            modifiers,
                        );
                        cx.notify();
                        return;
                    }
                    if let Some(Some((url, label))) = links.get(piece).cloned() {
                        this.follow_link(&url, &label, cx);
                    }
                    if let Some(Some(channel)) = channels.get(piece).copied() {
                        this.open_channel_mention(channel, cx);
                    }
                    if let Some(Some((name, id))) = commands.get(piece).cloned() {
                        this.press_command_mention(name, id, window, cx);
                    }
                });
            });
        }
        element
    }

    /// `#channel` in a message: the channel opens, in its own server.
    fn open_channel_mention(&mut self, channel: Id, cx: &mut Context<Self>) {
        let guild = self
            .stores
            .guilds
            .iter()
            .find(|guild| guild.channels.iter().any(|known| known.id == channel))
            .map(|guild| guild.id);
        if let Some(guild) = guild {
            self.select(guild, Some(channel));
            cx.notify();
        }
    }

    /// `h1`, `h2` and `h3` from the capture: size and line height per level, all bold.
    fn heading(&self, spans: &[Span], level: u8, key: (u64, u32)) -> Inline {
        let (size, line) = match level {
            1 => (24., 33.),
            2 => (20., 27.5),
            _ => (16., 22.),
        };
        // An official message's headings take its content colour (`:is(h1, ..., h6)`).
        let strong = self
            .official_text
            .get()
            .map_or_else(|| hsla("--text-strong"), Hsla::from);
        let spoilers = Self::spoiler_run_starts(spans);
        let pieces = spans
            .iter()
            .zip(&spoilers)
            .map(|(span, start)| {
                let mut piece =
                    self.piece_with_spoiler(span, *start, key, strong, (size, 700., size * 1.375));
                piece.font.weight = FontWeight(700.);
                piece
            })
            .collect();
        let element =
            inline(pieces, px(line)).selectable(key, self.selection.clone(), hsla_from(SELECTION));
        self.wire_inline_spans(element, spans, &spoilers, key, &[])
    }

    fn subtext(&self, spans: &[Span], key: (u64, u32)) -> Inline {
        // And its `small` subtext.
        let muted = self
            .official_text
            .get()
            .map_or_else(|| hsla("--interactive-text-default"), Hsla::from);
        let spoilers = Self::spoiler_run_starts(spans);
        let pieces = spans
            .iter()
            .zip(&spoilers)
            .map(|(span, start)| {
                self.piece_with_spoiler(span, *start, key, muted, (14., 400., 19.))
            })
            .collect();
        let element =
            inline(pieces, px(19.25)).selectable(key, self.selection.clone(), hsla_from(SELECTION));
        self.wire_inline_spans(element, spans, &spoilers, key, &[])
    }

    /// The official pills: 30px tall, a 20px emoji, the count 6px after it, 4px between pills.
    fn reactions(&self, message: &Message, cx: &mut Context<Self>) -> Option<AnyElement> {
        // "Show emoji reactions" off hides them under every message.
        if !self.pref_on("chat_emoji_render_reactions", true) {
            return None;
        }
        let pills: Vec<AnyElement> = message
            .reactions
            .iter()
            .enumerate()
            .map(|(index, reaction)| {
                let image = match reaction.emoji_id {
                    Some(id) => self.images.get(&format!("emoji-id:{id}")).cloned(),
                    None => self
                        .images
                        .get(&format!("emoji:{}", reaction.name))
                        .cloned(),
                };
                let (channel, id, emoji, add) = (
                    message.channel_id,
                    message.id,
                    reaction_route_name(reaction),
                    !reaction.me,
                );
                let group = SharedString::from(format!("reaction-{}-{index}", message.id));
                // Who reacted: the first three, fetched once when the pointer first reaches the pill.
                let fetch_key =
                    format!("reactors3:{}:{}", message.id, reaction_route_name(reaction));
                let fetch_route = format!(
                    "/channels/{}/messages/{}/reactions/{}?limit={}&type=0",
                    message.channel_id,
                    message.id,
                    reactions_modal::encode(&reaction_route_name(reaction)),
                    reaction_tip::NAMED,
                );
                let tip = Tip::body(
                    Body::Reaction {
                        emoji: format!("emoji:{}", reaction.name),
                        fetch_key: fetch_key.clone(),
                        label: reaction_tip::emoji_label(
                            &reaction.name,
                            reaction.emoji_id.is_some(),
                        ),
                        count: reaction.count as usize,
                    },
                    Side::Top,
                )
                .after(reaction_tip::DELAY_MS)
                .spaced(8.);
                let pill = div()
                    .id(group.clone())
                    .group(group.clone())
                    .h(px(30.))
                    .px(px(6.))
                    .rounded(px(8.))
                    .flex()
                    .flex_none()
                    .items_center()
                    .cursor_pointer()
                    .border_1()
                    .when(reaction.me, |this| {
                        this.bg(color("--reaction-background-reacted-default"))
                            .border_color(color("--reaction-border-reacted-default"))
                    })
                    .when(!reaction.me, |this| {
                        this.bg(color("--reaction-background-default"))
                            .border_color(gpui::transparent_black())
                            .hover(|this| {
                                this.bg(color("--reaction-background-hover"))
                                    .border_color(color("--reaction-border-hover"))
                            })
                            // `.reaction_MmKct8:active` (17256); a reacted pill keeps its own look.
                            .active(|this| {
                                this.bg(color("--reaction-background-active"))
                                    .border_color(color("--reaction-border-active"))
                            })
                    })
                    .rule(motion::Rule::Reaction)
                    .on_hover(cx.listener(move |this, inside: &bool, _, _| {
                        if *inside && this.requested.insert(fetch_key.clone()) {
                            let _ = this.requests.send(Request::Fetch {
                                key: fetch_key.clone(),
                                route: fetch_route.clone(),
                            });
                        }
                    }))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if add {
                            this.track_reaction(&emoji);
                        }
                        this.send_and_apply_reaction(
                            channel,
                            id,
                            menu::ReactionOp {
                                emoji: emoji.clone(),
                                add,
                                location: "Message",
                                burst: false,
                            },
                            cx,
                        );
                    }))
                    .child(
                        div()
                            .size(px(20.))
                            .children(image.map(|image| img(image).size(px(20.)))),
                    )
                    .child(
                        div()
                            .ml(px(6.))
                            .min_w(px(12.))
                            .flex()
                            .justify_center()
                            .text_size(px(16.))
                            .line_height(px(16.))
                            .font_weight(FontWeight(600.))
                            .text_color(if reaction.me {
                                color("--reaction-text-reacted-default")
                            } else {
                                color("--text-subtle")
                            })
                            .when(!reaction.me, |this| {
                                this.group_hover(group.clone(), |this| {
                                    this.text_color(color("--reaction-text-hover"))
                                })
                            })
                            .child(self.rolling_count(message.id, index, reaction.count)),
                    )
                    // Not in the pinned client: WhoReacted (`plugin_hooks/who_reacted.rs`).
                    .children(self.who_reacted(message, &reaction_route_name(reaction), cx));
                self.with_tip(
                    format!("reaction-tip-{}-{index}", message.id),
                    tip,
                    pill,
                    cx,
                )
                .mr(px(4.))
                .mb(px(4.))
                .into_any_element()
            })
            .collect();
        (!pills.is_empty()).then(|| {
            // `forceShow` in module 367790: the add button always follows the pills.
            let add_group = SharedString::from(format!("reaction-add-group-{}", message.id));
            let add = div()
                .id(SharedString::from(format!("reaction-add-{}", message.id)))
                .group(add_group.clone())
                .relative()
                .h(px(30.))
                .w(px(34.))
                .mb(px(4.))
                .mr(px(4.))
                .rounded(px(8.))
                .flex()
                .flex_none()
                .items_center()
                .justify_center()
                .cursor_pointer()
                .border_1()
                .border_color(gpui::transparent_black())
                .bg(color("--reaction-background-default"))
                .hover(|this| {
                    this.bg(color("--reaction-background-hover"))
                        .border_color(color("--reaction-border-hover"))
                })
                // `.reaction_MmKct8:active, .reactionBtn_MmKct8:active` (17256).
                .active(|this| {
                    this.bg(color("--reaction-background-active"))
                        .border_color(color("--reaction-border-active"))
                })
                .rule(motion::Rule::Reaction)
                .child(self.anchor_probe(format!("reaction-add-{}", message.id)))
                .on_click({
                    let key = format!("reaction-add-{}", message.id);
                    let channel = message.channel_id;
                    let target = message.id;
                    cx.listener(move |this, _, window, cx| {
                        let anchor = this.tip_anchors.borrow().get(key.as_str()).copied();
                        if let Some(bounds) = anchor {
                            this.open_picker(
                                picker::Target::Reaction {
                                    channel,
                                    message: target,
                                },
                                gpui::point(
                                    bounds.right() + px(PICKER_REACTION_OFFSET),
                                    bounds.top(),
                                ),
                                window,
                                cx,
                            );
                        }
                    })
                })
                // `.reactionBtn_MmKct8:hover .icon` (17256); `:active .icon` has the same `--reaction-text-active` value.
                .child(
                    icon("msg_react", 18., "--reaction-text-default")
                        .group_hover(add_group.clone(), |this| {
                            this.text_color(color("--reaction-text-hover"))
                        }),
                );
            div()
                .pt(px(2.))
                .pb(px(2.))
                .flex()
                .flex_wrap()
                .items_center()
                .children(pills)
                .child(add)
                .into_any_element()
        })
    }

    /// Images in their grid: 2px above, 4px between, natural size up to the official limits, 8px corners.
    fn attachments(&self, message: &Message, cx: &mut Context<Self>) -> Option<AnyElement> {
        // "When uploaded directly to Discord" off: pictures come as file cards instead (see `files`).
        let inline = self.pref_on("chat_inline_media_uploads", true);
        let pictures: Vec<&dc_state::Attachment> = message
            .attachments
            .iter()
            .filter(|attachment| inline && (attachment.is_image() || attachment.is_video()))
            .collect();
        if pictures.len() > 1 {
            return Some(self.mosaic(message, &pictures, cx));
        }
        let images: Vec<AnyElement> = pictures
            .into_iter()
            .map(|attachment| {
                let (width, height) = fit_media(
                    attachment.width.unwrap_or(1),
                    attachment.height.unwrap_or(1),
                );
                let (width, height) = (width as f32, height as f32);
                let shown = width.min(self.content_width.max(1.));
                let (width, height) = (px(shown), px(height * shown / width));
                if attachment.is_video() {
                    return self.attachment_video_obscured(
                        message,
                        attachment,
                        (width, height),
                        cx,
                    );
                }
                self.attachment_tile_obscured(message, attachment, (width, height), false, cx)
            })
            .collect();
        (!images.is_empty()).then(|| {
            // 3px under each image is the inline-block baseline gap of the official mosaic.
            div()
                .pb(px(3.))
                .flex()
                .flex_col()
                .gap(px(4.))
                .children(images)
                .into_any_element()
        })
    }

    /// Several pictures of one message as the official mosaic lays them out (`mosaic.rs`).
    fn mosaic(
        &self,
        message: &Message,
        pictures: &[&dc_state::Attachment],
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let sizes: Vec<(u32, u32)> = pictures
            .iter()
            .map(|picture| (picture.width.unwrap_or(1), picture.height.unwrap_or(1)))
            .collect();
        let width = self.content_width.max(1.);
        let cell = |index: usize, size: (f32, f32), cx: &mut Context<Self>| {
            self.attachment_tile_obscured(
                message,
                pictures[index],
                (px(size.0), px(size.1)),
                true,
                cx,
            )
        };
        let mut next = 0;
        let mut blocks: Vec<AnyElement> = Vec::new();
        for (block, count) in mosaic::layout(&sizes, width) {
            let start = next;
            next += count;
            blocks.push(match block {
                mosaic::Block::Row(cells) => div()
                    .flex()
                    .gap(px(4.))
                    .children(
                        cells
                            .into_iter()
                            .enumerate()
                            .map(|(offset, size)| cell(start + offset, size, cx)),
                    )
                    .into_any_element(),
                mosaic::Block::Three { left, right } => div()
                    .flex()
                    .gap(px(4.))
                    .child(cell(start, left, cx))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(4.))
                            .child(cell(start + 1, right, cx))
                            .child(cell(start + 2, right, cx)),
                    )
                    .into_any_element(),
                mosaic::Block::Grid {
                    columns,
                    cell: size,
                    count,
                } => {
                    let cells: Vec<AnyElement> = (0..count)
                        .map(|offset| cell(start + offset, size, cx))
                        .collect();
                    let mut rows: Vec<AnyElement> = Vec::new();
                    let mut pending = cells.into_iter().peekable();
                    while pending.peek().is_some() {
                        let line: Vec<AnyElement> = pending.by_ref().take(columns).collect();
                        rows.push(div().flex().gap(px(4.)).children(line).into_any_element());
                    }
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(4.))
                        .children(rows)
                        .into_any_element()
                }
            });
        }
        // 3px under the block is the inline-block baseline gap of the official mosaic.
        div()
            .pb(px(3.))
            .flex()
            .flex_col()
            .gap(px(4.))
            .children(blocks)
            .into_any_element()
    }

    /// `container_YUQhib`: pictures, files and embeds in one grid, 2px above and below, 4px between them.
    /// A forward (`container_4dy_KE`): a 4px quote bar, "Forwarded" in italic beside its arrow, the original's
    /// content and files, and a footer naming where it came from, which jumps there.
    fn forwarded_block(&self, message: &Message, cx: &mut Context<Self>) -> Option<AnyElement> {
        let forwarded = message.forwarded.as_ref()?;
        // The copy renders as a message of its own so content, files and embeds look the same.
        let copy = Message {
            content: forwarded.content.clone(),
            attachments: forwarded.attachments.clone(),
            embeds: forwarded.embeds.clone(),
            flags: forwarded.flags,
            reactions: Vec::new(),
            edited: false,
            edited_at: None,
            forwarded: None,
            ..message.clone()
        };
        let source = forwarded
            .channel_id
            .and_then(|channel| {
                self.stores
                    .guilds
                    .iter()
                    .flat_map(|guild| &guild.channels)
                    .find(|known| known.id == channel)
            })
            .map(|channel| format!("#{}", channel.name));
        // Only a guild channel this client knows gets the footer; a DM source shows none.
        let time = header_time(forwarded.local_time(), chrono::Local::now());
        let footer = source.map(|name| format!("{name}  \u{2022}  {time}"));
        let (guild, channel) = (forwarded.guild_id, forwarded.channel_id);
        let message_id = forwarded.message_id;
        Some(
            div()
                .py(px(2.))
                .flex()
                .child(
                    div()
                        .flex_none()
                        .w(px(4.))
                        .rounded(px(2.))
                        .bg(color("--border-subtle")),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.))
                        .pl(px(12.))
                        .flex()
                        .flex_col()
                        .child(
                            div()
                                .mt(px(2.))
                                .h(px(18.))
                                .flex()
                                .items_center()
                                .gap(px(4.))
                                .child(icon("forwarded_icon", 16., "--text-muted"))
                                .child(
                                    div()
                                        .text_size(px(14.))
                                        .line_height(px(18.))
                                        .font_weight(FontWeight(600.))
                                        .italic()
                                        .text_color(color("--text-muted"))
                                        .child(dc_intl::tr("Forwarded")),
                                ),
                        )
                        .when(
                            !copy.content.is_empty() && !self.hides_inline_media_content(&copy),
                            |this| this.child(self.content(&copy)),
                        )
                        .children(self.accessories(&copy, cx))
                        .children(footer.map(|footer| {
                            div()
                                .id(SharedString::from(format!(
                                    "forwarded-footer-{}",
                                    message.id
                                )))
                                .mt(px(4.))
                                .py(px(1.))
                                .rounded(px(4.))
                                .cursor_pointer()
                                .flex()
                                .items_center()
                                .gap(px(4.))
                                .text_size(px(14.))
                                .line_height(px(18.))
                                .font_weight(FontWeight(500.))
                                .text_color(color("--text-muted"))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    match (guild, channel) {
                                        // The footer goes to the original message (`jumpToMessage`).
                                        (_, Some(channel)) if message_id.is_some() => {
                                            if let Some(original) = message_id {
                                                this.jump_to_message_id(
                                                    channel, original, true, None,
                                                );
                                            }
                                        }
                                        (Some(guild), Some(channel)) => {
                                            this.select(guild, Some(channel))
                                        }
                                        (None, Some(channel)) => this.select_home(Some(channel)),
                                        _ => {}
                                    }
                                    cx.notify();
                                }))
                                .child(footer)
                                .child(icon("forwarded_chevron", 12., "--text-muted"))
                        })),
                )
                .into_any_element(),
        )
    }

    fn accessories(&self, message: &Message, cx: &mut Context<Self>) -> Option<AnyElement> {
        let parts: Vec<AnyElement> = [
            self.attachments(message, cx),
            self.files(message, cx),
            self.embeds(message, cx),
            self.message_stickers(message.id, &message.sticker_items, cx),
            self.message_components(message, cx),
            self.invite_cards(message, cx),
            self.poll_card(message, cx),
            // `renderSafetySystemNotification` (module 679636): last of the accessories.
            self.safety_system_notice(message, cx),
        ]
        .into_iter()
        .flatten()
        .collect();
        (!parts.is_empty()).then(|| {
            div()
                .py(px(2.))
                .flex()
                .flex_col()
                .gap(px(4.))
                .children(parts)
                .into_any_element()
        })
    }

    fn content(&self, message: &Message) -> impl IntoElement {
        if !self.failed_tint.get()
            && let Some(official) = self.official_text.get()
        {
            return self.content_colored(message, official.into());
        }
        // Not in the pinned client: RoleColorEverywhere's `colorChatMessages`, not on a failed send
        // (`plugin_hooks/role_color_everywhere.rs`).
        if !self.failed_tint.get()
            && let Some(mixed) =
                self.role_color_everywhere_message(message.author.id, hsla("--text-default"))
        {
            return self.content_colored(message, mixed);
        }
        self.content_tinted(
            message,
            if self.failed_tint.get() {
                "--text-feedback-critical"
            } else {
                "--text-default"
            },
        )
    }

    /// `content` in another text colour token (a forum gallery card's read preview is muted).
    fn content_tinted(&self, message: &Message, token: &str) -> gpui::Div {
        self.content_colored(message, hsla(token))
    }

    fn content_colored(&self, message: &Message, text: Hsla) -> gpui::Div {
        // `.guildOfficialMessageContent` colours the list markers it holds as well (they inherit the content colour).
        let marker_color = self
            .official_text
            .get()
            .unwrap_or_else(|| color("--text-default"));
        let blocks = parse(&message.content);
        let gaps = block_gaps(&blocks);
        // The last item's bottom margin stays inside the message.
        let trailing = if matches!(blocks.last(), Some(Block::ListItem { .. })) {
            4.
        } else {
            0.
        };
        let last = blocks.len().saturating_sub(1);
        let mut marks = Vec::new();
        if message.edited {
            marks.push(("(edited)".to_owned(), message.edited_at));
        }
        let reverted = self.automod_blocked_edit(message.id).then(|| {
            (
                format!("({})", dc_intl::t("Z7eEx9")),
                Some(message.timestamp),
            )
        });
        marks.extend(reverted.clone());
        // Module 469759 puts " (reverted)" after the content: a message whose content does not end in text (only
        // attachments, a code block) gets it on a line of its own.
        let reverted_alone = reverted
            .filter(|_| !matches!(blocks.last(), Some(Block::Paragraph(_))))
            .map(|mark| {
                div().child(self.spans_edited(
                    &[],
                    text,
                    (message.id, blocks.len() as u32),
                    &[mark],
                ))
            });
        div()
            .flex()
            .flex_col()
            .pb(px(trailing))
            .children(
                blocks
                    .into_iter()
                    .zip(gaps)
                    .enumerate()
                    .map(|(index, (block, gap))| {
                        let key = (message.id, index as u32);
                        match block {
                            Block::Paragraph(spans) => div()
                                .min_h(px((self.chat_font_size() * 1.375).round()))
                                .mt(px(gap))
                                .child(self.spans_edited(
                                    &spans,
                                    text,
                                    key,
                                    if index == last { &marks } else { &[] },
                                ))
                                .into_any_element(),
                            Block::Heading { level, spans } => div()
                                .mt(px(gap))
                                .child(self.heading(&spans, level, key))
                                .into_any_element(),
                            Block::Subtext(spans) => div()
                                .mt(px(gap))
                                .child(self.subtext(&spans, key))
                                .into_any_element(),
                            Block::ListItem {
                                depth,
                                number,
                                spans,
                            } => {
                                let indent = 16. * (f32::from(depth) + 1.);
                                let marker = match number {
                                    Some(number) => div()
                                        .absolute()
                                        .left(px(-18.))
                                        .top(px(0.))
                                        .line_height(px(22.))
                                        .text_color(marker_color)
                                        .child(format!("{number}."))
                                        .into_any_element(),
                                    None => {
                                        let disc = div()
                                            .absolute()
                                            .left(px(-17.))
                                            .top(px(8.))
                                            .size(px(5.))
                                            .rounded_full();
                                        if depth == 0 {
                                            disc.bg(marker_color).into_any_element()
                                        } else {
                                            disc.border_1()
                                                .border_color(marker_color)
                                                .into_any_element()
                                        }
                                    }
                                };
                                div()
                                    .relative()
                                    .mt(px(gap))
                                    .ml(px(indent))
                                    .min_h(px(22.))
                                    .child(marker)
                                    .child(self.spans(&spans, text, key))
                                    .into_any_element()
                            }
                            Block::Quote(spans) => div()
                                .mt(px(gap))
                                .flex()
                                .child(
                                    div()
                                        .flex_none()
                                        .w(px(4.))
                                        .rounded(px(4.))
                                        .bg(color("--spine-default")),
                                )
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w(px(0.))
                                        .pl(px(12.))
                                        .pr(px(8.))
                                        .child(self.spans(&spans, hsla("--text-subtle"), key)),
                                )
                                .into_any_element(),
                            Block::Code { language, text } => {
                                self.code_block(key, gap, language.as_deref(), &text)
                            }
                        }
                    }),
            )
            .children(reverted_alone)
    }

    fn row(&self, index: usize, cx: &mut Context<Self>) -> AnyElement {
        let row_ix = index;
        let Some(row) = self.rows.get(index) else {
            return div().into_any_element();
        };
        match row {
            Row::Spacer => div().h(px(40.)).into_any_element(),
            Row::NewDivider => self.new_divider(),
            Row::PostIntro => self.forum_post_intro(),
            Row::PostActions => self.forum_post_actions(cx),
            Row::Blocked { first, count, kind } => self.blocked_row(*first, *count, *kind, cx),
            Row::Welcome => if self.voice_view_open() {
                self.voice_welcome()
            } else if self.current_dm().is_none() {
                self.new_server_welcome(cx)
                    .or_else(|| self.channel_welcome(cx))
            } else {
                self.dm_welcome(cx)
            }
            .unwrap_or_else(|| div().into_any_element()),
            Row::Divider(date) => div()
                .pt(px(24.))
                .pl(px(16.))
                .pr(px(30.))
                .child(
                    div()
                        .h(px(1.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .bg(color("--background-mod-subtle"))
                        .child(
                            div()
                                .px(px(4.))
                                .bg(color("--background-base-lower"))
                                .text_size(px(12.))
                                .line_height(px(13.))
                                .font_weight(FontWeight(600.))
                                .text_color(color("--text-muted"))
                                .child(date.clone()),
                        ),
                )
                .into_any_element(),
            Row::Message { index, head } => {
                let channel = self.channel.unwrap_or_default();
                let Some(message) = self
                    .stores
                    .messages
                    .get(&channel)
                    .and_then(|messages| messages.get(*index))
                else {
                    return div().into_any_element();
                };
                // AutoMod's raid notification (module 117096 `iC`).
                if automod_alert::is_raid_card(message) {
                    let line = self.automod_raid_line(message, cx);
                    return self.system_row(message, *head, line, cx);
                }
                match dc_state::message_kinds::presentation(message) {
                    // The official table has no entry for this type, or its handler returns `null`.
                    dc_state::message_kinds::Presentation::Nothing => {
                        return div().into_any_element();
                    }
                    dc_state::message_kinds::Presentation::System if !dm::is_system(message) => {
                        return match self.kind_line(message, cx) {
                            Some(line) => self.system_row(message, *head, line, cx),
                            None => div().into_any_element(),
                        };
                    }
                    _ => {}
                }
                if dm::is_system(message) {
                    let line = self.system_message(message, cx);
                    return self.system_row(message, *head, line, cx);
                }
                // THREAD_STARTER_MESSAGE draws the message it references as an ordinary message (module 891522, `n$`
                // and `nq`: no toolbar, menu or reactions), or the "could not load" line when it is not known.
                let starter_copy;
                let is_starter = message.kind == dc_state::message_kinds::THREAD_STARTER_MESSAGE;
                let mut missing_starter = false;
                let (message, head) = if is_starter {
                    match self.starter_target(message) {
                        Some(target) => {
                            starter_copy = target;
                            (&starter_copy, &true)
                        }
                        None => {
                            missing_starter = true;
                            (message, &true)
                        }
                    }
                } else {
                    (message, head)
                };
                // The 17px between groups is outside the hover box, and the box stops 16px short of the edge. Padding,
                // not margin: gpui lists measure an item without its margins.
                let id = message.id;
                let own = self
                    .stores
                    .current_user
                    .as_ref()
                    .is_some_and(|user| user.id == message.author.id);
                // The row whose context menu is open keeps its hover tint and toolbar (the `contextMenuOpen` message
                // state; frames message-menu/*, audio-attachment/context-menu).
                let menu_open = self.menu.is_some_and(|open| open.message == id);
                // The bar's reaction picker selects the row too (frame pickers/emoji-reaction: `selected_SY_P2T`).
                let selected = menu_open || self.reaction_picker_on(message);
                let hover_locked = self.message_hover_locked();
                let pointer = !hover_locked && self.hovered == Some(id);
                let hovered = pointer || selected;
                let compact = self.compact_messages();
                // `.replying` and `.highlighted` (524434): the highlight tint with a 2px brand bar; `.mentioned`: the
                // warning tint with a 2px warning bar. The hover tints differ per state.
                let replying = self.replying.as_ref().is_some_and(|reply| reply.id == id);
                let mentioned = !replying && self.mentions_me(message);
                let ephemeral =
                    !replying && message.flags & dc_state::components::FLAG_EPHEMERAL != 0;
                // `.guildOfficialMessage_SY_P2T` (official_messages): its own hover tint, its text colour.
                let official = self.official_look(message);
                let (rest, over, bar) = if replying {
                    (
                        Some("--message-highlight-background-default"),
                        // `--brand-10a` is the brand colour at 10% (524434 `.replying:hover`).
                        gpui::Rgba {
                            a: 0.1,
                            ..color("--brand-500")
                        },
                        Some("--text-brand"),
                    )
                } else if mentioned {
                    (
                        Some("--message-mentioned-background-default"),
                        color("--message-mentioned-background-hover"),
                        Some("--icon-feedback-warning"),
                    )
                } else if let Some(look) = official {
                    (None, look.hover, ephemeral.then_some("--brand-500"))
                } else if ephemeral {
                    // `.ephemeral`: `--brand-05a` with a 2px `--brand-500` bar; hover `--brand-10a` (SY_P2T).
                    (
                        None,
                        gpui::Rgba {
                            a: 0.1,
                            ..color("--brand-500")
                        },
                        Some("--brand-500"),
                    )
                } else {
                    (None, color("--message-background-hover"), None)
                };
                let editing = self
                    .editing
                    .as_ref()
                    .filter(|(target, _)| *target == id)
                    .map(|(_, editor)| editor.clone());
                let body = self
                    .message_stop(
                        div().id(SharedString::from(format!("message-{id}"))),
                        id,
                        row_ix,
                        cx,
                    )
                    .relative()
                    .children(self.flash_layers(id))
                    .when_some(rest, |this, token| this.bg(color(token)))
                    .when(ephemeral && !mentioned, |this| {
                        this.bg(gpui::Rgba {
                            a: 0.05,
                            ..color("--brand-500")
                        })
                    })
                    .when(selected, |this| this.bg(over))
                    .children(bar.map(|token| {
                        div()
                            .absolute()
                            .left_0()
                            .top_0()
                            .bottom_0()
                            .w(px(2.))
                            .bg(color(token))
                    }))
                    .pl(px(72.))
                    .pr(px(24.))
                    .pt(px(2.))
                    .pb(px(2.))
                    // `.mouse-mode.full-motion .message_SY_P2T:hover` (stylesheet 524434): no transition, and none with
                    // reduced motion. The hover bar is inside the message, so the pointer on it keeps the tint.
                    .when(!motion::reduced() && !hover_locked, |this| {
                        this.hover(move |style| style.bg(over))
                            .when(pointer, |this| this.bg(over))
                    })
                    .on_hover(cx.listener(move |this, inside: &bool, window, cx| {
                        if this.message_hover_locked() {
                            return;
                        }
                        this.hovered = hover_bar::hover_after(
                            id,
                            *inside,
                            this.hovered,
                            window.mouse_position(),
                        );
                        cx.notify();
                    }))
                    .when(!is_starter, |this| {
                        this.on_mouse_up(
                            MouseButton::Right,
                            cx.listener(move |this, event: &gpui::MouseUpEvent, _, cx| {
                                this.open_menu(menu::Anchor::Pointer(event.position), id, own, cx);
                            }),
                        )
                    })
                    // Not in the pinned client: the MessageClickActions plugin's clicks (`plugin_hooks/message_clicks.rs`).
                    .when(
                        !is_starter && editing.is_none() && !crate::plugins::bypassed(),
                        |this| {
                            this.on_click(cx.listener(
                                move |this, event: &gpui::ClickEvent, window, cx| {
                                    this.message_clicked(id, event, window, cx);
                                },
                            ))
                        },
                    )
                    .when(hovered, |this| this.child(hover_bar::probe(id, false)))
                    .when(hovered && editing.is_none() && !is_starter, |this| {
                        // No bar while a message is sending, Retry and Delete once it failed, and none on a private
                        // message (module 891522).
                        match self.send_state_of(id) {
                            Some(send_state::SendState::Sending) => this,
                            Some(send_state::SendState::Failed) => {
                                this.child(self.failed_toolbar(id, cx))
                            }
                            None if message.flags & 64 != 0 => this,
                            None => this.child(self.message_toolbar_at(
                                message,
                                own,
                                hover_bar::bar_top(
                                    *head,
                                    compact,
                                    message.kind,
                                    message.reference_id.is_some(),
                                ),
                                cx,
                            )),
                        }
                    });
                let upload = self.upload_message_content(message, cx);
                let content = match (editing, upload) {
                    (Some(editor), _) => self.edit_box(editor).into_any_element(),
                    (None, _) if missing_starter => self.starter_missing_line(message),
                    (None, Some(upload)) => upload,
                    (None, None) => div()
                        .when(
                            !self.hides_inline_media_content(message)
                                && !self.command_replaces_content(message)
                                && (!message.content.is_empty()
                                    || message.edited
                                    || self.automod_blocked_edit(id)),
                            |this| {
                                // `isFailed` colours the text, the links and the inline code; `isSending` fades the
                                // content (module 74096).
                                let state = self.send_state_of(id);
                                // `blockedSend_REFxkR` keeps an AutoMod-blocked message in `--text-default`.
                                self.failed_tint.set(
                                    state == Some(send_state::SendState::Failed)
                                        && !self.automod_blocked(id),
                                );
                                self.official_text.set(official.and_then(|look| look.text));
                                let content = self.content(message);
                                self.failed_tint.set(false);
                                self.official_text.set(None);
                                this.child(
                                    div()
                                        .when(
                                            state == Some(send_state::SendState::Sending)
                                                && !matches!(message.kind, 20 | 23),
                                            |this| this.opacity(0.5),
                                        )
                                        .child(content),
                                )
                            },
                        )
                        // Module 469759 draws AutoMod's notice inside the content's own block, before the
                        // attachments, embeds and reactions.
                        .children(self.automod_blocked_notice(message, compact, cx))
                        .children(self.command_status(message))
                        .children(self.forwarded_block(message, cx))
                        .children(self.accessories(message, cx))
                        .children((!is_starter).then(|| self.reactions(message, cx)).flatten())
                        .children(self.ephemeral_notice(message, cx))
                        .into_any_element(),
                };
                let row = div().pr(px(16.));
                if !*head && !compact {
                    // `.timestamp.alt` with `timestampVisibleOnHover` (stylesheet 738015): the short time in the
                    // 56px gutter, right-aligned, 11px `--text-muted`, shown while the pointer is on the message.
                    let time = pointer.then(|| {
                        self.with_tip(
                            format!("message-alt-time-{id}"),
                            Tip::body(
                                Body::Text(timestamps::full_time(message.local_time()).into()),
                                Side::Top,
                            )
                            .after(timestamps::DELAY_MS),
                            div()
                                .w(px(56.))
                                .h(px(22.))
                                .flex()
                                .justify_end()
                                .text_size(px(11.))
                                .line_height(px(22.))
                                .font_weight(FontWeight(500.))
                                .text_color(color("--text-muted"))
                                .child(settings::prefs::clock(&message.local_time())),
                            cx,
                        )
                        .absolute()
                        .left_0()
                        .top(px(2.))
                    });
                    return row
                        .child(body.children(time).child(content))
                        .into_any_element();
                }
                let avatar = self
                    .images
                    .get(&self.guild_avatar_source(&message.author).0)
                    .cloned();
                // The group's avatar plays while any of its messages is hovered (module 21299 `F`).
                let playing = hovered || self.group_hovered(row_ix);
                // Module 549587: a timed-out author, for a moderator, has the clock mark and a half-faded avatar
                // (`communicationDisabledOpacity_2i8Bfm`).
                let timed_out = self.shows_timeout(message);
                let reference = self.reply_reference(message);
                // A command's reply has the "used /command" bar where a reply has its preview.
                let command_bar = if reference.is_none() {
                    self.interaction_bar(message)
                } else {
                    None
                };
                let reply_offset = if reference.is_some() || command_bar.is_some() {
                    22.
                } else {
                    0.
                };
                let preview = reference.map(|reference| match reference {
                    Reference::Loaded(target) => {
                        let preview = self.reply_preview(&target).into_any_element();
                        div()
                            .id(SharedString::from(format!("reply-jump-{}", message.id)))
                            .mb(px(4.))
                            .cursor_pointer()
                            // The preview jumps to the message replied to, through the check of module 81437
                            // (module 21299 `z`).
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.jump_to_message_checked(&target, cx);
                            }))
                            .child(preview)
                            .into_any_element()
                    }
                    Reference::NotLoaded { channel, id } => div()
                        .id(SharedString::from(format!("reply-jump-{}", message.id)))
                        .mb(px(4.))
                        .cursor_pointer()
                        // `onClickReply` without a loaded message: `jumpToMessage` with no check.
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.jump_to_message_id(channel, id, true, None);
                            cx.notify();
                        }))
                        .child(self.reply_missing(message.id, false))
                        .into_any_element(),
                    Reference::Deleted => div()
                        .mb(px(4.))
                        .child(self.reply_missing(message.id, true))
                        .into_any_element(),
                });
                if compact {
                    // `.compact` (stylesheet 738015): every message carries its own header line, the time in an
                    // `2.25rem` (`3.1rem` at 24 hours) box ending `0.25rem` before the name, 11px muted; the avatar,
                    // when "Show user avatars" is on, is `1em` wide with `.1em .25em` margins; the name is followed by
                    // `0.25rem`. The text starts after the name; wrapped lines here stay in that column, where the
                    // official wraps them back to the 5rem indent.
                    let size = self.chat_font_size();
                    let time_width = if settings::prefs::clock_24() {
                        49.6
                    } else {
                        36.
                    };
                    let show_avatar = self.pref_on("appearance_display_compact_avatars", true);
                    return row
                        .pt(px(self.message_group_spacing()))
                        .child(
                            body.pl(px(16.))
                                .children(preview)
                                .children(command_bar)
                                .child(
                                    div()
                                        .flex()
                                        .items_start()
                                        .child(
                                            div()
                                                .flex_none()
                                                .min_w(px(time_width))
                                                .mr(px(4.))
                                                .flex()
                                                .justify_end()
                                                .text_size(px(11.))
                                                .line_height(px(22.))
                                                .text_color(color("--text-muted"))
                                                .child(settings::prefs::clock(
                                                    &message.local_time(),
                                                )),
                                        )
                                        .when(show_avatar, |this| {
                                            this.child(
                                                div()
                                                    .flex_none()
                                                    .when(timed_out, |avatar| avatar.opacity(0.5))
                                                    .size(px(size))
                                                    .mt(px(size * 0.18 + 3.))
                                                    .mx(px(size * 0.1))
                                                    .mr(px(size * 0.25))
                                                    .children(
                                                        self.live_avatar(
                                                            &message.author,
                                                            size,
                                                            playing,
                                                            cx,
                                                        )
                                                        .or_else(|| {
                                                            avatar.map(|image| {
                                                                img(image)
                                                                    .size(px(size))
                                                                    .rounded_full()
                                                                    .into_any_element()
                                                            })
                                                        }),
                                                    ),
                                            )
                                        })
                                        .when(timed_out, |this| {
                                            this.child(self.timeout_mark(message, true, cx))
                                        })
                                        .child(
                                            div()
                                                .flex_none()
                                                .h(px(22.))
                                                .mr(px(4.))
                                                .flex()
                                                .items_center()
                                                .child(
                                                    self.profile_trigger(
                                                        format!("name-{id}"),
                                                        message.author.id,
                                                        div()
                                                            .id(SharedString::from(format!(
                                                                "name-link-{id}"
                                                            )))
                                                            .hover(|style| style.underline())
                                                            .font_weight(FontWeight(500.))
                                                            .text_color(self.guild_author_color(
                                                                message.author.id,
                                                                "--text-strong",
                                                            ))
                                                            .line_height(px(22.))
                                                            .child(self.author_name(
                                                                format!("author-{id}"),
                                                                &message.author,
                                                            )),
                                                        cx,
                                                    ),
                                                ),
                                        )
                                        .child(div().flex_1().min_w(px(0.)).child(content)),
                                ),
                        )
                        .into_any_element();
                }
                // `messageGroupSpacing` (16px by default) and the 1px of the group's own border.
                row.pt(px(self.message_group_spacing() + 1.))
                    .child(
                        body.min_h(px(48. + reply_offset))
                            .children(preview)
                            .children(command_bar)
                            .child(
                                div()
                                    .absolute()
                                    .left(px(16.))
                                    .top(px(4. + reply_offset))
                                    .size(px(40.))
                                    .when(timed_out, |avatar| avatar.opacity(0.5))
                                    .child(
                                        self.profile_trigger(
                                            format!("avatar-{id}"),
                                            message.author.id,
                                            div().size(px(40.)).children(
                                                self.live_avatar(&message.author, 40., playing, cx)
                                                    .or_else(|| {
                                                        avatar.map(|image| {
                                                            img(image)
                                                                .size(px(40.))
                                                                .rounded_full()
                                                                .into_any_element()
                                                        })
                                                    }),
                                            ),
                                            cx,
                                        ),
                                    ),
                            )
                            .child(
                                div()
                                    .h(px(22.))
                                    .flex()
                                    .items_center()
                                    .when(timed_out, |header| {
                                        header.child(self.timeout_mark(message, false, cx))
                                    })
                                    // "Next to names": a dot in the role's colour before the name (`roleStyle`).
                                    .children(
                                        self.role_dots_shown()
                                            .then(|| self.role_color_of(message.author.id))
                                            .flatten()
                                            .map(|rgb| {
                                                div()
                                                    .flex_none()
                                                    .size(px(20.))
                                                    .mr(px(4.))
                                                    .rounded(px(4.))
                                                    .bg(color("--background-mod-normal"))
                                                    .flex()
                                                    .items_center()
                                                    .justify_center()
                                                    // Not in the pinned client: BetterRoleDot copies the
                                                    // colour (`plugin_hooks/better_role_dot.rs`).
                                                    .child(self.role_dot_copy(
                                                        format!("message-{id}"),
                                                        rgb,
                                                        self.role_dot_of(message.author.id, rgb),
                                                        cx,
                                                    ))
                                            }),
                                    )
                                    .child(
                                        self.profile_trigger(
                                            format!("name-{id}"),
                                            message.author.id,
                                            div()
                                                .id(SharedString::from(format!("name-link-{id}")))
                                                // `.username.clickable:hover` underlines the name (module 2i8Bfm).
                                                .hover(|style| style.underline())
                                                .font_weight(FontWeight(500.))
                                                .text_color(self.guild_author_color(
                                                    message.author.id,
                                                    "--text-strong",
                                                ))
                                                .line_height(px(22.))
                                                .child(self.author_name(
                                                    format!("author-{id}"),
                                                    &message.author,
                                                )),
                                            cx,
                                        ),
                                    )
                                    .children(self.header_decorations(message, cx))
                                    .child(
                                        // `Timestamp` (module 776579): the long date opens after the pointer rests 750ms.
                                        self.with_tip(
                                            format!("message-time-{id}"),
                                            Tip::body(
                                                Body::Text(
                                                    timestamps::full_time(message.local_time())
                                                        .into(),
                                                ),
                                                Side::Top,
                                            )
                                            .after(timestamps::DELAY_MS),
                                            div()
                                                .text_size(px(12.))
                                                .line_height(px(22.))
                                                .font_weight(FontWeight(500.))
                                                .text_color(color("--channels-default"))
                                                .child(header_time(
                                                    message.local_time(),
                                                    chrono::Local::now(),
                                                )),
                                            cx,
                                        )
                                        .ml(px(8.)),
                                    )
                                    // Not in the pinned client: UserMessagesPronouns
                                    // (`plugin_hooks/message_pronouns.rs`).
                                    .children(self.message_pronouns(message, cx))
                                    .children(self.header_badges(message, cx)),
                            )
                            .child(content),
                    )
                    .into_any_element()
            }
        }
    }

    /// The row around a system line ("X started a call.", a join, a pin): a `.message` like any other, so it takes
    /// the hover tint, the hover bar and the message menu. The 17px between groups is outside the hover box, and the
    /// box stops 16px short of the edge, as for an ordinary message.
    fn system_row(
        &self,
        message: &Message,
        head: bool,
        line: AnyElement,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let id = message.id;
        let own = self
            .stores
            .current_user
            .as_ref()
            .is_some_and(|user| user.id == message.author.id);
        let menu_open = self.menu.is_some_and(|open| open.message == id);
        let selected = menu_open || self.reaction_picker_on(message);
        let hover_locked = self.message_hover_locked();
        let pointer = !hover_locked && self.hovered == Some(id);
        let hovered = pointer || selected;
        let over = color("--message-background-hover");
        div()
            .pt(px(if head { 17. } else { 0. }))
            .pr(px(16.))
            .child(
                div()
                    .id(SharedString::from(format!("system-row-{id}")))
                    .relative()
                    .when(selected, |this| this.bg(over))
                    .when(!motion::reduced() && !hover_locked, |this| {
                        this.hover(move |style| style.bg(over))
                            .when(pointer, |this| this.bg(over))
                    })
                    .on_hover(cx.listener(move |this, inside: &bool, window, cx| {
                        if this.message_hover_locked() {
                            return;
                        }
                        this.hovered = hover_bar::hover_after(
                            id,
                            *inside,
                            this.hovered,
                            window.mouse_position(),
                        );
                        cx.notify();
                    }))
                    .on_mouse_up(
                        MouseButton::Right,
                        cx.listener(move |this, event: &gpui::MouseUpEvent, _, cx| {
                            this.open_menu(menu::Anchor::Pointer(event.position), id, own, cx);
                        }),
                    )
                    .child(line)
                    .when(hovered, |this| this.child(hover_bar::probe(id, false)))
                    .when(hovered, |this| {
                        // A system line is never `isHeader` (module 143413), so its bar is 25px up.
                        this.child(self.message_toolbar_at(
                            message,
                            own,
                            hover_bar::bar_top(
                                head,
                                self.compact_messages(),
                                message.kind,
                                message.reference_id.is_some(),
                            ),
                            cx,
                        ))
                    }),
            )
            .into_any_element()
    }

    /// The hover bar of a message (`nA` in module 891522), `top` px from its message's box (`hover_bar::bar_top`).
    /// `.container_V_VY9Q` is 14px in from the row's right edge; `.popover_DpHsW6` is a 1px `--border-muted` box with
    /// `--shadow-low`, turning `--border-subtle` with `--shadow-medium` while the pointer is over it.
    fn message_toolbar_at(
        &self,
        message: &Message,
        own: bool,
        top: f32,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        use hover_bar::Button;
        let id = message.id;
        let text = message.content.clone();
        let mut caps = self.bar_caps(message, own);
        let picker_open = self.reaction_picker_on(message);
        let more_open = self.menu.is_some_and(|open| {
            open.message == id && matches!(open.anchor, menu::Anchor::MoreButton(_))
        });
        // `isExpanded`: Shift held, not in keyboard mode, and no picker or menu of the bar open.
        caps.expanded =
            hover_bar::shift_held() && !self.keyboard_mode() && !picker_open && !more_open;
        let quick: Vec<&'static dc_tokens::emoji_data::Emoji> = match plugin_hooks::more_quick_grid() {
            // Not in the pinned client: MoreQuickReactions shows `reactionCount` of them
            // (`plugin_hooks/more_quick_reactions.rs`).
            Some(grid) => self.hover_bar_reactions_up_to(grid.reaction_count),
            None => self.hover_bar_reactions(),
        };
        // `.hoverBarButton:hover .icon` is `scale(1.1)` over `.4s` (module 194085, stylesheet 28636); the button's
        // colour is `--interactive-text-default`, `-hover` under the pointer, `-active` when selected (319714).
        let button = |name: &str| {
            let over = motion::hovered(motion::messages::hover_key(name));
            let selected = name == "msg_more" && more_open;
            let dangerous = name == "msg_trash";
            let tint = match (dangerous, over || selected) {
                (true, false) => "--control-critical-secondary-text-default",
                (true, true) => "--control-critical-secondary-text-hover",
                (false, false) => "--interactive-text-default",
                (false, true) if selected => "--interactive-text-active",
                (false, true) => "--interactive-text-hover",
            };
            div()
                .id(SharedString::from(format!("toolbar-button-{name}")))
                .relative()
                .size(px(28.))
                .rounded(px(6.))
                .flex()
                .items_center()
                .justify_center()
                .cursor_pointer()
                .when(selected, |this| {
                    this.bg(color("--interactive-background-selected"))
                })
                .hover(|style| style.bg(color("--interactive-background-hover")))
                // `.hoverBarButton:active:not([aria-disabled=true])` (28636).
                .active(|style| style.bg(color("--interactive-background-active")))
                .child(motion::messages::icon_box(name, icon(name, 20., tint)))
                .child(motion::hover_probe(motion::messages::hover_key(name)))
                .child(motion::messages::press_probe(name))
        };
        let popover = motion::key(&("hover-bar-popover", id));
        let lift = motion::hover_value(popover, motion::css(100., motion::Ease::EaseOut));
        let mut bar = div()
            .id(SharedString::from(format!("toolbar-{id}")))
            .occlude()
            .on_hover(cx.listener(move |this, inside: &bool, window, cx| {
                // The bar reaches above its row, so it keeps the row's hover alive; leaving it for somewhere outside
                // its message ends it.
                if *inside {
                    this.hovered = Some(id);
                } else if this.hovered == Some(id)
                    && !hover_bar::row_contains(id, window.mouse_position())
                {
                    this.hovered = None;
                }
                cx.notify();
            }))
            .absolute()
            .top(px(top))
            .right(px(14.))
            .h(px(34.))
            .p(px(2.))
            .rounded(px(8.))
            .bg(color("--background-surface-high"))
            .border_1()
            .border_color(color(if motion::hovered(popover) {
                "--border-subtle"
            } else {
                "--border-muted"
            }))
            .shadow(vec![hover_bar::popover_shadow(lift)])
            .flex()
            .items_center()
            .child(hover_bar::probe(id, true))
            .child(motion::hover_probe(popover));
        for which in hover_bar::buttons(caps) {
            let (label, name) = hover_bar::label_icon(which, caps);
            let tip_key = format!("toolbar-{name}");
            let tipped = |cx: &mut Context<Self>| {
                self.with_tip(
                    tip_key.clone(),
                    Tip::text(label, Side::Top),
                    button(name),
                    cx,
                )
                .cursor_pointer()
            };
            bar = match which {
                // The three most frecent reactions, then a separator (`np`, `separator_DpHsW6`).
                Button::Quick => bar
                    .children(quick.iter().enumerate().map(|(index, emoji)| {
                        let emoji = *emoji;
                        let picture = self
                            .images
                            .get(&format!("emoji:{}", emoji.surrogates))
                            .cloned();
                        let channel = message.channel_id;
                        let reacted = message
                            .reactions
                            .iter()
                            .any(|reaction| reaction.me && reaction.name == emoji.surrogates);
                        let key = format!("quick-{index}");
                        let target = div()
                            .id(SharedString::from(format!("toolbar-quick-{index}")))
                            .relative()
                            .size(px(28.))
                            .rounded(px(6.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .cursor_pointer()
                            .hover(|style| style.bg(color("--interactive-background-hover")))
                            .active(|style| style.bg(color("--interactive-background-active")))
                            .children(picture.map(|picture| {
                                motion::messages::icon_box(&key, img(picture).size(px(20.)))
                            }))
                            .child(motion::hover_probe(motion::messages::hover_key(&key)))
                            .child(motion::messages::press_probe(&key));
                        self.with_tip(
                            format!("toolbar-quick-tip-{index}"),
                            Tip::body(
                                Body::Emoji(
                                    format!(":{}:", emoji.names[0]).into(),
                                    hover_bar::quick_hint(reacted).into(),
                                ),
                                Side::Top,
                            ),
                            target,
                            cx,
                        )
                        .on_click(cx.listener(move |this, _, _, cx| {
                            // `nC`: a reaction of your own is taken back.
                            if !reacted {
                                this.track_reaction(emoji.surrogates);
                            }
                            this.send_and_apply_reaction(
                                channel,
                                id,
                                menu::ReactionOp {
                                    emoji: emoji.surrogates.to_owned(),
                                    add: !reacted,
                                    location: "Message Hover Bar",
                                    burst: false,
                                },
                                cx,
                            );
                        }))
                    }))
                    .when(!quick.is_empty(), |this| {
                        this.child(
                            div()
                                .mx(px(4.))
                                .w(px(1.))
                                .h(px(24.))
                                .rounded_full()
                                .bg(color("--border-subtle")),
                        )
                    }),
                Button::AddReaction => {
                    bar.child(tipped(cx).on_click(cx.listener(move |this, _, window, cx| {
                        let anchor = this.tip_anchors.borrow().get("toolbar-msg_react").copied();
                        if let (Some(bounds), Some(channel)) = (anchor, this.channel) {
                            this.tip = None;
                            this.open_picker(
                                picker::Target::Reaction {
                                    channel,
                                    message: id,
                                },
                                gpui::point(bounds.left(), bounds.top()),
                                window,
                                cx,
                            );
                        }
                    })))
                }
                Button::Edit => {
                    let text = text.clone();
                    bar.child(tipped(cx).on_click(cx.listener(move |this, _, window, cx| {
                        this.tip = None;
                        this.begin_edit(id, &text, window, cx);
                    })))
                }
                Button::Reply => {
                    let found = message.clone();
                    bar.child(tipped(cx).on_click(cx.listener(move |this, _, window, cx| {
                        this.tip = None;
                        this.begin_reply(found.clone(), window, cx);
                        // `$b` of module 519222: a Shift click replies without the mention.
                        if hover_bar::shift_held() {
                            this.reply_ping = false;
                        }
                    })))
                }
                Button::Forward => {
                    let found = message.clone();
                    bar.child(tipped(cx).on_click(cx.listener(move |this, _, window, cx| {
                        this.tip = None;
                        this.open_forward(found.clone(), window, cx);
                    })))
                }
                Button::Publish => bar.children(self.publish_button(message, &button, cx)),
                Button::ViewThread => {
                    let guild = self.guild;
                    bar.child(tipped(cx).on_click(cx.listener(move |this, _, _, cx| {
                        this.tip = None;
                        if let Some(guild) = guild {
                            this.select(guild, Some(id));
                        }
                        cx.notify();
                    })))
                }
                Button::CopyId => {
                    let channel = message.channel_id;
                    bar.child(tipped(cx).on_click(cx.listener(move |_, _, _, cx| {
                        // `DT` of module 519222: with Shift, `<channel>-<message>`.
                        let value = if hover_bar::shift_held() {
                            format!("{channel}-{id}")
                        } else {
                            id.to_string()
                        };
                        actions::log_effect("Copy Message ID", "clipboard text", value.len());
                        cx.write_to_clipboard(gpui::ClipboardItem::new_string(value));
                    })))
                }
                Button::CopyLink
                | Button::MarkUnread
                | Button::Bookmark
                | Button::Pin
                | Button::Thread
                | Button::Delete => {
                    let action = match which {
                        Button::CopyLink => menu::Action::CopyLink,
                        Button::MarkUnread => menu::Action::MarkUnread,
                        Button::Bookmark => menu::Action::Bookmark,
                        Button::Pin => menu::Action::Pin,
                        Button::Thread => menu::Action::Thread,
                        _ => menu::Action::Delete,
                    };
                    bar.child(tipped(cx).on_click(cx.listener(move |this, _, window, cx| {
                        this.tip = None;
                        // `rS` and `RC` of module 519222: with Shift, pin and delete ask nothing.
                        if matches!(action, menu::Action::Pin | menu::Action::Delete)
                            && hover_bar::shift_held()
                        {
                            this.run_unconfirmed(action, id, cx);
                        } else {
                            this.run_action(action, id, window, cx);
                        }
                    })))
                }
                // Not in the pinned client: QuickMention puts `<@author> ` in the composer (`plugin_hooks.rs`).
                Button::QuickMention => {
                    let author = message.author.id;
                    bar.child(tipped(cx).on_click(cx.listener(move |this, _, _, cx| {
                        this.tip = None;
                        let text = plugin_hooks::mention_text(author);
                        this.composer
                            .update(cx, |composer, cx| composer.insert(&text, cx));
                        cx.notify();
                    })))
                }
                // Not in the pinned client: DownloadAllAttachments (`plugin_hooks/download_all.rs`).
                Button::DownloadAll => {
                    let attachments = message.attachments.clone();
                    bar.child(tipped(cx).on_click(cx.listener(move |this, _, _, cx| {
                        this.tip = None;
                        this.download_all_attachments(id, &attachments, cx);
                    })))
                }
                Button::More => {
                    bar.child(tipped(cx).on_click(cx.listener(move |this, _, _, cx| {
                        let anchor = this.tip_anchors.borrow().get("toolbar-msg_more").copied();
                        if let Some(bounds) = anchor {
                            this.tip = None;
                            this.open_menu(
                                menu::Anchor::MoreButton(gpui::point(bounds.left(), bounds.top())),
                                id,
                                own,
                                cx,
                            );
                        }
                    })))
                }
            };
        }
        bar
    }

    fn edit_box(&self, editor: Entity<Editor>) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .child(
                div()
                    .mt(px(8.))
                    .w_full()
                    .min_h(px(58.))
                    .rounded(px(8.))
                    .bg(color("--chat-background-default"))
                    .border_1()
                    .border_color(color("--background-mod-subtle"))
                    .px(px(15.))
                    .py(px(17.))
                    .child(editor.clone()),
            )
            .child(
                div()
                    .h(px(26.))
                    .pt(px(7.))
                    .text_size(px(12.))
                    .line_height(px(12.))
                    .text_color(color("--text-default"))
                    .flex()
                    // `wDsPXs`: "escape to [cancel] • enter to [save]", its four runs in order.
                    .child(edit_hint(0))
                    .child(
                        div()
                            .id("edit-cancel")
                            .cursor_pointer()
                            .text_color(color("--text-link"))
                            .on_click({
                                let root = self.weak.clone();
                                move |_, _, cx| {
                                    let _ = root.update(cx, |this, cx| {
                                        this.editing = None;
                                        cx.notify();
                                    });
                                }
                            })
                            .child(edit_hint(1)),
                    )
                    .child(edit_hint(2))
                    .child(
                        div()
                            .id("edit-save")
                            .cursor_pointer()
                            .text_color(color("--text-link"))
                            .on_click({
                                let editor = editor.clone();
                                move |_, _, cx| editor.update(cx, |editor, cx| editor.submit(cx))
                            })
                            .child(edit_hint(3)),
                    ),
            )
    }

    fn composer_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        // A thread's composer quotes its name: Message "name".
        // Module 480870: "Message {channel}" (`8lzR/R`, a DM's `4c+CAx`) with the channel's name as module 47167
        // writes it (`#name`, `@name`, `"thread"`).
        let placeholder = if self.current_thread().is_some() {
            dc_intl::t!("8lzR/R", channel = format!("\"{}\"", self.channel_title()))
        } else {
            // A voice channel's chat names it without the hash.
            let prefix = if self.voice_view_open() {
                ""
            } else if self.guild.is_some() {
                "#"
            } else if self.current_dm().is_some_and(|channel| channel.kind == 3) {
                ""
            } else {
                "@"
            };
            let channel = format!("{prefix}{}", self.channel_title());
            if self.guild.is_none() && self.current_dm().is_some() {
                dc_intl::t!("4c+CAx", channel = channel)
            } else {
                dc_intl::t!("8lzR/R", channel = channel)
            }
        };
        self.composer
            .update(cx, |composer, _| composer.set_placeholder(placeholder));
        let ping = self.reply_ping;
        let reply_bar = self.replying.as_ref().map(|message| {
            let own = self
                .stores
                .current_user
                .as_ref()
                .is_some_and(|me| me.id == message.author.id);
            div()
                .h(px(36.))
                .rounded_t(px(8.))
                .bg(color("--background-surface-higher"))
                .flex()
                .items_center()
                .pl(px(16.))
                .text_size(px(14.))
                .line_height(px(18.))
                .text_color(color("--text-default"))
                // Module 989161: "Replying to {userHook}" (`8E4GxS`), the hook the name in semibold.
                .children(
                    dc_intl::format_runs(
                        "8E4GxS",
                        &[("userHook", message.author.display_name().into())],
                    )
                    .into_iter()
                    .map(|(text, tag)| {
                        div()
                            .whitespace_nowrap()
                            .when(tag.is_some(), |this| this.font_weight(FontWeight(600.)))
                            .child(text)
                    }),
                )
                .child(div().flex_1())
                .children((!own).then(|| {
                    div()
                        .flex()
                        .items_center()
                        .child(
                            div()
                                .id("reply-ping")
                                .px(px(12.))
                                .py(px(8.))
                                .flex()
                                .items_center()
                                .gap(px(4.))
                                .cursor_pointer()
                                .text_size(px(12.))
                                .line_height(px(16.))
                                .font_weight(FontWeight(600.))
                                .text_color(if ping {
                                    color("--text-link")
                                } else {
                                    color("--text-muted")
                                })
                                .child(
                                    div()
                                        .w(px(16.))
                                        .text_size(px(16.))
                                        .line_height(px(16.))
                                        .child("@"),
                                )
                                .child(if ping {
                                    dc_intl::t("p9jC2r")
                                } else {
                                    dc_intl::t("U7f3bK")
                                })
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.reply_ping = !this.reply_ping;
                                    cx.notify();
                                })),
                        )
                        .child(div().w(px(1.)).h(px(20.)).bg(color("--border-subtle")))
                }))
                .child(
                    div()
                        .id("reply-close")
                        .w(px(40.))
                        .h(px(32.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .cursor_pointer()
                        // `.closeButton_Jldl6o:hover` (841136): the icon takes the hovered text colour.
                        .text_color(color("--interactive-text-default"))
                        .hover(|style| style.text_color(color("--interactive-text-hover")))
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.replying = None;
                            cx.notify();
                        }))
                        .child(
                            svg()
                                .path("icons/reply_close.svg")
                                .size(px(16.))
                                .flex_none(),
                        ),
                )
        });
        let emoji = self.images.get("emoji_button").cloned();
        let (colored_sprite, sprite_scale, sprite_cell) = motion::composer::emoji_button(false);
        let colored = self
            .images
            .get(&format!("button-sprite:{sprite_cell}"))
            .cloned();
        let emoji_button = self
            .header_stop(div().id("composer-emoji"), "emoji", cx)
            .size(px(32.))
            .flex()
            .items_center()
            .justify_center()
            .cursor_pointer()
            // `.emojiButton_2x8-Qn`: 8px corners, `--interactive-background-selected` on hover, `transition-duration:.2s`.
            .rounded(px(8.))
            .hover(|style| style.bg(color("--interactive-background-selected")))
            .rule(motion::Rule::ComposerButton)
            .on_click(cx.listener(|this, _, window, cx| {
                let composer = this.tip_anchors.borrow().get("composer-box").copied();
                if let Some(bounds) = composer {
                    this.open_picker(
                        picker::Target::Composer,
                        // The probe sits inside the composer's 1px border.
                        gpui::point(bounds.left(), bounds.top() - px(9.)),
                        window,
                        cx,
                    );
                }
            }))
            .relative()
            .child(
                div()
                    .size(px(18.))
                    .paint_scale(sprite_scale, gpui::point(0.5, 0.5))
                    .children(if colored_sprite && colored.is_some() {
                        colored.map(|image| img(image).size(px(18.)))
                    } else {
                        emoji.map(|image| img(image).size(px(18.)))
                    }),
            )
            .child(motion::hover_probe(motion::composer::hover_key()));
        let tool_sized = |name: &'static str, size: f32, cx: &mut Context<Self>| {
            self.header_stop(div().id(name), name, cx)
                .size(px(32.))
                .flex_none()
                .flex()
                .items_center()
                .justify_center()
                .cursor_pointer()
                // `.button_hfM7yP:not(.noHover_hfM7yP):hover`: 8px corners, `--interactive-background-selected` and the
                // hovered text colour over `.2s`; `.buttonWrapper_hfM7yP:hover` and `:active` the active text colour.
                .rounded(px(8.))
                .text_color(color("--interactive-text-default"))
                .hover(|style| {
                    style
                        .text_color(color("--interactive-text-active"))
                        .bg(color("--interactive-background-selected"))
                })
                .rule(motion::Rule::ComposerButton)
                .child(
                    svg()
                        .path(format!("icons/{name}.svg"))
                        .size(px(size))
                        .flex_none(),
                )
        };
        let tool = |name: &'static str, cx: &mut Context<Self>| tool_sized(name, 20., cx);
        // GIF opens the expression picker on its tab, from the same place as the emoji button.
        let gif_button = tool("gif", cx).on_click(cx.listener(move |this, _, window, cx| {
            let composer = this.tip_anchors.borrow().get("composer-box").copied();
            if let Some(bounds) = composer {
                this.open_picker_tab(
                    picker::Tab::Gifs,
                    gpui::point(bounds.left(), bounds.top() - px(9.)),
                    window,
                    cx,
                );
            }
        }));
        let docked = self.voice_chat_docked();
        // The gift button opens the gift purchase modal (`openGiftModal`, module 194509, location "gift-button").
        let gift_button = tool("gift", cx)
            .on_click(cx.listener(move |this, _, _, cx| this.open_purchase(true, cx)));
        // Not in the pinned client: PreviewMessage's eye, first in the bar (`plugin_hooks/preview_message.rs`).
        let preview_button = self.preview_message_shown(cx).then(|| {
            tool("clips_eye", cx).on_click(cx.listener(|this, _, _, cx| this.preview_message(cx)))
        });
        // Not in the pinned client: SilentMessageToggle's bell after it, drawn 1.2 times larger (`scale: 1.2`), crossed
        // out in the danger colour while off (`plugin_hooks/silent_message_toggle.rs`).
        let silent_button = self.silent_message_state().map(|silent| {
            let button = tool_sized(
                if silent {
                    "plugin_silent"
                } else {
                    "plugin_silent_off"
                },
                24.,
                cx,
            )
            .relative()
            .when(!silent, |this| {
                this.child(
                    svg()
                        .path("icons/plugin_silent_slash.svg")
                        .absolute()
                        .left(px(4.))
                        .top(px(4.))
                        .size(px(24.))
                        .text_color(color("--status-danger")),
                )
            })
            .on_click(cx.listener(|this, _, _, cx| this.silent_message_toggle(cx)));
            self.with_tip(
                "silent-message-toggle",
                tooltip::Tip::text(
                    if silent {
                        "Disable Silent Message"
                    } else {
                        "Enable Silent Message"
                    },
                    tooltip::Side::Top,
                ),
                button,
                cx,
            )
        });
        let sticker_button =
            tool("sticker", cx).on_click(cx.listener(move |this, _, window, cx| {
                let composer = this.tip_anchors.borrow().get("composer-box").copied();
                if let Some(bounds) = composer {
                    this.open_picker_tab(
                        picker::Tab::Stickers,
                        gpui::point(bounds.left(), bounds.top() - px(9.)),
                        window,
                        cx,
                    );
                }
            }));
        let apps_button = tool("apps", cx).on_click(cx.listener(|this, _, window, cx| {
            this.open_launcher(window, cx);
        }));
        // The send button (`eu`, module 4025): a 1px separator and a 32px button after the other buttons, drawn when
        // `AccessibilityStore.isSubmitButtonEnabled` is on and disabled while there is no text, upload or sticker
        // (`.button_plpOf8`, `.activeButtonChild_plpOf8`: `--text-brand`, `--icon-muted` when disabled).
        let send_button = self
            .pref_on("enable_send_button_outside_experiment", false)
            .then(|| {
                let empty =
                    self.composer.read(cx).text().is_empty() && self.pending_files.is_empty();
                let light = matches!(theme(), Theme::Light);
                div()
                    .flex()
                    .items_center()
                    .child(
                        div()
                            .w(px(1.))
                            .h(px(22.))
                            .mr(px(8.))
                            .bg(color("--border-normal")),
                    )
                    .child(
                        div()
                            .id("composer-send")
                            .size(px(32.))
                            .flex_none()
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded(px(8.))
                            .when(empty, |button| {
                                button
                                    .cursor_not_allowed()
                                    .text_color(color("--icon-muted"))
                            })
                            .when(!empty, |button| {
                                button
                                    .cursor_pointer()
                                    .text_color(color("--text-brand"))
                                    .hover(|style| {
                                        style
                                            .bg(color("--interactive-background-selected"))
                                            .text_color(color(if light {
                                                "--brand-560"
                                            } else {
                                                "--brand-430"
                                            }))
                                    })
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.autocomplete.open = None;
                                        this.composer.update(cx, |editor, cx| editor.submit(cx));
                                        cx.notify();
                                    }))
                            })
                            .child(
                                svg()
                                    .path("icons/send_message.svg")
                                    .size(px(20.))
                                    .flex_none(),
                            ),
                    )
            });
        // The form overlaps the list by 16px, as the official layout does.
        div()
            .relative()
            .flex_none()
            .mt(px(-16.))
            .px(px(8.))
            .pb(px(8.))
            .children(self.typing_bar(cx))
            .children(self.slowmode_notice(cx))
            .child(self.regions.composer_probe())
            .child(
                div()
                    .parity_with_border("composer", 1.)
                    .relative()
                    .children(settings::client_theme::cover(8.))
                    .child(self.anchor_probe("composer-box"))
                    .children(self.autocomplete_popup(cx))
                    .children(self.slash_popup(cx))
                    .children(self.option_popup(cx))
                    .children(self.launcher_popup(cx))
                    .children(self.length_counter(cx))
                    .min_h(px(58.))
                    .rounded(px(8.))
                    .when(settings::client_theme::active().is_none(), |composer| {
                        composer.bg(color("--chat-background-default"))
                    })
                    .border_1()
                    .border_color(color("--background-mod-subtle"))
                    .flex()
                    .flex_col()
                    .children(self.upload_area(cx))
                    .children(reply_bar)
                    .children(self.schedule_bar(cx))
                    .child(match self.voice_bar(cx) {
                        Some(bar) => bar,
                        None => div()
                            .flex()
                            .items_start()
                            .pl(px(11.))
                            .pr(px(12.))
                            .cursor(gpui::CursorStyle::IBeam)
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|this, event: &MouseDownEvent, window, cx| {
                                    if window.default_prevented() {
                                        return;
                                    }
                                    this.composer.update(cx, |editor, cx| {
                                        editor.on_mouse_down(event, window, cx);
                                    });
                                }),
                            )
                            .child(
                                div()
                                    .id("attach")
                                    .mt(px(12.))
                                    .cursor_pointer()
                                    .on_mouse_down(MouseButton::Left, |_, window, _| {
                                        window.prevent_default();
                                    })
                                    // Not in the pinned client: BetterUploadButton uploads on a click and opens the
                                    // menu on a right click (`plugin_hooks.rs`).
                                    .when(plugin_hooks::better_upload_button(), |this| {
                                        this.on_mouse_up(
                                            MouseButton::Right,
                                            cx.listener(|this, _: &gpui::MouseUpEvent, _, cx| {
                                                this.toggle_plus_menu(cx)
                                            }),
                                        )
                                    })
                                    .child(tool("attach", cx).on_click(cx.listener(|this, _, _, cx| {
                                        if plugin_hooks::better_upload_button() {
                                            this.pick_files(cx);
                                        } else {
                                            this.toggle_plus_menu(cx);
                                        }
                                    }))),
                            )
                            .child(
                                div()
                                    .ml(px(20.))
                                    .flex_1()
                                    .min_w(px(0.))
                                    .py(px(17.))
                                    .parity("composer-text")
                                    // The padding above and under the text belongs to the text area: a press there
                                    // focuses it like one on the text.
                                    .cursor(gpui::CursorStyle::IBeam)
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(|this, event: &MouseDownEvent, window, cx| {
                                            this.composer.update(cx, |editor, cx| {
                                                editor.on_mouse_down(event, window, cx);
                                            });
                                        }),
                                    )
                                    .flex()
                                    .flex_wrap()
                                    .items_center()
                                    .children(self.pending_chips(cx))
                                    .child(
                                        div()
                                            .flex_1()
                                            // The text area has no minimum of its own (`.textArea_2x8-Qn` fills the
                                            // scroller): a narrow window clips the hint under the buttons instead of
                                            // pushing it over them. After a command's chip the editor keeps 80px so
                                            // it wraps onto its own line.
                                            .min_w(px(if self.slash.pending.is_some() {
                                                80.
                                            } else {
                                                0.
                                            }))
                                            .on_mouse_up(
                                                MouseButton::Right,
                                                cx.listener(
                                                    |this,
                                                     event: &gpui::MouseUpEvent,
                                                     window,
                                                     cx| {
                                                        this.open_composer_menu(
                                                            event.position,
                                                            window,
                                                            cx,
                                                        );
                                                    },
                                                ),
                                            )
                                            .child(self.composer.clone()),
                                    ),
                            )
                            .child(
                                div()
                                    .mt(px(12.))
                                    .flex()
                                    .gap(px(8.))
                                    .cursor_default()
                                    .on_mouse_down(MouseButton::Left, |_, window, _| {
                                        window.prevent_default();
                                    })
                                    .children(preview_button)
                                    .children(silent_button)
                                    .when(!docked, |this| {
                                        this.child(gift_button)
                                            .child(gif_button)
                                            .child(sticker_button)
                                    })
                                    .child(emoji_button)
                                    .child(apps_button)
                                    .children(send_button),
                            )
                            .into_any_element(),
                    }),
            )
    }
}

fn editor_style() -> EditorStyle {
    let mut text_font: Font = font(SANS);
    text_font.features = kerning();
    EditorStyle {
        font: text_font,
        size: crate::editor::FONT_SIZE,
        line_height: crate::editor::LINE_HEIGHT,
        text: hsla("--text-default"),
        placeholder: hsla("--channel-text-area-placeholder"),
        placeholder_italic: false,
        selection: hsla_from(SELECTION),
        caret: hsla("--text-default"),
    }
}

/// A message's text as copied: its blocks, one per line.
/// The spans of the first text block, for one-line previews of a message.
fn first_spans(content: &str) -> Vec<Span> {
    parse(content)
        .into_iter()
        .find_map(|block| match block {
            Block::Paragraph(spans)
            | Block::Quote(spans)
            | Block::Subtext(spans)
            | Block::Heading { spans, .. }
            | Block::ListItem { spans, .. } => Some(spans),
            Block::Code { .. } => None,
        })
        .unwrap_or_default()
}

/// For each span, the link it belongs to: its address and the whole run's visible text (a masked link's label can span
/// several styled spans).
fn link_runs(spans: &[Span]) -> Vec<Option<(String, String)>> {
    let mut out = vec![None; spans.len()];
    let mut index = 0;
    while index < spans.len() {
        // An attachment chip is a link of its own, named by its file.
        if let Kind::AttachmentLink(url) = &spans[index].kind {
            out[index] = Some((url.clone(), spans[index].text.clone()));
            index += 1;
            continue;
        }
        let Kind::Link(url) = &spans[index].kind else {
            index += 1;
            continue;
        };
        let start = index;
        let mut label = String::new();
        while index < spans.len() && matches!(&spans[index].kind, Kind::Link(other) if other == url)
        {
            label.push_str(&spans[index].text);
            index += 1;
        }
        for slot in &mut out[start..index] {
            *slot = Some((url.clone(), label.clone()));
        }
    }
    out
}

fn parse_block_text(content: &str) -> String {
    parse(content)
        .iter()
        .map(block_text)
        .collect::<Vec<_>>()
        .join("\n")
}

/// The route form of a reaction: the emoji itself, or `name:id` for a custom one.
fn reaction_route_name(reaction: &dc_state::Reaction) -> String {
    match reaction.emoji_id {
        Some(id) => format!("{}:{id}", reaction.name),
        None => reaction.name.clone(),
    }
}

fn block_spans(block: &Block) -> &[Span] {
    match block {
        Block::Paragraph(spans)
        | Block::Quote(spans)
        | Block::Subtext(spans)
        | Block::Heading { spans, .. }
        | Block::ListItem { spans, .. } => spans,
        Block::Code { .. } => &[],
    }
}

/// Plain text of a block, the same bytes the inline engine lays out.
fn block_text(block: &Block) -> String {
    match block {
        Block::Paragraph(spans)
        | Block::Quote(spans)
        | Block::Subtext(spans)
        | Block::Heading { spans, .. }
        | Block::ListItem { spans, .. } => spans.iter().map(|span| span.text.as_str()).collect(),
        Block::Code { text, .. } => text.clone(),
    }
}

/// The space above each block. CSS margins collapse, so a pair of neighbors takes the larger of its two.
fn block_gaps(blocks: &[Block]) -> Vec<f32> {
    let top = |block: &Block| match block {
        Block::Heading { level: 1, .. } => 8.,
        Block::Heading { .. } => 16.,
        Block::Quote(_) | Block::ListItem { .. } => 4.,
        Block::Code { .. } => 6.,
        Block::Paragraph(_) | Block::Subtext(_) => 0.,
    };
    let bottom = |block: &Block| match block {
        Block::Heading { .. } => 8.,
        Block::Quote(_) | Block::ListItem { .. } => 4.,
        _ => 0.,
    };
    blocks
        .iter()
        .enumerate()
        .map(|(index, block)| match index.checked_sub(1) {
            Some(previous) => f32::max(top(block), bottom(&blocks[previous])),
            None => top(block),
        })
        .collect()
}

/// `gq(color, alpha)` of module 317097: the role colour at an opacity.
fn role_tint(color: u32, alpha: f32) -> Hsla {
    let mut tint = Hsla::from(gpui::rgb(color));
    tint.a = alpha;
    tint
}

/// A rounded box behind inline text: mentions, stamps and spoilers.
fn pill(padding: f32, radius: f32, background: &str) -> BoxStyle {
    BoxStyle {
        icon: None,
        padding_x: px(padding),
        height: px(21.),
        top: px(0.),
        background: hsla(background),
        border: Hsla::transparent_black(),
        radius: px(radius),
        text_line_height: px(22.),
        text_top: px(0.),
    }
}

/// `<t:...>` styles, formatted for en-US as the official client does.
fn format_timestamp(unix: i64, style: char) -> String {
    let Some(time) = chrono::DateTime::from_timestamp(unix, 0) else {
        return unix.to_string();
    };
    let local = time.with_timezone(&chrono::Local);
    // Module 379418: each style is one long format of module 5377 `i$`.
    let token = match style {
        't' => "LT",
        'T' => "LTS",
        'd' => "L",
        'D' => "LL",
        'F' => "LLLL",
        's' => "L LT",
        'S' => "L LTS",
        'R' => return relative_time(chrono::Utc::now().timestamp() - unix),
        _ => "LLL",
    };
    timestamps::long(token, &local)
}

/// moment's relative time with its default thresholds, which is what `R` uses.
fn relative_time(seconds_ago: i64) -> String {
    let (seconds, past) = (seconds_ago.abs(), seconds_ago >= 0);
    let minutes = (seconds + 30) / 60;
    let hours = (seconds + 1800) / 3600;
    let days = (seconds + 43200) / 86400;
    let months = (days as f64 / 30.4).round() as i64;
    let years = (days as f64 / 365.).round() as i64;
    let span = match () {
        () if seconds < 45 => dc_intl::tr("a few seconds").to_owned(),
        () if seconds < 90 => "a minute".to_owned(),
        () if minutes < 45 => format!("{minutes} minutes"),
        () if minutes < 90 => "an hour".to_owned(),
        () if hours < 22 => format!("{hours} hours"),
        () if hours < 36 => "a day".to_owned(),
        () if days < 26 => format!("{days} days"),
        () if days < 46 => "a month".to_owned(),
        () if days < 320 => format!("{months} months"),
        () if days < 548 => "a year".to_owned(),
        () => format!("{years} years"),
    };
    if past {
        format!("{span} ago")
    } else {
        format!("in {span}")
    }
}

/// `code.inline` from the captured DOM: 0.2em padding plus the border, 20px tall, 1px below the line top.
fn inline_code_box() -> BoxStyle {
    BoxStyle {
        icon: None,
        padding_x: px(3.72),
        height: px(20.),
        top: px(1.),
        background: hsla("--background-code"),
        border: hsla("--background-mod-strong"),
        radius: px(4.),
        text_line_height: px(18.),
        text_top: px(2.),
    }
}

/// Inline code inside a reply preview: 0.85 of 14px, so its box is smaller than in the message.
fn preview_code_box() -> BoxStyle {
    BoxStyle {
        icon: None,
        padding_x: px(3.38),
        height: px(18.),
        top: px(0.),
        background: hsla("--background-code"),
        border: hsla("--background-mod-strong"),
        radius: px(4.),
        text_line_height: px(18.),
        text_top: px(0.),
    }
}

fn clamp_char_boundary(text: &str, index: usize) -> usize {
    let mut at = index.min(text.len());
    while at > 0 && !text.is_char_boundary(at) {
        at -= 1;
    }
    at
}

fn plain_code_piece(code_font: &Font, color: Hsla, text: String) -> Piece {
    Piece {
        text,
        font: code_font.clone(),
        size: px(14.),
        color,
        underline: false,
        strike: false,
        boxed: None,
        image: None,
        hover: None,
        hover_underline: false,
        gradient: None,
    }
}

impl Root {
    fn selectable_code(&self, language: Option<&str>, text: &str, key: (u64, u32)) -> Inline {
        let mut code_font: Font = font(MONO);
        code_font.features = kerning();
        let highlighted = highlight(language, text);
        let mut pieces: Vec<Piece> = Vec::new();
        for (line_ix, line) in text.lines().enumerate() {
            if line_ix > 0 {
                pieces.push(plain_code_piece(
                    &code_font,
                    hsla("--text-code"),
                    "\n".to_owned(),
                ));
            }
            let spans = highlighted.get(line_ix).map(Vec::as_slice).unwrap_or(&[]);
            let mut pos = 0;
            for &(len, token) in spans {
                let end = (pos + len).min(line.len());
                if end <= pos || !line.is_char_boundary(pos) || !line.is_char_boundary(end) {
                    continue;
                }
                pieces.push(plain_code_piece(
                    &code_font,
                    hsla(token.css_name()),
                    line[pos..end].to_owned(),
                ));
                pos = end;
            }
            if pos < line.len() {
                pieces.push(plain_code_piece(
                    &code_font,
                    hsla("--text-code"),
                    line[pos..].to_owned(),
                ));
            }
        }
        inline(pieces, px(18.)).selectable(key, self.selection.clone(), hsla_from(SELECTION))
    }
}

/// Each line as styled runs in the code font, colored from the official `--text-code-*` tokens.
fn code_lines(language: Option<&str>, text: &str) -> Vec<AnyElement> {
    let mut code_font: Font = font(MONO);
    code_font.features = kerning();
    text.lines()
        .zip(highlight(language, text))
        .map(|(line, spans)| {
            let runs = spans
                .into_iter()
                .map(|(len, token)| TextRun {
                    len,
                    font: code_font.clone(),
                    color: hsla(token.css_name()),
                    background_color: None,
                    underline: None,
                    strikethrough: None,
                })
                .collect();
            div()
                .min_h(px(18.))
                .child(StyledText::new(line.to_owned()).with_runs(runs))
                .into_any_element()
        })
        .collect()
}

/// Minimize, maximize and close. They sit in the official `systemBar`, above every layer and its scrim.
fn window_controls() -> impl IntoElement {
    let control = |id: &'static str, name: &str, area: WindowControlArea| {
        let close = id == "close";
        let hover_background = if close {
            "--control-critical-primary-background-default"
        } else {
            "--interactive-background-hover"
        };
        let hover_icon = if close {
            "--control-critical-primary-icon-default"
        } else {
            "--interactive-text-hover"
        };
        div()
            .id(id)
            .group(id)
            .w(px(32.))
            .h(px(32.))
            .cursor_pointer()
            .flex()
            .items_center()
            .justify_center()
            // gpui answers with the first control area under the pointer: the drag area behind must not count.
            .occlude()
            .window_control_area(area)
            .hover(|style| style.bg(color(hover_background)))
            .when(!close, |button| {
                button.active(|style| style.bg(color("--interactive-background-active")))
            })
            .child(
                svg()
                    .path(format!("icons/{name}.svg"))
                    .id(SharedString::from(format!("{id}-icon")))
                    .size(px(12.))
                    .text_color(color("--interactive-text-default"))
                    .group_hover(id, |style| style.text_color(color(hover_icon)))
                    .when(!close, |image| {
                        image.group_active(id, |style| {
                            style.text_color(color("--interactive-text-active"))
                        })
                    }),
            )
    };
    div()
        .flex()
        .gap(px(4.))
        .child(control("min", "minimize", WindowControlArea::Min))
        .child(control("max", "maximize", WindowControlArea::Max))
        .child(control("close", "close", WindowControlArea::Close))
}

/// How a downloaded picture is decoded: by its key for the vector kinds, else by its first bytes.
fn image_format(key: &str, bytes: &[u8]) -> ImageFormat {
    if key.starts_with("emoji:")
        || key.starts_with("filekind:")
        || bytes.starts_with(b"<svg")
        || bytes.starts_with(b"<?xml")
    {
        ImageFormat::Svg
    } else if bytes.starts_with(b"\x89PNG") {
        ImageFormat::Png
    } else if bytes.starts_with(b"RIFF") {
        ImageFormat::Webp
    } else if bytes.starts_with(b"GIF8") {
        ImageFormat::Gif
    } else {
        ImageFormat::Jpeg
    }
}

fn avatar_source(user: &User) -> (String, String) {
    // Not in the pinned client: UserPFP's avatar (`plugin_hooks/userpfp.rs`).
    if let Some(url) = plugin_hooks::userpfp_avatar(user, false) {
        return (format!("userpfp:{url}"), url);
    }
    match &user.avatar {
        // Clyde's local notices: the bot's own image, a client asset (`/assets/9380e4b5bd8d267c.png` in the row of
        // design/dom/failed-message/clyde-notice-darker-cozy.json).
        Some(hash) if user.bot && user.id == 1 && hash == "clyde" => (
            "avatar:clyde".to_owned(),
            "https://discord.com/assets/9380e4b5bd8d267c.png".to_owned(),
        ),
        Some(hash) => (
            format!("avatar:{}:{hash}", user.id),
            format!(
                "https://cdn.discordapp.com/avatars/{}/{hash}.webp?size=80",
                user.id
            ),
        ),
        None => {
            let index = user.default_avatar_index();
            (
                format!("avatar:default:{index}"),
                format!("https://cdn.discordapp.com/embed/avatars/{index}.png"),
            )
        }
    }
}

impl Render for Root {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        #[cfg(feature = "capture")]
        crate::perf::RENDERS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        self.interaction_modal_frame(window, cx);
        self.open_pending_overlay_survey(cx);
        self.av_errors_frame(cx);
        self.unread_notice_frame(cx);
        self.upload_area_frame(cx);
        #[cfg(feature = "capture")]
        let _build = crate::perf::Timer::build();
        // The motion clock for this frame; reduced motion as module 775602 decides it.
        motion::begin_frame(motion::decide_reduced(
            self.pref_on("sync_reduced_motion", true),
            self.pref_on("reduced_motion", false),
            motion::cached_system_preference(),
        ));
        self.selection.borrow_mut().begin_frame();
        motion::set_window_active(window.is_window_active());
        // Not in the pinned client: VoiceStats follows its own switch (`plugin_hooks/voice_stats.rs`).
        self.voice_stats_observe(cx);
        // Not in the pinned client: MuteChannelMentions keeps the read states' filter current
        // (`plugin_hooks/mute_channel_mentions.rs`).
        self.mute_channel_mentions_sync();
        self.streamer_apply(window);
        motion::begin_view(cx.entity_id());
        self.regions.ensure(&cx.entity(), cx);
        window.set_ui_zoom(self.zoom_factor(), cx);
        self.sync_client_theme();
        self.ui_scale.set(window.scale_factor());
        self.account_form_focus(window, cx);
        self.focus_new_dialog(window, cx);
        // The base layer of the layer stack fades while a full-window layer is open (module 596339).
        let base_opacity = motion::layers::base_opacity(
            self.server_settings.is_some() || self.channel_settings.is_some(),
        );
        let focused = window.focused(cx);
        let top = self.list.logical_scroll_top();
        self.keys.stops.begin_frame(
            focused.as_ref(),
            top.item_ix + usize::from(top.offset_in_item > px(0.)),
        );
        self.sync_focus(window, cx);
        self.pump_window(window);
        self.trace_start_frame(window, cx);
        self.request_rail_dm_images();
        self.request_group_images();
        self.rail_folders_frame(cx);
        self.channel_drag_frame(cx);
        self.accounts_frame(cx);
        self.prepare_nameplate_animations(window, cx);
        self.profile_effects_sweep(window, cx);
        self.sticker_player_sweep(window, cx);
        let avatars = (
            self.data_revision,
            self.guild,
            self.profile.as_ref().map(|profile| profile.user),
        );
        if changed(&mut self.prepared.avatars, avatars) {
            self.prepare_member_avatars(window, cx);
        }
        self.prepare_inline_videos(window, cx);
        media_image::set_pixel_ratio(window.scale_factor());
        let media_channel = self.channel;
        if self
            .prepared
            .media_channel
            .is_some_and(|last| last != media_channel)
        {
            self.media_left_channel();
        }
        self.prepared.media_channel = Some(media_channel);
        self.prepare_videos(window, cx);
        self.prepare_gif_favorites();
        // The pointer left the window: the browser's `mouseleave` on the open menu closes a hover-opened submenu.
        // Windows reports only the window's hover change (no last move outside it), so the menus would keep the last
        // point inside them.
        let hovered = window.is_window_hovered();
        // A submenu the keyboard opened has no pointer to lose.
        if changed(&mut self.prepared.window_hovered, hovered)
            && !hovered
            && !self.keys.keyboard_mode_is_on()
        {
            self.menu_pointer_left(cx);
        }
        self.prepare_gif_tiles(window, cx);
        self.prepare_live_avatars();
        self.prepare_guild_guide();
        self.prepare_sidebar_events();
        self.prepare_channel_browser(cx);
        self.prepare_live_activities(cx);
        // Not in the pinned client: StreamingCodecDisabler's filter (`plugin_hooks/streaming_codec_disabler.rs`).
        plugin_hooks::sync_streaming_codecs();
        // Not in the pinned client: USRBG's list, once (`plugin_hooks/usrbg.rs`).
        self.usrbg_prepare();
        // Not in the pinned client: UserPFP's list, once (`plugin_hooks/userpfp.rs`).
        self.userpfp_prepare();
        // Not in the pinned client: GlobalBadges' list, at start and every 30 minutes (`plugin_hooks/global_badges.rs`).
        self.global_badges_prepare();
        // Not in the pinned client: SteamStatusSync (`plugin_hooks/steam_status_sync.rs`).
        self.steam_status_sync(cx);
        if changed(&mut self.prepared.badge, self.data_revision) {
            self.refresh_notification_badge_soon(cx);
        }
        self.tray_runtime.attach(window);
        self.restore_slowmode_draft(cx);
        set_saturation(self.saturation_factor());
        // The composer styles typed markdown unless the legacy input is on or the preview is off (module 814390).
        crate::markup_preview::set_enabled(
            self.pref_on("chat_text_box_previews", true)
                && !self.pref_on("enable_legacy_chat_input", false),
        );
        // The formatting functions without a `Root` read the Time format from here.
        settings::prefs::set_time_format(self.pref_index(settings::prefs::TIME_FORMAT, 0));
        // The name styles read the accessibility switches from here.
        name_styles::set_preferences(self.pref_on("display_name_styles", true), motion::reduced());
        name_styles::pump(cx);
        // The interface language, set before anything below formats text (`crate::intl`).
        crate::intl::apply(self.interface_locale(), cx);
        // The rows keep their date dividers as text: a change of language rebuilds them.
        {
            static ROWS_GENERATION: std::sync::atomic::AtomicU64 =
                std::sync::atomic::AtomicU64::new(0);
            let generation = dc_intl::generation();
            if ROWS_GENERATION.swap(generation, std::sync::atomic::Ordering::Relaxed) != generation
            {
                self.rebuild_rows();
            }
        }
        // The regions reuse their frame until notified: a global they read that changed without a notify of this
        // view redraws them on the next frame.
        regions::sync_look(
            motion::key(&(
                (
                    theme() as u8,
                    saturation().to_bits(),
                    self.zoom_factor().to_bits(),
                    motion::reduced(),
                    window.is_window_active(),
                    self.pref_on("high_contrast_mode", false),
                ),
                (
                    self.pref_on("display_name_styles", true),
                    settings::client_theme::active().map(|swatch| swatch.name),
                    settings::client_theme::previewing(),
                    settings::client_theme::generation(),
                    self.pref_index(settings::prefs::TIME_FORMAT, 0),
                    dc_intl::generation(),
                    self.combined_sidebar_width().to_bits(),
                ),
            )),
            window,
        );
        self.onboarding_note_focus(window, cx);
        self.activity_host_frame(window, cx);
        self.provider_player_frame(window, cx);
        self.web_frames_frame(window, cx);
        self.sync_voice_page(cx);
        let feed = (self.data_revision, self.guild, self.ready);
        let feed_changed = changed(&mut self.prepared.inventory, feed);
        self.prepare_inventory_feed(feed_changed, cx);
        self.prepare_inventory_popout();
        // The header's search field keeps its own editor; its placeholder names the place.
        let place = match self
            .current_dm()
            .and_then(|channel| self.stores.dm_recipient(channel))
        {
            Some(user) => user.username.clone(),
            None => match self.current_dm().filter(|channel| channel.kind == 3) {
                Some(channel) => self.private_channel_name(channel),
                None => self
                    .current_guild()
                    .map(|guild| guild.name.clone())
                    .unwrap_or_default(),
            },
        };
        self.search_field(dc_intl::t!("LDZtFO", name = place), cx);
        // The chat column less the message's 72px gutter and its 24 + 16px right padding.
        // The member list in a guild, the profile beside a DM.
        let members = if self.mod_view.is_some() {
            // The Mod View panel (480) opens beside the member list, which stays (menus2/mod-view-self).
            480. + if self.show_members && self.guild.is_some() {
                self.member_list_width() + 1.
            } else {
                0.
            }
        } else if self.new_thread.is_some() {
            // The New Thread panel replaces the member list: 480 wide, its 1px border and 8px of gap.
            480.
        } else if self.current_dm().is_some() {
            self.dm_profile_width(f32::from(window.viewport_size().width))
        } else if self.show_members && self.guild.is_some() {
            self.member_list_width()
        } else if self.group_members_shown() {
            self.member_list_width() + 1.
        } else {
            0.
        };
        self.content_width = f32::from(window.viewport_size().width)
            - self.combined_sidebar_width()
            - members
            - 112.;
        self.viewport = (
            f32::from(window.viewport_size().width),
            f32::from(window.viewport_size().height),
        );
        crate::net::SHOP_SCALE.store(
            (window.scale_factor().max(1.) * 100.).round() as u32,
            std::sync::atomic::Ordering::Relaxed,
        );
        crate::net::SHOP_IMAGE_SIDE.store(
            image_fit::side_for(self.viewport.0, self.viewport.1, window.scale_factor()),
            std::sync::atomic::Ordering::Relaxed,
        );
        if self.voice_chat_docked() {
            // The docked voice chat is 450 wide with its 1px border.
            self.content_width = 450. - 2. - 112.;
        }
        if self.channel_browser_open && self.channel.is_some() {
            self.content_width = self.browser_preview_width() - 2. - 112.;
        }
        let rows = list(
            self.list.clone(),
            cx.processor(|this, index, _, cx| {
                #[cfg(feature = "capture")]
                let _row = crate::perf::Timer::row();
                this.row(index, cx)
            }),
        )
        .size_full();
        let image_crop_layers = self.image_crop_layers(window, cx);
        let element = div()
            .id("root")
            .track_focus(&self.focus)
            // Not in the pinned client: MessageClickActions follows held keys and presses (`plugin_hooks.rs`).
            .when(!crate::plugins::bypassed(), |root| {
                plugin_hooks::follow_message_clicks(root, window.is_window_active())
            })
            .on_key_down(cx.listener(Self::on_key))
            // Escape closes settings before the focused field, which would take it for itself, sees it.
            // Push to talk and the keybind recorder see every key in the window first.
            .capture_any_mouse_down(cx.listener(|this, event: &MouseDownEvent, _, cx| {
                if this.voice_mouse(event.button, true, cx) {
                    cx.stop_propagation();
                }
            }))
            .capture_any_mouse_up(cx.listener(|this, event: &gpui::MouseUpEvent, _, cx| {
                this.voice_mouse(event.button, false, cx);
                this.end_browser_resize();
                this.clips_mouse_up(cx);
                this.video_mouse_up(cx);
            }))
            // A scroll moves the content under a pointer that stays put; Chromium then sends the element a
            // mouseleave and its tooltip closes. gpui only rereads hover on a move, so the tooltip closes here.
            .on_scroll(cx.listener(|this, _: &gpui::ScrollEvent, _, cx| {
                if this.tip.is_some() || this.tip_pending.is_some() {
                    this.tip = None;
                    this.tip_pending = None;
                    cx.notify();
                }
            }))
            .on_mouse_move(cx.listener(|this, event: &gpui::MouseMoveEvent, _, cx| {
                this.move_browser_resize(event, cx);
                this.video_mouse_move(event, cx);
                if hover_bar::set_shift(event.modifiers.shift) && this.hovered.is_some() {
                    cx.notify();
                }
            }))
            .capture_key_up(cx.listener(|this, event: &gpui::KeyUpEvent, _, cx| {
                this.voice_key_up(&event.keystroke, cx);
            }))
            .on_modifiers_changed(cx.listener(
                |this, event: &gpui::ModifiersChangedEvent, _, cx| {
                    // The message hover bar expands while Shift is held (module 311283).
                    if hover_bar::set_shift(event.modifiers.shift) && this.hovered.is_some() {
                        cx.notify();
                    }
                    this.voice_modifiers(event.modifiers, cx);
                },
            ))
            .capture_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                let keys = &event.keystroke;
                if !event.is_held && this.voice_key_down(keys, cx) {
                    cx.stop_propagation();
                    return;
                }
                if this.image_crop_key(keys, window, cx) {
                    cx.stop_propagation();
                    return;
                }
                if keys.key == "escape" && this.voice_rec.borrow().confirm {
                    this.voice_rec.borrow_mut().confirm = false;
                    cx.notify();
                    cx.stop_propagation();
                    return;
                }
                // Enter in the recorder bar stops the recording or sends the draft; the bar has no text field.
                if keys.key == "enter"
                    && !keys.modifiers.modified()
                    && this
                        .channel
                        .is_some_and(|channel| this.voice_rec.borrow().active_in(channel))
                {
                    this.voice_record_send(cx);
                    cx.stop_propagation();
                    return;
                }
                if keys.modifiers.control && !keys.modifiers.alt && this.zoom_key(&keys.key, cx) {
                    cx.stop_propagation();
                    return;
                }
                // The media viewer is the top layer: Escape and the arrows are its.
                if this.guild_events_key(&keys.key, cx) {
                    cx.stop_propagation();
                    return;
                }
                if this.badge_modal_key(&keys.key, cx) {
                    cx.stop_propagation();
                    return;
                }
                if keys.key == "escape" && this.spotify_actions.premium_dialog {
                    this.spotify_actions.premium_dialog = false;
                    cx.stop_propagation();
                    cx.notify();
                    return;
                }
                if this.video_key(&event.keystroke, window, cx) {
                    cx.stop_propagation();
                    return;
                }
                if this.lightbox_key(event, cx) {
                    cx.stop_propagation();
                    return;
                }
                if this.clips_key(event, window, cx) {
                    cx.stop_propagation();
                    return;
                }
                if keys.key == "escape" && this.escape_purchase(cx) {
                    cx.stop_propagation();
                    return;
                }
                if keys.key == "escape"
                    && this.picker.as_ref().is_some_and(|picker| {
                        matches!(picker.target, picker::Target::InventoryReply(_))
                    })
                {
                    this.picker = None;
                    this.skin_menu = false;
                    window.focus(&this.focus, cx);
                    cx.notify();
                    cx.stop_propagation();
                    return;
                }
                // 165862 closes the whole menu on Escape, before an underlying field consumes it.
                if keys.key == "escape" && this.picker.is_none() && this.context.open.is_some() {
                    this.close_context_menu(cx);
                    cx.stop_propagation();
                    return;
                }
                // The server header dropdown is a menu like the others: Escape closes it.
                if keys.key == "escape" && this.picker.is_none() && this.guild_menu {
                    this.guild_menu = false;
                    cx.notify();
                    cx.stop_propagation();
                    return;
                }
                if keys.key == "escape" && this.incoming_escape(cx) {
                    cx.stop_propagation();
                    return;
                }
                if keys.key == "escape" && alt_text::close() {
                    cx.notify();
                    cx.stop_propagation();
                    return;
                }
                // A settings select list and the add-game popout close before the settings page does.
                if keys.key == "escape" && this.escape_layer(cx) {
                    cx.stop_propagation();
                    return;
                }
                if keys.key == "escape"
                    && this.picker.is_none()
                    && this.profile.is_none()
                    && this.close_inventory_popout()
                {
                    cx.notify();
                    cx.stop_propagation();
                    return;
                }
                if keys.key == "escape" && this.inventory.about {
                    this.inventory.about = false;
                    cx.notify();
                    cx.stop_propagation();
                    return;
                }
                if keys.key == "escape" && this.close_forum_menu() {
                    cx.notify();
                    cx.stop_propagation();
                    return;
                }
                if keys.key == "escape" && this.screening.take().is_some() {
                    cx.notify();
                    cx.stop_propagation();
                    return;
                }
                if keys.key == "escape" && this.discover_escape() {
                    cx.notify();
                    cx.stop_propagation();
                    return;
                }
                if keys.key == "escape" && this.add_server.is_some() {
                    this.add_server = None;
                    cx.notify();
                    cx.stop_propagation();
                    return;
                }
                // Escape in the search box: its field would emit Cancel, but the chain below takes the key first.
                if keys.key == "escape"
                    && this.search.as_ref().is_some_and(|search| search.focused)
                    && this.search_filters.is_none()
                {
                    this.clear_search(cx);
                    if this.composer_reachable() {
                        this.focus_composer(window, cx);
                    } else {
                        window.focus(&this.focus, cx);
                    }
                    cx.stop_propagation();
                    return;
                }
                if keys.key == "escape"
                    && let Some(filters) = this.search_filters.as_mut()
                {
                    if filters.open.take().is_none() {
                        this.search_filters = None;
                    }
                    cx.notify();
                    cx.stop_propagation();
                    return;
                }
                if keys.key == "escape" && this.thread_modal {
                    this.thread_modal = false;
                    cx.notify();
                    cx.stop_propagation();
                    return;
                }
                // A dialog opened with a text field in focus: the field would emit Cancel, but the chain below
                // takes Escape first, so the dialogs close here.
                if keys.key == "escape" && this.create_channel.is_some() {
                    this.create_channel = None;
                    cx.notify();
                    cx.stop_propagation();
                    return;
                }
                // Dialogs of the server menu and the Stage and announcement flows: a modal's own Escape.
                if keys.key == "escape" && this.exit_video_fullscreen(window, cx) {
                    cx.stop_propagation();
                    return;
                }
                if keys.key == "escape" && this.vv.fullscreen {
                    this.toggle_call_fullscreen(window, cx);
                    cx.stop_propagation();
                    return;
                }
                if keys.key == "escape" && this.close_boost_apply() {
                    cx.notify();
                    cx.stop_propagation();
                    return;
                }
                // A voice alert closes as its own close does (the blocked warnings' close joins or stays).
                if keys.key == "escape" && this.voice_alert.is_some() {
                    this.dismiss_voice_alert(cx);
                    cx.notify();
                    cx.stop_propagation();
                    return;
                }
                if keys.key == "escape" && this.feedback_escape(cx) {
                    cx.stop_propagation();
                    return;
                }
                if keys.key == "escape" && this.overlay_survey.take().is_some() {
                    cx.notify();
                    cx.stop_propagation();
                    return;
                }
                if keys.key == "escape"
                    && (this.guild_notifications.is_some()
                        || this.channel_status.is_some()
                        || this.follow.is_some()
                        || this.start_stage.is_some()
                        || this.stage_welcome.is_some()
                        || this.stage_end_confirm.is_some()
                        || this.voice_alert.is_some()
                        || this.publish_confirm.is_some())
                {
                    this.guild_notifications = None;
                    this.channel_status = None;
                    this.follow = None;
                    this.start_stage = None;
                    this.stage_welcome = None;
                    this.stage_end_confirm = None;
                    this.voice_alert = None;
                    this.publish_confirm = None;
                    cx.notify();
                    cx.stop_propagation();
                    return;
                }
                if keys.key == "escape" && this.report.is_some() {
                    this.report_closed(cx);
                    cx.notify();
                    cx.stop_propagation();
                    return;
                }
                if keys.key == "escape" && this.invite.is_some() {
                    // An open select closes first (module 873200's Escape).
                    match this.invite.as_mut() {
                        Some(invite) if invite.open_select.is_some() => invite.open_select = None,
                        _ => this.invite = None,
                    }
                    cx.notify();
                    cx.stop_propagation();
                    return;
                }
                if keys.key == "escape" && this.sent_edit.is_some() {
                    this.sent_edit = None;
                    cx.notify();
                    cx.stop_propagation();
                    return;
                }
                if keys.key == "escape" && this.upload_error.take().is_some() {
                    cx.notify();
                    cx.stop_propagation();
                    return;
                }
                if keys.key == "escape" && this.game_profile.is_some() {
                    this.game_profile = None;
                    cx.notify();
                    cx.stop_propagation();
                    return;
                }
                if keys.key == "escape" && this.report_issue.is_some() {
                    this.report_issue = None;
                    cx.notify();
                    cx.stop_propagation();
                    return;
                }
                if keys.key == "escape" && this.modify_attachment.is_some() {
                    this.modify_attachment = None;
                    cx.notify();
                    cx.stop_propagation();
                    return;
                }
                if keys.key == "escape" && this.settings_extra.has_dialog() {
                    this.settings_extra.close_dialog();
                    cx.notify();
                    cx.stop_propagation();
                    return;
                }
                if keys.key == "escape" && this.account_switcher_escape(cx) {
                    cx.stop_propagation();
                    return;
                }
                if keys.key == "escape" && this.folder_settings_escape(cx) {
                    cx.stop_propagation();
                    return;
                }
                if keys.key == "escape" && this.reorder_hint.is_some() {
                    this.close_reorder_hint();
                    cx.notify();
                    cx.stop_propagation();
                    return;
                }
                if keys.key == "escape" && this.destructive.is_some() {
                    this.close_destructive();
                    cx.notify();
                    cx.stop_propagation();
                    return;
                }
                if keys.key == "escape" && this.logout_confirm {
                    this.logout_confirm = false;
                    cx.notify();
                    cx.stop_propagation();
                    return;
                }
                if keys.key == "escape"
                    && (this.leave_guild.is_some() || this.snooze_menu.is_some())
                {
                    if this.snooze_menu.take().is_none() {
                        this.leave_guild = None;
                    }
                    cx.notify();
                    cx.stop_propagation();
                    return;
                }
                if keys.key == "escape" && this.schedule.modal.is_some() {
                    this.schedule_escape(cx);
                    cx.stop_propagation();
                    return;
                }
                if keys.key == "escape" && (this.plus_menu || this.poll.is_some()) {
                    if this.poll.as_ref().is_some_and(|form| form.duration_open) {
                        if let Some(form) = this.poll.as_mut() {
                            form.duration_open = false;
                        }
                    } else if this.poll.is_some() {
                        this.poll = None;
                    } else {
                        this.plus_menu = false;
                    }
                    cx.notify();
                    cx.stop_propagation();
                    return;
                }
                if keys.key == "escape"
                    && (this.custom_status.is_some() || this.status_popout.is_some())
                {
                    if this
                        .custom_status
                        .as_ref()
                        .is_some_and(|open| open.clear_open)
                    {
                        if let Some(open) = this.custom_status.as_mut() {
                            open.clear_open = false;
                        }
                    } else if this.custom_status.is_some() {
                        this.custom_status = None;
                    } else {
                        this.status_popout = None;
                    }
                    cx.notify();
                    cx.stop_propagation();
                    return;
                }
                // `QUICKSWITCHER_SHOW`: Ctrl+K anywhere.
                if keys.key == "k"
                    && keys.modifiers.control
                    && !keys.modifiers.shift
                    && !keys.modifiers.alt
                {
                    this.open_quick_switcher(window, cx);
                    cx.stop_propagation();
                    return;
                }
                if this.group_dm.is_some() && this.group_dm_key(&keys.key, cx) {
                    cx.stop_propagation();
                    return;
                }
                // A dialog over the settings page (Failed to upload files) closes before the page does.
                if keys.key == "escape"
                    && (this.chat_alert_escape()
                        || this.graphic_dialog_escape()
                        || this.birthday_escape())
                {
                    cx.notify();
                    cx.stop_propagation();
                    return;
                }
                // "Upload a Sound" / "Edit Sound" and a stream's "Report a problem", over Server Settings or the app.
                if keys.key == "escape"
                    && (this.sound_dialog.take().is_some() || this.stream_report.take().is_some())
                {
                    cx.notify();
                    cx.stop_propagation();
                    return;
                }
                if this.server_settings.is_some() && keys.key == "escape" {
                    if this.close_server_settings_layer() {
                    } else if let Some(open) = this.server_settings.as_mut()
                        && open.delete_name.is_some()
                    {
                        open.delete_name = None;
                    } else if let Some(open) = this.server_settings.as_mut()
                        && open.sticker_dialog.is_some()
                    {
                        open.sticker_dialog = None;
                    } else if this.close_boost_page()
                        || this.close_access_overlay()
                        || this.close_onboarding_overlay()
                        || this.close_automod_overlay()
                        || this.close_safety_overlay()
                        || this.close_community_wizard()
                        || this.close_perks()
                    {
                        cx.notify();
                    } else if let Some(open) = this.server_settings.as_mut()
                        && open.games_open
                    {
                        open.games_open = false;
                    } else if let Some(bans) = this
                        .server_settings
                        .as_mut()
                        .and_then(|open| open.bans.as_mut())
                        && bans.open.is_some()
                    {
                        bans.open = None;
                    } else {
                        this.server_settings = None;
                    }
                    cx.notify();
                    cx.stop_propagation();
                    return;
                }
                if this.channel_settings.is_some() && keys.key == "escape" {
                    if let Some(settings) = this.channel_settings.as_mut()
                        && settings.confirm_delete_open()
                    {
                        settings.close_delete();
                    } else {
                        this.channel_settings = None;
                    }
                    cx.notify();
                    cx.stop_propagation();
                    return;
                }
                if keys.key == "escape"
                    && (this.oauth_escape(cx)
                        || this.now_playing_escape(cx)
                        || this.activity_dialog_escape(cx)
                        || this.activity_shelf_escape(cx))
                {
                    cx.stop_propagation();
                    return;
                }
                if (keys.modifiers.control || keys.modifiers.platform)
                    && !keys.modifiers.alt
                    && !keys.modifiers.shift
                    && keys.key.eq_ignore_ascii_case("c")
                {
                    let text = this.selected_text();
                    if !text.is_empty() {
                        cx.write_to_clipboard(gpui::ClipboardItem::new_string(text));
                        cx.stop_propagation();
                        return;
                    }
                }
                if this.keyboard_key(event, window, cx) {
                    cx.stop_propagation();
                    return;
                }
                if this.composer_key(keys, window, cx) {
                    cx.stop_propagation();
                    return;
                }
                if this.quick_switcher.is_some() {
                    if keys.key == "escape" {
                        this.quick_switcher = None;
                        cx.notify();
                        cx.stop_propagation();
                        return;
                    }
                    if this.switcher_key(&keys.key, cx) {
                        cx.stop_propagation();
                        return;
                    }
                }
                // `NAVIGATE_BACK`/`NAVIGATE_FORWARD`: global combos, so they work from the composer too.
                let alt_only = keys.modifiers.alt
                    && !keys.modifiers.control
                    && !keys.modifiers.shift
                    && !keys.modifiers.platform;
                if alt_only && (keys.key == "left" || keys.key == "right") {
                    if keys.key == "left" {
                        this.go_back(cx);
                    } else {
                        this.go_forward(cx);
                    }
                    cx.stop_propagation();
                    return;
                }
                // Module 113367's listener only records the time (the overdue notice ends); the key goes on.
                if event.keystroke.key == "escape" && this.overdue_notice_escape() {
                    cx.notify();
                }
                if event.keystroke.key == "escape" && this.voice_dialog_close(cx) {
                    cx.stop_propagation();
                    return;
                }
                if event.keystroke.key == "escape" && this.vv.menu.take().is_some() {
                    cx.notify();
                    cx.stop_propagation();
                    return;
                }
                if event.keystroke.key == "escape" && this.thread_browser.is_some() {
                    this.thread_browser = None;
                    cx.notify();
                    cx.stop_propagation();
                    return;
                }
                if event.keystroke.key == "escape" && this.inbox.is_some() {
                    this.inbox = None;
                    cx.notify();
                    cx.stop_propagation();
                    return;
                }
                if event.keystroke.key == "escape"
                    && (this.friend_menu.is_some()
                        || this.confirm_remove_friend.is_some()
                        || this.friend_nickname.is_some()
                        || this.favorite_nickname.is_some()
                        || this.group_edit.is_some()
                        || this.group_leave.is_some()
                        || this.member_action.is_some())
                {
                    this.group_edit = None;
                    this.group_leave = None;
                    this.friend_menu = None;
                    this.confirm_remove_friend = None;
                    this.friend_nickname = None;
                    this.favorite_nickname = None;
                    this.member_action = None;
                    cx.notify();
                    cx.stop_propagation();
                    return;
                }
                // A submenu whose menu is already gone (Add Reaction opens the picker and closes the menu) is not a
                // layer anyone sees: it must not take the key from the picker above it.
                if event.keystroke.key == "escape" && this.menu.is_none() {
                    this.submenu = None;
                }
                if event.keystroke.key == "escape" && this.notify_open {
                    this.notify_open = false;
                    this.notify_submenu = None;
                    cx.notify();
                    cx.stop_propagation();
                } else if event.keystroke.key == "escape" && this.confirm_unpin.is_some() {
                    this.confirm_unpin = None;
                    cx.notify();
                    cx.stop_propagation();
                } else if event.keystroke.key == "escape" && this.pins_open.is_some() {
                    this.pins_open = None;
                    cx.notify();
                    cx.stop_propagation();
                } else if event.keystroke.key == "escape" && this.polls.modal_open() {
                    this.polls.escape();
                    cx.notify();
                    cx.stop_propagation();
                } else if event.keystroke.key == "escape" && this.forward.is_some() {
                    this.forward = None;
                    cx.notify();
                    cx.stop_propagation();
                } else if event.keystroke.key == "escape"
                    && (this.interaction_modal_escape() || this.chat_alert_escape())
                {
                    cx.notify();
                    cx.stop_propagation();
                } else if event.keystroke.key == "escape" && this.custom_reminder.is_some() {
                    this.custom_reminder = None;
                    cx.notify();
                    cx.stop_propagation();
                } else if event.keystroke.key == "escape" && this.submenu.is_some() {
                    // 165862 closes the whole menu on Escape, the submenu with it.
                    this.submenu = None;
                    this.menu = None;
                    cx.notify();
                    cx.stop_propagation();
                } else if event.keystroke.key == "escape" && this.picker.is_some() {
                    this.picker = None;
                    cx.notify();
                    cx.stop_propagation();
                } else if event.keystroke.key == "escape"
                    && (this.confirm_pin.is_some()
                        || this.too_long.is_some()
                        || this.leaving.is_some()
                        || this.confirm_delete.is_some()
                        || this.menu_extra.reactions_confirm.is_some()
                        || this.menu.is_some())
                {
                    // The top layer closes first: a modal, then the menu under it.
                    if this.too_long.is_some() {
                        this.too_long = None;
                    } else if this.confirm_pin.is_some() {
                        this.confirm_pin = None;
                    } else if this.leaving.is_some() {
                        this.leaving = None;
                    } else if this.confirm_delete.is_some() {
                        this.confirm_delete = None;
                    } else if this.menu_extra.reactions_confirm.is_some() {
                        this.menu_extra.reactions_confirm = None;
                    } else {
                        this.menu = None;
                    }
                    cx.notify();
                    cx.stop_propagation();
                } else if event.keystroke.key == "escape" && this.profile_editor.is_some() {
                    this.profile_editor_escape(cx);
                    cx.stop_propagation();
                } else if event.keystroke.key == "escape" && this.sticker_popout.is_some() {
                    this.close_sticker_popout(cx);
                    cx.stop_propagation();
                } else if event.keystroke.key == "escape" && this.emoji_popout.is_some() {
                    this.close_emoji_popout(cx);
                    cx.stop_propagation();
                // Dialogs over the page, top first, under the menus and popouts that open over them.
                } else if event.keystroke.key == "escape" && this.file_expanded.is_some() {
                    this.file_expanded = None;
                    cx.notify();
                    cx.stop_propagation();
                } else if event.keystroke.key == "escape" && this.remediation.is_some() {
                    this.remediation = None;
                    cx.notify();
                    cx.stop_propagation();
                } else if event.keystroke.key == "escape" && this.full_profile.is_some() {
                    this.close_full_profile(cx);
                    cx.stop_propagation();
                } else if event.keystroke.key == "escape" && this.go_live.is_some() {
                    this.close_go_live(cx);
                    cx.stop_propagation();
                } else if event.keystroke.key == "escape" && this.shop.modal.is_some() {
                    this.shop_close_modal(cx);
                    cx.stop_propagation();
                } else if event.keystroke.key == "escape"
                    && this.profile.is_none()
                    && this.role_popout.is_some()
                {
                    this.close_role_popout(cx);
                    cx.stop_propagation();
                } else if event.keystroke.key == "escape" && this.profile.is_some() {
                    this.profile = None;
                    cx.notify();
                    cx.stop_propagation();
                } else if event.keystroke.key == "escape" && this.settings.is_some() {
                    this.settings_escape(cx);
                    cx.stop_propagation();
                } else if event.keystroke.key == "escape" && this.escape_chain(window, cx) {
                    cx.stop_propagation();
                }
            }))
            .on_mouse_down(
                MouseButton::Navigate(gpui::NavigationDirection::Back),
                cx.listener(|this, _, _, cx| this.go_back(cx)),
            )
            .on_mouse_down(
                MouseButton::Navigate(gpui::NavigationDirection::Forward),
                cx.listener(|this, _, _, cx| this.go_forward(cx)),
            )
            .on_drop(
                cx.listener(|this, paths: &gpui::ExternalPaths, window, cx| {
                    this.drop_files(paths, window.modifiers().shift, cx);
                }),
            )
            // Capture runs before the text under the pointer starts its own selection.
            .capture_any_mouse_down(
                cx.listener(|this, event: &gpui::MouseDownEvent, window, _| {
                    // A right press keeps the selection: the message menu then offers Copy and Search with Google.
                    // Shift+left click preserves the anchor so Inline can extend it on bubble.
                    if event.button != MouseButton::Right
                        && !(event.button == MouseButton::Left && event.modifiers.shift)
                    {
                        this.selection.borrow_mut().clear();
                    }
                    this.keys.pointer_used();
                    this.vv.row_focus = false;
                    window.refresh();
                }),
            )
            .relative()
            .size_full()
            .flex()
            .flex_col()
            .font_family(SANS)
            // The locale's `--font-primary` after gg sans (`crate::intl`).
            .map(|mut root| {
                root.text_style().font_fallbacks = Some(crate::intl::font_fallbacks());
                root
            })
            .font_features(kerning())
            .text_size(px(16.))
            .line_height(px(22.))
            .text_color(color("--text-default"))
            .bg(if settings::client_theme::active().is_some() {
                gpui::rgba(0)
            } else {
                color("--background-base-lowest")
            })
            .when(settings::client_theme::previewing(), |root| {
                root.pr(px(settings::client_theme::panel_width()))
            })
            .children({
                let size = window.viewport_size();
                self.theme_backdrop(f32::from(size.width), f32::from(size.height))
            })
            // Any mouse press ends keyboard mode (module 203480 listens on the window). The listener above is tied
            // to this element's hitbox, which a layer that takes the mouse (User Settings, a dialog) covers: a press
            // inside such a layer left the ring on. This one asks no hitbox.
            .child({
                let root = cx.entity().downgrade();
                let root_move = root.clone();
                let root_up = root.clone();
                gpui::canvas(
                    |_, _, _| (),
                    move |_, (), window, _| {
                        window.on_mouse_event(move |_: &MouseDownEvent, phase, window, cx| {
                            if phase != gpui::DispatchPhase::Capture {
                                return;
                            }
                            let _ = root.update(cx, |this, _| {
                                let was_keyboard = this.keys.keyboard_mode_is_on();
                                this.keys.pointer_used();
                                if was_keyboard {
                                    window.refresh();
                                }
                            });
                        });
                        window.on_mouse_event(
                            move |event: &gpui::MouseMoveEvent, phase, window, cx| {
                                if phase != gpui::DispatchPhase::Capture {
                                    return;
                                }
                                let _ = root_move.update(cx, |this, cx| {
                                    this.menu_pointer_moved(event.position, cx);
                                    if this.active_menu_slider_drag.is_some() {
                                        this.menu_slider_mouse_move(event, window, cx);
                                    }
                                    if this.sidebar_resizing.is_some() {
                                        this.sidebar_resize_move(event, cx);
                                    }
                                    if this.selection.borrow().is_dragging() {
                                        let mut changed = this
                                            .selection
                                            .borrow_mut()
                                            .drag_to_point(event.position);
                                        let bounds = this.list.viewport_bounds();
                                        if bounds.size.height > px(0.) {
                                            let edge = px(28.);
                                            let delta = if event.position.y < bounds.top() + edge {
                                                -((bounds.top() + edge - event.position.y)
                                                    .min(px(24.)))
                                            } else if event.position.y > bounds.bottom() - edge {
                                                (event.position.y - (bounds.bottom() - edge))
                                                    .min(px(24.))
                                            } else {
                                                px(0.)
                                            };
                                            if delta != px(0.) {
                                                this.list.scroll_by(delta);
                                                changed = true;
                                                cx.notify();
                                            }
                                        }
                                        if changed {
                                            window.refresh();
                                        }
                                    }
                                });
                            },
                        );
                        window.on_mouse_event(
                            move |event: &gpui::MouseUpEvent, phase, window, cx| {
                                if phase != gpui::DispatchPhase::Capture {
                                    return;
                                }
                                let _ = root_up.update(cx, |this, cx| {
                                    if this.active_menu_slider_drag.is_some() {
                                        this.menu_slider_mouse_up(event, window, cx);
                                    }
                                    if this.sidebar_resizing.is_some() {
                                        this.sidebar_resize_end(cx);
                                    }
                                    if this.selection.borrow().is_dragging() {
                                        this.selection.borrow_mut().end();
                                        window.refresh();
                                    }
                                });
                            },
                        );
                    },
                )
                .absolute()
                .size_0()
            })
            .child(autoscroll::layer(self.autoscroll.clone()))
            .children(self.theme_preview_panel(cx))
            .children(self.custom_theme_panel(cx))
            .children(self.settings_layer(window, cx))
            .children(self.profile_editor_layer(window, cx))
            .children(self.profile_editor_picker_layer(window, cx))
            .children(self.profile_editor_styles_layer(window, cx))
            .children(self.channel_settings_layer(cx))
            .children(self.server_settings_layer(cx))
            .children(self.delete_server_modal(cx))
            .children(self.server_games_dropdown(window, cx))
            .children(self.ban_dialog(cx))
            .children(self.role_menu_layer(cx))
            .children(self.view_as_role_picker(cx))
            .children(self.role_delete_modal(cx))
            .children(self.role_add_members_modal(cx))
            .children(self.role_colour_layer(cx))
            .children(self.template_delete_modal(cx))
            .children(self.members_sort_layer(cx))
            .children(self.members_join_layer(window, cx))
            .children(self.members_prune_modal(cx))
            .children(self.engagement_select_layer(window, cx))
            .children(self.sound_dialog_modal(cx))
            .children(self.stream_report_modal(cx))
            .children(self.stream_report_list(cx))
            .children(self.sticker_dialog_modal(cx))
            .children(self.access_dialog_modal(cx))
            .children(self.onboarding_layer(cx))
            .children(self.screening_layer(window, cx))
            .children(self.automod_picker_layer(cx))
            .children(self.automod_bar_layer(cx))
            .children(self.automod_dialog_modal(cx))
            .children(self.automod_confirm_modal(cx))
            .children(self.automod_menu_layer(cx))
            .children(self.automod_select_layer(cx))
            .children(self.safety_verify_modal(cx))
            .children(self.safety_risky_modal(cx))
            .children(self.audit_select_layer(cx))
            .children(self.safety_select_layer(cx))
            .children(self.community_wizard_modal(cx))
            .children(self.community_select_layer(cx))
            .children(self.overview_select_layer(cx))
            .children(self.boost_perks_modal(cx))
            .children({
                self.call_popout_frame(cx);
                self.overlay_frame(cx);
                self.call_fullscreen_layer(window, cx)
            })
            .children(self.boost_page_overlay(cx))
            .children(self.boost_apply_layer(window, cx))
            .children(self.overview_confirm_modal(cx))
            .children(self.access_menu_layer(cx))
            .children(self.discovery_select_layer(cx))
            .children(self.discovery_agreement_modal(cx))
            .children(self.welcome_layers(cx))
            .children(self.landing_platform_layer(cx))
            .children(
                self.server_settings
                    .as_ref()
                    .map(|open| open.guild)
                    .and_then(|guild| self.access_preview_modal(guild, cx)),
            )
            .children(self.webhook_channel_select(cx))
            .children(self.webhook_delete_modal(cx))
            .children(self.channel_settings_select(window, cx))
            .children(self.delete_channel_modal(cx))
            .children(self.sync_permissions_modal(cx))
            .children(self.add_target_popout(cx))
            .children(self.security_actions_modal(cx))
            .children(self.security_actions_select(cx))
            .children(self.inventory_popout_layer(window, cx))
            .children(self.role_popout_layer(cx))
            .children(self.header_popout_layer(cx))
            .children(self.profile_layer(window, cx))
            .children(self.role_picker_layer(window, cx))
            .children(self.sticker_popout_layer(cx))
            .children(self.emoji_popout_layer(cx))
            .children(self.shop_modal(window, cx))
            .children(self.discover_layers(window, cx))
            .children(self.purchase_modal(window, cx))
            .children(self.tip_layer())
            .children(self.lottie_icon_probe())
            .children(self.system_bar())
            .child(motion::driver())
            .child(motion::layers::faded(
                self.regions.place(
                    regions::Kind::Titlebar,
                    regions::style_of(regions::Kind::Titlebar),
                ),
                base_opacity,
            ))
            .children(self.notice_bar(cx))
            .child({
                let rail = self
                    .regions
                    .place(regions::Kind::Rail, regions::style_of(regions::Kind::Rail));
                let sidebar_width = self.sidebar_width();
                let sidebar = if self.guild.is_some() {
                    self.regions.place(
                        regions::Kind::Sidebar,
                        regions::style_of_sidebar(sidebar_width),
                    )
                } else if self.discover_active() {
                    self.discover_sidebar(cx).into_any_element()
                } else {
                    self.regions.place(
                        regions::Kind::Sidebar,
                        regions::style_of_sidebar(sidebar_width),
                    )
                };
                let content = if self.discover_active() {
                    self.discover_page(window, cx)
                } else if self.guild.is_none() && self.channel.is_none() && self.nitro_open {
                    self.nitro_page(window, cx)
                } else if self.quests_active() {
                    self.quests_page(window, cx)
                } else if self.shop_active() {
                    self.shop_page(window, cx)
                } else if self.message_requests_active() {
                    self.message_requests_page(window, cx)
                } else if self.library_active() {
                    self.library_view(window, cx)
                } else if self.guild.is_none() && self.channel.is_none() {
                    self.friends_panel(window, cx).into_any_element()
                } else if self
                    .member_view
                    .as_ref()
                    .is_some_and(|view| Some(view.guild) == self.guild)
                {
                    self.member_safety_view(cx)
                } else if !self.guild_guide_open
                    && !self.channel_browser_open
                    && self.onboarding_blocked()
                {
                    // Module 140354 in place of the channel, header included (`gk` of 596339).
                    self.onboarding_blocked_page(cx)
                } else if self.voice_view_open() {
                    self.voice_view(window, cx)
                } else if self.stage_view_open() {
                    self.stage_view(cx)
                } else if self.guild_guide_open && self.guild.is_some() {
                    self.guild_guide_view(cx)
                } else if self.channel_browser_open && self.guild.is_some() {
                    self.channel_browser_view(window, cx, rows.into_any_element())
                } else if self.current_channel().is_some_and(Channel::is_forum) {
                    self.forum_view_in(window, cx)
                } else if self.open_post_forum().is_some() && self.forum_split_fits(window) {
                    self.forum_split_view(window, cx)
                        .unwrap_or_else(|| div().into_any_element())
                } else {
                    // The New Thread panel sits beside the whole chat column, header included.
                    div()
                        .flex()
                        .flex_1()
                        .min_w(px(0.))
                        .h_full()
                        .child(
                            div()
                                .parity("chat")
                                .flex()
                                .flex_col()
                                .flex_1()
                                .min_w(px(0.))
                                .h_full()
                                .pt(px(1.))
                                .overflow_hidden()
                                .child(match self.dm_call_area(window, cx) {
                                    // The header moves into the call area while a call shows (module 540462
                                    // `useExternalHeader`).
                                    Some(area) => area,
                                    None => self.regions.place(
                                        regions::Kind::Header,
                                        regions::style_of(regions::Kind::Header),
                                    ),
                                })
                                .child(
                                    div()
                                        .flex()
                                        .flex_col()
                                        .flex_1()
                                        .min_h(px(0.))
                                        .bg(color("--background-base-lower"))
                                        .flex_row()
                                        // "Hide chat": the call has the whole column (`.wrapper.noChat`).
                                        .when(self.dm_call_chat_hidden(), |chat| {
                                            chat.flex_none().h(px(0.)).overflow_hidden()
                                        })
                                        .child(
                                            div()
                                                .flex()
                                                .flex_col()
                                                .flex_1()
                                                .min_w(px(0.))
                                                .h_full()
                                                // Module 626977 over the list: a locked thread's bar, or the
                                                // notice of a channel just marked unread for every message.
                                                .children(self.thread_lock_notice(cx))
                                                .children(self.unread_settings_notice(cx))
                                                .children(self.opt_in_channel_notice(cx))
                                                .child(self.regions.place(
                                                    regions::Kind::Messages,
                                                    regions::style_of(regions::Kind::Messages),
                                                ))
                                                .child(match self.chat_blocker() {
                                                    // `wrapper_ST_Y69` sits where the composer's form does: 8px in
                                                    // and over the list's last 16px.
                                                    Some(blocker) => div()
                                                        .flex_none()
                                                        .mt(px(-16.))
                                                        .px(px(8.))
                                                        .pb(px(8.))
                                                        .child(match blocker {
                                                            blocked::ChatBlocker::System {
                                                                changelog,
                                                            } => self
                                                                .system_dm_composer(changelog, cx),
                                                            blocked::ChatBlocker::Blocked(user) => {
                                                                self.blocked_composer(user, cx)
                                                            }
                                                        })
                                                        .into_any_element(),
                                                    None if self.screening_pending() => {
                                                        self.screening_composer(cx)
                                                    }
                                                    None if self.follow_blocker_applies() => {
                                                        self.follow_composer(cx)
                                                    }
                                                    None => self
                                                        .regions
                                                        .place_composer()
                                                        .unwrap_or_else(|| {
                                                            self.composer_bar(cx).into_any_element()
                                                        }),
                                                }),
                                        )
                                        .children(self.search_panel(cx))
                                        .when(
                                            self.show_members
                                                && self.guild.is_some()
                                                && self.new_thread.is_none()
                                                && self
                                                    .search
                                                    .as_ref()
                                                    .is_none_or(|search| search.shown.is_none()),
                                            |this| {
                                                this.child(
                                                    div()
                                                        .flex_none()
                                                        .w(px(1.))
                                                        .h_full()
                                                        .bg(color("--background-mod-subtle")),
                                                )
                                                .child(self.regions.place(
                                                    regions::Kind::Members,
                                                    regions::style_of(regions::Kind::Members),
                                                ))
                                            },
                                        )
                                        .children(self.dm_profile(window, cx))
                                        .children(self.group_members_panel(cx)),
                                ),
                        )
                        .children(self.new_thread_panel(cx))
                        .children(self.mod_view_panel(cx))
                        .into_any_element()
                };
                motion::layers::faded(self.main_row(rail, sidebar, content, cx), base_opacity)
            })
            .child(self.sidebar_resize_handle(cx))
            .child(motion::layers::faded(
                self.user_panel(cx).into_any_element(),
                base_opacity,
            ))
            .children(self.forum_layer(cx))
            .children(self.context_menu(window, cx))
            .children(self.menu_submenu(window, cx))
            .children(self.delete_modal(cx))
            .children(self.leaving_modal(cx))
            .children(self.voice_dialog_layer(cx))
            .children(self.pin_modal(cx))
            .children(self.voice_discard_modal(cx))
            .children(self.too_long_modal(cx))
            .children(
                (!self.context.in_overlay)
                    .then(|| self.context_menu_layer(window, cx))
                    .flatten(),
            )
            .children(self.search_popout(window, cx))
            .children(self.file_menu_layer(cx))
            .children(self.file_expanded_layer(cx))
            .children(self.view_reactions_modal(cx))
            .children(self.poll_modals(cx))
            .children(self.audio_menu_layer(cx))
            .children(self.voice_panel_layer(window, cx))
            .children(self.go_live_layer(window, cx))
            .children(self.camera_error_modal(cx))
            .children(self.incoming_call_layer(window, cx))
            .children(self.search_filters_modal(cx))
            .children(self.add_server_modal(cx))
            .children(self.invite_modal(cx))
            .children(self.invite_select_layer(cx))
            .children(self.guild_notifications_modal(cx))
            .children(self.create_channel_modal(cx))
            .children(self.channel_status_modal(window, cx))
            .children(self.follow_modal(cx))
            .children(self.start_stage_modal(window, cx))
            .children(self.stage_welcome_modal(cx))
            .children(self.stage_end_modal(cx))
            .children(self.voice_alert_modal(cx))
            .children(self.overlay_survey_modal(cx))
            .children(self.feedback_modal(cx))
            .children(self.publish_modal(cx))
            .children(self.guild_events_layer(window, cx))
            .children(self.guild_menu_layer(cx))
            .children(self.thread_browser_popout(window, cx))
            .children(self.active_threads_modal(window, cx))
            .children(self.inbox_popout(window, cx))
            .children(self.mentions_filter_layer(cx))
            .children(self.friend_menu_layer(cx))
            .children(self.remove_friend_modal(cx))
            // Not in the pinned client: ServerInfo's dialog (`plugin_hooks/server_info.rs`).
            .children(self.server_info_layer(cx))
            // Not in the pinned client: MessageTags' tag dialog (`plugin_hooks/message_tags.rs`).
            .children(self.message_tags_dialog(window, cx))
            // Not in the pinned client: VoiceMessages' dialog (`plugin_hooks/voice_messages.rs`).
            .children(self.voice_messages_dialog(cx))
            // Not in the pinned client: UserPFP's Custom Avatar dialog (`plugin_hooks/userpfp.rs`).
            .children(self.userpfp_dialog(window, cx))
            // Not in the pinned client: ZipPreview's file dialog (`plugin_hooks/zip_preview.rs`).
            .children(self.zip_preview_dialog(window, cx))
            .children(self.not_friend_modal(cx))
            .children(self.favorites_full_modal(cx))
            .children(self.chat_alert_modal(cx))
            .children(self.disable_family_center_modal(cx))
            .children(self.follow_success_modal(cx))
            .children(self.guild_cap_upsell_layer(cx))
            .children(self.app_directory_promo_modal(cx))
            .children(self.guide_celebration_layer(window))
            .children(self.graphic_dialog(cx))
            .children(self.birthday_layers(window, cx))
            .children(self.library_remove_modal(cx))
            .children(self.composer_ai_modal(cx))
            .children(self.friend_nickname_layer(window, cx))
            .children(self.favorite_nickname_layer(window, cx))
            .children(self.group_edit_layer(window, cx))
            .children(self.group_leave_layer(cx))
            .children(self.member_action_layer(window, cx))
            .children(self.inventory_about_layer(cx))
            .children(self.group_dm_modal(window, cx))
            .children(self.quick_switcher_modal(window, cx))
            .children(self.keyboard_help_modal(window, cx))
            .children(self.keyboard_explainer_modal(cx))
            .children(self.activity_shelf_layer(window, cx))
            .children(self.activity_dialog_layer(cx))
            .children(self.update_dialog_layer(cx))
            .children(self.oauth_layer(window, cx))
            .children(self.activity_pip_layer(window))
            .children(self.video_pip_layer(window, cx))
            .children(self.notify_menu(window, cx))
            .children(self.notify_mute_submenu(window, cx))
            .children(self.unpin_modal(cx))
            .children(self.pins_popout(window, cx))
            .children(self.forward_modal(window, cx))
            .children(self.full_profile_modal(window, cx))
            .children(self.profile_menu_layer(cx))
            .children(self.profile_submenu_layer(window, cx))
            .children(self.remediation_modal(cx))
            .children(self.report_modal(cx))
            .children(self.custom_reminder_modal(cx))
            .children(self.interaction_modal(cx))
            .children(self.status_popout_layer(cx))
            .children(self.custom_status_modal(cx))
            .children(self.plus_menu_layer(window, cx))
            .children(self.leave_modal(cx))
            .children(self.destructive_modal(cx))
            .children(self.reorder_hint_modal(cx))
            .children(self.tutorial_layer(cx))
            .children(self.logout_modal(cx))
            .children(self.manage_accounts_modal(cx))
            .children(self.add_account_modal(cx))
            .children(self.account_more_menu(cx))
            .children(self.folder_settings_modal(cx))
            .children(self.folder_picker_layer(cx))
            .children(self.folder_menu_layer(cx))
            .children(self.settings_dialogs(cx))
            .children(image_crop_layers)
            .children(self.modify_attachment_modal(cx))
            .children(self.report_issue_modal(cx))
            .children(self.game_profile_layer(cx))
            .children(self.upload_error_modal(cx))
            .children(self.try_it_out_upsell(cx))
            .children(self.sent_edit_modal(cx))
            .children(self.snooze_layer(cx))
            .children(self.poll_modal(cx))
            .children(self.schedule_modal_layer(window, cx))
            .children(self.badge_modal_layer(cx))
            .children(self.spotify_premium_layer(cx))
            .children(self.lightbox_layer(cx))
            .children(self.video_fullscreen_layer(window, cx))
            .children(self.clips_modal(window, cx))
            .children(self.clips_delete_modal(cx))
            .children(self.clips_menu_layer(window, cx))
            .children(self.clips_card_menu_layer(cx))
            .children(self.toast_layer())
            .children(self.app_toast_layer())
            // Not in the pinned client: the plugins' notification card (`plugin_hooks/plugin_notifications.rs`).
            .children(self.plugin_notification_layer(window, cx))
            .children(self.picker_layer(window, cx))
            .children(self.soundboard_gift_picker(window, cx))
            // Module 738876: the drop area over everything (`z-index: 2000`) while files are dragged in.
            .children(self.file_drop_area(window, cx))
            .children(self.splash_layer(window, cx));
        // Capture only: the display name style comparison sheet (`name-sheet` step).
        #[cfg(feature = "capture")]
        let element = element
            .children(name_styles::sheet_layer())
            .children(self.capture_file_card_layer(cx))
            .children(self.capture_upload_card_layer(cx));
        self.focus_new_menu(window, cx);
        self.end_call_background_frame(cx);
        motion::leave_view();
        element
    }
}

pub fn bind_focus(root: &gpui::Entity<Root>, window: &mut Window, cx: &mut App) {
    let handle = root.read(cx).composer.read(cx).focus_handle(cx);
    window.focus(&handle, cx);
    // Without the system-wide hook, push to talk is only seen while the window is active.
    root.update(cx, |this, cx| {
        this.notification_focused = window.is_window_active();
        this.start_inventory_idle_timer(cx);
        this.notification_runtime.attach(window);
        this.refresh_notification_badge();
        this.tray_runtime.attach(window);
        cx.observe_window_activation(window, |this, window, cx| {
            this.notification_focus_changed(window.is_window_active(), cx);
            this.schedule_inventory(cx);
            if !window.is_window_active() {
                this.release_push_to_talk_on_blur();
            }
        })
        .detach();
    });
}

/// Built-in sections; anything else is a hoisted role, named from the guild's roles.
fn group_label(id: &str, roles: Option<&HashMap<Id, String>>) -> String {
    match id {
        "online" => dc_intl::tr("Online").to_owned(),
        "offline" => dc_intl::tr("Offline").to_owned(),
        other => other
            .parse::<Id>()
            .ok()
            .and_then(|role| roles?.get(&role).cloned())
            .unwrap_or_else(|| other.to_owned()),
    }
}

/// English labels of the status picker. The full catalog comes with `STRINGS/`.
fn status_label(status: &str) -> &'static str {
    match status {
        "dnd" => dc_intl::tr("Do Not Disturb"),
        "idle" => dc_intl::tr("Idle"),
        "invisible" => dc_intl::tr("Invisible"),
        "offline" => dc_intl::tr("Offline"),
        _ => dc_intl::tr("Online"),
    }
}

/// The presence dot in its cut-out ring. Do Not Disturb adds the bar across the dot.
/// The reaction picker opens beside the add button, its right edge this far right of the button's.
const PICKER_REACTION_OFFSET: f32 = picker::PICKER_WIDTH + 8.;

/// `clickable_fErCLM` (`687828.*.css`): a 24px top bar button, `--icon-muted`, on hover `--background-mod-normal`
/// behind `--interactive-icon-hover`, active or `selected` `--interactive-icon-active`. `.occlude()` keeps presses off
/// the titlebar's drag area (`trailing_3zSH5d`: `-webkit-app-region: no-drag`).
fn bar_button(id: &'static str, name: &'static str, selected: bool) -> gpui::Stateful<gpui::Div> {
    let group = SharedString::from(format!("bar-{id}"));
    div()
        .id(id)
        .group(group.clone())
        .size(px(24.))
        .rounded(px(6.))
        .cursor_pointer()
        .flex()
        .items_center()
        .justify_center()
        .hover(|style| style.bg(color("--background-mod-normal")))
        .child(
            svg()
                .path(format!("icons/{name}.svg"))
                .id(SharedString::from(format!("{id}-icon")))
                .size(px(18.))
                .flex_none()
                .text_color(color(if selected {
                    "--interactive-icon-active"
                } else {
                    "--icon-muted"
                }))
                .when(!selected, |icon| {
                    icon.group_hover(group.clone(), |style| {
                        style.text_color(color("--interactive-icon-hover"))
                    })
                    .group_active(group, |style| {
                        style.text_color(color("--interactive-icon-active"))
                    })
                }),
        )
}

/// The status of a 20px avatar (`svg-mask-avatar-status-round-20`): a 6px dot at (14, 14) in a 10px hole.
fn status_dot_small(status: &str, ring: &str) -> impl IntoElement {
    let tint = match status {
        "dnd" => "--icon-status-dnd",
        "idle" => "--icon-status-idle",
        "invisible" | "offline" => "--icon-status-offline",
        _ => "--icon-status-online",
    };
    div()
        .absolute()
        .left(px(12.))
        .top(px(12.))
        .size(px(10.))
        .rounded_full()
        .bg(color(ring))
        .flex()
        .items_center()
        .justify_center()
        .child(
            div()
                .size(px(6.))
                .rounded_full()
                .bg(color(tint))
                .flex()
                .items_center()
                .justify_center()
                .when(status == "dnd", |this| {
                    this.child(div().w(px(4.)).h(px(1.)).rounded(px(0.5)).bg(color(ring)))
                }),
        )
}

fn status_dot(status: &str, ring: &str) -> impl IntoElement {
    status_dot_sized(status, ring, 10., 3.)
}

/// The status dot of a 40px avatar (`svg-mask-avatar-status-round-40`): 12px at (28, 28) in a 16px hole.
fn status_dot_40(status: &str, ring: &str) -> impl IntoElement {
    status_dot_sized(status, ring, 12., 2.)
}

/// A status dot of `dot` px in a ring-colored hole reaching `out` px past the avatar corner; the dnd bar,
/// offline hole and idle bite scale with the dot (masks `svg-mask-status-*`).
fn status_dot_sized(status: &str, ring: &str, dot: f32, out: f32) -> impl IntoElement {
    let hole = dot + if dot >= 12. { 4. } else { 6. };
    div()
        .absolute()
        .right(px(-out))
        .bottom(px(-out))
        .size(px(hole))
        .rounded_full()
        .bg(color(ring))
        .flex()
        .items_center()
        .justify_center()
        .children(settings::client_theme::token_cover(ring, hole / 2.))
        .child(status_dot_inner(status, ring, dot))
}

/// The dot itself, `dot` px: the dnd bar, offline hole and idle bite cut in the ring colour.
fn status_dot_inner(status: &str, ring: &str, dot: f32) -> gpui::Div {
    let tint = match status {
        "dnd" => "--icon-status-dnd",
        "idle" => "--icon-status-idle",
        "invisible" | "offline" => "--icon-status-offline",
        // `PLATFORM_TWITCH` (module 935154).
        "streaming" => "--platform-twitch",
        _ => "--icon-status-online",
    };
    // Not in the pinned client: ColorSighted keeps the colour without the shapes cut in it (`plugin_hooks.rs`).
    let status = if plugin_hooks::color_sighted() {
        "online"
    } else {
        status
    };
    div()
        .size(px(dot))
        .rounded_full()
        .bg(color(tint))
        .flex()
        .items_center()
        .justify_center()
        .relative()
        .when(status == "dnd", |this| {
            this.child(
                div()
                    .w(px(dot * 0.6))
                    .h(px(dot * 0.2))
                    .rounded(px(dot * 0.1))
                    .bg(color(ring))
                    .children(settings::client_theme::token_cover(ring, dot * 0.1)),
            )
        })
        // `svg-mask-status-streaming`: the play triangle cut out of the dot.
        .when(status == "streaming", |this| {
            this.child(
                svg()
                    .path("icons/status_streaming_play.svg")
                    .absolute()
                    .left_0()
                    .top_0()
                    .size(px(dot))
                    .text_color(color(ring)),
            )
        })
        // `svg-mask-status-offline`: a hole of half the dot's size.
        .when(matches!(status, "offline" | "invisible"), |this| {
            this.child(
                div()
                    .size(px(dot * 0.5))
                    .rounded_full()
                    .bg(color(ring))
                    .children(settings::client_theme::token_cover(ring, dot * 0.25)),
            )
        })
        // `svg-mask-status-idle`: the dot less a circle of radius 0.375 centred at a quarter.
        .when(status == "idle", |this| {
            this.child(
                div()
                    .absolute()
                    .left(px(-dot * 0.125))
                    .top(px(-dot * 0.125))
                    .size(px(dot * 0.75))
                    .rounded_full()
                    .bg(color(ring))
                    .children(settings::client_theme::token_cover(ring, dot * 0.375)),
            )
        })
}

/// One run of the edit box's hint (`wDsPXs`): the text, the cancel link, the text, the save link.
fn edit_hint(index: usize) -> String {
    message_part("wDsPXs", &[], index)
}

/// The `index`th run of a message (text between and inside its tags, in order), for a line the client draws in
/// pieces with links and hooks in between.
pub(super) fn message_part(key: &str, values: &dc_intl::Args<'_>, index: usize) -> String {
    dc_intl::format_runs(key, values)
        .into_iter()
        .nth(index)
        .map(|(text, _)| text)
        .unwrap_or_default()
}

/// A message whose `$b` parts are bold (the React builder's `<strong>`), as one styled text.
pub(super) fn bold_message(
    key: &str,
    values: &dc_intl::Args<'_>,
    weight: FontWeight,
) -> gpui::StyledText {
    let mut text = String::new();
    let mut highlights = Vec::new();
    for (part, tag) in dc_intl::format_runs(key, values) {
        let start = text.len();
        text.push_str(&part);
        if tag.as_deref() == Some("$b") {
            highlights.push((
                start..text.len(),
                gpui::HighlightStyle {
                    font_weight: Some(weight),
                    ..Default::default()
                },
            ));
        }
    }
    gpui::StyledText::new(text).with_highlights(highlights)
}

/// `C(timestamp, true)` in the bundle's date helpers (`timestamps::header_time`).
fn header_time(
    local: chrono::DateTime<chrono::Local>,
    now: chrono::DateTime<chrono::Local>,
) -> String {
    timestamps::header_time(local, now)
}

/// The status of an 80px avatar (`svg-mask-avatar-status-round-80`): a 16px dot at (60, 60) in a 24px hole.
fn status_dot_large(status: &str, ring: &str) -> impl IntoElement {
    let tint = match status {
        "dnd" => "--icon-status-dnd",
        "idle" => "--icon-status-idle",
        "invisible" | "offline" => "--icon-status-offline",
        _ => "--icon-status-online",
    };
    div()
        .absolute()
        .left(px(56.))
        .top(px(56.))
        .size(px(24.))
        .rounded_full()
        .bg(color(ring))
        .flex()
        .items_center()
        .justify_center()
        .child(if status == "dnd" && !plugin_hooks::color_sighted() {
            div()
                .size(px(16.))
                .rounded_full()
                .bg(color(tint))
                .flex()
                .items_center()
                .justify_center()
                .child(div().w(px(10.)).h(px(3.)).rounded(px(1.5)).bg(color(ring)))
        } else {
            // Offline keeps a hole and idle its bite, as the smaller dots do.
            status_dot_inner(status, ring, 16.)
        })
}

#[cfg(test)]
mod tests {
    use super::{block_gaps, relative_time};
    use dc_markdown::parse;

    #[test]
    fn relative_time_follows_the_captured_wording() {
        assert_eq!(relative_time(90_000), "a day ago");
        assert_eq!(relative_time(10), "a few seconds ago");
        assert_eq!(relative_time(3 * 3600), "3 hours ago");
        assert_eq!(relative_time(-600), "in 10 minutes");
    }

    #[test]
    fn neighboring_margins_collapse_as_in_the_capture() {
        // Heading one, two, three and subtext in the fixture: 8, then 16, 16, 8.
        let blocks = parse("# a\n## b\n### c\n-# d");
        assert_eq!(block_gaps(&blocks), [8., 16., 16., 8.]);
    }

    #[test]
    fn a_quote_keeps_its_margin_on_both_sides() {
        let blocks = parse("> a\nb");
        assert_eq!(block_gaps(&blocks), [4., 4.]);
    }

    #[test]
    fn hoisted_role_groups_carry_the_role_name() {
        let roles = std::collections::HashMap::from([(42u64, "Moderators".to_owned())]);
        assert_eq!(super::group_label("online", Some(&roles)), "Online");
        assert_eq!(super::group_label("offline", None), "Offline");
        assert_eq!(super::group_label("42", Some(&roles)), "Moderators");
        assert_eq!(super::group_label("43", Some(&roles)), "43");
    }
}
