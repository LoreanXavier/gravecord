//! The menus of a call's tiles and the press rules of the call bar's popups.
//!
//! A tile's menu opens on a right click and from its Options button (module 175203: `onContextMenu` passes
//! `minimal = participant is a stream`, the Options button `minimal = true`), and module 540462 `handleContextMenu`
//! picks the menu:
//! - a stream tile: `stream-context` (module 16001) in its `minimal` form, `variant: "fixed"` (220px wide);
//! - a person's tile, Options button: module 326567 `r`, the voice user menu of module 335253 in a server and 661455
//!   elsewhere (`fixed`);
//! - a person's tile, right click: the user menu of the channel's kind with the call's items (`showMediaItems`,
//!   `showChannelCallItems`): 107632 in a server (with `showStageChannelItems` on a Stage), 385913 in a DM, 183245 in a
//!   group DM. These menus have no variant, so they are `flexible` (188 to 320px, module 165862 with stylesheet
//!   476662: `fixed_M3qk4j{width:220px}`, `flexible_M3qk4j{max-width:320px;min-width:188px}`).
//!
//! The person's own rows (Profile, Message, Call, Ring, Note, Apps, friends, roles, moderation, Copy ID) are the ones
//! the person menu of `context_menu.rs` already builds and runs; this file orders them as the call's menus do and adds
//! the call's own rows (volume, local mute, soundboard, Disable Video, Focus, Show Non-Video Participants, Show My Own
//! Camera, the stream rows). The menu is drawn inside the call view, so it opens in the popped-out window and over Full
//! Screen as well.

use super::context_menu::{Act, Entry, Row, Target};
use super::video_tiles::TileKind;
use super::voice_view::Menu;
use super::{Root, color, icon};
use dc_state::Id;
use gpui::{
    AnyElement, Bounds, Context, FontWeight, InteractiveElement, IntoElement, Pixels, Point,
    SharedString, Window, anchored, deferred, div, prelude::*, px,
};

/// Whether a popup button's click leaves its popup open: the press found it open (its press-outside may already
/// have closed it), or it is still open, closes it; otherwise the click opens it.
pub(super) fn opens_on_click(open_at_press: bool, open_now: bool) -> bool {
    !(open_at_press || open_now)
}

/// A checkbox row keeps the menu open (the `sL` items of module 477782 do not close it); every other row closes it.
fn keeps_open(run: &Run) -> bool {
    matches!(
        run,
        Run::StreamMute(_)
            | Run::Attenuation
            | Run::ShowOwnStream
            | Run::PausePreview
            | Run::ShowNonVideo
            | Run::ShowOwnCamera
            | Run::DisableVideo(_)
            | Run::ShareAudio
            | Run::Context(Act::VoiceMute | Act::VoiceDeafen | Act::SoundboardMute, _)
    )
}

/// `fixed_M3qk4j`.
const FIXED_WIDTH: f32 = 220.;
/// `flexible_M3qk4j` and `submenu_M3qk4j`.
const FLEXIBLE_MIN: f32 = 188.;
const FLEXIBLE_MAX: f32 = 320.;

/// Whose tile was opened.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Subject {
    Stream(Id),
    User(Id),
}

/// The kind of call the tile is in, for the person menu it opens.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum CallKind {
    Guild {
        stage: bool,
    },
    Dm,
    /// A group DM; `chat` is `showChatItems` (the layout is MINIMUM or NORMAL, module 540462).
    Group {
        chat: bool,
    },
}

/// An open tile menu.
pub(super) struct Open {
    pub subject: Subject,
    pub minimal: bool,
    pub at: Point<Pixels>,
    pub kind: CallKind,
    /// The person's rows as the person menu builds them, taken when the menu opened.
    pub person: Vec<Row>,
    pub person_target: Option<Target>,
    /// A DM's channel rows (Mark As Read, pins, Close DM, Mute) and its target.
    pub channel: Vec<Row>,
    pub channel_target: Option<Target>,
    /// The open submenu: the index of its row among the menu's items, and where that row is.
    pub sub: Option<(usize, Bounds<Pixels>)>,
}

/// What a row of a tile menu does.
#[derive(Clone, Debug, PartialEq)]
pub(super) enum Run {
    /// A row of the person menu, run by it.
    Context(Act, Target),
    /// A row of one of the person menu's submenus.
    ContextSub(Act, Target, usize),
    Watch(Id),
    StopWatching(Id),
    StopStreaming,
    ChangeStream,
    StreamMute(Id),
    ShareAudio,
    /// Stream Quality: a resolution or a frame rate (module 600125).
    Resolution(u32),
    FrameRate(u32),
    Attenuation,
    ShowOwnStream,
    PausePreview,
    ShowNonVideo,
    ShowOwnCamera,
    Focus(Id),
    Unfocus,
    VideoBackground,
    DisableVideo(Id),
    VoiceSettings,
    CopyText(String),
    /// A row whose feature this client does not have: drawn disabled, never live.
    Missing,
}

/// What a slider of a tile menu sets.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Slide {
    UserVolume(Id),
    StreamVolume(Id),
    AttenuationStrength,
}

/// A row of a tile menu.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Line {
    pub key: String,
    pub label: String,
    pub sub: Option<String>,
    pub icon: Option<&'static str>,
    pub check: Option<bool>,
    pub radio: Option<bool>,
    pub danger: bool,
    pub disabled: bool,
    pub run: Run,
    /// A submenu's rows (`Dr` with children).
    pub children: Vec<Item>,
    /// A person-menu row with a submenu (Roles, Move to, Apps, Invite to Server, Mute): its rows are the person
    /// menu's own, filled in when the menu is drawn.
    pub person_sub: bool,
}

impl Line {
    fn new(key: impl Into<String>, label: impl Into<String>, run: Run) -> Self {
        Self {
            key: key.into(),
            label: label.into(),
            sub: None,
            icon: None,
            check: None,
            radio: None,
            danger: false,
            disabled: run == Run::Missing,
            run,
            children: Vec::new(),
            person_sub: false,
        }
    }

    fn check(mut self, on: bool) -> Self {
        self.check = Some(on);
        self
    }

    fn radio(mut self, on: bool) -> Self {
        self.radio = Some(on);
        self
    }

    fn icon(mut self, name: &'static str) -> Self {
        self.icon = Some(name);
        self
    }

    fn danger(mut self) -> Self {
        self.danger = true;
        self
    }

    fn sub(mut self, text: impl Into<String>) -> Self {
        self.sub = Some(text.into());
        self
    }

    fn children(mut self, items: Vec<Item>) -> Self {
        self.disabled = items.is_empty();
        self.children = items;
        self
    }

