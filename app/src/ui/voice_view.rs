//! The voice channel's call view (`callContainer_hbKzX7`, `rTsZa_`, `9bvEB0`, `K9Xq13` in the captured build): the
//! tile grid on black, the header and control bar that fade after two seconds without the pointer (module 540462,
//! `timeout: 2e3`), the More Options and tile Options menus, the disconnected state and the docked chat.
//! Geometry is measured in `design/dom/channel/voice-view-*-darker-cozy.json`; the grid sizing is module 511205.

use super::lottie_icon::{self, Anim};
use super::motion::{PopKind, PopSide, Popout as _};
use super::tooltip::{Side, Tip};
use super::video_tiles::{self, Candidate, TileKind};
use super::{Root, color, icon};
use crate::parity::Parity;
use crate::storage;
use dc_state::{Id, User};
use dc_tokens::Theme;
use gpui::{
    AnyElement, Context, FontWeight, Hsla, IntoElement, MouseMoveEvent, Pixels, Point, Rgba,
    SharedString, Window, deferred, div, img, linear_color_stop, linear_gradient, point,
    prelude::*, px, rgba,
};
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

/// `el.Ay` `timeout: 2e3`: the bar and header leave after this long without the pointer.
const IDLE: Duration = Duration::from_millis(2000);
/// The "Invite to Voice" row leaves a call by itself after this long (module 220995).
pub(super) const INVITE_ROW_TIMEOUT: Duration = Duration::from_secs(10);

pub(super) const GRID_VIEW: &str = "audio.call_grid_view";
pub(super) const SHOW_OWN_CAMERA: &str = "audio.call_show_own_camera";
pub(super) const SHOW_NON_VIDEO: &str = "audio.call_show_non_video";
pub(super) const MESSAGE_PREVIEWS: &str = "audio.call_message_previews";

const ART_URL: &str = "https://discord.com/assets/664390de11a80444.svg";
/// `CONNECT`: permission bit 20.
const CONNECT: u64 = 1 << 20;
const CHAT_WIDTH: f32 = 450.;
const MORE_WIDTH: f32 = 239.6;
const TILE_MENU_WIDTH: f32 = 220.;

/// The four checkboxes of the More Options menu; all on until the person changes them.
pub(super) fn call_setting(key: &str) -> bool {
    storage::get(key)
        .and_then(|value| value.as_bool())
        .unwrap_or(true)
}

pub(super) fn set_call_setting(key: &str, on: bool) {
    storage::set(key, serde_json::json!(on));
}

fn toggle_call_setting(key: &str) {
    storage::set(key, serde_json::json!(!call_setting(key)));
}

/// The animated icon `id` of a call tray button in the call on `channel`: a call in another channel mounts its own.
pub(super) fn tray_icon_id(name: &str, channel: Id) -> String {
    format!("tray-{name}-{channel}")
}

/// A button's `onMouseEnter` / `onMouseLeave` for its animated icon `id` (the hooks' `events`).
pub(super) fn lottie_hover(
    id: String,
    cx: &mut Context<Root>,
) -> impl Fn(&bool, &mut Window, &mut gpui::App) + 'static {
    cx.listener(move |_, entered: &bool, _, cx| {
        lottie_icon::hover(&id, *entered);
        cx.notify();
    })
}

/// `midnight`: the call container carries `theme-midnight`, whatever the app's own theme is.
fn mid(name: &str) -> Rgba {
    rgba(Theme::Midnight.color(name))
}

/// The wandering-cubes spinner of a tile that waits for its picture (module 259264): two diamonds in a 32px box.
fn spinner() -> AnyElement {
    div()
        .absolute()
        .inset_0()
        .flex()
        .items_center()
        .justify_center()
        .child(
            div()
                .relative()
                .size(px(32.))
                .child(div().absolute().left(px(23.42)).top(px(5.07)).child(icon(
                    "spinner_cube",
                    7.16,
                    "--brand-400",
                )))
                .child(div().absolute().left(px(1.42)).top(px(19.78)).child(icon(
                    "spinner_cube",
                    7.16,
                    "--brand-400",
                ))),
        )
        .into_any_element()
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Menu {
    More,
    /// The chevron of the tray's red button: the watched streams of others, each with Stop Watching.
    Watched,
    /// A tile's Options: whose, and where the pointer was (the menu's top-left corner).
    Tile(Id, Point<Pixels>),
    /// A stream tile's Options, by its owner.
    Stream(Id, Point<Pixels>),
    /// The chevron of the camera button: devices and Video Settings.
    Camera,
    /// The chevron of the Share button while we stream.
    Share,
}

pub(super) struct State {
    /// Channels whose chat is docked at the right.
    pub chat_open: HashSet<Id>,
    pub last_move: Instant,
    /// A task is waiting for the idle time to run out.
    watching: bool,
    /// A task redraws every second for the call timer.
    ticking: bool,
    pub menu: Option<Menu>,
    /// "Invite to Voice" in the sidebar was closed with its X.
    pub invite_dismissed: bool,
    /// The channel row was the last thing pressed: it keeps its action icons instead of the timer.
    pub row_focus: bool,
    /// The Stream Quality submenu of the Share menu is open.
    pub quality_open: bool,
    /// The pointer is on the soundboard button or its Suggested Sounds popup.
    pub suggest: bool,
    /// The focused tile, if any (`selectedParticipant`).
    pub focus: video_tiles::Focus,
    /// Full Screen: the call fills the window and the window fills the screen.
    pub fullscreen: bool,
    /// "Hide Members" of the focus mode (`participantsOpen` is false).
    pub members_hidden: bool,
    /// The window the call was popped out to (`call_popout.rs`).
    pub popout: Option<super::call_popout::Handle>,
    /// The call view is being built for that window now.
    pub popout_render: bool,
    /// The popped-out window stays above the others (`popoutWindowAlwaysOnTop`).
    pub popout_on_top: bool,
    /// The call the view was last built for (guild, channel), for what a tile looks up by its stream's key.
    pub shown_call: (Id, Id),
    /// The red button of the tray shows Stop Watching, not Disconnect (`D` of `eT` in module 105225: it changes
    /// once the button has shrunk).
    pub end_shows_stream: std::cell::Cell<bool>,
    /// The call view is being built for a call this account has not joined (`inCall` is false): every participant
    /// has a tile, none can be focused, and the bar and its menus are left to the join buttons.
    pub outside: bool,
    /// Dominant colour of an avatar by its image key, decoded once.
    colors: RefCell<HashMap<String, Option<[u8; 3]>>>,
    pub(super) background: super::call_background::State,
}

impl Default for State {
    fn default() -> Self {
        Self {
            chat_open: HashSet::new(),
            last_move: Instant::now(),
            watching: false,
            ticking: false,
            menu: None,
            invite_dismissed: false,
            row_focus: false,
            quality_open: false,
            suggest: false,
            focus: Default::default(),
            fullscreen: false,
            members_hidden: false,
            popout: None,
            popout_render: false,
            popout_on_top: false,
            outside: false,
            end_shows_stream: std::cell::Cell::new(false),
            shown_call: (0, 0),
            colors: RefCell::new(HashMap::new()),
            background: Default::default(),
        }
    }
}

/// What `Grid` lays out: columns, rows, the tile's width (its height is that over 16:9) and the list's paddings.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct GridLayout {
    pub columns: usize,
    pub rows: usize,
    pub tile_width: f32,
    pub tile_height: f32,
    pub pad_top: f32,
    pub pad_bottom: f32,
}

/// What the header shows beside the channel name while a tile is focused.
struct FocusCrumb {
    title: String,
    avatar: Option<std::sync::Arc<gpui::Image>>,
    /// The focused tile is a stream: LIVE shows, after the quality when the stream told it.
    stream: bool,
    quality: Option<String>,
    /// The viewers of a focused stream, for the pile of the header (`546871`): their avatars, in order.
    viewers: Vec<Option<std::sync::Arc<gpui::Image>>>,
}

/// The live indicator of a focused stream (module 872363 with `showQuality`): the quality, when the stream told it,
/// joined to LIVE.
pub(super) fn live_indicator(quality: Option<String>) -> AnyElement {
    let pill = |text: String, fill: &'static str| {
        div()
            .h(px(16.))
            .px(px(6.))
            .flex()
            .items_center()
            .bg(mid(fill))
            .text_size(px(12.))
            .line_height(px(16.))
            .font_weight(FontWeight(600.))
            .text_color(rgba(0xffffffff))
            .child(text)
    };
    let live = pill("LIVE".to_owned(), "--badge-critical-background").rounded_r(px(16.));
    div()
        .flex()
        .flex_none()
        .items_center()
        .child(match quality {
            Some(text) => div()
                .flex()
                .child(pill(text, "--interactive-muted").rounded_l(px(16.)))
                .child(live),
            None => div().flex().child(live.rounded_l(px(16.))),
        })
        .into_any_element()
}

/// `maxVisibleUsers` of the header's viewer pile.
const PILE_MAX: usize = 5;

/// `Ay` of module 525788: up to `PILE_MAX` 24px avatars a couple of pixels over one another; past that the last place
/// reads "+N" in a 25px chip (`overflow_Xa_Vtp`). Nothing shows without viewers. The bite the official cuts out of
/// each avatar for the next is not drawn.
fn viewer_pile(viewers: &[Option<std::sync::Arc<gpui::Image>>]) -> AnyElement {
    let shown = viewers.len().min(PILE_MAX);
    let overflow = viewers.len() > PILE_MAX;
    div()
        .mr(px(16.))
        .flex()
        .items_center()
        .children(
            viewers
                .iter()
                .take(shown)
                .enumerate()
                .map(|(index, avatar)| {
                    if overflow && index + 1 == shown {
                        div()
                            .ml(px(-4.))
                            .size(px(25.))
                            .rounded(px(15.))
                            .bg(mid("--primary-600"))
                            .flex()
                            .items_center()
                            .justify_center()
                            .text_size(px(12.))
                            .line_height(px(16.))
                            .font_weight(FontWeight::BOLD)
                            .text_color(mid("--primary-300"))
                            .child(format!("+{}", viewers.len() - PILE_MAX + 1))
                            .into_any_element()
                    } else {
                        div()
                            .when(index > 0, |this| this.ml(px(-2.)))
                            .size(px(24.))
                            .rounded_full()
                            .overflow_hidden()
                            .bg(mid("--background-mod-strong"))
                            .children(avatar.clone().map(|picture| img(picture).size(px(24.))))
                            .into_any_element()
                    }
                }),
        )
        .into_any_element()
}

const ASPECT: f32 = 16. / 9.;
const TILE_GAP: f32 = 8.;
const GRID_PAD: f32 = 64.;

/// Module 511205 `d`: tiles of width `tile` that fit across `width`.
fn columns_for(tile: f32, width: f32) -> f32 {
    f32::max(1., ((width - tile) / (TILE_GAP + tile)).floor() + 1.)
}

/// Module 511205 `u`: the first value in `lo..=hi` where `pred` turns false.
fn lower_bound(lo: f32, hi: f32, pred: impl Fn(f32) -> bool) -> f32 {
    let (mut lo, mut span) = (lo, hi - lo);
    while span > 0. {
        let half = (span / 2.).floor();
        let probe = lo + half;
        if pred(probe) {
            lo = probe + 1.;
            span -= half + 1.;
        } else {
            span = half;
        }
    }
    lo
}

/// Where a row of `row_width` starts in the grid of a view `width` wide: centred in the space between the 8px paddings,
/// a half pixel rounded up as Chromium paints it.
fn row_indent(width: f32, row_width: f32) -> f32 {
    ((width - 16. - row_width) / 2.).ceil().max(0.)
}

/// The official grid for `count` tiles in a view `width` by `height` (module 511205 with 526369's 64px paddings).
pub(super) fn grid_layout(count: usize, width: f32, height: f32) -> GridLayout {
    let available_w = width - 16.;
    let available_h = height - GRID_PAD * 2.;
    let n = count as f32;
    let start = (available_w / 25.).floor();
    let tile = if count > 25 {
        lower_bound(start, available_w, |e| {
            let t = e / ASPECT;
            columns_for(e, available_w) * (((available_h - t) / (TILE_GAP + t)).ceil() + 1.) > 25.
        })
    } else {
        lower_bound(start, available_w, |e| {
            let t = e / ASPECT;
            columns_for(e, available_w) * (((available_h - t) / (TILE_GAP + t)).floor() + 1.) >= n
        }) - 1.
    };
    let columns = columns_for(tile, available_w).max(1.);
    let rows = (n / columns).ceil();
    let row_height = (tile / ASPECT).floor() + TILE_GAP;
    let centre = f32::max(0., available_h - row_height * rows) / 2.;
    GridLayout {
        columns: columns as usize,
        rows: rows as usize,
        tile_width: tile,
        tile_height: tile / ASPECT,
        pad_top: GRID_PAD + centre,
        pad_bottom: GRID_PAD + centre - TILE_GAP,
    }
}

/// What a tile shows behind its name.
#[derive(Clone, Copy)]
pub(super) enum TileMedia {
    /// The 80px avatar centred in the tile.
    Avatar,
    /// Nothing yet: the spinner or the Watch Stream button is drawn over the tile.
    None,
    /// The latest picture of a camera or stream, kept whole inside the tile (`fit: contain`).
    Video(super::video_tiles::Key),
}

struct Participant {
    user: User,
    mute: bool,
    deaf: bool,
    speaking: bool,
    tile: Candidate,
}

/// The fade of `gradientTop_rTsZa_` / `gradientBottom_rTsZa_`: black down to nothing over 160px.
const FADE: [(f32, f32); 13] = [
    (0., 1.),
    (0.19, 0.737),
    (0.34, 0.54),
    (0.47, 0.38),
    (0.565, 0.28),
    (0.65, 0.192),
    (0.73, 0.125),
    (0.802, 0.075),
    (0.861, 0.043),
    (0.91, 0.02),
    (0.952, 0.008),
    (0.982, 0.004),
    (1., 0.),
];