    /// `labelContainer_M3qk4j`: 32px of min height with 8px of padding; the captured rows are 34, 36 with an icon or a
    /// control, 52 with a second line (`context_menu::Row::height`).
    fn height(&self) -> f32 {
        if self.sub.is_some() {
            52.
        } else if self.icon.is_some()
            || self.check.is_some()
            || self.radio.is_some()
            || !self.children.is_empty()
        {
            36.
        } else {
            34.
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(super) enum Item {
    Separator,
    /// A group's label (`groupLabel_M3qk4j`), as Stream Quality's "Frame Rate" and "Resolution".
    Label(String),
    Line(Line),
    Slider {
        key: &'static str,
        label: String,
        fraction: f32,
        slide: Slide,
    },
}

/// Groups in order with a separator between non-empty ones (`rX` groups of module 165862).
fn groups(list: Vec<Vec<Item>>) -> Vec<Item> {
    let mut out = Vec::new();
    for group in list.into_iter().filter(|group| !group.is_empty()) {
        if !out.is_empty() {
            out.push(Item::Separator);
        }
        out.extend(group);
    }
    out
}

/// The person menu's row for `act`, as a tile menu row that the person menu runs.
fn person_row(rows: &[Row], target: Option<Target>, act: Act) -> Option<Item> {
    let target = target?;
    rows.iter().find(|row| row.act == act).map(|row| {
        Item::Line(Line {
            key: format!("person-{act:?}"),
            label: row.label.clone(),
            sub: row.sub.clone(),
            icon: row.icon,
            check: row.check,
            radio: None,
            danger: row.danger,
            disabled: row.disabled,
            run: Run::Context(act, target),
            children: Vec::new(),
            person_sub: row.caret,
        })
    })
}

fn person_rows(rows: &[Row], target: Option<Target>, acts: &[Act]) -> Vec<Item> {
    acts.iter()
        .filter_map(|act| person_row(rows, target, *act))
        .collect()
}

/// What a person's tile menu shows besides the person menu's own rows.
#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct UserFacts {
    pub user: Id,
    pub me: bool,
    pub developer: bool,
    /// Our own switches (`isSelfMute`, `isSelfDeaf`) or the person's local mute.
    pub muted: bool,
    pub deafened: bool,
    pub soundboard_muted: bool,
    /// `isLocalVideoDisabled` of the person.
    pub video_disabled: bool,
    /// The person's volume as the slider shows it, 0 to 1.
    pub volume: f32,
    /// Module 984894: `Some(focused)` when the person is in the channel with video and the layout is neither MINIMUM
    /// nor NORMAL.
    pub focus: Option<bool>,
    /// Module 855187 on our own tile: `Some(camera_on)` when the row shows (a camera exists, and with the camera on the
    /// background filter is offered).
    pub video_background: Option<bool>,
    pub show_non_video: bool,
    pub show_own_camera: bool,
    /// The person's tile carries video (`streamId` of the participant): Pop Out Video instead of Pop Out User.
    pub has_video: bool,
}

/// 198229: our own Mute and Deafen, or the person's local Mute, Mute Soundboard and Disable Video.
fn media_rows(facts: &UserFacts, target: Option<Target>) -> Vec<Item> {
    let Some(target) = target else {
        return Vec::new();
    };
    if facts.me {
        vec![
            Item::Line(
                Line::new(
                    "mute",
                    dc_intl::tr("Mute"),
                    Run::Context(Act::VoiceMute, target),
                )
                .check(facts.muted),
            ),
            Item::Line(
                Line::new(
                    "deafen",
                    dc_intl::tr("Deafen"),
                    Run::Context(Act::VoiceDeafen, target),
                )
                .check(facts.deafened),
            ),
        ]
    } else {
        vec![
            Item::Line(
                Line::new(
                    "mute",
                    dc_intl::tr("Mute"),
                    Run::Context(Act::VoiceMute, target),
                )
                .check(facts.muted),
            ),
            Item::Line(
                Line::new(
                    "soundboard-sound-mute",
                    dc_intl::tr("Mute Soundboard"),
                    Run::Context(Act::SoundboardMute, target),
                )
                .check(facts.soundboard_muted),
            ),
            Item::Line(
                Line::new(
                    "disable-video",
                    dc_intl::tr("Disable Video"),
                    Run::DisableVideo(facts.user),
                )
                .check(facts.video_disabled),
            ),
        ]
    }
}

/// 972432: User Volume, not on our own tile.
fn volume_row(facts: &UserFacts) -> Option<Item> {
    (!facts.me).then(|| Item::Slider {
        key: "user-volume",
        label: dc_intl::tr("User Volume").to_owned(),
        fraction: facts.volume,
        slide: Slide::UserVolume(facts.user),
    })
}

/// 984894: Focus This Person / Unfocus This Person.
fn focus_row(facts: &UserFacts) -> Option<Item> {
    facts.focus.map(|focused| {
        Item::Line(Line::new(
            "focus-video",
            if focused {
                dc_intl::tr("Unfocus This Person")
            } else {
                dc_intl::tr("Focus This Person")
            },
            if focused {
                Run::Unfocus
            } else {
                Run::Focus(facts.user)
            },
        ))
    })
}

/// 855187: Preview Camera with the camera off, Change Video Background with it on; our own tile only.
fn video_background_row(facts: &UserFacts) -> Option<Item> {
    let on = facts.video_background.filter(|_| facts.me)?;
    Some(Item::Line(
        Line::new(
            "change-video-background",
            if on {
                dc_intl::tr("Change Video Background")
            } else {
                dc_intl::tr("Preview Camera")
            },
            Run::VideoBackground,
        )
        .icon(if on { "vbg_blur" } else { "voice_view_preview" }),
    ))
}

/// `showChannelCallItems`: 810845 (Pop Out), 854182 (Show Non-Video Participants), 575079 (Show My Own Camera).
fn channel_call_rows(facts: &UserFacts) -> Vec<Item> {
    let mut out = vec![
        // Popping one tile out into a window of its own is not built here.
        Item::Line(
            Line::new(
                "call-tile-popout",
                if facts.has_video {
                    dc_intl::tr("Pop Out Video")
                } else {
                    dc_intl::tr("Pop Out User")
                },
                Run::Missing,
            )
            .icon("voice_view_popout"),
        ),
        Item::Line(
            Line::new(
                "no-video-hide",
                dc_intl::tr("Show Non-Video Participants"),
                Run::ShowNonVideo,
            )
            .check(facts.show_non_video),
        ),
    ];
    if facts.me {
        out.push(Item::Line(
            Line::new(
                "self-video-hide",
                dc_intl::tr("Show My Own Camera"),
                Run::ShowOwnCamera,
            )
            .check(facts.show_own_camera),
        ));
    }
    out
}

/// The right-click menu of a person's tile.
pub(super) fn user_tile_items(
    kind: CallKind,
    facts: &UserFacts,
    person: &[Row],
    target: Option<Target>,
    channel: &[Row],
    channel_target: Option<Target>,
) -> Vec<Item> {
    let social = [
        Act::Profile,
        Act::Mention,
        Act::Message,
        Act::Call,
        Act::RingUser,
        Act::StopRinging,
        Act::Note,
        Act::FriendNickname,
    ];
    let people = [
        Act::EditProfile,
        Act::ChangeNickname,
        Act::Apps,
        Act::InviteToServer,
        Act::AddFriend,
        Act::RemoveFriend,
        Act::Ignore,
        Act::Block,
    ];
    let moderation = [
        Act::ModView,
        Act::Verify,
        Act::ServerMute,
        Act::ServerDeafen,
        Act::Disconnect,
        Act::Timeout,
        Act::RemoveTimeout,
        Act::Kick,
        Act::Ban,
    ];
    let volume: Vec<Item> = volume_row(facts).into_iter().collect();
    let copy_user = person_rows(person, target, &[Act::CopyId]);
    match kind {
        // Module 107632 with `showMediaItems`, `showChannelCallItems` and `showChatItems: false`.
        CallKind::Guild { .. } => {
            let first: Vec<Act> = social
                .iter()
                .copied()
                .filter(|act| *act != Act::Mention)
                .collect();
            let mut media = media_rows(facts, target);
            media.extend(focus_row(facts));
            media.extend(video_background_row(facts));
            media.extend(person_rows(person, target, &people));
            groups(vec![
                person_rows(person, target, &first),
                volume,
                media,
                person_rows(person, target, &[Act::Roles, Act::MoveMember]),
                channel_call_rows(facts),
                person_rows(person, target, &moderation),
                copy_user,
            ])
        }
        // Module 385913 `V` with `showMediaItems` and `showChannelCallItems`.
        CallKind::Dm => {
            let mut dm_first = person_rows(person, target, &[Act::Profile, Act::Call]);
            dm_first.extend(person_rows(
                person,
                target,
                &[
                    Act::RingUser,
                    Act::StopRinging,
                    Act::Note,
                    Act::FriendNickname,
                ],
            ));
            if !facts.me {
                dm_first.extend(person_rows(channel, channel_target, &[Act::CloseDm]));
            }
            let mut media = person_rows(person, target, &[Act::Apps]);
            media.extend(video_background_row(facts));
            media.extend(media_rows(facts, target));
            media.extend(focus_row(facts));
            media.extend(person_rows(
                person,
                target,
                &[
                    Act::InviteToServer,
                    Act::AddFriend,
                    Act::RemoveFriend,
                    Act::Ignore,
                    Act::Block,
                ],
            ));
            let mut copy = copy_user;
            if facts.developer
                && let Some(Target::Dm(id)) = channel_target
            {
                copy.push(Item::Line(Line::new(
                    "devmode-copy-channel-id",
                    dc_intl::tr("Copy Channel ID"),
                    Run::CopyText(id.to_string()),
                )));
            }
            let mut pins = person_rows(channel, channel_target, &[Act::PinDm]);
            if !facts.me {
                pins.extend(person_rows(channel, channel_target, &[Act::Favorite]));
            }
            groups(vec![
                person_rows(channel, channel_target, &[Act::MarkRead]),
                pins,
                dm_first,
                volume,
                media,
                person_rows(channel, channel_target, &[Act::Mute]),
                channel_call_rows(facts),
                copy,
            ])
        }
        // Module 183245 with `showMediaItems` and `showChannelCallItems`.
        CallKind::Group { chat } => {
            let first: Vec<Act> = social
                .iter()
                .copied()
                .filter(|act| chat || *act != Act::Mention)
                .collect();
            let mut media = media_rows(facts, target);
            media.extend(focus_row(facts));
            media.extend(video_background_row(facts));
            media.extend(person_rows(
                person,
                target,
                &[
                    Act::Apps,
                    Act::InviteToServer,
                    Act::AddFriend,
                    Act::RemoveFriend,
                    Act::Ignore,
                    Act::Block,
                ],
            ));
            groups(vec![
                person_rows(person, target, &first),
                volume,
                channel_call_rows(facts),
                media,
                copy_user,
            ])
        }
    }
}

/// The Options button of a person's tile: module 335253 in a server, 661455 elsewhere (`fixed`).
pub(super) fn user_tile_minimal_items(
    in_guild: bool,
    facts: &UserFacts,
    person: &[Row],
    target: Option<Target>,
) -> Vec<Item> {
    let mut head = Vec::new();
    if let Some(target) = target {
        head.push(Item::Line(
            Line::new(
                "user-profile",
                dc_intl::tr("View Profile"),
                Run::Context(Act::Profile, target),
            )
            .icon("voice_view_profile"),
        ));
        if !facts.me && person.iter().any(|row| row.act == Act::Message) {
            head.push(Item::Line(
                Line::new(
                    "message-user",
                    dc_intl::tr("Message"),
                    Run::Context(Act::Message, target),
                )
                .icon("voice_view_chat"),
            ));
        }
    }
    let mut media: Vec<Item> = volume_row(facts).into_iter().collect();
    media.extend(media_rows(facts, target));
    if facts.me {
        media.push(Item::Line(
            Line::new(
                "self-video-hide",
                dc_intl::tr("Show My Own Camera"),
                Run::ShowOwnCamera,
            )
            .check(facts.show_own_camera),
        ));
    }
    media.extend(video_background_row(facts));
    if facts.me {
        media.push(Item::Line(
            Line::new(
                "voice-and-video-settings",
                dc_intl::tr("Voice & Video Settings"),
                Run::VoiceSettings,
            )
            .icon("voice_view_gear"),
        ));
    }
    let copy = person_rows(person, target, &[Act::CopyId]);
    let tail = if facts.me {
        copy
    } else {
        let mut more = groups(vec![
            person_rows(person, target, &[Act::Ignore, Act::Block]),
            if in_guild {
                person_rows(
                    person,
                    target,
                    &[
                        Act::Verify,
                        Act::ServerMute,
                        Act::ServerDeafen,
                        Act::Disconnect,
                        Act::Timeout,
                        Act::RemoveTimeout,
                        Act::Kick,
                        Act::Ban,
                    ],
                )
            } else {
                Vec::new()
            },
            copy,
        ]);
        if more.is_empty() {
            more = Vec::new();
        }
        if more.is_empty() {
            Vec::new()
        } else {
            vec![Item::Line(
                Line::new("more-options", dc_intl::tr("More Options"), Run::Missing).children(more),
            )]
        }
    };
    let mut out = groups(vec![head, media, tail]);
    // A submenu row is never "missing": it opens its rows.
    for item in &mut out {
        if let Item::Line(line) = item
            && !line.children.is_empty()
        {
            line.disabled = false;
        }
    }
    out
}

/// What a stream tile's menu depends on.
#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct StreamFacts {
    pub owner: Id,
    /// The stream is our own.
    pub own: bool,
    /// `getActiveStreamForApplicationStream` is this stream: we watch it, or it is ours and live.
    pub active: bool,
    /// The call is on a Stage (`isGuildStageVoice`): a watched stream has no Stop Watching.
    pub stage: bool,
    pub muted: bool,
    pub volume: f32,
    /// `supports(SIDECHAIN_COMPRESSION)` and the switch, and its strength 0 to 1.
    pub attenuation: Option<(bool, f32)>,
    /// Our own stream: the quality that runs and what the account may pick.
    pub resolution: u32,
    pub fps: u32,
    pub allowed: Vec<(u32, u32)>,
    /// A desktop source with sound (`soundshare`), and whether its sound is shared.
    pub share_audio: Option<bool>,
    pub own_hidden: bool,
    pub pause_preview: bool,
}

/// `stream-context` (module 16001) in its `minimal` form: what a stream tile's right click and Options open.
pub(super) fn stream_tile_items(facts: &StreamFacts) -> Vec<Item> {
    let owner = facts.owner;
    let first: Vec<Item> = if facts.own {
        // Module 600125, `minimal`: Stop Streaming, Change Stream, Stream Quality, Share Stream Audio.
        let mut out = vec![
            Item::Line(
                Line::new(
                    "stop-streaming",
                    dc_intl::tr("Stop Streaming"),
                    Run::StopStreaming,
                )
                .icon("menu_stop_stream")
                .danger(),
            ),
            Item::Line(
                Line::new(
                    "change-windows",
                    dc_intl::tr("Change Stream"),
                    Run::ChangeStream,
                )
                .icon("menu_change_stream"),
            ),
            Item::Line(
                Line::new(
                    "stream-settings",
                    dc_intl::tr("Stream Quality"),
                    Run::Missing,
                )
                .children(quality_items(facts)),
            ),
        ];
        if let Some(on) = facts.share_audio {
            out.push(Item::Line(
                Line::new(
                    "stream-settings-audio-enable",
                    dc_intl::tr("Share Stream Audio"),
                    Run::ShareAudio,
                )
                .check(on),
            ));
        }
        out
    } else if facts.active && facts.stage {
        Vec::new()
    } else if facts.active {
        vec![Item::Line(
            Line::new(
                "watch",
                dc_intl::tr("Stop Watching"),
                Run::StopWatching(owner),
            )
            .icon("menu_stop_stream"),
        )]
    } else {
        vec![Item::Line(Line::new(
            "watch",
            dc_intl::tr("Watch Stream"),
            Run::Watch(owner),
        ))]
    };
    let mut sound = Vec::new();
    if facts.active && !facts.own {
        sound.push(Item::Line(
            Line::new("mute", dc_intl::tr("Mute"), Run::StreamMute(owner)).check(facts.muted),
        ));
        sound.push(Item::Slider {
            key: "stream-volume",
            label: dc_intl::tr("Stream Volume").to_owned(),
            fraction: facts.volume,
            slide: Slide::StreamVolume(owner),
        });
    }
    // 810845: Pop Out Stream; one tile in a window of its own is not built here.
    let popout = vec![Item::Line(
        Line::new(
            "call-tile-popout",
            dc_intl::tr("Pop Out Stream"),
            Run::Missing,
        )
        .icon("voice_view_popout"),
    )];
    let mut attenuation = Vec::new();
    if let (false, true, Some((on, strength))) = (facts.own, facts.active, facts.attenuation) {
        attenuation.push(Item::Line(
            Line::new(
                "sidechain-compression",
                dc_intl::tr("Stream Attenuation"),
                Run::Attenuation,
            )
            .sub(dc_intl::tr(
                "Automatically reduce stream volume when people are talking.",
            ))
            .check(on),
        ));
        if on {
            attenuation.push(Item::Slider {
                key: "sidechain-parameters",
                label: dc_intl::tr("Stream Attenuation Strength").to_owned(),
                fraction: strength,
                slide: Slide::AttenuationStrength,
            });
        }
    }
    let mut out = groups(vec![first, sound, popout, attenuation]);
    if facts.active {
        // `et`: a separator and More Options with 666328, 469707, 304006, 479335, 74329.
        let mut more = Vec::new();
        if facts.own {
            more.push(Item::Line(
                Line::new(
                    "self-stream-hide",
                    dc_intl::tr("Show My Screen Share"),
                    Run::ShowOwnStream,
                )
                .check(!facts.own_hidden),
            ));
            more.push(Item::Line(
                Line::new(
                    "self-stream-pause-preview-when-unfocused",
                    dc_intl::tr("Pause Preview When Discord Isn\u{2019}t Focused"),
                    Run::PausePreview,
                )
                .check(facts.pause_preview),
            ));
        }
        // 74329: the report form this client has no counterpart for.
        more.push(Item::Line(
            Line::new(
                "report-stream-problem",
                dc_intl::tr("Report Problem"),
                Run::Missing,
            )
            .icon("shop_circle_error")
            .danger(),
        ));
        out.push(Item::Separator);
        out.push(Item::Line(
            Line::new("more-options", dc_intl::tr("More Options"), Run::Missing).children(more),
        ));
    }
    out
}

/// Stream Quality (600125 `W`): Frame Rate (`ce`: 15, 30, 60) then Resolution (`Jk`: 480p to Source), as radios.
fn quality_items(facts: &StreamFacts) -> Vec<Item> {
    let mut out = vec![Item::Label(dc_intl::tr("Frame Rate").to_owned())];
    for fps in [15, 30, 60] {
        let line = Line::new(
            format!("stream-settings-fps-{fps}"),
            dc_intl::t!("bW+JCW", value = fps),
            Run::FrameRate(fps),
        )
        .radio(fps == facts.fps);
        out.push(Item::Line(line));
    }
    out.push(Item::Label(dc_intl::tr("Resolution").to_owned()));
    for resolution in [480, 720, 1080, 1440, 0] {
        let label = if resolution == 0 {
            dc_intl::tr("Source").to_owned()
        } else {
            dc_intl::t!("TEOC0I", resolution = resolution)
        };
        let line = Line::new(
            format!("stream-settings-resolution-{resolution}"),
            label,
            Run::Resolution(resolution),
        )
        .radio(resolution == facts.resolution);
        out.push(Item::Line(line));
    }
    out
}

/// A flexible menu's width estimated from its labels (14px medium text is about 7.2px a character), with the icon,
/// control and caret and the paddings, 188 to 320: only used to choose the side a submenu opens on.
fn estimated_width(items: &[Item]) -> f32 {
    let widest = items
        .iter()
        .map(|item| match item {
            Item::Line(line) => {
                line.label.chars().count() as f32 * 7.2
                    + if line.icon.is_some() { 28. } else { 0. }
                    + if line.check.is_some() || line.radio.is_some() {
                        28.
                    } else {
                        0.
                    }
                    + if line.children.is_empty() { 0. } else { 30. }
            }
            Item::Slider { label, .. } | Item::Label(label) => label.chars().count() as f32 * 7.2,
            Item::Separator => 0.,
        })
        .fold(0., f32::max);
    (widest + 34.).clamp(FLEXIBLE_MIN, FLEXIBLE_MAX)
}

impl Root {
    // ---- the call bar's popups ----