fn fade(top: bool) -> AnyElement {
    let black = |alpha: f32| -> Hsla { gpui::hsla(0., 0., 0., alpha) };
    let mut bands = Vec::new();
    for pair in FADE.windows(2) {
        let ((from, a), (to, b)) = (pair[0], pair[1]);
        let (a, b) = if top { (a, b) } else { (b, a) };
        bands.push(
            div()
                .w_full()
                .h(px(160. * (to - from)))
                .bg(linear_gradient(
                    180.,
                    linear_color_stop(black(a), 0.),
                    linear_color_stop(black(b), 1.),
                ))
                .into_any_element(),
        );
    }
    if !top {
        bands.reverse();
    }
    div()
        .absolute()
        .left_0()
        .right_0()
        .h(px(160.))
        .flex()
        .flex_col()
        .when(top, |this| this.top_0())
        .when(!top, |this| this.bottom_0())
        .children(bands)
        .into_any_element()
}

fn names(users: &[User]) -> String {
    match users {
        [] => "No one is currently in voice".to_owned(),
        [a] => format!("{} is currently in voice", a.display_name()),
        [a, b] => format!(
            "{} and {} are currently in voice",
            a.display_name(),
            b.display_name()
        ),
        [a, b, rest @ ..] => format!(
            "{}, {}, and {} others are currently in voice",
            a.display_name(),
            b.display_name(),
            rest.len()
        ),
    }
}

impl Root {
    /// The click on a voice channel (or on the panel's channel name) opens its view in the main area.
    pub(super) fn open_voice_view(&mut self, guild: Id, channel: Id, cx: &mut Context<Self>) {
        if self.channel != Some(channel) {
            self.select(guild, Some(channel));
            self.request_images();
        }
        self.vv.last_move = Instant::now();
        self.vv.menu = None;
        self.start_voice_tick(cx);
        cx.notify();
    }

    pub(super) fn voice_view_open(&self) -> bool {
        self.current_channel()
            .is_some_and(|channel| channel.kind == dc_state::ChannelKind::Voice)
    }