    /// A call-bar control that opens a popup. The press is seen in the capture phase, before the popup's own
    /// press-outside closes it, so the click can tell "it was open" (close it) from "it was closed" (open it): the
    /// official popout toggles instead of closing and opening again with its animation.
    pub(super) fn bar_opener<E: InteractiveElement>(
        &self,
        element: E,
        key: &'static str,
        open: bool,
        cx: &mut Context<Self>,
    ) -> E {
        element.capture_any_mouse_down(cx.listener(move |this, _: &gpui::MouseDownEvent, _, _| {
            this.vv.opener.set(Some((key, open)));
        }))
    }

    /// The last press landed on `key` while its popup was open.
    pub(super) fn opener_was_open(&self, key: &'static str) -> bool {
        matches!(self.vv.opener.take(), Some((pressed, true)) if pressed == key)
    }

    pub(super) fn clear_tip(&mut self) {
        self.tip = None;
        self.tip_pending = None;
    }

    /// A call-bar menu's button: open its menu, or close it when the press found it open.
    pub(super) fn toggle_bar_menu(
        &mut self,
        key: &'static str,
        menu: Menu,
        cx: &mut Context<Self>,
    ) {
        self.clear_tip();
        let was_open = self.opener_was_open(key);
        self.vv.menu = opens_on_click(was_open, self.vv.menu == Some(menu)).then_some(menu);
        self.vv.tile_menu = None;
        cx.notify();
    }

    /// A call-bar control's tooltip: none while its popup is open (`shouldShowTooltip: !popoutOpen`, module 105225).
    pub(super) fn bar_tip(
        &self,
        key: &'static str,
        tip: super::tooltip::Tip,
        target: impl IntoElement,
        open: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if open {
            div()
                .id(gpui::ElementId::Name(format!("tip-{key}").into()))
                .relative()
                .flex_none()
                .child(target)
                .into_any_element()
        } else {
            self.with_tip(key, tip, target, cx).into_any_element()
        }
    }

    /// A press outside a call popup closes it. The press goes no further (the layer rule of module 82038), unless it is
    /// on the call bar, where it is the press of the control under it.
    pub(super) fn close_call_popup(&mut self, position: Point<Pixels>, cx: &mut Context<Self>) {
        self.vv.menu = None;
        self.vv.tile_menu = None;
        if self.vv.bar_press.get() != Some(position) {
            cx.stop_propagation();
        }
        cx.notify();
    }

    #[cfg(feature = "capture")]
    /// Harness: what decides the call view's layout and what is open over it.
    pub(super) fn call_view_state(&self) -> String {
        format!(
            "focus {:?}; fullscreen {}; members hidden {}; menu {:?}; tile menu {}; audio menu {}; panel popout {:?}",
            self.vv.focus.tile,
            self.vv.fullscreen,
            self.vv.members_hidden,
            self.vv.menu,
            self.vv.tile_menu.is_some()
                && matches!(self.vv.menu, Some(Menu::Tile(..) | Menu::Stream(..))),
            self.audio_menu.is_some(),
            self.panel.popout,
        )
    }

    // ---- tile menus ----

    /// The kind of the call the view shows.
    fn shown_call_kind(&self) -> CallKind {
        let (guild, channel) = self.vv.shown_call;
        if guild != 0 {
            let stage = self.stores.guild(guild).is_some_and(|known| {
                known.channels.iter().any(|found| {
                    found.id == channel && found.kind == dc_state::ChannelKind::Other(13)
                })
            });
            return CallKind::Guild { stage };
        }
        let group = self
            .stores
            .private_channels
            .iter()
            .any(|dm| dm.id == channel && dm.kind == 3);
        if group {
            // The DM column's layout is NORMAL (module 628983 `getLayout`): its chat is beside the call.
            CallKind::Group {
                chat: !self.vv.fullscreen && !self.dm_call_chat_hidden(),
            }
        } else {
            CallKind::Dm
        }
    }

    /// Opens a tile's menu at the pointer: `minimal` from the Options button or on a stream.
    pub(super) fn open_tile_menu(
        &mut self,
        subject: Subject,
        minimal: bool,
        at: Point<Pixels>,
        cx: &mut Context<Self>,
    ) {
        let kind = self.shown_call_kind();
        let (guild, channel) = self.vv.shown_call;
        let (mut person, mut person_target, mut channel_rows, mut channel_target) =
            (Vec::new(), None, Vec::new(), None);
        if let Subject::User(user) = subject {
            let target = Target::User {
                user,
                guild: (guild != 0).then_some(guild),
            };
            // The person menu's own rows, with the media rows it shows in a voice row.
            self.context_voice_row();
            self.open_context_menu(target, at, cx);
            person = rows_of(self.context.open.take().map(|open| open.items));
            person_target = Some(target);
            if kind == CallKind::Dm {
                let target = Target::Dm(channel);
                self.open_context_menu(target, at, cx);
                channel_rows = rows_of(self.context.open.take().map(|open| open.items));
                channel_target = Some(target);
            }
        }
        self.clear_tip();
        self.menu_scroll.set_offset(gpui::point(px(0.), px(0.)));
        self.vv.menu = Some(match subject {
            Subject::Stream(owner) => Menu::Stream(owner, at),
            Subject::User(user) => Menu::Tile(user, at),
        });
        self.vv.tile_menu = Some(Open {
            subject,
            minimal,
            at,
            kind,
            person,
            person_target,
            channel: channel_rows,
            channel_target,
            sub: None,
        });
        cx.notify();
    }

    fn user_tile_facts(&self, user: Id) -> UserFacts {
        let me = self.stores.current_user.as_ref().map(|me| me.id) == Some(user);
        let (factor, local_mute) = self.context.mix.get(&user).copied().unwrap_or((1., false));
        let percent = factor * 100.;
        let fraction = (super::audio_menu::volume_to_slider(percent)
            / super::stream_volume::MAX_PERCENT)
            .clamp(0., 1.);
        let (guild, channel) = self.vv.shown_call;
        let has_video = self
            .stores
            .voice_members(guild, channel)
            .iter()
            .any(|state| state.user_id == user && state.self_video)
            || (me && self.call.as_ref().is_some_and(|call| call.camera));
        // Module 628983: a server's voice call is NO_CHAT, a DM's NORMAL until its chat is hidden or it goes Full Screen.
        let wide = guild != 0 || self.vv.fullscreen || self.dm_call_chat_hidden();
        let camera_on = self.call.as_ref().is_some_and(|call| call.camera);
        UserFacts {
            user,
            me,
            developer: self.developer_mode(),
            muted: if me {
                self.self_mute || self.self_deaf
            } else {
                local_mute
            },
            deafened: me && self.self_deaf,
            soundboard_muted: self.context.soundboard_muted.contains(&user),
            video_disabled: video_disabled(user),
            volume: fraction,
            focus: (has_video && wide).then(|| self.vv.focus.tile == Some((user, TileKind::User))),
            video_background: (me
                && !dc_voice::camera_devices().is_empty()
                && (!camera_on || self.video_background_shown()))
            .then_some(camera_on),
            show_non_video: super::voice_view::call_setting(super::voice_view::SHOW_NON_VIDEO),
            show_own_camera: super::voice_view::call_setting(super::voice_view::SHOW_OWN_CAMERA),
            has_video,
        }
    }