    /// One redraw a second while in a call, for the timers.
    pub(super) fn start_voice_tick(&mut self, cx: &mut Context<Self>) {
        if self.vv.ticking {
            return;
        }
        self.vv.ticking = true;
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(Duration::from_secs(1)).await;
                let Ok(keep) = this.update(cx, |this, cx| {
                    let keep = this.call.is_some();
                    if !keep {
                        this.vv.ticking = false;
                    }
                    cx.notify();
                    keep
                }) else {
                    return;
                };
                if !keep {
                    return;
                }
            }
        })
        .detach();
    }

    /// The pointer moved over the view: the bar shows, and a redraw is due when the two seconds are over.
    pub(super) fn voice_view_moved(&mut self, cx: &mut Context<Self>) {
        let was_idle = self.vv.last_move.elapsed() >= IDLE;
        self.vv.last_move = Instant::now();
        if was_idle {
            cx.notify();
        }
        if self.vv.watching {
            return;
        }
        self.vv.watching = true;
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(100))
                    .await;
                let Ok(done) = this.update(cx, |this, cx| {
                    let done = this.vv.last_move.elapsed() >= IDLE;
                    if done {
                        this.vv.watching = false;
                        cx.notify();
                    }
                    done
                }) else {
                    return;
                };
                if done {
                    return;
                }
            }
        })
        .detach();
    }

    pub(super) fn controls_shown(&self) -> bool {
        // `dmcall controls`: a capture keeps the idle fade away, as the pointer resting in the area does.
        #[cfg(feature = "capture")]
        if self.dmc.pin_controls {
            return true;
        }
        // The zoom controls hold the idle off while hovered or pressed (`onPreventIdle("interact")`, 117029).
        self.vv.last_move.elapsed() < IDLE
            || self.vv.menu.is_some()
            || self.stream_zoom.box_hover
            || self.stream_zoom.held()
    }

    /// Where the call started on this client, as `m:ss` (`h:mm:ss` from an hour), for the sidebar row.
    pub(super) fn call_timer(&self) -> Option<String> {
        let seconds = self.call.as_ref()?.connected_at?.elapsed().as_secs();
        Some(if seconds >= 3600 {
            format!(
                "{}:{:02}:{:02}",
                seconds / 3600,
                seconds / 60 % 60,
                seconds % 60
            )
        } else {
            format!("{}:{:02}", seconds / 60, seconds % 60)
        })
    }

    fn user_of(&self, id: Id) -> User {
        self.stores
            .users
            .get(&id)
            .cloned()
            .or_else(|| {
                self.stores
                    .current_user
                    .clone()
                    .filter(|user| user.id == id)
            })
            .unwrap_or_default()
    }

    /// `useAvatarColor`: the avatar's dominant colour, `--primary-800` while it is unknown.
    fn tile_color(&self, user: &User) -> Rgba {
        let (key, _) = super::avatar_source(user);
        let found = *self
            .vv
            .colors
            .borrow_mut()
            .entry(key.clone())
            .or_insert_with(|| {
                let picture = self.images.get(&key)?;
                let decoded = image::load_from_memory(picture.bytes()).ok()?;
                Some(crate::palette::avatar_palette(decoded.to_rgba8().as_raw())[0])
            });
        match found {
            Some(rgb) => rgba(
                (u32::from(rgb[0]) << 24)
                    | (u32::from(rgb[1]) << 16)
                    | (u32::from(rgb[2]) << 8)
                    | 0xff,
            ),
            None => mid("--primary-800"),
        }
    }

    /// The candidates of the grid before the filter: a user tile for everyone in the channel, and a stream tile for
    /// everyone going live (`self_stream`) or whose stream this client sends or watches.
    fn participants(&self, guild: Id, channel: Id) -> Vec<Participant> {
        let me = self.stores.current_user.as_ref().map(|user| user.id);
        let own_camera = self.call.as_ref().is_some_and(|call| call.camera);
        let hide_own = !call_setting(SHOW_OWN_CAMERA);
        let mut out = Vec::new();
        for state in self.stores.voice_members(guild, channel) {
            let user = self.user_of(state.user_id);
            let is_me = Some(state.user_id) == me;
            let video = state.self_video || (is_me && own_camera);
            let stream = self.video_stream_of(state.user_id);
            let going_live = state.self_stream || stream.is_some();
            let mute = state.self_mute || state.mute;
            let deaf = state.self_deaf || state.deaf;
            let speaking = self.speaking.contains(&state.user_id)
                || self.soundboard.user_playing(state.user_id);
            let tile = Candidate {
                user: state.user_id,
                kind: TileKind::User,
                name: user.display_name().to_owned(),
                video,
                going_live,
                local_video_disabled: is_me && hide_own,
                connected: false,
            };
            if going_live {
                out.push(Participant {
                    user: user.clone(),
                    mute,
                    deaf,
                    speaking: false,
                    tile: Candidate {
                        kind: TileKind::Stream,
                        connected: stream.is_some(),
                        ..tile.clone()
                    },
                });
            }
            out.push(Participant {
                user,
                mute,
                deaf,
                speaking,
                tile,
            });
        }
        out
    }

    /// Module 689874: clips are on for this account while it streams or a game shows, or a participant's client
    /// announced them (`RTC_CONNECTION_FLAGS`, bit 1).
    fn clips_in_call(&self) -> bool {
        let Some(call) = &self.call else {
            return false;
        };
        let own = self.clips_enabled()
            && (self.own_stream().is_some() || self.detect.store.visible_game().is_some());
        own || self
            .stores
            .voice_members(call.guild, call.channel)
            .iter()
            .any(|state| {
                call.flags
                    .get(&state.user_id)
                    .is_some_and(|flags| flags & 1 != 0)
            })
    }

    /// The ENABLED badge of a call's header (`clipsEnabledIndicator_U6LhXD`: margin 0 8px; a secondary badge of
    /// module 805251 with the clips icon, `body-xs/semibold`, capitals).
    pub(super) fn clips_enabled_badge(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.clips_in_call() {
            return None;
        }
        Some(
            div()
                .mx(px(8.))
                .flex()
                .flex_none()
                .items_center()
                .child(
                    self.with_tip(
                        "call-clips-enabled",
                        Tip::text(
                            "Participants in this call have clips enabled.",
                            Side::Bottom,
                        ),
                        div()
                            .id("call-clips-enabled-badge")
                            .px(px(8.))
                            .rounded_full()
                            .bg(mid("--badge-secondary-background"))
                            .flex()
                            .items_center()
                            .gap(px(4.))
                            .text_size(px(12.))
                            .line_height(px(16.))
                            .font_weight(FontWeight(600.))
                            .text_color(mid("--badge-secondary-text"))
                            .child(icon("clips_clips", 12., "--badge-secondary-text"))
                            .child("ENABLED"),
                        cx,
                    ),
                )
                .into_any_element(),
        )
    }

    /// The items a direct message's header gets from its call (module 289287 `renderHeaderToolbar`): the clips
    /// indicator while in the call, then the quality and LIVE of a focused stream.
    pub(super) fn dm_call_header_items(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        if !self.dm_call_in(self.channel) {
            return Vec::new();
        }
        let mut out = Vec::new();
        out.extend(self.clips_enabled_badge(cx));
        if let Some((user, TileKind::Stream)) = self.vv.focus.tile {
            let label = self.video_stream_of(user).and_then(|info| info.label);
            out.push(live_indicator(label));
        }
        out
    }

    /// The header crumb of a focused tile: the name with Screen for a stream, the name for a camera.
    fn focus_crumb(&self, person: &Participant) -> FocusCrumb {
        let name = person.user.display_name().to_owned();
        let stream = person.tile.kind == TileKind::Stream;
        FocusCrumb {
            title: if stream {
                format!("{name}\u{2019}s Screen")
            } else {
                name
            },
            avatar: self
                .images
                .get(&super::avatar_source(&person.user).0)
                .cloned(),
            stream,
            quality: stream
                .then(|| self.video_stream_of(person.user.id))
                .flatten()
                .and_then(|info| info.label),
            viewers: stream
                .then(|| self.video_stream_of(person.user.id))
                .flatten()
                .map(|info| {
                    info.viewer_ids
                        .into_iter()
                        .map(|id| {
                            self.images
                                .get(&super::avatar_source(&self.user_of(id)).0)
                                .cloned()
                        })
                        .collect()
                })
                .unwrap_or_default(),
        }
    }

    /// Focus mode (module 699707): the tile large in the middle, the others in a strip of 195x112 tiles above the
    /// bar. A click on the large tile, or Grid View, goes back to the grid.
    fn focus_body(
        &self,
        people: &[Participant],
        index: usize,
        width: f32,
        height: f32,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let others: Vec<&Participant> = people
            .iter()
            .enumerate()
            .filter(|(at, _)| *at != index)
            .map(|(_, person)| person)
            .collect();
        // Module 699707. The strip shows while the members are open and the area is over 384px high (`Q`); with it
        // the tile keeps 140px clear above and under it (72 + 68) and sits 60px up, without it the tile takes the
        // whole area, as large as its 16:9 fits.
        let strip_shown = !self.vv.members_hidden && height > 384. && !others.is_empty();
        // The toggle exists outside the layouts that share the column with a chat (`et`).
        let toggle_shown = height > 384. && !others.is_empty() && self.call_layout_is_full();
        let strip_top = height - 192.;
        let (main_top, main_bottom) = if strip_shown {
            (80., strip_top - 8.)
        } else {
            (0., height)
        };
        let room = (main_bottom - main_top).max(120.);
        let main_width = (room * ASPECT).min(width);
        let main_height = (main_width / ASPECT).floor();
        // `actionRow`: 16px under the tile with the strip, else inside it, 40px (and what the bar needs) from its end.
        let tile_bottom = main_top + (room + main_height) / 2.;
        let toggle_top = if strip_shown {
            tile_bottom + 16.
        } else {
            tile_bottom - 40. - (72. - (height - main_height) / 2.).max(0.) - 32.
        };
        let big = GridLayout {
            columns: 1,
            rows: 1,
            tile_width: main_width,
            tile_height: main_height,
            pad_top: 0.,
            pad_bottom: 0.,
        };
        let small = GridLayout {
            tile_width: 195.,
            tile_height: 112.,
            ..big
        };
        let strip: Vec<AnyElement> = if strip_shown {
            others
                .into_iter()
                .map(|person| self.tile(person, small, cx))
                .collect()
        } else {
            Vec::new()
        };
        let members_open = !self.vv.members_hidden;
        div()
            .absolute()
            .inset_0()
            .child(
                div()
                    .absolute()
                    .left_0()
                    .right_0()
                    .top(px(main_top))
                    .h(px(main_bottom - main_top))
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(self.tile(&people[index], big, cx)),
            )
            .when(toggle_shown && self.controls_shown(), |this| {
                // Module 80051: the chevron and the group, "Hide Members" while they show.
                this.child(
                    div()
                        .absolute()
                        .left_0()
                        .right_0()
                        .top(px(toggle_top))
                        .flex()
                        .justify_center()
                        .child(
                            self.with_tip(
                                "call-members-toggle-tip",
                                Tip::text(
                                    if members_open {
                                        "Hide Members"
                                    } else {
                                        "Show Members"
                                    },
                                    Side::Top,
                                ),
                                div()
                                    .id("call-members-toggle")
                                    .occlude()
                                    .h(px(32.))
                                    .px(px(8.))
                                    .rounded(px(8.))
                                    .bg(gpui::hsla(0., 0., 0., 0.6))
                                    .flex()
                                    .items_center()
                                    .gap(px(4.))
                                    .cursor_pointer()
                                    .child(icon(
                                        if members_open {
                                            "clips_chevron_down"
                                        } else {
                                            "clips_chevron_up"
                                        },
                                        24.,
                                        "--white",
                                    ))
                                    .child(icon("members", 24., "--white"))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.vv.members_hidden = !this.vv.members_hidden;
                                        cx.notify();
                                    })),
                                cx,
                            ),
                        ),
                )
            })
            .when(!strip.is_empty(), |this| {
                this.child(
                    div()
                        .absolute()
                        .left(px(8.))
                        .right(px(8.))
                        .top(px(strip_top))
                        .h(px(112.))
                        .flex()
                        .justify_center()
                        .gap(px(TILE_GAP))
                        .children(strip),
                )
            })
            .into_any_element()
    }

    /// Whether the call has its whole area to itself (`NO_CHAT`, `FULL_SCREEN`, a server's voice channel) and not
    /// the upper part of a direct message's column (`MINIMUM`, `NORMAL`).
    fn call_layout_is_full(&self) -> bool {
        self.vv.fullscreen
            || self.dmc.no_chat
            || self.call.as_ref().is_some_and(|call| call.guild != 0)
    }

    /// Where the call view's top left corner is in the window: beside the sidebar under the title bar, or the
    /// window's own corner in Full Screen. The menus are placed against it.
    pub(super) fn call_origin(&self) -> (f32, f32) {
        if self.vv.fullscreen || self.vv.popout_render {
            (0., 0.)
        } else {
            (375., 32.)
        }
    }

    /// Full Screen of the call bar: the call takes the whole window and the window the whole screen; again, and
    /// both go back.
    pub(super) fn toggle_call_fullscreen(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // In the popped-out window Full Screen is that window's own: the main window keeps its message.
        if self
            .vv
            .popout
            .is_some_and(|handle| gpui::AnyWindowHandle::from(handle) == window.window_handle())
        {
            self.vv.menu = None;
            window.toggle_fullscreen();
            cx.notify();
            return;
        }
        self.vv.fullscreen = !self.vv.fullscreen;
        self.vv.menu = None;
        if window.is_fullscreen() != self.vv.fullscreen {
            window.toggle_fullscreen();
        }
        cx.notify();
    }

    /// The call over everything else while Full Screen is on. It ends with the call.
    pub(super) fn call_fullscreen_layer(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if !self.vv.fullscreen {
            return None;
        }
        let Some((guild, channel)) = self.call.as_ref().map(|call| (call.guild, call.channel))
        else {
            self.vv.fullscreen = false;
            if window.is_fullscreen() {
                window.toggle_fullscreen();
            }
            return None;
        };
        let size = window.viewport_size();
        let (width, height) = (f32::from(size.width), f32::from(size.height));
        let body = self.call_view(guild, channel, width, height, cx);
        Some(
            gpui::deferred(
                div()
                    .id("call-fullscreen")
                    .occlude()
                    .absolute()
                    .inset_0()
                    .bg(mid("--background-base-lowest"))
                    .on_mouse_move(cx.listener(|this, _: &MouseMoveEvent, _, cx| {
                        this.voice_view_moved(cx);
                    }))
                    .child(body),
            )
            .with_priority(2)
            .into_any_element(),
        )
    }

    /// The main area for a voice channel: the call while in it, else the disconnected state.
    pub(super) fn voice_view(&mut self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let (Some(guild), Some(channel)) = (self.guild, self.channel) else {
            return div().into_any_element();
        };
        let size = window.viewport_size();
        let width = f32::from(size.width) - 375.;
        let height = f32::from(size.height) - 32.;
        let in_call = self
            .call
            .as_ref()
            .is_some_and(|call| call.channel == channel);
        let chat = in_call && self.vv.chat_open.contains(&channel);
        let body = if in_call && self.call_popped_out() {
            self.call_popped_out_message()
        } else if in_call && self.vv.fullscreen {
            // Full Screen draws the call in its own layer; a second copy here would open every menu twice.
            div().into_any_element()
        } else if in_call {
            self.call_view(guild, channel, width, height, cx)
        } else {
            self.disconnected_view(guild, channel, width, height, cx)
        };
        let mut view = div()
            .parity("voice-view")
            .id("voice-view")
            .relative()
            .flex_1()
            .min_w(px(0.))
            .h_full()
            .overflow_hidden()
            .bg(mid("--background-base-lowest"))
            .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, _, cx| {
                if event.pressed_button.is_none() {
                    this.call_drags_end();
                }
                this.voice_view_moved(cx);
            }))
            .child(body);
        if chat {
            view = view.child(self.voice_chat_panel(guild, channel, cx));
        }
        view.into_any_element()
    }

    pub(super) fn call_view(
        &mut self,
        guild: Id,
        channel: Id,
        width: f32,
        height: f32,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let outside = self.vv.outside;
        let shown = self.controls_shown();
        let everyone = self.participants(guild, channel);
        self.vv.shown_call = (guild, channel);
        // A stream nobody here watches shows its preview behind Watch Stream; one that ended is forgotten.
        for person in &everyone {
            let key = super::stream_previews::key_of(guild, channel, person.tile.user);
            match person.tile.kind {
                TileKind::Stream if !person.tile.connected => self.stream_preview_want(key),
                TileKind::User if !person.tile.going_live => {
                    self.stream_previews.forget(&key.to_string());
                }
                _ => {}
            }
        }
        // Module 874739: outside the call the grid takes `participants`, not `filteredParticipants`.
        let show_non_video = outside || call_setting(SHOW_NON_VIDEO);
        let candidates: Vec<_> = everyone.iter().map(|person| person.tile.clone()).collect();
        let hides_participants = video_tiles::hides_participants(&candidates, show_non_video);
        // `getUserParticipantCount`: users, not streams, whatever the filter hides.
        let user_count = everyone
            .iter()
            .filter(|person| person.tile.kind == TileKind::User)
            .count();
        let by_key: HashMap<(u64, TileKind), Participant> = everyone
            .into_iter()
            .map(|person| ((person.tile.user, person.tile.kind), person))
            .collect();
        let order = video_tiles::select(candidates, show_non_video);
        let mut by_key = by_key;
        let people: Vec<Participant> = order
            .iter()
            .filter_map(|tile| by_key.remove(&(tile.user, tile.kind)))
            .collect();
        // The focused tile left the call: back to the grid.
        self.vv.focus.keep_if(|user, kind| {
            people
                .iter()
                .any(|person| person.tile.user == user && person.tile.kind == kind)
        });
        // `selectedParticipant: v ? n : null`, `v` being in the call with video.
        let focused = self
            .vv
            .focus
            .tile
            .filter(|_| !outside)
            .and_then(|(user, kind)| {
                people
                    .iter()
                    .position(|person| person.tile.user == user && person.tile.kind == kind)
            });
        let can_invite = self.current_channel().is_some_and(|channel| {
            self.can_in_channel(channel, dc_state::permissions::CREATE_INSTANT_INVITE)
        });
        // Alone in the call with the right to invite: the invite card keeps the user's tile company (526369:
        // `userLimit <= 0 || > 1`, one user, `canInvite`). A person's stream does not count as a user.
        let card = user_count == 1 && can_invite && !outside;
        let count = (people.len() + usize::from(card)).max(1);
        let layout = grid_layout(count, width, height);
        if card {
            self.request_image(ART_URL.to_owned(), ART_URL.to_owned());
        }
        let mut tiles: Vec<AnyElement> = people
            .iter()
            .map(|person| self.tile(person, layout, cx))
            .collect();
        if card {
            tiles.push(self.voice_invite_card(layout, cx));
        }
        let mut rows = Vec::new();
        while !tiles.is_empty() {
            let take = layout.columns.min(tiles.len());
            let row: Vec<AnyElement> = tiles.drain(..take).collect();
            // Chromium paints a row centred on a half pixel (582.5 for one 490px tile in 889) at the next whole one.
            let row_width = take as f32 * layout.tile_width + (take as f32 - 1.) * TILE_GAP;
            let indent = row_indent(width, row_width);
            rows.push(
                div()
                    .flex()
                    .flex_none()
                    .w_full()
                    .pl(px(indent))
                    .gap(px(TILE_GAP))
                    .children(row)
                    .into_any_element(),
            );
        }
        let focus_head = focused.map(|index| self.focus_crumb(&people[index]));
        self.stream_zoom.follow(focused.and_then(|index| {
            let tile = &people[index].tile;
            (tile.kind == TileKind::Stream).then_some(tile.user)
        }));
        let activity_stage = self
            .activities
            .running
            .as_ref()
            .is_some_and(|running| running.channel == channel && running.url.is_some());
        if activity_stage {
            self.activities.stage_seen = true;
        }
        let grid = if activity_stage {
            // The running activity's page is a native surface laid over this box (`dc-webhost`).
            div()
                .absolute()
                .left(px(8.))
                .right(px(8.))
                .top(px(56.))
                .bottom(px(96.))
                .rounded(px(8.))
                .bg(mid("--background-base-lowest"))
                .child(self.anchor_probe("activity-stage"))
                .into_any_element()
        } else if let Some(index) = focused {
            self.focus_body(&people, index, width, height, cx)
        } else if people.is_empty() && hides_participants {
            self.no_video_view(width, height, cx)
        } else {
            div()
                .absolute()
                .inset_0()
                .px(px(8.))
                .pt(px(layout.pad_top))
                .pb(px(layout.pad_bottom.max(0.)))
                .flex()
                .flex_col()
                .gap(px(TILE_GAP))
                .overflow_hidden()
                .children(rows)
                .into_any_element()
        };
        if outside {
            // `disableGradients: !inCall`, and the bottom centre is the join buttons of the caller.
            return div().absolute().inset_0().child(grid).into_any_element();
        }
        div()
            .absolute()
            .inset_0()
            .child(grid)
            .child(
                div()
                    .absolute()
                    .inset_0()
                    .when(!shown, |this| this.opacity(0.).invisible())
                    .child(fade(true))
                    .child(fade(false)),
            )
            .when(guild != 0, |this| {
                this.child(self.call_header(channel, shown, focus_head, cx))
            })
            .children(activity_stage.then(|| self.activity_leave_button(cx)))
            .child(self.call_bar(guild, channel, shown, cx))
            .children(self.voice_menus(channel, width, height, cx))
            .children(self.soundboard_suggest(cx))
            .into_any_element()
    }

    /// Leave, over the stage's top right while an activity runs (`Hi1/aQ`; the official panel has no capture here).
    fn activity_leave_button(&self, cx: &mut Context<Self>) -> AnyElement {
        div()
            .id("activity-leave")
            .absolute()
            .top(px(12.))
            .right(px(72.))
            .h(px(32.))
            .px(px(12.))
            .rounded(px(8.))
            .bg(mid("--control-critical-primary-background-default"))
            .flex()
            .items_center()
            .cursor_pointer()
            .text_size(px(14.))
            .line_height(px(18.))
            .font_weight(FontWeight(500.))
            .text_color(mid("--white"))
            .on_click(cx.listener(|this, _, _, cx| this.leave_activity(cx)))
            .child("Leave")
            .into_any_element()
    }

    /// `713397.js`: nobody has video and the others are hidden. The call container's glow with the heading and
    /// the button that turns "Show Non-Video Participants" back on.
    fn no_video_view(&mut self, width: f32, height: f32, cx: &mut Context<Self>) -> AnyElement {
        let background = self.call_empty_background(width, height, cx);
        div()
            .absolute()
            .inset_0()
            .children(background.map(|image| img(image).absolute().inset_0().size_full()))
            .child(
                div()
                    .absolute()
                    .inset_0()
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .gap(px(24.))
                    .child(
                        div()
                            .parity("voice-hidden-notice")
                            .text_size(px(20.))
                            .line_height(px(24.))
                            .font_weight(FontWeight(500.))
                            .text_color(mid("--text-strong"))
                            .child("Participants without video are currently hidden"),
                    )
                    .child(
                        div()
                            .parity_with_border("voice-show-all", 1.)
                            .id("voice-show-all")
                            .h(px(40.))
                            .px(px(15.))
                            .rounded(px(8.))
                            .border_1()
                            .border_color(mid("--control-secondary-border-default"))
                            .bg(mid("--control-secondary-background-default"))
                            .flex()
                            .items_center()
                            .cursor_pointer()
                            .hover(|style| style.bg(mid("--control-secondary-background-hover")))
                            .text_size(px(16.))
                            .line_height(px(20.))
                            .font_weight(FontWeight(500.))
                            .text_color(mid("--control-secondary-text-default"))
                            .on_click(cx.listener(|_, _, _, cx| {
                                super::voice_view::set_call_setting(SHOW_NON_VIDEO, true);
                                cx.notify();
                            }))
                            .child("Show All Participants"),
                    ),
            )
            .into_any_element()
    }

    fn tile(&self, person: &Participant, layout: GridLayout, cx: &mut Context<Self>) -> AnyElement {
        let kind = person.tile.kind;
        let user_id = person.user.id;
        let stream_tile = kind == TileKind::Stream;
        let tag = if stream_tile { "vstream" } else { "vtile" };
        let group = SharedString::from(format!("{tag}-{user_id}"));
        let menu_open = match self.vv.menu {
            Some(Menu::Tile(id, _)) => id == user_id && !stream_tile,
            Some(Menu::Stream(id, _)) => id == user_id && stream_tile,
            _ => false,
        };
        let stream = if stream_tile {
            self.video_stream_of(user_id)
        } else {
            None
        };
        let pill = div().h(px(32.)).flex().items_center().child(
            div()
                .h(px(32.))
                .px(px(8.))
                .py(px(6.))
                .rounded(px(8.))
                .bg(mid("--control-overlay-secondary-background-default"))
                .flex()
                .items_center()
                .gap(px(4.))
                .text_size(px(16.))
                .line_height(px(20.))
                .text_color(rgba(0xffffffff))
                .when(stream_tile, |this| {
                    this.child(icon("voice_view_stream", 16., "--white"))
                })
                .when(!stream_tile && person.mute, |this| {
                    this.child(icon("voice_view_mic_slash", 16., "--white"))
                })
                .when(!stream_tile && person.deaf && !person.mute, |this| {
                    this.child(icon("headphones_deafened", 16., "--white"))
                })
                .child(person.user.display_name().to_owned()),
        );
        let options = div()
            .id(SharedString::from(format!("{tag}-options-{user_id}")))
            .size(px(32.))
            .rounded(px(8.))
            .bg(mid("--control-overlay-secondary-background-default"))
            .flex()
            .items_center()
            .justify_center()
            .cursor_pointer()
            .when(!menu_open, |this| {
                this.opacity(0.)
                    .group_hover(group.clone(), |style| style.opacity(1.))
            })
            .on_click(cx.listener(move |this, event: &gpui::ClickEvent, _, cx| {
                cx.stop_propagation();
                this.vv.menu = Some(if stream_tile {
                    Menu::Stream(user_id, event.position())
                } else {
                    Menu::Tile(user_id, event.position())
                });
                cx.notify();
            }))
            .child(icon("voice_view_more", 16., "--white"));
        let outside = self.vv.outside;
        let media = self.tile_media_for(person);
        let video = matches!(media, TileMedia::Video(_));
        let waiting = stream_tile && stream.is_none();
        let loading = if stream_tile {
            stream.is_some() && !video
        } else {
            video_tiles::shows_video(&person.tile) && !video
        };
        let background = if stream_tile || video_tiles::shows_video(&person.tile) {
            mid("--primary-700")
        } else {
            self.tile_color(&person.user)
        };
        // The focused stream's quality and LIVE sit in the header instead.
        let is_focus = self.vv.focus.tile == Some((user_id, kind));
        let zoom_controls = stream_tile && is_focus && video;
        let indicators = stream.as_ref().filter(|_| !is_focus).map(|stream| {
            let pill = |text: String, fill: &'static str| {
                div()
                    .h(px(16.))
                    .px(px(6.))
                    .flex()
                    .items_center()
                    .gap(px(4.))
                    .bg(mid(fill))
                    .text_size(px(12.))
                    .line_height(px(16.))
                    .font_weight(FontWeight(600.))
                    .text_color(rgba(0xffffffff))
                    .child(text)
            };
            let quality = stream.label.clone();
            let live = pill("LIVE".to_owned(), "--badge-critical-background").rounded_r(px(16.));
            div()
                .absolute()
                .top(px(8.))
                .right(px(8.))
                .flex()
                .when_some(quality, |this, text| {
                    this.child(pill(text, "--interactive-muted").rounded_l(px(16.)))
                })
                .child(if stream.label.is_none() {
                    live.rounded_l(px(16.))
                } else {
                    live
                })
        });
        div()
            .id(SharedString::from(format!("{tag}-{user_id}")))
            .group(group)
            .relative()
            .flex_none()
            .w(px(layout.tile_width))
            .h(px(layout.tile_height))
            .rounded(px(8.))
            .overflow_hidden()
            .bg(background)
            .when(outside && stream_tile, |this| {
                // `handleSelectParticipant` on a stream from outside the call: join it and watch.
                this.cursor_pointer()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.watch_stream_joining(user_id, cx);
                    }))
            })
            .when(stream_tile && is_focus && video && !outside, |this| {
                // Module 555148 `M`: the wheel zooms the focused stream about the pointer; zoomed, a drag pans.
                let bounds = self.stream_zoom.bounds.clone();
                let dragging = self.stream_zoom.drag.is_some();
                let weak = cx.entity().downgrade();
                this.on_scroll(cx.listener(|this, event: &gpui::ScrollEvent, _, cx| {
                    let delta_y = match event.delta {
                        gpui::ScrollDelta::Pixels(delta) => -f32::from(delta.y),
                        gpui::ScrollDelta::Lines(delta) => -delta.y * 100.,
                    };
                    this.stream_zoom_wheel(event.position.x, event.position.y, delta_y, cx);
                    cx.stop_propagation();
                }))
                .on_mouse_down(
                    gpui::MouseButton::Left,
                    cx.listener(|this, event: &gpui::MouseDownEvent, _, cx| {
                        this.stream_zoom.press(event.position.x, event.position.y);
                        cx.notify();
                    }),
                )
                .child(
                    gpui::canvas(
                        |_, _, _| (),
                        move |tile, (), window, _| {
                            bounds.set(tile);
                            if !dragging {
                                return;
                            }
                            let moved = weak.clone();
                            window.on_mouse_event(
                                move |event: &gpui::MouseMoveEvent, phase, _, cx| {
                                    if phase != gpui::DispatchPhase::Capture {
                                        return;
                                    }
                                    let _ = moved.update(cx, |this, cx| {
                                        this.stream_zoom
                                            .drag_to(event.position.x, event.position.y);
                                        cx.notify();
                                    });
                                },
                            );
                            let released = weak.clone();
                            window.on_mouse_event(move |_: &gpui::MouseUpEvent, phase, _, cx| {
                                if phase != gpui::DispatchPhase::Capture {
                                    return;
                                }
                                let _ = released.update(cx, |this, cx| {
                                    this.stream_zoom.release();
                                    cx.notify();
                                });
                            });
                        },
                    )
                    .absolute()
                    .inset_0(),
                )
            })
            .when(waiting && !outside, |this| {
                // `handleSelectParticipant` on a stream nobody here watches: watch it, beside the others when
                // shift is held (`forceMultiple: t.shiftKey`).
                this.cursor_pointer().on_click(cx.listener(
                    move |this, event: &gpui::ClickEvent, _, cx| {
                        this.watch_stream_with(user_id, event.modifiers().shift, cx);
                    },
                ))
            })
            .when(!waiting && !outside, |this| {
                this.cursor_pointer().on_click(cx.listener(
                    move |this, event: &gpui::ClickEvent, window, cx| {
                        if this.stream_zoom.take_moved() {
                            // The press panned the zoomed picture.
                            return;
                        }
                        if event.click_count() >= 2 {
                            // `handleFullscreenParticipant` (module 540462): the tile is the selected one in Full
                            // Screen, and the same tile again leaves both. The first press of the pair has already
                            // run `handleSelectParticipant`, as a browser sends click before dblclick.
                            let selected = this.vv.focus.tile == Some((user_id, kind));
                            if this.vv.fullscreen == selected {
                                this.vv.focus.click(user_id, kind);
                            }
                            this.toggle_call_fullscreen(window, cx);
                        } else {
                            this.vv.focus.click(user_id, kind);
                        }
                        cx.notify();
                    },
                ))
            })
            .child(self.tile_media(media, person, layout))
            .when(loading, |this| this.child(spinner()))
            .when(waiting, |this| {
                let (guild, channel) = self.vv.shown_call;
                this.children(
                    self.stream_preview_image(guild, channel, user_id)
                        .map(|picture| {
                            div().absolute().inset_0().child(
                                img(picture)
                                    .w(px(layout.tile_width))
                                    .h(px(layout.tile_height))
                                    .aspect_ratio(layout.tile_width / layout.tile_height)
                                    .object_fit(gpui::ObjectFit::Contain),
                            )
                        }),
                )
                .child(self.watch_overlay(user_id, layout.tile_width, cx))
            })
            .children(indicators)
            .child(
                div()
                    .absolute()
                    .inset(px(12.))
                    .flex()
                    .items_end()
                    .justify_between()
                    .child(pill)
                    // Module 175203 `eM`: a focused tile's action row has no Options; a focused stream's has its zoom
                    // controls (`zoom-controls`, design/dom/popouts/stream-focused-darker-cozy.json).
                    .when(!outside, |this| {
                        if zoom_controls {
                            let image = match &media {
                                TileMedia::Video(key) => self.video.images.image(key).cloned(),
                                _ => None,
                            };
                            this.child(self.stream_zoom_controls(image, cx))
                        } else {
                            this.child(options)
                        }
                    }),
            )
            .when(person.speaking && !stream_tile, |this| {
                this.child(
                    div()
                        .absolute()
                        .inset_0()
                        .rounded(px(8.))
                        .border_2()
                        .border_color(color("--status-positive")),
                )
            })
            .into_any_element()
    }

    /// What the tile draws behind its name (module 83982 and 555148): a camera or stream picture when there is one,
    /// else the avatar.
    fn tile_media_for(&self, person: &Participant) -> TileMedia {
        let user = person.user.id;
        match person.tile.kind {
            TileKind::Stream => {
                let key = (user, dc_voice::VideoSource::Stream(user));
                if self.video.images.image(&key).is_some() && self.video_stream_of(user).is_some() {
                    TileMedia::Video(key)
                } else {
                    TileMedia::None
                }
            }
            TileKind::User => {
                let key = (user, dc_voice::VideoSource::Camera);
                if video_tiles::shows_video(&person.tile) {
                    if self.video.images.image(&key).is_some() {
                        TileMedia::Video(key)
                    } else {
                        TileMedia::None
                    }
                } else {
                    TileMedia::Avatar
                }
            }
        }
    }

    /// The tile's picture.
    fn tile_media(&self, media: TileMedia, person: &Participant, layout: GridLayout) -> AnyElement {
        match media {
            TileMedia::None => div().into_any_element(),
            TileMedia::Avatar => {
                let picture = self
                    .images
                    .get(&super::avatar_source(&person.user).0)
                    .cloned();
                div()
                    .absolute()
                    .inset_0()
                    .flex()
                    .items_center()
                    .justify_center()
                    .children(picture.map(|picture| img(picture).size(px(80.)).rounded_full()))
                    .into_any_element()
            }
            TileMedia::Video(key) => match self.video.images.image(&key) {
                Some(image) => div()
                    .absolute()
                    .inset_0()
                    .bg(rgba(0x000000ff))
                    .child(
                        // A stream keeps its own shape inside the tile (`fit` of module 555148). The element takes
                        // the tile's ratio, or the layout gives it the picture's and the tile crops it.
                        if key.1 == dc_voice::VideoSource::Camera {
                            img(image.clone())
                                .size_full()
                                .object_fit(gpui::ObjectFit::Contain)
                        } else {
                            // `--custom-zoom-scale` and the pan: the picture grows about the tile's centre and
                            // moves by the pan; the tile crops it.
                            let zoom = self.stream_zoom.of(key.0);
                            // `--custom-zoom-transition`: a step eases 0.15s, but not under a drag, the wheel or the
                            // slider (`.full-motion` rule).
                            let ease = if self.stream_zoom.picture_eases(super::motion::now()) {
                                super::motion::css(150., super::motion::Ease::EaseOut).full_motion()
                            } else {
                                super::motion::css(0., super::motion::Ease::EaseOut)
                            };
                            let [pan_x, pan_y, level, _] = super::motion::animate(
                                super::motion::key(&("vstream-zoom", key.0)),
                                [
                                    f64::from(zoom.pan.0),
                                    f64::from(zoom.pan.1),
                                    f64::from(zoom.level),
                                    0.,
                                ],
                                ease,
                            )
                            .map(|value| value as f32);
                            let zoom = super::stream_zoom::Zoom {
                                level,
                                pan: (pan_x, pan_y),
                            };
                            let (width, height) = (
                                layout.tile_width * zoom.level,
                                layout.tile_height * zoom.level,
                            );
                            img(image.clone())
                                .absolute()
                                .left(px((layout.tile_width - width) / 2. + zoom.pan.0))
                                .top(px((layout.tile_height - height) / 2. + zoom.pan.1))
                                .w(px(width))
                                .h(px(height))
                                .aspect_ratio(layout.tile_width / layout.tile_height)
                                .object_fit(gpui::ObjectFit::Contain)
                        },
                    )
                    .into_any_element(),
                None => div().into_any_element(),
            },
        }
    }

    /// `Watch Stream` over a stream nobody here watches yet (module 555148: a secondary button; `Watch` under 175px).
    fn watch_overlay(&self, owner: u64, width: f32, cx: &mut Context<Self>) -> AnyElement {
        div()
            .absolute()
            .inset_0()
            .flex()
            .items_center()
            .justify_center()
            .child(
                div()
                    .id(SharedString::from(format!("vstream-watch-{owner}")))
                    .h(px(40.))
                    .px(px(16.))
                    .rounded(px(8.))
                    .border_1()
                    .border_color(mid("--control-secondary-border-default"))
                    .bg(mid("--control-secondary-background-default"))
                    .flex()
                    .items_center()
                    .cursor_pointer()
                    .hover(|style| style.bg(mid("--control-secondary-background-hover")))
                    .text_size(px(16.))
                    .line_height(px(20.))
                    .font_weight(FontWeight(500.))
                    .text_color(mid("--control-secondary-text-default"))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        cx.stop_propagation();
                        this.watch_stream_joining(owner, cx);
                    }))
                    .child(if width < 175. {
                        "Watch"
                    } else {
                        "Watch Stream"
                    }),
            )
            // While another stream is watched: the eye that adds this one beside it (`forceMultiple`).
            .when(self.in_a_stream(), |this| {
                this.gap(px(8.)).child(
                    self.with_tip(
                        "vstream-watch-multiple-tip",
                        Tip::text("Start watching multiple streams", Side::Top),
                        div()
                            .id(SharedString::from(format!(
                                "vstream-watch-multiple-{owner}"
                            )))
                            .size(px(40.))
                            .rounded(px(8.))
                            .border_1()
                            .border_color(mid("--control-secondary-border-default"))
                            .bg(mid("--control-secondary-background-default"))
                            .hover(|style| style.bg(mid("--control-secondary-background-hover")))
                            .flex()
                            .items_center()
                            .justify_center()
                            .cursor_pointer()
                            .on_click(cx.listener(move |this, _, _, cx| {
                                cx.stop_propagation();
                                this.watch_stream_with(owner, true, cx);
                            }))
                            .child(icon(
                                "voice_view_eye_plus",
                                20.,
                                "--control-secondary-text-default",
                            )),
                        cx,
                    ),
                )
            })
            .into_any_element()
    }

    /// `686502.js` `D`: the card beside a lone user, with the Turbine art, Invite to Voice and Choose Activity.
    fn voice_invite_card(&self, layout: GridLayout, cx: &mut Context<Self>) -> AnyElement {
        let art = self.images.get(ART_URL).cloned();
        let button = |id: &'static str, glyph: &'static str, label: &'static str| {
            div()
                .id(id)
                .h(px(40.))
                .px(px(15.))
                .rounded(px(8.))
                .border_1()
                .border_color(mid("--control-secondary-border-default"))
                .bg(mid("--control-secondary-background-default"))
                .flex()
                .items_center()
                .gap(px(4.))
                .cursor_pointer()
                .text_size(px(16.))
                .line_height(px(20.))
                .font_weight(FontWeight(500.))
                .text_color(mid("--control-secondary-text-default"))
                .child(icon(glyph, 20., "--control-secondary-text-default"))
                .child(label)
        };
        div()
            .flex_none()
            .relative()
            .w(px(layout.tile_width))
            .h(px(layout.tile_height))
            .rounded(px(8.))
            .overflow_hidden()
            .bg(mid("--background-base-lower"))
            .child(
                img(self.empty_card_background(layout.tile_width, layout.tile_height))
                    .rounded(px(8.))
                    .absolute()
                    .inset_0()
                    .size_full(),
            )
            .child(
                div()
                    .absolute()
                    .inset_0()
                    .p(px(16.))
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(px(16.))
                    .child(
                        div()
                            .flex_1()
                            .min_h(px(0.))
                            .mb(px(24.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .children(art.map(|art| img(art).w(px(311.66)).h(px(163.62)))),
                    )
                    .child(
                        div()
                            .flex()
                            .gap(px(8.))
                            .child(
                                button(
                                    "vcard-invite",
                                    "voice_view_invite_button",
                                    "Invite to Voice",
                                )
                                .on_click(cx.listener(|this, _, _, cx| this.open_invite(cx))),
                            )
                            .child(
                                button("vcard-activity", "voice_view_activity", "Choose Activity")
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.voice_activity_clicked(window, cx);
                                    })),
                            ),
                    ),
            )
            .into_any_element()
    }

    /// The card's `radial-gradient(110.79% 100% at 50% 100%, base-lower 60%, transparent 100%)` over the pink
    /// `linear-gradient(270deg, pink-end 0, pink-start 80%)`, drawn once per size (gpui has no radial gradient).
    fn empty_card_background(&self, width: f32, height: f32) -> std::sync::Arc<gpui::Image> {
        let (w, h) = (width.round().max(1.) as u32, height.round().max(1.) as u32);
        let key = format!("voice-card:{w}x{h}");
        if let Some(found) = self.generated_images.borrow().get(&key) {
            return found.clone();
        }
        // The pink stops carry 30% alpha (`0x4d`): they tint the base colour rather than replace it.
        let channels = |name: &str| {
            let value = Theme::Midnight.color(name);
            [
                (value >> 24) as f32,
                ((value >> 16) & 0xff) as f32,
                ((value >> 8) & 0xff) as f32,
                (value & 0xff) as f32 / 255.,
            ]
        };
        let (start, end, base) = (
            channels("--background-tile-gradient-pink-start"),
            channels("--background-tile-gradient-pink-end"),
            channels("--background-base-lower"),
        );
        let mut pixels = image::RgbaImage::new(w, h);
        for (x, y, pixel) in pixels.enumerate_pixels_mut() {
            // 270deg runs right to left: the end colour sits at the right edge, the start colour at 80%.
            let along = ((w as f32 - (x as f32 + 0.5)) / (w as f32 * 0.8)).clamp(0., 1.);
            let dx = (x as f32 + 0.5 - w as f32 / 2.) / (1.1079 * w as f32);
            let dy = (y as f32 + 0.5 - h as f32) / h as f32;
            let t = (dx * dx + dy * dy).sqrt();
            let cover = if t <= 0.6 {
                1.
            } else {
                (1. - (t - 0.6) / 0.4).clamp(0., 1.)
            };
            let mut out = [0u8; 3];
            for i in 0..3 {
                let pink = end[i] + (start[i] - end[i]) * along;
                let alpha = end[3] + (start[3] - end[3]) * along;
                let tinted = base[i] + (pink - base[i]) * alpha;
                out[i] = (tinted + (base[i] - tinted) * cover).round() as u8;
            }
            *pixel = image::Rgba([out[0], out[1], out[2], 255]);
        }
        let mut bytes = Vec::new();
        let _ = image::ImageEncoder::write_image(
            image::codecs::png::PngEncoder::new(&mut bytes),
            pixels.as_raw(),
            w,
            h,
            image::ExtendedColorType::Rgba8,
        );
        let made = std::sync::Arc::new(gpui::Image::from_bytes(gpui::ImageFormat::Png, bytes));
        self.generated_images.borrow_mut().insert(key, made.clone());
        made
    }

    /// `topControls_rTsZa_`: the channel's speaker and name, Show Chat at the right.
    fn call_header(
        &self,
        channel: Id,
        shown: bool,
        focus: Option<FocusCrumb>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let name = self
            .current_channel()
            .map(|channel| channel.name.clone())
            .unwrap_or_default();
        let chat_open = self.vv.chat_open.contains(&channel);
        let chat = div()
            .id("voice-show-chat")
            .size(px(24.))
            .cursor_pointer()
            .on_click(cx.listener(move |this, _, _, cx| {
                this.vv.chat_open.insert(channel);
                cx.notify();
            }))
            .child(icon("voice_view_chat", 24., "--interactive-icon-default"));
        div()
            .absolute()
            .top_0()
            .left_0()
            .right_0()
            .h(px(49.))
            .px(px(16.))
            .flex()
            .items_center()
            .when(!shown, |this| this.opacity(0.).invisible())
            .child(
                div()
                    .ml(px(-1.))
                    .flex()
                    .items_center()
                    .gap(px(9.))
                    .text_size(px(16.))
                    .line_height(px(20.))
                    .font_weight(FontWeight(500.))
                    .text_color(mid("--text-strong"))
                    .child(icon("voice_view_channel", 20., "--channels-default"))
                    .child(name)
                    .children(focus.as_ref().map(|crumb| {
                        div()
                            .flex()
                            .items_center()
                            .gap(px(9.))
                            .child(
                                div()
                                    .size(px(4.))
                                    .rounded_full()
                                    .bg(mid("--interactive-icon-default")),
                            )
                            .children(
                                crumb
                                    .avatar
                                    .clone()
                                    .map(|picture| img(picture).size(px(24.)).rounded_full()),
                            )
                            .child(crumb.title.clone())
                    })),
            )
            .child(div().flex_1())
            .children(
                focus
                    .as_ref()
                    .filter(|crumb| !crumb.viewers.is_empty())
                    .map(|crumb| viewer_pile(&crumb.viewers)),
            )
            // Module 416528: the clips indicator, then the focused stream's quality and LIVE.
            .children(self.clips_enabled_badge(cx))
            .children(
                focus
                    .filter(|crumb| crumb.stream)
                    .map(|crumb| div().mr(px(16.)).child(live_indicator(crumb.quality))),
            )
            .when(!chat_open, |this| {
                this.child(self.with_tip(
                    "voice-show-chat-tip",
                    Tip::text("Show Chat", Side::Bottom),
                    chat,
                    cx,
                ))
            })
            .into_any_element()
    }

    /// `bottomControls_rTsZa_`: Invite, the centre buttons, Pop Out and Full Screen.
    pub(super) fn call_bar(
        &self,
        guild: Id,
        channel: Id,
        shown: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let muted = self.self_mute || self.self_deaf;
        let soundboard_reason = self.soundboard_disabled();
        let tray = |id: &'static str, glyph: &'static str| {
            div().id(id).size(px(24.)).cursor_pointer().child(icon(
                glyph,
                24.,
                "--interactive-icon-default",
            ))
        };
        let primary_box = |id: &'static str| {
            div()
                .id(id)
                .size(px(40.))
                .rounded(px(8.))
                .flex()
                .items_center()
                .justify_center()
                .cursor_pointer()
                .hover(|style| style.bg(gpui::hsla(0., 0., 1., 0.1)))
        };
        let primary_tinted = |id: &'static str, glyph: &'static str, tint: &'static str| {
            primary_box(id).child(icon(glyph, 20., tint))
        };
        let primary = |id: &'static str, glyph: &'static str| primary_tinted(id, glyph, "--white");
        // The animated centre icons (`centerIcon_K9Xq13`, 20px; module 693409 draws the hook's `Component` with
        // `color: currentColor`): white on `primaryDark`, `--red-400` on `redGlow`, `--green-300` on `greenGlow`, and
        // `--control-critical-primary-text-default` on `disconnect` (stylesheet 491899; the captured
        // `popouts/stream-live` shows 218,62,68 muted and 115,196,139 streaming).
        let mic_id = tray_icon_id("mic", channel);
        let camera_id = tray_icon_id("camera", channel);
        let screen_id = tray_icon_id("screen", channel);
        let activity_id = tray_icon_id("activity", channel);
        let soundboard_id = tray_icon_id("soundboard", channel);
        let disconnect_id = tray_icon_id("disconnect", channel);
        let stop_watching_id = tray_icon_id("stop-watching", channel);
        let animated = |id: &'static str,
                        lottie: &str,
                        anim: Anim,
                        state: Option<&'static str>,
                        tint: &'static str| {
            primary_box(id).child(lottie_icon::icon(lottie, anim, state, 20., color(tint)))
        };
        // The microphone and camera buttons each carry a 20px chevron nub after a pixel of gap.
        let nub = |id: &'static str, tint: &'static str| {
            div()
                .id(id)
                .w(px(20.))
                .h(px(40.))
                .rounded_r(px(8.))
                .flex()
                .items_center()
                .justify_center()
                .cursor_pointer()
                .child(icon("voice_view_caret", 16., tint))
        };
        let mic_main = div()
            .id("call-mic")
            .size(px(40.))
            .rounded_l(px(8.))
            .flex()
            .items_center()
            .justify_center()
            .cursor_pointer()
            .on_click(cx.listener(|this, _, _, cx| this.toggle_self_mute(cx)))
            .on_hover(lottie_hover(mic_id.clone(), cx))
            .when(muted, |this| {
                this.bg(mid("--opacity-red-12"))
                    .hover(|style| style.bg(mid("--opacity-red-16")))
            })
            .when(!muted, |this| {
                this.hover(|style| style.bg(gpui::hsla(0., 0., 1., 0.1)))
            })
            // Module 577062: `useMicrophoneLottie(muted ? "unmute" : "mute")`.
            .child(lottie_icon::icon(
                &mic_id,
                Anim::Microphone,
                Some(if muted { "unmute" } else { "mute" }),
                20.,
                color(if muted { "--red-400" } else { "--white" }),
            ));
        let mic_open = self.audio_menu.as_ref().is_some_and(|menu| menu.call);
        let mic_nub = nub(
            "call-mic-chevron",
            if muted { "--red-400" } else { "--white" },
        )
        .relative()
        .child(self.anchor_probe("call-mic-chevron"))
        .when(muted, |this| this.bg(mid("--opacity-red-12")))
        .when(mic_open, |this| {
            // The open menu turns its chevron up.
            this.child(
                div()
                    .absolute()
                    .inset_0()
                    .rounded_r(px(8.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .bg(if muted {
                        mid("--opacity-red-12")
                    } else {
                        rgba(0)
                    })
                    .child(
                        gpui::svg()
                            .path("icons/voice_view_caret.svg")
                            .size(px(16.))
                            .text_color(color(if muted { "--red-400" } else { "--white" }))
                            .with_transformation(gpui::Transformation::rotate(gpui::radians(
                                std::f32::consts::PI,
                            ))),
                    ),
            )
        })
        .on_click(cx.listener(|this, _, _, cx| this.toggle_call_audio_menu(cx)));
        let mic = div()
            .flex()
            .gap(px(1.))
            .child(self.with_tip(
                "call-mic-tip",
                Tip::text(if muted { "Unmute" } else { "Mute" }, Side::Top),
                mic_main,
                cx,
            ))
            .child(self.with_tip(
                "call-mic-chevron-tip",
                Tip::text("More Microphone Options", Side::Top),
                mic_nub,
                cx,
            ));
        let camera_on = self.call.as_ref().is_some_and(|call| call.camera);
        let active_fill = gpui::rgba(0x00854520);
        let active_text = "--text-feedback-positive";
        // Module 970636: the popped-out window keeps the still `VideoIcon` / `VideoSlashIcon`, elsewhere
        // `useVideoLottie(enabled ? "disable" : "enable")` (color `green` while on: `greenGlow`).
        let camera_tint = if camera_on { "--green-300" } else { "--white" };
        let camera_button = if self.vv.popout_render {
            primary_tinted(
                "call-camera",
                if camera_on {
                    "dm_video"
                } else {
                    "voice_camera"
                },
                camera_tint,
            )
        } else {
            animated(
                "call-camera",
                &camera_id,
                Anim::Camera,
                Some(if camera_on { "disable" } else { "enable" }),
                camera_tint,
            )
            .on_hover(lottie_hover(camera_id.clone(), cx))
        };
        let camera_button = if camera_on {
            camera_button.rounded_l(px(8.)).bg(active_fill)
        } else {
            camera_button
        };
        let camera = div()
            .relative()
            .flex()
            .gap(px(1.))
            .child(self.anchor_probe("call-camera-group"))
            .child(self.with_tip(
                "call-camera-tip",
                Tip::text(
                    if camera_on {
                        "Turn Off Camera"
                    } else {
                        "Turn On Camera"
                    },
                    Side::Top,
                ),
                camera_button.on_click(cx.listener(|this, _, _, cx| this.voice_camera_clicked(cx))),
                cx,
            ))
            .child(
                self.with_tip(
                    "call-camera-chevron-tip",
                    Tip::text("More Camera Options", Side::Top),
                    nub(
                        "call-camera-chevron",
                        if camera_on { active_text } else { "--white" },
                    )
                    .when(camera_on, |this| this.bg(active_fill))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.vv.menu = if this.vv.menu == Some(Menu::Camera) {
                            None
                        } else {
                            Some(Menu::Camera)
                        };
                        cx.notify();
                    })),
                    cx,
                ),
            );
        let streaming = self.own_stream().is_some();
        let section = |children: Vec<AnyElement>| {
            div()
                .flex()
                .flex_none()
                .items_center()
                .gap(px(8.))
                .h(px(50.))
                .p(px(4.))
                .rounded(px(12.))
                .border_1()
                .border_color(mid("--border-muted"))
                .bg(mid("--background-surface-high"))
                .children(children)
        };
        let more_open = self.vv.menu == Some(Menu::More);
        let more = primary("call-more", "voice_view_more")
            .relative()
            .child(self.anchor_probe("call-more"))
            .when(more_open, |this| this.bg(gpui::hsla(0., 0., 1., 0.1)))
            .on_click(cx.listener(|this, _, _, cx| {
                this.vv.menu = if this.vv.menu == Some(Menu::More) {
                    None
                } else {
                    Some(Menu::More)
                };
                cx.notify();
            }));
        // Module 533978: `play()` when a click opens the picker (`ed`), and the leave half of the hover events only
        // while no popout is open.
        let soundboard = animated(
            "call-soundboard",
            &soundboard_id,
            Anim::Soundboard,
            None,
            "--white",
        )
        .when(soundboard_reason.is_some(), |this| this.opacity(0.3))
        .when(soundboard_reason.is_none(), |this| {
            let id = soundboard_id.clone();
            this.on_click(cx.listener(move |this, _, window, cx| {
                this.toggle_soundboard(window, cx);
                if this.panel.popout == Some(super::voice_panel::Popout::Soundboard) {
                    lottie_icon::click(&id);
                }
            }))
        });
        let centre = div()
            .parity("dm-call-tray")
            .flex()
            .flex_none()
            .items_center()
            .gap(px(12.))
            .child(section(vec![
                mic.into_any_element(),
                camera.into_any_element(),
            ]))
            .child(section(vec![
                if streaming {
                    div()
                        .relative()
                        .flex()
                        .gap(px(1.))
                        .child(self.anchor_probe("call-screen-group"))
                        .child(
                            self.with_tip(
                                "call-screen-tip",
                                Tip::text("Stop Streaming", Side::Top),
                                // Module 958449: `useScreenshareLottie(streamActive ? "disable" : "enable")`.
                                animated(
                                    "call-screen",
                                    &screen_id,
                                    Anim::ScreenShare,
                                    Some("disable"),
                                    "--green-300",
                                )
                                .on_hover(lottie_hover(screen_id.clone(), cx))
                                .rounded_l(px(8.))
                                .bg(active_fill)
                                .on_click(
                                    cx.listener(|this, _, _, cx| this.voice_screen_clicked(cx)),
                                ),
                                cx,
                            ),
                        )
                        .child(
                            self.with_tip(
                                "call-screen-chevron-tip",
                                Tip::text("More Stream Options", Side::Top),
                                nub("call-screen-chevron", "--icon-voice-connected")
                                    .bg(active_fill)
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.vv.quality_open = false;
                                        this.vv.menu = if this.vv.menu == Some(Menu::Share) {
                                            None
                                        } else {
                                            Some(Menu::Share)
                                        };
                                        cx.notify();
                                    })),
                                cx,
                            ),
                        )
                        .into_any_element()
                } else {
                    self.with_tip(
                        "call-screen-tip",
                        Tip::text("Share Your Screen", Side::Top),
                        animated(
                            "call-screen",
                            &screen_id,
                            Anim::ScreenShare,
                            Some("enable"),
                            "--white",
                        )
                        .on_hover(lottie_hover(screen_id.clone(), cx))
                        .on_click(cx.listener(|this, _, _, cx| this.voice_screen_clicked(cx))),
                        cx,
                    )
                    .into_any_element()
                },
                self.with_tip(
                    "call-activity-tip",
                    Tip::text("Start An Activity", Side::Top),
                    // Module 168754: the hover events, and `play()` (the `click` marker) on click.
                    animated(
                        "call-activity",
                        &activity_id,
                        Anim::Activity,
                        None,
                        "--white",
                    )
                    .on_hover(lottie_hover(activity_id.clone(), cx))
                    .on_click({
                        let id = activity_id.clone();
                        cx.listener(move |this, _, window, cx| {
                            lottie_icon::click(&id);
                            this.voice_activity_clicked(window, cx);
                        })
                    }),
                    cx,
                )
                .into_any_element(),
                div()
                    .id("call-soundboard-hover")
                    .on_hover({
                        let id = soundboard_id.clone();
                        cx.listener(move |this, inside: &bool, _, cx| {
                            if *inside
                                || this.panel.popout != Some(super::voice_panel::Popout::Soundboard)
                            {
                                lottie_icon::hover(&id, *inside);
                            }
                            this.soundboard_hover(*inside, cx);
                        })
                    })
                    .child(self.with_tip(
                        "call-soundboard-tip",
                        Tip::text(soundboard_reason.unwrap_or("Open Soundboard"), Side::Top),
                        soundboard,
                        cx,
                    ))
                    .into_any_element(),
                self.with_tip(
                    "call-more-tip",
                    Tip::text("More Options", Side::Top),
                    more,
                    cx,
                )
                .into_any_element(),
            ]))
            .child({
                // `eT` of module 105225: Stop Watching while the focused tile is a stream of someone else that this
                // client watches, else Disconnect. On a change the button shrinks to 0.7 and fades to 0.2 (spring:
                // mass 2, tension 600, friction 45, clamped), swaps, and comes back (tension 1000).
                let watched = self.vv.focus.tile.and_then(|(user, kind)| {
                    (kind == TileKind::Stream
                        && self
                            .video_stream_of(user)
                            .is_some_and(|info| info.role == dc_voice::streams::Role::Watch))
                    .then_some(user)
                });
                let wanted = watched.is_some();
                let shown_stream = self.vv.end_shows_stream.get();
                let changing = wanted != shown_stream;
                let key = super::motion::key(&"call-end-swap");
                let mut config =
                    super::motion::SpringConfig::new(if changing { 600. } else { 1000. }, 45.);
                config.mass = 2.;
                config.clamp = changing;
                config.precision = Some(0.01);
                let t = super::motion::spring_value(key, if changing { 0. } else { 1. }, config);
                if changing && super::motion::settled(&[key]) {
                    self.vv.end_shows_stream.set(wanted);
                    cx.notify();
                }
                let scale = if super::motion::reduced() {
                    1.
                } else {
                    0.7 + 0.3 * t
                };
                let stream = self.vv.end_shows_stream.get();
                let others = self.watched_streams().len();
                let nub_shown = others > usize::from(stream);
                let button = div()
                    .id("call-disconnect")
                    .w(px(66. * scale))
                    .h(px(50. * scale))
                    .rounded(px(12. * scale))
                    .when(nub_shown, |this| this.rounded_r(px(0.)))
                    .opacity(0.2 + 0.8 * t)
                    .bg(mid("--control-critical-primary-background-default"))
                    .hover(|style| style.bg(mid("--control-critical-primary-background-hover")))
                    .active(|style| style.bg(mid("--control-critical-primary-background-active")))
                    .flex()
                    .items_center()
                    .justify_center()
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        // What the button shows decides, also while it is swapping.
                        if this.vv.end_shows_stream.get() {
                            if let Some(owner) = watched {
                                this.unwatch_stream_of(owner, cx);
                            }
                        } else {
                            this.leave_voice(cx);
                        }
                    }))
                    // Stop Watching is 958449 with `streamActive` (`useScreenshareLottie("disable")`), Disconnect
                    // 404355 (`useDisconnectLottie`, hover only); the swap mounts the other one afresh.
                    .on_hover(lottie_hover(
                        if stream {
                            stop_watching_id.clone()
                        } else {
                            disconnect_id.clone()
                        },
                        cx,
                    ))
                    .child({
                        let tint = color("--control-critical-primary-text-default");
                        if stream {
                            lottie_icon::forget(&disconnect_id);
                            lottie_icon::icon(
                                &stop_watching_id,
                                Anim::ScreenShare,
                                Some("disable"),
                                20. * scale,
                                tint,
                            )
                        } else {
                            lottie_icon::forget(&stop_watching_id);
                            lottie_icon::icon(
                                &disconnect_id,
                                Anim::Disconnect,
                                None,
                                20. * scale,
                                tint,
                            )
                        }
                    });
                // `onPopoutClick`: the chevron exists while other streams are watched, more than one beside Stop
                // Watching (module 105225: `N.length > 0`, `N.length > 1`).
                div()
                    .relative()
                    .h(px(50.))
                    .flex()
                    .flex_none()
                    .items_center()
                    .justify_center()
                    .gap(px(1.))
                    // The chevron is added first and shown last.
                    .flex_row_reverse()
                    .child(self.anchor_probe("call-end-group"))
                    .when(nub_shown, |this| {
                        this.child(
                            div()
                                .id("call-end-chevron")
                                .w(px(20.))
                                .h(px(50.))
                                .rounded_r(px(12.))
                                .bg(mid("--control-critical-primary-background-default"))
                                .hover(|style| {
                                    style.bg(mid("--control-critical-primary-background-hover"))
                                })
                                .flex()
                                .items_center()
                                .justify_center()
                                .cursor_pointer()
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.vv.menu = if this.vv.menu == Some(Menu::Watched) {
                                        None
                                    } else {
                                        Some(Menu::Watched)
                                    };
                                    cx.notify();
                                }))
                                .child(icon("voice_view_caret", 16., "--white")),
                        )
                    })
                    .child(self.with_tip(
                        "call-disconnect-tip",
                        Tip::text(
                            if stream {
                                "Stop Watching"
                            } else {
                                "Disconnect"
                            },
                            Side::Top,
                        ),
                        button,
                        cx,
                    ))
            });
        div()
            .absolute()
            .left(px(16.))
            .right(px(16.))
            .bottom(px(16.))
            .h(px(50.))
            .flex()
            .items_center()
            .when(!shown, |this| this.opacity(0.).invisible())
            .child(
                // A DM call has no Invite: its left slot is empty (Show/Hide chat only appears with video, module 156652).
                div()
                    .flex_1()
                    .flex()
                    .items_center()
                    // `eZ` of module 540462 in the left of the tray: "Hide chat" while a direct message's video
                    // call shares the column with the chat (the chevron points down), "Show chat" once it has all of
                    // it (it points up).
                    .when(guild == 0 && self.dm_call_can_hide_chat(), |this| {
                        let hidden = self.dm_call_chat_hidden();
                        this.child(
                            self.with_tip(
                                "call-chat-tip",
                                Tip::text(
                                    if hidden { "Show chat" } else { "Hide chat" },
                                    Side::Top,
                                ),
                                tray(
                                    "call-chat",
                                    if hidden {
                                        "clips_chevron_up"
                                    } else {
                                        "clips_chevron_down"
                                    },
                                )
                                .on_click(cx.listener(
                                    |this, _, _, cx| {
                                        this.tip = None;
                                        this.toggle_dm_call_chat(cx);
                                    },
                                )),
                                cx,
                            ),
                        )
                    })
                    .when(guild != 0, |this| {
                        this.child(
                            self.with_tip(
                                "call-invite-tip",
                                Tip::text("Invite", Side::Top),
                                tray("call-invite", "voice_view_invite")
                                    .on_click(cx.listener(|this, _, _, cx| this.open_invite(cx))),
                                cx,
                            ),
                        )
                    }),
            )
            .child(centre)
            .child(
                div()
                    .flex_1()
                    .flex()
                    .items_center()
                    .justify_end()
                    .gap(px(20.))
                    .children(self.call_volume_control(cx))
                    // `canStayOnTop`: in the popped-out window (module 619344).
                    .when(self.vv.popout_render, |this| {
                        let on = self.vv.popout_on_top;
                        this.child(
                            self.with_tip(
                                "call-stay-on-top-tip",
                                Tip::text(
                                    if on { "Remove From Top" } else { "Stay On Top" },
                                    Side::Top,
                                ),
                                tray(
                                    "call-stay-on-top",
                                    if on {
                                        "voice_view_pin_upright_slash"
                                    } else {
                                        "voice_view_pin_upright"
                                    },
                                )
                                .on_click(
                                    cx.listener(|this, _, _, cx| {
                                        this.toggle_call_popout_on_top(cx)
                                    }),
                                ),
                                cx,
                            ),
                        )
                    })
                    // Module 540462: Pop Out and Full Screen only exist in video mode (`mode === VIDEO && inCall`),
                    // so a DM's voice call has neither (golden `dm-call/in-call-alone`).
                    .when(guild != 0 || self.dmc.video, |this| {
                        this.child(self.with_tip(
                            "call-popout-tip",
                            Tip::text("Pop Out", Side::Top),
                            tray("call-popout", "voice_view_popout").on_click(
                                cx.listener(|this, _, _, cx| this.toggle_call_popout(cx)),
                            ),
                            cx,
                        ))
                        .child(self.with_tip(
                            "call-fullscreen-tip",
                            Tip::text(
                                if self.vv.fullscreen {
                                    "Exit Full Screen"
                                } else {
                                    "Full Screen"
                                },
                                Side::Top,
                            ),
                            tray("call-fullscreen", "voice_view_fullscreen").on_click(cx.listener(
                                |this, _, window, cx| this.toggle_call_fullscreen(window, cx),
                            )),
                            cx,
                        ))
                    }),
            )
            .into_any_element()
    }

    // ---- menus ----

    pub(super) fn menu_panel(&self, width: f32, theme_mid: bool) -> gpui::Div {
        let panel = div()
            .occlude()
            .w(px(width))
            .p(px(8.))
            .rounded(px(8.))
            .border_1()
            .shadow(vec![gpui::BoxShadow {
                color: gpui::hsla(0., 0., 0., 0.24),
                offset: point(px(0.), px(12.)),
                blur_radius: px(24.),
                spread_radius: px(0.),
                inset: false,
            }]);
        if theme_mid {
            panel
                .border_color(mid("--border-subtle"))
                .bg(mid("--background-surface-higher"))
        } else {
            panel
                .border_color(color("--border-subtle"))
                .bg(color("--background-surface-higher"))
        }
    }

    pub(super) fn menu_separator(theme_mid: bool) -> AnyElement {
        div()
            .my(px(8.))
            .h(px(1.))
            .bg(if theme_mid {
                mid("--border-subtle")
            } else {
                color("--border-subtle")
            })
            .into_any_element()
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn vmenu_row(
        &self,
        id: &'static str,
        glyph: Option<&'static str>,
        label: &'static str,
        checked: Option<bool>,
        theme_mid: bool,
        cx: &mut Context<Self>,
        run: impl Fn(&mut Root, &mut Window, &mut Context<Root>) + 'static,
    ) -> AnyElement {
        self.vmenu_row_ex(id, glyph, label, checked, theme_mid, false, false, cx, run)
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn vmenu_row_ex(
        &self,
        id: impl Into<gpui::ElementId>,
        glyph: Option<&'static str>,
        label: impl Into<SharedString>,
        checked: Option<bool>,
        theme_mid: bool,
        danger: bool,
        caret: bool,
        cx: &mut Context<Self>,
        run: impl Fn(&mut Root, &mut Window, &mut Context<Root>) + 'static,
    ) -> AnyElement {
        let (strong, subtle, hover) = if theme_mid {
            (
                mid("--text-strong"),
                "--interactive-text-default",
                mid("--interactive-background-hover"),
            )
        } else {
            (
                color("--text-strong"),
                "--interactive-text-default",
                color("--interactive-background-hover"),
            )
        };
        let strong = if danger {
            color("--text-feedback-critical")
        } else {
            strong
        };
        let subtle = if danger {
            "--text-feedback-critical"
        } else {
            subtle
        };
        div()
            .id(id)
            .h(px(36.))
            .p(px(8.))
            .rounded(px(2.))
            .cursor_pointer()
            .flex()
            .items_center()
            .hover(move |style| style.bg(hover).rounded(px(4.)))
            .on_click(cx.listener(move |this, _, window, cx| {
                run(this, window, cx);
                cx.notify();
            }))
            .children(glyph.map(|glyph| div().mr(px(8.)).child(icon(glyph, 20., subtle))))
            .child(
                div()
                    .flex_1()
                    .text_size(px(14.))
                    .line_height(px(18.))
                    .font_weight(FontWeight(500.))
                    .text_color(strong)
                    .child(label.into()),
            )
            .when(caret, |this| {
                this.child(icon("menu_caret_right", 20., "--interactive-icon-default"))
            })
            .children(checked.map(|on| {
                div()
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
                        this.child(icon("checkbox_check", 18., "--white"))
                    })
            }))
            .into_any_element()
    }

    /// The More Options menu (module M3qk4j) and a tile's Options menu.
    pub(super) fn voice_menus(
        &self,
        channel: Id,
        width: f32,
        height: f32,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let Some(menu) = self.vv.menu else {
            return Vec::new();
        };
        let close = |this: &mut Root, cx: &mut Context<Root>| {
            this.vv.menu = None;
            cx.notify();
        };
        let _ = channel;
        match menu {
            Menu::More => {
                let Some(anchor) = self.tip_anchors.borrow().get("call-more").copied() else {
                    return Vec::new();
                };
                let rows = vec![
                    self.vmenu_row(
                        "vmenu-invite",
                        Some("voice_view_invite_menu"),
                        "Invite to Voice",
                        None,
                        true,
                        cx,
                        |this, _, cx| {
                            this.vv.menu = None;
                            this.open_invite(cx);
                        },
                    ),
                    Self::menu_separator(true),
                    self.vmenu_row(
                        "vmenu-grid",
                        None,
                        "Grid View",
                        Some(self.vv.focus.tile.is_none()),
                        true,
                        cx,
                        |this, _, _| {
                            // Checked: the grid shows. Unchecking it focuses the first tile (a stream sorts first).
                            if this.vv.focus.tile.is_some() {
                                this.vv.focus.grid();
                            } else if let (Some(guild), Some(channel)) = (this.guild, this.channel)
                                && let Some(first) =
                                    this.participants(guild, channel).into_iter().next()
                            {
                                this.vv.focus.click(first.tile.user, first.tile.kind);
                            }
                            set_call_setting(GRID_VIEW, this.vv.focus.tile.is_none());
                        },
                    ),
                    self.vmenu_row(
                        "vmenu-own-camera",
                        None,
                        "Show My Own Camera",
                        Some(call_setting(SHOW_OWN_CAMERA)),
                        true,
                        cx,
                        |_, _, _| toggle_call_setting(SHOW_OWN_CAMERA),
                    ),
                    self.vmenu_row(
                        "vmenu-non-video",
                        None,
                        "Show Non-Video Participants",
                        Some(call_setting(SHOW_NON_VIDEO)),
                        true,
                        cx,
                        |_, _, _| toggle_call_setting(SHOW_NON_VIDEO),
                    ),
                    self.vmenu_row(
                        "vmenu-previews",
                        None,
                        "Show New Message Previews",
                        Some(call_setting(MESSAGE_PREVIEWS)),
                        true,
                        cx,
                        |_, _, _| toggle_call_setting(MESSAGE_PREVIEWS),
                    ),
                    Self::menu_separator(true),
                    self.vmenu_row(
                        "vmenu-settings",
                        Some("voice_view_gear"),
                        "Voice & Video Settings",
                        None,
                        true,
                        cx,
                        |this, _, cx| {
                            this.vv.menu = None;
                            this.open_settings_section("Voice & Video", cx);
                        },
                    ),
                ];
                // Right edges meet, 16px above the button; the panel is 268px tall.
                let (ox, oy) = self.call_origin();
                let left = f32::from(anchor.right()) - MORE_WIDTH - ox;
                // Over the button when there is room, under it in a call area at the top of the window.
                let top = super::audio_menu::popout_top(
                    f32::from(anchor.top()),
                    f32::from(anchor.bottom()),
                    268.,
                    16.,
                    self.viewport.1,
                ) - oy;
                vec![
                    deferred(
                        div().absolute().left(px(left)).top(px(top)).child(
                            self.menu_panel(MORE_WIDTH, true)
                                .id("voice-more-menu")
                                .popout("voice-more-menu", PopKind::Fade, PopSide::Top)
                                .on_mouse_down_out(cx.listener(move |this, _, _, cx| {
                                    close(this, cx);
                                }))
                                .children(rows),
                        ),
                    )
                    .with_priority(2)
                    .into_any_element(),
                ]
            }
            Menu::Tile(user, at) => {
                let me = self.stores.current_user.as_ref().map(|user| user.id);
                let own = Some(user) == me;
                let mut rows = vec![
                    self.vmenu_row(
                        "vmenu-profile",
                        Some("voice_view_profile"),
                        "View Profile",
                        None,
                        false,
                        cx,
                        move |this, window, cx| {
                            this.vv.menu = None;
                            this.open_full_profile(user, window, cx);
                        },
                    ),
                    Self::menu_separator(false),
                ];
                if own {
                    rows.push(self.vmenu_row(
                        "vmenu-mute",
                        None,
                        "Mute",
                        Some(self.self_mute || self.self_deaf),
                        false,
                        cx,
                        |this, _, cx| this.toggle_self_mute(cx),
                    ));
                    rows.push(self.vmenu_row(
                        "vmenu-deafen",
                        None,
                        "Deafen",
                        Some(self.self_deaf),
                        false,
                        cx,
                        |this, _, cx| this.toggle_self_deaf(cx),
                    ));
                    rows.push(self.vmenu_row(
                        "vmenu-own-camera",
                        None,
                        "Show My Own Camera",
                        Some(call_setting(SHOW_OWN_CAMERA)),
                        false,
                        cx,
                        |_, _, _| toggle_call_setting(SHOW_OWN_CAMERA),
                    ));
                    // `Preview Camera` (module 855187): with a camera to preview and video off.
                    if !self.call.as_ref().is_some_and(|call| call.camera)
                        && !dc_voice::camera_devices().is_empty()
                    {
                        rows.push(self.vmenu_row(
                            "vmenu-preview-camera",
                            Some("voice_view_preview"),
                            "Preview Camera",
                            None,
                            false,
                            cx,
                            |this, _, cx| {
                                this.vv.menu = None;
                                this.open_camera_preview(cx);
                            },
                        ));
                    }
                }
                rows.push(self.vmenu_row(
                    "vmenu-settings",
                    Some("voice_view_gear"),
                    "Voice & Video Settings",
                    None,
                    false,
                    cx,
                    |this, _, cx| {
                        this.vv.menu = None;
                        this.open_settings_section("Voice & Video", cx);
                    },
                ));
                let rows_height = 36. * (rows.len() as f32 - 1.) + 17. + 18.;
                // Opens at the pointer, 16px from the window's edges.
                let (ox, oy) = self.call_origin();
                let window_w = width + ox;
                let window_h = height + oy;
                let left = f32::from(at.x).min(window_w - 16. - TILE_MENU_WIDTH);
                let top = f32::from(at.y).min(window_h - 16. - rows_height);
                vec![
                    deferred(
                        div()
                            .absolute()
                            .left(px(left - ox))
                            .top(px(top - oy))
                            .child(
                                self.menu_panel(TILE_MENU_WIDTH, false)
                                    .id("voice-tile-menu")
                                    .popout("voice-tile-menu", PopKind::Fade, PopSide::Top)
                                    .on_mouse_down_out(cx.listener(move |this, _, _, cx| {
                                        close(this, cx);
                                    }))
                                    .children(rows),
                            ),
                    )
                    .with_priority(2)
                    .into_any_element(),
                ]
            }
            Menu::Camera => self.camera_menu(cx),
            Menu::Share => self.share_menu(cx),
            Menu::Watched => self.watched_menu(cx),
            Menu::Stream(owner, at) => {
                let me = self.stores.current_user.as_ref().map(|user| user.id);
                let info = self.video_stream_of(owner);
                let watching = info
                    .as_ref()
                    .is_some_and(|info| info.role == dc_voice::streams::Role::Watch);
                let mut rows = Vec::new();
                if Some(owner) == me {
                    rows.push(self.vmenu_row(
                        "vmenu-stop-stream",
                        None,
                        "Stop Streaming",
                        None,
                        false,
                        cx,
                        |this, _, cx| {
                            this.vv.menu = None;
                            this.stream_stop(cx);
                        },
                    ));
                } else if watching {
                    rows.push(self.vmenu_row(
                        "vmenu-stop-watching",
                        None,
                        "Stop Watching",
                        None,
                        false,
                        cx,
                        move |this, _, cx| {
                            this.vv.menu = None;
                            this.unwatch_stream_of(owner, cx);
                        },
                    ));
                } else {
                    rows.push(self.vmenu_row(
                        "vmenu-watch-stream",
                        None,
                        "Watch Stream",
                        None,
                        false,
                        cx,
                        move |this, _, cx| {
                            this.vv.menu = None;
                            this.watch_stream_of(owner, cx);
                        },
                    ));
                }
                let mut rows_height = 36. * rows.len() as f32 + 16.;
                if watching && Some(owner) != me {
                    // `stream-context`: Mute, then Stream Volume, then Stream Attenuation with its strength.
                    rows.push(self.vmenu_row(
                        "vmenu-stream-mute",
                        None,
                        "Mute",
                        Some(super::stream_volume::muted(owner)),
                        false,
                        cx,
                        move |this, _, _| {
                            super::stream_volume::set_muted(
                                owner,
                                !super::stream_volume::muted(owner),
                            );
                            this.apply_stream_volume(owner);
                        },
                    ));
                    rows.push(Self::menu_separator(false));
                    rows.push(self.stream_volume_row(owner, cx));
                    rows.push(Self::menu_separator(false));
                    rows.push(self.stream_attenuation_rows(cx));
                    rows.push(Self::menu_separator(false));
                    let attenuation = self.stream_attenuation_on();
                    // The mute row and a separator, the volume row, a separator, and the attenuation rows.
                    rows_height +=
                        36. + 18. + 58. + 18. + 36. + if attenuation { 58. } else { 0. } + 18.;
                }
                rows.push(self.vmenu_row(
                    "vmenu-stream-settings",
                    Some("voice_view_gear"),
                    "Voice & Video Settings",
                    None,
                    false,
                    cx,
                    |this, _, cx| {
                        this.vv.menu = None;
                        this.open_settings_section("Voice & Video", cx);
                    },
                ));
                let (ox, oy) = self.call_origin();
                let window_w = width + ox;
                let window_h = height + oy;
                let left = f32::from(at.x).min(window_w - 16. - TILE_MENU_WIDTH);
                let top = f32::from(at.y).min(window_h - 16. - rows_height);
                vec![
                    deferred(
                        div()
                            .absolute()
                            .left(px(left - ox))
                            .top(px(top - oy))
                            .child(
                                self.menu_panel(TILE_MENU_WIDTH, false)
                                    .id("voice-stream-menu")
                                    .on_mouse_down_out(cx.listener(move |this, _, _, cx| {
                                        close(this, cx);
                                    }))
                                    .children(rows),
                            ),
                    )
                    .with_priority(2)
                    .into_any_element(),
                ]
            }
        }
    }

    /// The call-bar flavour of the microphone chevron's menu.
    fn toggle_call_audio_menu(&mut self, cx: &mut Context<Self>) {
        self.toggle_audio_menu_at("call-mic-chevron", cx);
    }

    // ---- sidebar ----

    /// `220995.js`: under the members of the call this client is in, "Invite to Voice" with a ring timer; it leaves
    /// by itself after ten seconds, or with its X.
    pub(super) fn voice_invite_row(
        &self,
        guild: Id,
        channel: Id,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let call = self.call.as_ref()?;
        let since = call.connected_at?;
        if call.guild != guild
            || call.channel != channel
            || self.vv.invite_dismissed
            || since.elapsed() >= INVITE_ROW_TIMEOUT
        {
            return None;
        }
        let allowed = self.current_guild().is_some_and(|known| {
            known
                .channels
                .iter()
                .find(|known| known.id == channel)
                .is_some_and(|found| {
                    self.can_in_channel(found, dc_state::permissions::CREATE_INSTANT_INVITE)
                })
        });
        if !allowed {
            return None;
        }
        Some(
            div()
                .id("voice-invite-row")
                .group("voice-invite-row")
                .flex_none()
                .h(px(32.))
                .pl(px(44.))
                .pr(px(17.))
                .flex()
                .items_center()
                .gap(px(8.))
                .cursor_pointer()
                .on_click(cx.listener(|this, _, _, cx| this.open_invite(cx)))
                .child(
                    div()
                        .relative()
                        .size(px(24.))
                        .flex_none()
                        .child(
                            img(self.invite_ring(
                                since.elapsed().as_secs_f32() / INVITE_ROW_TIMEOUT.as_secs_f32(),
                            ))
                            .absolute()
                            .size(px(24.)),
                        )
                        .child(
                            div()
                                .absolute()
                                .inset_0()
                                .flex()
                                .items_center()
                                .justify_center()
                                .child(icon("voice_view_invite_button", 14., "--channels-default")),
                        ),
                )
                .child(
                    div()
                        .flex_1()
                        .text_size(px(14.))
                        .line_height(px(18.))
                        .font_weight(FontWeight(500.))
                        .text_color(color("--channels-default"))
                        .child("Invite to Voice"),
                )
                .child(
                    div()
                        .id("voice-invite-dismiss")
                        .size(px(16.))
                        .child(
                            div()
                                .group_hover("voice-invite-row", |style| style.opacity(0.))
                                .child(icon("menu_caret", 16., "--channels-default")),
                        )
                        .child(
                            div()
                                .absolute()
                                .opacity(0.)
                                .group_hover("voice-invite-row", |style| style.opacity(1.))
                                .child(icon("close", 16., "--channels-default")),
                        )
                        .on_click(cx.listener(|this, _, _, cx| {
                            cx.stop_propagation();
                            this.vv.invite_dismissed = true;
                            cx.notify();
                        })),
                )
                .into_any_element(),
        )
    }

    /// `timer_MRjIbW`: a 2px ring of `--border-normal` with `--border-strong` over it, a dash of 24 pi that the
    /// animation walks off the path from the top, clockwise, as the ten seconds pass. Drawn per tenth of a second.
    fn invite_ring(&self, progress: f32) -> std::sync::Arc<gpui::Image> {
        let step = (progress.clamp(0., 1.) * 100.).round() as u32;
        let key = format!("voice-invite-ring:{step}:{:?}", crate::ui::theme() as u8);
        if let Some(found) = self.generated_images.borrow().get(&key) {
            return found.clone();
        }
        let hex = |name: &str| {
            let value = color(name);
            let byte = |channel: f32| (channel * 255.).round() as u8;
            format!(
                "#{:02x}{:02x}{:02x}",
                byte(value.r),
                byte(value.g),
                byte(value.b)
            )
        };
        let dash = 24. * std::f32::consts::PI;
        let offset = -(step as f32 / 100.) * dash;
        let svg = format!(
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24"><g transform="rotate(-90 12 12)" fill="none" stroke-width="2"><circle cx="12" cy="12" r="11" stroke="{}"/><circle cx="12" cy="12" r="11" stroke="{}" stroke-dasharray="{dash}" stroke-dashoffset="{offset}"/></g></svg>"#,
            hex("--border-normal"),
            hex("--border-strong"),
        );
        let made = std::sync::Arc::new(gpui::Image::from_bytes(
            gpui::ImageFormat::Svg,
            svg.into_bytes(),
        ));
        self.generated_images.borrow_mut().insert(key, made.clone());
        made
    }

    // ---- not in the call ----

    /// `484191.js`: the channel's name and who is in it, over the blue gradient, with Join Voice.
    fn disconnected_view(
        &self,
        guild: Id,
        channel: Id,
        width: f32,
        height: f32,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let name = self
            .current_channel()
            .map(|channel| channel.name.clone())
            .unwrap_or_default();
        let people: Vec<User> = self
            .stores
            .voice_members(guild, channel)
            .into_iter()
            .map(|state| self.user_of(state.user_id))
            .collect();
        let allowed = self
            .current_channel()
            .is_none_or(|channel| self.can_in_channel(channel, CONNECT));
        let avatars = people.iter().take(5).map(|user| {
            let picture = self.images.get(&super::avatar_source(user).0).cloned();
            div()
                .size(px(48.))
                .rounded_full()
                .overflow_hidden()
                .children(picture.map(|picture| img(picture).size(px(48.)).rounded_full()))
        });
        let shown = self.controls_shown();
        div()
            .absolute()
            .inset_0()
            .child(
                img(self.disconnected_background(width, height))
                    .absolute()
                    .inset_0()
                    .size_full(),
            )
            .child(self.call_header(channel, shown, None, cx))
            .child(
                div()
                    .absolute()
                    .left_0()
                    .right_0()
                    .top(px(if people.is_empty() { 293. } else { 245. }))
                    .flex()
                    .flex_col()
                    .items_center()
                    .child(
                        div()
                            .flex()
                            .gap(px(8.))
                            .when(!people.is_empty(), |this| this.mb(px(16.)))
                            .children(avatars),
                    )
                    .child(
                        div()
                            .text_size(px(32.))
                            .line_height(px(40.))
                            .font_weight(FontWeight(600.))
                            .text_color(mid("--text-strong"))
                            .child(name),
                    )
                    .child(
                        div()
                            .mt(px(8.))
                            .text_size(px(14.))
                            .line_height(px(18.))
                            .text_color(mid("--text-strong"))
                            .child(names(&people)),
                    )
                    .child(
                        div()
                            .id("voice-join")
                            .mt(px(24.))
                            .h(px(40.))
                            .px(px(16.))
                            .rounded(px(8.))
                            .flex()
                            .items_center()
                            .text_size(px(16.))
                            .line_height(px(20.))
                            .font_weight(FontWeight(500.))
                            .bg(mid("--control-overlay-primary-background-default"))
                            .text_color(mid("--control-overlay-primary-text-default"))
                            .when(!allowed, |this| this.opacity(0.5))
                            .when(allowed, |this| {
                                this.cursor_pointer().on_click(cx.listener(
                                    move |this, _, _, cx| {
                                        this.request_join_voice(guild, channel, cx)
                                    },
                                ))
                            })
                            .child("Join Voice"),
                    ),
            )
            .into_any_element()
    }

    /// The call container's blue glow while no call is joined. The official one is a canvas of blurred spheres
    /// (`pulseGradient_BUFBam`), so there is no CSS to port: this is its still frame, the centre and edge colour of
    /// each row measured in `voice-view-disconnected-darker-cozy`, spread sideways as a gaussian.
    fn disconnected_background(&self, width: f32, height: f32) -> std::sync::Arc<gpui::Image> {
        // Row (from the view's top), centre colour, colour at the left and right edge.
        const ROWS: [(f32, [f32; 3], [f32; 3]); 17] = [
            (0., [16., 16., 36.], [13., 13., 21.]),
            (43., [18., 18., 44.], [16., 16., 27.]),
            (86., [20., 20., 51.], [16., 16., 31.]),
            (129., [23., 23., 61.], [17., 17., 37.]),
            (172., [24., 25., 68.], [18., 18., 42.]),
            (215., [28., 28., 78.], [20., 20., 50.]),
            (258., [29., 31., 88.], [21., 21., 55.]),
            (301., [34., 35., 104.], [23., 24., 63.]),
            (344., [36., 37., 117.], [25., 26., 69.]),
            (387., [39., 42., 130.], [28., 29., 78.]),
            (430., [42., 47., 144.], [29., 30., 83.]),
            (473., [48., 51., 154.], [31., 32., 89.]),
            (516., [50., 55., 166.], [33., 34., 94.]),
            (559., [56., 61., 177.], [35., 36., 100.]),
            (602., [57., 64., 184.], [35., 38., 104.]),
            (645., [62., 69., 192.], [37., 40., 110.]),
            (687., [66., 75., 199.], [39., 42., 115.]),
        ];
        let (w, h) = (width.round().max(1.) as u32, height.round().max(1.) as u32);
        let key = format!("voice-disconnected:{w}x{h}");
        if let Some(found) = self.generated_images.borrow().get(&key) {
            return found.clone();
        }
        let sigma = 300. * width / 905.;
        let edge = (-0.5 * (width / 2. / sigma).powi(2)).exp();
        let mut pixels = image::RgbaImage::new(w, h);
        for (x, y, pixel) in pixels.enumerate_pixels_mut() {
            let row = (y as f32 + 0.5) / h as f32 * 687.;
            let at = ROWS.partition_point(|(top, _, _)| *top <= row).max(1) - 1;
            let (a, b) = (ROWS[at], ROWS[(at + 1).min(ROWS.len() - 1)]);
            let t = if b.0 > a.0 {
                ((row - a.0) / (b.0 - a.0)).clamp(0., 1.)
            } else {
                0.
            };
            let dx = (x as f32 + 0.5 - width / 2.).abs();
            let across = (((-0.5 * (dx / sigma).powi(2)).exp() - edge) / (1. - edge)).clamp(0., 1.);
            let mut out = [0u8; 3];
            for (i, slot) in out.iter_mut().enumerate() {
                let centre = a.1[i] + (b.1[i] - a.1[i]) * t;
                let side = a.2[i] + (b.2[i] - a.2[i]) * t;
                *slot = (side + (centre - side) * across).round() as u8;
            }
            *pixel = image::Rgba([out[0], out[1], out[2], 255]);
        }
        let mut bytes = Vec::new();
        let _ = image::ImageEncoder::write_image(
            image::codecs::png::PngEncoder::new(&mut bytes),
            pixels.as_raw(),
            w,
            h,
            image::ExtendedColorType::Rgba8,
        );
        let made = std::sync::Arc::new(gpui::Image::from_bytes(gpui::ImageFormat::Png, bytes));
        self.generated_images.borrow_mut().insert(key, made.clone());
        made
    }

    /// The channel's own messages and composer, docked over the call's right edge (module 518530): 450px with a
    /// pixel of border, a 49px header with the chat icon, the name and a close button.
    fn voice_chat_panel(&self, _guild: Id, channel: Id, cx: &mut Context<Self>) -> AnyElement {
        let name = self
            .current_channel()
            .map(|channel| channel.name.clone())
            .unwrap_or_default();
        let rows = gpui::list(
            self.list.clone(),
            cx.processor(|this, index, _, cx| this.row(index, cx)),
        )
        .size_full();
        div()
            .absolute()
            .top_0()
            .right_0()
            .bottom_0()
            .w(px(CHAT_WIDTH))
            .flex()
            .flex_col()
            .occlude()
            .rounded_tl(px(12.))
            .border_l_1()
            .border_t_1()
            .border_color(color("--background-mod-subtle"))
            .bg(color("--background-base-lower"))
            .child(
                div()
                    .flex_none()
                    .h(px(49.))
                    .pl(px(15.))
                    .pr(px(9.))
                    .flex()
                    .items_center()
                    .gap(px(9.))
                    .border_b_1()
                    .border_color(color("--background-mod-subtle"))
                    .text_size(px(16.))
                    .line_height(px(20.))
                    .font_weight(FontWeight(500.))
                    .text_color(color("--text-strong"))
                    .child(icon("voice_view_chat", 20., "--channels-default"))
                    .child(div().flex_1().child(name))
                    .child(
                        div()
                            .id("voice-chat-close")
                            .size(px(32.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .cursor_pointer()
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.vv.chat_open.remove(&channel);
                                cx.notify();
                            }))
                            .child(icon("close", 16., "--interactive-icon-default")),
                    ),
            )
            .child(div().flex_1().min_h(px(0.)).relative().child(rows))
            .child(self.composer_bar(cx))
            .into_any_element()
    }

    /// The first row of a voice channel's chat: "Welcome to General!".
    /// Whether the call view currently shows the docked chat.
    pub(super) fn voice_chat_docked(&self) -> bool {
        self.voice_view_open()
            && self.call.as_ref().is_some_and(|call| {
                self.current_channel().is_some_and(|c| c.id == call.channel)
                    && self.vv.chat_open.contains(&call.channel)
            })
    }

    pub(super) fn voice_welcome(&self) -> Option<AnyElement> {
        let channel = self.current_channel()?;
        let name = channel.name.clone();
        Some(
            div()
                .pt(px(12.))
                .child(
                    div()
                        .ml(px(16.))
                        .mr(px(32.))
                        .flex()
                        .flex_col()
                        .child(
                            div()
                                .size(px(68.))
                                .rounded_full()
                                .bg(rgba(0x97979f14))
                                .flex()
                                .items_center()
                                .justify_center()
                                .child(icon("voice_view_chat", 42., "--text-strong")),
                        )
                        .child(
                            div()
                                .mt(px(8.))
                                .text_size(px(32.))
                                .line_height(px(40.))
                                .font_weight(FontWeight(700.))
                                .text_color(color("--text-strong"))
                                .child(format!("Welcome to {name}!")),
                        )
                        .child(
                            div()
                                .mt(px(8.))
                                .text_size(px(16.))
                                .line_height(px(22.))
                                .text_color(color("--text-default"))
                                .child(format!("This is the start of the {name} channel.")),
                        )
                        .child(
                            div().mt(px(12.)).flex().child(
                                div()
                                    .id("voice-edit-channel")
                                    .h(px(32.))
                                    .px(px(11.))
                                    .rounded(px(8.))
                                    .bg(rgba(0x97979f1f))
                                    .flex()
                                    .items_center()
                                    .gap(px(4.))
                                    .cursor_pointer()
                                    .text_size(px(14.))
                                    .line_height(px(18.))
                                    .font_weight(FontWeight(500.))
                                    .text_color(color("--text-strong"))
                                    .child(icon("settings_edit_pencil", 16., "--text-strong"))
                                    .child("Edit Channel"),
                            ),
                        ),
                )
                .into_any_element(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_row_centred_on_a_half_pixel_starts_on_the_next_whole_one() {
        // The golden has the 490px tile at x 582.5 of the 905px view at 375: 207.5 in, painted at 583.
        assert_eq!(row_indent(905., 490.), 200.);
        assert_eq!(375. + 8. + row_indent(905., 490.), 583.);
        // A whole-pixel centre stays, and a row wider than the view starts at the padding.
        assert_eq!(row_indent(906., 490.), 200.);
        assert_eq!(row_indent(300., 490.), 0.);
    }

    #[test]
    fn two_tiles_in_the_captured_view_are_490_wide() {
        let layout = grid_layout(2, 905., 688.);
        assert_eq!(layout.columns, 1);
        assert_eq!(layout.rows, 2);
        assert_eq!(layout.tile_width, 490.);
        assert!((layout.tile_height - 275.625).abs() < 1e-3);
        assert_eq!(layout.pad_top, 64.);
    }

    /// Every tile fits inside the padded area, all participants get a cell, and one more pixel of width would not fit.
    fn assert_fits(count: usize, width: f32, height: f32) -> GridLayout {
        let layout = grid_layout(count, width, height);
        let area_w = width - 16.;
        let area_h = height - GRID_PAD * 2.;
        assert!(layout.columns * layout.rows >= count, "{count} cells");
        let used_w =
            layout.columns as f32 * layout.tile_width + (layout.columns - 1) as f32 * TILE_GAP;
        assert!(used_w <= area_w, "{count} wide: {used_w} > {area_w}");
        let used_h = layout.rows as f32 * (layout.tile_height.floor() + TILE_GAP) - TILE_GAP;
        assert!(used_h <= area_h + 1., "{count} high: {used_h} > {area_h}");
        assert!((layout.tile_height - layout.tile_width / ASPECT).abs() < 1e-3);
        layout
    }

    #[test]
    fn grid_fits_one_to_nine_participants_at_1280x720() {
        let expected = [(1, 1, 1), (2, 1, 2), (3, 2, 2), (4, 2, 2), (9, 3, 3)];
        for (count, columns, rows) in expected {
            let layout = assert_fits(count, 905., 688.);
            assert_eq!(
                (layout.columns, layout.rows),
                (columns, rows),
                "{count} tiles"
            );
        }
    }

    #[test]
    fn grid_fits_after_the_window_is_resized() {
        for count in [1, 2, 3, 4, 9] {
            assert_fits(count, 1225., 868.);
            assert_fits(count, 640., 400.);
        }
        // A bigger window never makes the tiles smaller.
        assert!(grid_layout(4, 1225., 868.).tile_width >= grid_layout(4, 905., 688.).tile_width);
    }
}