    fn stream_tile_facts(&self, owner: Id) -> StreamFacts {
        let me = self.stores.current_user.as_ref().map(|me| me.id);
        let own = Some(owner) == me;
        let active = if own {
            self.own_stream().is_some()
        } else {
            self.video_stream_of(owner)
                .is_some_and(|info| info.role == dc_voice::streams::Role::Watch)
        };
        let account = self.stream_account();
        let prefs = super::go_live::Prefs::load();
        let (resolution, fps) = self
            .own_started()
            .map(|started| started.quality)
            .unwrap_or_else(|| prefs.shown(account));
        let mut allowed = Vec::new();
        for candidate in [
            dc_media::screen::Resolution::P480,
            dc_media::screen::Resolution::P720,
            dc_media::screen::Resolution::P1080,
            dc_media::screen::Resolution::P1440,
            dc_media::screen::Resolution::Source,
        ] {
            for rate in [15, 30, 60] {
                if super::go_live::can_stream(
                    account,
                    super::go_live::Preset::Custom,
                    candidate,
                    rate,
                ) {
                    allowed.push((resolution_height(candidate), rate));
                }
            }
        }
        let (guild, channel) = self.vv.shown_call;
        let stage = guild != 0
            && self.stores.guild(guild).is_some_and(|known| {
                known.channels.iter().any(|found| {
                    found.id == channel && found.kind == dc_state::ChannelKind::Other(13)
                })
            });
        let volume = (super::audio_menu::volume_to_slider(super::stream_volume::percent(owner))
            / super::stream_volume::MAX_PERCENT)
            .clamp(0., 1.);
        StreamFacts {
            owner,
            own,
            active,
            stage,
            muted: super::stream_volume::muted(owner),
            volume,
            attenuation: Some((
                self.stream_attenuation_on(),
                self.stream_attenuation_fraction(),
            )),
            resolution: resolution_height(resolution),
            fps,
            allowed,
            share_audio: own.then_some(prefs.sound),
            own_hidden: own_stream_hidden(channel),
            pause_preview: super::voice_video::pause_own_preview_unfocused(),
        }
    }

    /// The items of the open tile menu, built from the state now.
    fn tile_menu_items(&self, open: &Open) -> Vec<Item> {
        let mut items = self.tile_menu_items_bare(open);
        self.fill_person_submenus(&mut items);
        items
    }

    /// A person-menu row with a submenu gets the person menu's own submenu rows (`context_sub_rows`), each run by it.
    fn fill_person_submenus(&self, items: &mut [Item]) {
        for item in items {
            let Item::Line(line) = item else {
                continue;
            };
            if !line.children.is_empty() {
                self.fill_person_submenus(&mut line.children);
                continue;
            }
            let (true, Run::Context(act, target)) = (line.person_sub, line.run.clone()) else {
                continue;
            };
            line.children = self
                .context_sub_rows(act, target)
                .into_iter()
                .enumerate()
                .map(|(index, sub)| {
                    let mut child = Line::new(
                        format!("person-sub-{index}"),
                        sub.label,
                        Run::ContextSub(act, target, index),
                    );
                    if sub.radio {
                        child.radio = Some(sub.on);
                    } else if sub.dot.is_some() || sub.on {
                        child.check = Some(sub.on);
                    }
                    child.sub = sub.sub;
                    child.disabled = sub.disabled || sub.plain;
                    Item::Line(child)
                })
                .collect();
        }
    }

    fn tile_menu_items_bare(&self, open: &Open) -> Vec<Item> {
        match open.subject {
            Subject::Stream(owner) => stream_tile_items(&self.stream_tile_facts(owner)),
            Subject::User(user) => {
                let facts = self.user_tile_facts(user);
                if open.minimal {
                    user_tile_minimal_items(
                        matches!(open.kind, CallKind::Guild { .. }),
                        &facts,
                        &open.person,
                        open.person_target,
                    )
                } else {
                    user_tile_items(
                        open.kind,
                        &facts,
                        &open.person,
                        open.person_target,
                        &open.channel,
                        open.channel_target,
                    )
                }
            }
        }
    }

    fn run_tile_item(&mut self, run: Run, window: &mut Window, cx: &mut Context<Self>) {
        let (_, channel) = self.vv.shown_call;
        match run {
            Run::Context(act, target) => self.run_context(act, target, window, cx),
            Run::ContextSub(act, target, index) => self.run_context_sub(act, target, index, cx),
            Run::Watch(owner) => self.watch_stream_of(owner, cx),
            Run::StopWatching(owner) => self.unwatch_stream_of(owner, cx),
            Run::StopStreaming => self.stream_stop(cx),
            Run::ChangeStream => self.open_go_live(cx),
            Run::StreamMute(owner) => {
                super::stream_volume::set_muted(owner, !super::stream_volume::muted(owner));
                self.apply_stream_volume(owner);
            }
            Run::ShareAudio => {
                let mut prefs = super::go_live::Prefs::load();
                prefs.sound = !prefs.sound;
                self.restart_stream_with(prefs, cx);
            }
            Run::Resolution(height) | Run::FrameRate(height) => {
                let facts = self
                    .stores
                    .current_user
                    .as_ref()
                    .map(|me| self.stream_tile_facts(me.id))
                    .unwrap_or_default();
                // 600125 `P`: a pair the account may use becomes the Custom stream; any other opens the upsell.
                let (resolution, fps) = match run {
                    Run::Resolution(_) => (height, facts.fps),
                    _ => (facts.resolution, height),
                };
                if !facts.allowed.contains(&(resolution, fps)) {
                    self.open_stream_upsell(false, cx);
                    return;
                }
                let Some(resolution) = dc_media::screen::Resolution::from_height(resolution) else {
                    return;
                };
                let mut prefs = super::go_live::Prefs::load();
                prefs.preset = super::go_live::Preset::Custom;
                prefs.resolution = resolution;
                prefs.fps = fps;
                self.restart_stream_with(prefs, cx);
            }
            Run::Attenuation => self.toggle_stream_attenuation(),
            Run::ShowOwnStream => set_own_stream_hidden(channel, !own_stream_hidden(channel)),
            Run::PausePreview => super::voice_video::set_pause_own_preview_unfocused(
                !super::voice_video::pause_own_preview_unfocused(),
            ),
            Run::ShowNonVideo => {
                super::voice_view::toggle_call_setting(super::voice_view::SHOW_NON_VIDEO)
            }
            Run::ShowOwnCamera => {
                super::voice_view::toggle_call_setting(super::voice_view::SHOW_OWN_CAMERA)
            }
            Run::Focus(user) => {
                if self.vv.focus.tile != Some((user, TileKind::User)) {
                    self.vv.focus.click(user, TileKind::User);
                }
            }
            Run::Unfocus => self.vv.focus.grid(),
            Run::VideoBackground => self.open_camera_preview(cx),
            Run::DisableVideo(user) => set_video_disabled(user, !video_disabled(user)),
            Run::VoiceSettings => self.open_settings_section("Voice & Video", cx),
            Run::CopyText(text) => cx.write_to_clipboard(gpui::ClipboardItem::new_string(text)),
            Run::Missing => {}
        }
        cx.notify();
    }

    fn close_tile_menu(&mut self, cx: &mut Context<Self>) {
        self.vv.tile_menu = None;
        if matches!(self.vv.menu, Some(Menu::Tile(..) | Menu::Stream(..))) {
            self.vv.menu = None;
        }
        cx.notify();
    }

    /// Harness: the open tile menu's rows, and running one by its label.
    #[cfg(feature = "capture")]
    pub(crate) fn script_tile_menu(
        &mut self,
        words: &[&str],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> String {
        let me = self.stores.current_user.as_ref().map(|me| me.id);
        match words {
            ["open", what, who, at @ ..] => {
                let (guild, channel) = self.vv.shown_call;
                let user = match *who {
                    "me" => me,
                    // The first other person in the call shown.
                    "other" => self
                        .stores
                        .voice_members(guild, channel)
                        .iter()
                        .map(|state| state.user_id)
                        .find(|id| Some(*id) != me),
                    id => id.parse().ok(),
                };
                let Some(user) = user else {
                    return "who: me or an id".to_owned();
                };
                let point = match at {
                    [x, y] => {
                        gpui::point(px(x.parse().unwrap_or(600.)), px(y.parse().unwrap_or(300.)))
                    }
                    _ => gpui::point(px(600.), px(300.)),
                };
                let (subject, minimal) = match *what {
                    "stream" => (Subject::Stream(user), true),
                    "user" => (Subject::User(user), false),
                    "options" => (Subject::User(user), true),
                    _ => return "open stream|user|options me|<id> [x y]".to_owned(),
                };
                self.open_tile_menu(subject, minimal, point, cx);
                format!("opened {subject:?} minimal {minimal}")
            }
            // Where a probed control is (`call-more`, `call-mic-chevron-tip`, ...), for `click` steps.
            ["anchor", key] => match self.tip_anchors.borrow().get(*key) {
                Some(bounds) => format!(
                    "{key}: x {:.0}..{:.0} y {:.0}..{:.0} centre {:.0} {:.0}",
                    f32::from(bounds.left()),
                    f32::from(bounds.right()),
                    f32::from(bounds.top()),
                    f32::from(bounds.bottom()),
                    f32::from(bounds.center().x),
                    f32::from(bounds.center().y)
                ),
                None => format!("{key}: not drawn"),
            },
            ["state"] => self.call_view_state(),
            // The gateway's VOICE_STATE_UPDATE of this account from another device, into the call shown: dispatched as
            // the harness's `gw` step does.
            ["takeover"] => {
                let (guild, channel) = self.vv.shown_call;
                let Some(me) = me else {
                    return "no current user".to_owned();
                };
                let data = serde_json::json!({
                    "guild_id": (guild != 0).then(|| guild.to_string()),
                    "channel_id": channel.to_string(),
                    "user_id": me.to_string(),
                    "session_id": "another-device",
                    "self_mute": false, "self_deaf": false, "self_video": false,
                    "mute": false, "deaf": false,
                });
                self.update(
                    crate::net::Update::Dispatch {
                        name: "VOICE_STATE_UPDATE".to_owned(),
                        data,
                    },
                    cx,
                );
                format!(
                    "dispatched; in voice {}; notice {:?}",
                    self.call.is_some(),
                    self.vv.notices.shown()
                )
            }
            // The notice bar's state, and its Reconnect as a click on it runs it.
            ["notice"] => format!("notice {:?}", self.vv.notices.shown()),
            ["notice", "reconnect"] => {
                let Some(super::notice_bar::Notice::VoiceDisabled(guild, channel)) =
                    self.vv.notices.shown()
                else {
                    return "no notice".to_owned();
                };
                let before = crate::inert::line_count();
                self.notice_reconnect(guild, channel, cx);
                let mut lines = vec![format!(
                    "reconnect ran; in voice {}; notice {:?}",
                    self.call.is_some(),
                    self.vv.notices.shown()
                )];
                lines.extend(crate::inert::lines_since(before));
                lines.join(" | ")
            }
            ["rows"] => {
                let Some(open) = self.vv.tile_menu.as_ref() else {
                    return "no tile menu".to_owned();
                };
                describe(&self.tile_menu_items(open))
            }
            ["run", label @ ..] => {
                let label = label.join(" ");
                let Some(open) = self.vv.tile_menu.as_ref() else {
                    return "no tile menu".to_owned();
                };
                let items = self.tile_menu_items(open);
                let found = find_line(&items, &label);
                let Some(line) = found else {
                    return format!("no row {label}");
                };
                if line.disabled {
                    return format!("row {label} is disabled");
                }
                let before = crate::inert::line_count();
                super::actions::take_effects();
                if line.children.is_empty() && !keeps_open(&line.run) {
                    self.close_tile_menu(cx);
                }
                self.run_tile_item(line.run.clone(), window, cx);
                let mut lines = vec![format!("ran {label}")];
                lines.extend(super::actions::take_effects());
                lines.extend(crate::inert::lines_since(before));
                lines.join(" | ")
            }
            _ => "tilemenu open stream|user|options me|<id> [x y] | rows | run <label>".to_owned(),
        }
    }

    /// The open tile menu, drawn at the pointer inside the call view.
    pub(super) fn tile_menu_layer(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let open = self.vv.tile_menu.as_ref()?;
        let still = match (self.vv.menu, open.subject) {
            (Some(Menu::Stream(owner, _)), Subject::Stream(subject)) => owner == subject,
            (Some(Menu::Tile(user, _)), Subject::User(subject)) => user == subject,
            _ => false,
        };
        if !still {
            return None;
        }
        let items = self.tile_menu_items(open);
        let minimal = open.minimal;
        let rows: Vec<AnyElement> = items
            .iter()
            .enumerate()
            .map(|(index, item)| self.tile_menu_item(index, item, false, cx))
            .collect();
        let menu = deferred(
            anchored()
                .position(open.at)
                // `--custom-menu-viewport-padding: 16px`; a menu that would pass the right edge opens to the left.
                .flip_horizontal_snap_with_margin(px(16.))
                .child(
                    menu_frame(minimal.then_some(FIXED_WIDTH))
                        .id("tile-menu")
                        .child(self.anchor_probe("tile-menu"))
                        .max_h(px(self.viewport.1 - 32.))
                        .on_mouse_down_out(cx.listener(
                            |this, event: &gpui::MouseDownEvent, _, cx| {
                                let in_sub = this
                                    .tip_anchors
                                    .borrow()
                                    .get("tile-submenu")
                                    .is_some_and(|bounds| bounds.contains(&event.position))
                                    && this
                                        .vv
                                        .tile_menu
                                        .as_ref()
                                        .is_some_and(|open| open.sub.is_some());
                                if !in_sub {
                                    this.close_call_popup(event.position, cx);
                                }
                            },
                        ))
                        .child(
                            div()
                                .id("tile-menu-scroll")
                                .track_scroll(&self.menu_scroll)
                                .min_h(px(0.))
                                .overflow_y_scroll()
                                .p(px(8.))
                                .flex()
                                .flex_col()
                                .children(rows),
                        )
                        // A menu taller than the window scrolls with the menus' thin scrollbar.
                        .child(super::menu::menu_scrollbar(self.menu_scroll.clone())),
                ),
        )
        .with_priority(3)
        .into_any_element();
        let sub = open.sub.and_then(|(index, row)| {
            let Some(Item::Line(line)) = items.get(index) else {
                return None;
            };
            if line.children.is_empty() {
                return None;
            }
            let width = estimated_width(&line.children);
            let rows: Vec<AnyElement> = line
                .children
                .iter()
                .enumerate()
                .map(|(sub_index, item)| self.tile_menu_item(1000 + sub_index, item, true, cx))
                .collect();
            let right = f32::from(row.right()) + 4. + width <= self.viewport.0 - 16.;
            Some(
                deferred(
                    anchored()
                        .anchor(if right {
                            gpui::Anchor::TopLeft
                        } else {
                            gpui::Anchor::TopRight
                        })
                        .position(gpui::point(
                            if right {
                                row.right() + px(4.)
                            } else {
                                row.left() - px(4.)
                            },
                            row.top() - px(9.),
                        ))
                        .snap_to_window_with_margin(px(16.))
                        .child(
                            menu_frame(None)
                                .id("tile-submenu")
                                .child(self.anchor_probe("tile-submenu"))
                                .child(div().p(px(8.)).flex().flex_col().children(rows)),
                        ),
                )
                .with_priority(4)
                .into_any_element(),
            )
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

    fn tile_menu_item(
        &self,
        index: usize,
        item: &Item,
        in_sub: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        match item {
            Item::Separator => div()
                .mx(px(8.))
                .my(px(8.))
                .h(px(1.))
                .flex_none()
                .bg(color("--border-subtle"))
                .into_any_element(),
            // `groupLabel_M3qk4j`: 14px medium, 20px line, `--text-muted`, in a label container's padding.
            Item::Label(text) => div()
                .flex_none()
                .px(px(8.))
                .py(px(4.))
                .text_size(px(14.))
                .line_height(px(20.))
                .font_weight(FontWeight(500.))
                .text_color(color("--text-muted"))
                .child(text.clone())
                .into_any_element(),
            Item::Slider {
                key,
                label,
                fraction,
                slide,
            } => self.tile_slider(key, label.clone(), *fraction, *slide, cx),
            Item::Line(line) => self.tile_line(index, line, in_sub, cx),
        }
    }

    fn tile_line(
        &self,
        index: usize,
        line: &Line,
        in_sub: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let danger = line.danger;
        let disabled = line.disabled;
        let has_sub = !line.children.is_empty();
        let key = format!("tile-row-{index}-{}", line.key);
        let probe = key.clone();
        let hover_key = key.clone();
        let click_key = key.clone();
        let run = line.run.clone();
        let label_tint = if danger {
            color("--text-feedback-critical")
        } else {
            color("--text-strong")
        };
        let focus_bg = if danger {
            color("--background-feedback-mod-critical")
        } else {
            color("--interactive-background-hover")
        };
        self.menu_stop(
            div().id(SharedString::from(key.clone())),
            &key,
            focus_bg,
            cx,
        )
        .relative()
        .flex_none()
        .child(self.anchor_probe(probe))
        .min_h(px(line.height()))
        .p(px(8.))
        .rounded(px(2.))
        .flex()
        .items_center()
        // `disabled_M3qk4j`: half opacity, no pointer events.
        .when(disabled, |this| this.opacity(0.5))
        .when(!disabled, |this| {
            this.cursor_pointer()
                .hover(move |style| style.bg(focus_bg).rounded(px(4.)))
        })
        .when(!in_sub, |this| {
            this.on_hover(cx.listener(move |this, inside: &bool, _, cx| {
                if !*inside {
                    return;
                }
                let bounds = this.tip_anchors.borrow().get(hover_key.as_str()).copied();
                if let Some(open) = this.vv.tile_menu.as_mut() {
                    open.sub = if has_sub {
                        bounds.map(|bounds| (index, bounds))
                    } else {
                        None
                    };
                }
                cx.notify();
            }))
        })
        .when(!disabled, |this| {
            this.on_click(cx.listener(move |this, _, window, cx| {
                cx.stop_propagation();
                if has_sub {
                    // A click opens the submenu, as the hover does.
                    let bounds = this.tip_anchors.borrow().get(click_key.as_str()).copied();
                    if let (Some(open), Some(bounds)) = (this.vv.tile_menu.as_mut(), bounds) {
                        open.sub = Some((index, bounds));
                    }
                    cx.notify();
                    return;
                }
                // A checkbox or a radio keeps the menu open (the `sL`/`iD` items of module 477782 do not
                // close it); every other row closes it first.
                if !keeps_open(&run) {
                    this.close_tile_menu(cx);
                }
                this.run_tile_item(run.clone(), window, cx);
            }))
        })
        .children(line.icon.map(|name| {
            div().mr(px(8.)).flex_none().child(icon(
                name,
                20.,
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
                        .text_size(px(14.))
                        .line_height(px(18.))
                        .font_weight(FontWeight(500.))
                        .text_color(label_tint)
                        .truncate()
                        .child(line.label.clone()),
                )
                .children(line.sub.clone().map(|text| {
                    div()
                        .mt(px(2.))
                        .text_size(px(12.))
                        .line_height(px(16.))
                        .text_color(color("--text-muted"))
                        .child(text)
                })),
        )
        .children(line.check.map(|on| {
            div()
                .ml(px(8.))
                .flex_none()
                .size(px(20.))
                .rounded(px(4.))
                .border_1()
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
        }))
        .children(line.radio.map(|on| {
            div()
                .ml(px(8.))
                .flex_none()
                .size(px(20.))
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
                .when(on, |this| {
                    this.child(div().size(px(8.)).rounded_full().bg(gpui::white()))
                })
        }))
        .when(has_sub, |this| {
            this.child(div().ml(px(10.)).flex_none().child(icon(
                "menu_caret",
                18.,
                "--interactive-text-default",
            )))
        })
        .into_any_element()
    }

    /// A slider row (`aK` with the menu slider of module 964355): the label, then a 24px control with a 4px bar and a
    /// 16px grabber (`--grabber-size: 16px; --bar-size: 4px`, design/dom/popouts/voice-sb-volume).
    fn tile_slider(
        &self,
        key: &'static str,
        label: String,
        fraction: f32,
        slide: Slide,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        use gpui::{MouseButton, MouseDownEvent, MouseMoveEvent, relative};
        let probe = format!("tile-slider-{key}");
        let (down_probe, move_probe) = (probe.clone(), probe.clone());
        let at = |this: &Root, probe: &str, x: Pixels| -> Option<f32> {
            let bounds = this.tip_anchors.borrow().get(probe).copied()?;
            let width = f32::from(bounds.size.width);
            (width > 0.).then(|| ((f32::from(x) - f32::from(bounds.left())) / width).clamp(0., 1.))
        };
        div()
            .flex_none()
            .flex()
            .flex_col()
            .child(
                div()
                    .p(px(8.))
                    .text_size(px(14.))
                    .line_height(px(18.))
                    .font_weight(FontWeight(500.))
                    .text_color(color("--text-strong"))
                    .child(label),
            )
            .child(
                div()
                    .id(SharedString::from(probe.clone()))
                    .relative()
                    .mx(px(8.))
                    .mb(px(4.))
                    .h(px(24.))
                    .cursor_pointer()
                    .child(self.anchor_probe(probe.clone()))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                            cx.stop_propagation();
                            if let Some(fraction) = at(this, &down_probe, event.position.x) {
                                this.slide_tile(slide, fraction);
                                cx.notify();
                            }
                        }),
                    )
                    .on_mouse_move(cx.listener(move |this, event: &MouseMoveEvent, _, cx| {
                        if event.dragging()
                            && let Some(fraction) = at(this, &move_probe, event.position.x)
                        {
                            this.slide_tile(slide, fraction);
                            cx.notify();
                        }
                    }))
                    .child(
                        div()
                            .absolute()
                            .left(px(0.))
                            .top(px(10.))
                            .w_full()
                            .h(px(4.))
                            .rounded(px(4.))
                            .bg(color("--background-mod-strong"))
                            .child(
                                div()
                                    .h_full()
                                    .w(relative(fraction))
                                    .rounded(px(4.))
                                    .bg(color("--background-brand")),
                            ),
                    )
                    .child(
                        div()
                            .absolute()
                            .left(relative(fraction))
                            .ml(px(-8.))
                            .top(px(4.))
                            .size(px(16.))
                            .rounded_full()
                            .bg(gpui::rgb(0xffffff)),
                    ),
            )
            .into_any_element()
    }

    fn slide_tile(&mut self, slide: Slide, fraction: f32) {
        use super::audio_menu::slider_to_volume;
        use super::stream_volume::MAX_PERCENT;
        let percent = slider_to_volume((fraction * MAX_PERCENT).round());
        match slide {
            Slide::UserVolume(user) => {
                let muted = self.context.mix.get(&user).is_some_and(|mix| mix.1);
                self.set_user_mix(user, percent / 100., muted);
            }
            Slide::StreamVolume(owner) => {
                super::stream_volume::set_percent(owner, percent);
                self.apply_stream_volume(owner);
            }
            Slide::AttenuationStrength => {
                self.commit_stream_attenuation(fraction);
                self.apply_stream_attenuation();
            }
        }
    }
}

/// `menu_M3qk4j`: the surface, its border and radius and `--shadow-high`.
fn menu_frame(width: Option<f32>) -> gpui::Div {
    let frame = match width {
        Some(width) => div().w(px(width)),
        None => div().min_w(px(FLEXIBLE_MIN)).max_w(px(FLEXIBLE_MAX)),
    };
    frame
        .relative()
        .occlude()
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
        .flex()
        .flex_col()
}

fn rows_of(items: Option<Vec<Entry>>) -> Vec<Row> {
    items
        .unwrap_or_default()
        .into_iter()
        .filter_map(|entry| match entry {
            Entry::Row(row) => Some(row),
            Entry::Separator => None,
        })
        .collect()
}

fn resolution_height(resolution: dc_media::screen::Resolution) -> u32 {
    match resolution {
        dc_media::screen::Resolution::Source => 0,
        other => other.height(),
    }
}

/// `isLocalVideoDisabled` of each person, kept as the media engine keeps it (`localVideoDisabled`).
const VIDEO_DISABLED: &str = "audio.local_video_disabled";
/// `isSelfStreamHidden` of each channel.
const OWN_STREAM_HIDDEN: &str = "audio.self_stream_hidden";

pub(super) fn video_disabled(user: Id) -> bool {
    crate::storage::get(VIDEO_DISABLED)
        .is_some_and(|stored| stored[user.to_string()].as_bool() == Some(true))
}

fn set_video_disabled(user: Id, on: bool) {
    let mut stored = crate::storage::get(VIDEO_DISABLED).unwrap_or_else(|| serde_json::json!({}));
    if let Some(map) = stored.as_object_mut() {
        if on {
            map.insert(user.to_string(), serde_json::json!(true));
        } else {
            map.remove(&user.to_string());
        }
    }
    crate::storage::set(VIDEO_DISABLED, stored);
}

pub(super) fn own_stream_hidden(channel: Id) -> bool {
    crate::storage::get(OWN_STREAM_HIDDEN)
        .is_some_and(|stored| stored[channel.to_string()].as_bool() == Some(true))
}

fn set_own_stream_hidden(channel: Id, on: bool) {
    let mut stored =
        crate::storage::get(OWN_STREAM_HIDDEN).unwrap_or_else(|| serde_json::json!({}));
    if let Some(map) = stored.as_object_mut() {
        if on {
            map.insert(channel.to_string(), serde_json::json!(true));
        } else {
            map.remove(&channel.to_string());
        }
    }
    crate::storage::set(OWN_STREAM_HIDDEN, stored);
}

#[cfg(any(test, feature = "capture"))]
/// The rows as text, for the harness and the tests: a check as `[x]`/`[ ]`, a radio as `(o)`/`( )`, a submenu with
/// `>` and its rows in braces, a disabled row with `~`.
pub(super) fn describe(items: &[Item]) -> String {
    items
        .iter()
        .map(|item| match item {
            Item::Separator => "|".to_owned(),
            Item::Label(text) => format!("#{text}"),
            Item::Slider {
                label, fraction, ..
            } => format!("{label} <{:.2}>", fraction),
            Item::Line(line) => {
                let mut text = line.label.clone();
                if line.disabled {
                    text = format!("~{text}");
                }
                if let Some(on) = line.check {
                    text.push_str(if on { " [x]" } else { " [ ]" });
                }
                if let Some(on) = line.radio {
                    text.push_str(if on { " (o)" } else { " ( )" });
                }
                if !line.children.is_empty() {
                    text.push_str(&format!(" > {{{}}}", describe(&line.children)));
                }
                text
            }
        })
        .collect::<Vec<_>>()
        .join("; ")
        .replace("; |; ", "; | ")
}

#[cfg(feature = "capture")]
fn find_line(items: &[Item], label: &str) -> Option<Line> {
    items.iter().find_map(|item| match item {
        Item::Line(line) if line.label == label => Some(line.clone()),
        Item::Line(line) => find_line(&line.children, label),
        _ => None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(act: Act, label: &str) -> Row {
        Row {
            act,
            label: label.to_owned(),
            sub: None,
            icon: None,
            caret: false,
            check: None,
            danger: false,
            disabled: false,
            shortcut: None,
        }
    }

    fn person() -> Vec<Row> {
        vec![
            row(Act::Profile, "Profile"),
            row(Act::Mention, "Mention"),
            row(Act::Message, "Message"),
            row(Act::Call, "Start a Call"),
            row(Act::Note, "Add Note"),
            row(Act::Apps, "Apps"),
            row(Act::AddFriend, "Add Friend"),
            row(Act::Ignore, "Ignore"),
            row(Act::Block, "Block"),
            row(Act::ModView, "Open in Mod View"),
            row(Act::CopyId, "Copy User ID"),
        ]
    }

    fn other() -> UserFacts {
        UserFacts {
            user: 2,
            volume: 0.5,
            show_non_video: true,
            show_own_camera: true,
            ..UserFacts::default()
        }
    }

    const TARGET: Option<Target> = Some(Target::User {
        user: 2,
        guild: Some(1),
    });

    #[test]
    fn a_server_tile_follows_the_guild_channel_user_menu_without_chat_items() {
        let items = user_tile_items(
            CallKind::Guild { stage: false },
            &other(),
            &person(),
            TARGET,
            &[],
            None,
        );
        assert_eq!(
            describe(&items),
            "Profile; Message; Start a Call; Add Note; | User Volume <0.50>; | Mute [ ]; \
             Mute Soundboard [ ]; Disable Video [ ]; Apps; Add Friend; Ignore; Block; | ~Pop Out User; \
             Show Non-Video Participants [x]; | Open in Mod View; | Copy User ID"
        );
    }

    #[test]
    fn our_own_tile_has_mute_deafen_and_own_camera_but_no_volume() {
        let facts = UserFacts {
            me: true,
            muted: true,
            video_background: Some(false),
            focus: Some(true),
            ..other()
        };
        let items = user_tile_items(
            CallKind::Guild { stage: false },
            &facts,
            &[
                row(Act::Profile, "Profile"),
                row(Act::EditProfile, "Edit Per-server Profile"),
            ],
            TARGET,
            &[],
            None,
        );
        assert_eq!(
            describe(&items),
            "Profile; | Mute [x]; Deafen [ ]; Unfocus This Person; Preview Camera; Edit Per-server Profile; \
             | ~Pop Out User; Show Non-Video Participants [x]; Show My Own Camera [x]"
        );
    }

    #[test]
    fn a_dm_tile_starts_with_the_channel_rows_and_ends_with_both_ids() {
        let channel = vec![
            row(Act::MarkRead, "Mark As Read"),
            row(Act::PinDm, "Pin DM"),
            row(Act::Favorite, "Add to Favorites"),
            row(Act::CloseDm, "Close DM"),
            row(Act::Mute, "Mute @them"),
        ];
        let facts = UserFacts {
            developer: true,
            ..other()
        };
        let items = user_tile_items(
            CallKind::Dm,
            &facts,
            &person(),
            Some(Target::User {
                user: 2,
                guild: None,
            }),
            &channel,
            Some(Target::Dm(9)),
        );
        assert_eq!(
            describe(&items),
            "Mark As Read; | Pin DM; Add to Favorites; | Profile; Start a Call; Add Note; Close DM; \
             | User Volume <0.50>; | Apps; Mute [ ]; Mute Soundboard [ ]; Disable Video [ ]; Add Friend; \
             Ignore; Block; | Mute @them; | ~Pop Out User; Show Non-Video Participants [x]; \
             | Copy User ID; Copy Channel ID"
        );
    }

    #[test]
    fn a_group_tile_has_mention_only_beside_its_chat() {
        let with_chat = user_tile_items(
            CallKind::Group { chat: true },
            &other(),
            &person(),
            TARGET,
            &[],
            None,
        );
        assert!(describe(&with_chat).starts_with("Profile; Mention; Message"));
        let without = user_tile_items(
            CallKind::Group { chat: false },
            &other(),
            &person(),
            TARGET,
            &[],
            None,
        );
        assert!(describe(&without).starts_with("Profile; Message"));
    }

    #[test]
    fn the_options_button_opens_the_fixed_voice_user_menu_with_more_options() {
        let items = user_tile_minimal_items(true, &other(), &person(), TARGET);
        assert_eq!(
            describe(&items),
            "View Profile; Message; | User Volume <0.50>; Mute [ ]; Mute Soundboard [ ]; Disable Video [ ]; \
             | More Options > {Ignore; Block; | Copy User ID}"
        );
        let own = UserFacts {
            me: true,
            ..other()
        };
        let items = user_tile_minimal_items(true, &own, &person(), TARGET);
        assert_eq!(
            describe(&items),
            "View Profile; | Mute [ ]; Deafen [ ]; Show My Own Camera [x]; Voice & Video Settings; \
             | Copy User ID"
        );
    }

    #[test]
    fn a_popup_button_toggles_and_never_reopens_what_its_press_closed() {
        // Closed: the click opens it.
        assert!(opens_on_click(false, false));
        // Open at the press, already closed by its press-outside: the click leaves it closed.
        assert!(!opens_on_click(true, false));
        // Open at the press and still open: the click closes it.
        assert!(!opens_on_click(true, true));
        assert!(!opens_on_click(false, true));
    }

    #[test]
    fn a_stream_not_watched_offers_watch_and_pop_out_only() {
        let facts = StreamFacts {
            owner: 2,
            ..StreamFacts::default()
        };
        assert_eq!(
            describe(&stream_tile_items(&facts)),
            "Watch Stream; | ~Pop Out Stream"
        );
    }

    #[test]
    fn a_watched_stream_has_its_sound_rows_attenuation_and_more_options() {
        let facts = StreamFacts {
            owner: 2,
            active: true,
            muted: true,
            volume: 0.5,
            attenuation: Some((true, 0.3)),
            ..StreamFacts::default()
        };
        assert_eq!(
            describe(&stream_tile_items(&facts)),
            "Stop Watching; | Mute [x]; Stream Volume <0.50>; | ~Pop Out Stream; | Stream Attenuation [x]; \
             Stream Attenuation Strength <0.30>; | More Options > {~Report Problem}"
        );
        let stage = StreamFacts {
            stage: true,
            attenuation: None,
            ..facts
        };
        assert!(describe(&stream_tile_items(&stage)).starts_with("Mute [x]"));
    }

    #[test]
    fn our_own_stream_has_the_go_live_rows_and_the_quality_submenu() {
        let facts = StreamFacts {
            owner: 1,
            own: true,
            active: true,
            resolution: 720,
            fps: 30,
            allowed: vec![(480, 15), (480, 30), (720, 15), (720, 30)],
            share_audio: Some(true),
            pause_preview: true,
            ..StreamFacts::default()
        };
        assert_eq!(
            describe(&stream_tile_items(&facts)),
            "Stop Streaming; Change Stream; Stream Quality > {#Frame Rate; 15 FPS ( ); 30 FPS (o); \
             60 FPS ( ); #Resolution; 480p ( ); 720p (o); 1080p ( ); 1440p ( ); Source ( )}; \
             Share Stream Audio [x]; | ~Pop Out Stream; | More Options > {Show My Screen Share [x]; \
             Pause Preview When Discord Isn\u{2019}t Focused [x]; ~Report Problem}"
        );
    }
}
