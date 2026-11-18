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
    AnyElement, Bounds, Context, FontWeight, Hsla, IntoElement, MouseMoveEvent, PathBuilder,
    Pixels, Point, Rgba, SharedString, Window, canvas, deferred, div, img, linear_color_stop,
    linear_gradient, point, prelude::*, px, rgba,
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

/// The four checkboxes of the More Options menu; all on until the person changes them.
pub(super) fn call_setting(key: &str) -> bool {
    storage::get(key)
        .and_then(|value| value.as_bool())
        .unwrap_or(true)
}

pub(super) fn set_call_setting(key: &str, on: bool) {
    storage::set(key, serde_json::json!(on));
}

pub(super) fn toggle_call_setting(key: &str) {
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

/// The notice over the sharer's own tile while no window of the client is focused (modules 638480, 919706, 200749:
/// `root_eYyGVJ` over the tile, a `body-md/semibold` header 4px above a `body-sm/medium` line, 4px of padding).
fn own_preview_paused() -> AnyElement {
    div()
        .absolute()
        .inset_0()
        .bg(gpui::hsla(0., 0., 0., 0.6))
        .p(px(4.))
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .text_center()
        .child(
            div()
                .mb(px(4.))
                .text_size(px(16.))
                .line_height(px(20.))
                .font_weight(FontWeight(600.))
                .text_color(mid("--white"))
                .child("Your stream is still running!"),
        )
        .child(
            div()
                .text_size(px(14.))
                .line_height(px(18.))
                .font_weight(FontWeight(500.))
                .text_color(mid("--primary-200"))
                .child("We\u{2019}ve paused this preview to save your resources."),
        )
        .into_any_element()
}

/// The wandering-cubes spinner of a tile that waits for its picture (module 259264): two cubes in a 32px box.
pub(super) fn spinner() -> AnyElement {
    div()
        .absolute()
        .inset_0()
        .size_full()
        .flex()
        .items_center()
        .justify_center()
        .child(super::login::wandering_cubes())
        .into_any_element()
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Menu {
    More,
    /// The chevron of the tray's red button: the watched streams of others, each with Stop Watching.
    Watched,
    /// "Disconnect Actions" (module 984624 `exit-options`): End Event.
    ExitOptions,
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
    /// The pointer left the call area (`onForceIdle`).
    pub forced_idle: bool,
    /// A task is waiting for the idle time to run out.
    watching: bool,
    /// Identifiers of call control regions the pointer is currently over.
    pub hovered_controls: HashSet<&'static str>,
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
    /// Bumped by every enter and leave of the button or the popup: a pending show or hide that finds another
    /// number was cancelled.
    pub suggest_timer: u64,
    /// The focused tile, if any (`selectedParticipant`).
    pub focus: video_tiles::Focus,
    /// Full Screen: the call fills the window and the window fills the screen.
    pub fullscreen: bool,
    /// "Hide Members" of the focus mode (`participantsOpen` is false).
    pub members_hidden: bool,
    /// The call header's Participants popout (module 195007) is open.
    pub participants_open: bool,
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
    /// `controlsBottom` of the focused tile (module 699707): how far the call bar reaches into it.
    pub controls_bottom: std::cell::Cell<f32>,
    /// The call view is being built for a call this account has not joined (`inCall` is false): every participant
    /// has a tile, none can be focused, and the bar and its menus are left to the join buttons.
    pub outside: bool,
    /// The call-bar control the last press landed on, and whether its popup was open at that press (read in the
    /// capture phase, before a popup's press-outside closes it), so its click closes instead of reopening.
    pub opener: std::cell::Cell<Option<(&'static str, bool)>>,
    /// Where the last press on the call bar was: a popup closed by that press lets it through to the control.
    pub bar_press: std::cell::Cell<Option<Point<Pixels>>>,
    /// The open right-click or Options menu of a tile (`call_menus.rs`).
    pub tile_menu: Option<super::call_menus::Open>,
    /// The notices of the call (`notice_bar.rs`): "Your voice has been disconnected ...".
    pub notices: super::notice_bar::State,
    /// Dominant colour of an avatar by its image key, decoded once.
    colors: RefCell<HashMap<String, Option<[u8; 3]>>>,
    pub(super) background: super::call_background::State,
}

impl Default for State {
    fn default() -> Self {
        Self {
            chat_open: HashSet::new(),
            last_move: Instant::now(),
            forced_idle: false,
            watching: false,
            ticking: false,
            menu: None,
            invite_dismissed: false,
            row_focus: false,
            quality_open: false,
            suggest: false,
            suggest_timer: 0,
            focus: Default::default(),
            fullscreen: false,
            members_hidden: false,
            participants_open: false,
            popout: None,
            popout_render: false,
            popout_on_top: false,
            outside: false,
            end_shows_stream: std::cell::Cell::new(false),
            controls_bottom: std::cell::Cell::new(0.),
            shown_call: (0, 0),
            opener: std::cell::Cell::new(None),
            bar_press: std::cell::Cell::new(None),
            tile_menu: None,
            notices: Default::default(),
            colors: RefCell::new(HashMap::new()),
            background: Default::default(),
            hovered_controls: HashSet::new(),
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

/// The quality half of a stream's live pill (module 872363, CSS 102137): the Nitro wheel (`premiumStreamIcon`, 14px,
/// white, 4px before the text) when the quality is above the free tier, the resolution, 4px, the frame rate.
pub(super) fn quality_pill(text: String) -> gpui::Div {
    let premium = super::go_live::label_is_premium(&text);
    let (resolution, rate) = text
        .split_once(' ')
        .map_or((text.clone(), String::new()), |(a, b)| {
            (a.to_owned(), b.to_owned())
        });
    div()
        .h(px(16.))
        .px(px(6.))
        .flex()
        .items_center()
        .bg(mid("--interactive-muted"))
        .text_size(px(12.))
        .line_height(px(16.))
        .font_weight(FontWeight(600.))
        .text_color(rgba(0xffffffff))
        .when(premium, |this| {
            this.child(
                gpui::svg()
                    .path("icons/nitro_wheel.svg")
                    .size(px(14.))
                    .mr(px(4.))
                    .flex_none()
                    .text_color(rgba(0xffffffff)),
            )
        })
        .child(div().mr(px(4.)).child(resolution))
        .child(rate)
}

/// The live indicator of a focused stream (module 872363 with `showQuality`): the quality, when the stream told it,
/// joined to LIVE.
pub(super) fn live_indicator(quality: Option<String>) -> AnyElement {
    let quality_pill = |text: String| quality_pill(text).rounded_l(px(16.));
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
            Some(text) => div().flex().child(quality_pill(text)).child(live),
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
    /// Our own stream with Show My Screen Share off: `HIDDEN_STREAM` (module 568598), drawn as module 256195.
    hidden: bool,
    /// The focused stream its owner ended (`ended_focus`), drawn as module 566331.
    ended: bool,
}

/// The art of an ended stream's tile (`art_XFjmsP`).
const ENDED_STREAM_ART: &str = "https://discord.com/assets/422703837071ae97.svg";

/// The art of a hidden stream's tile (module 913516).
const HIDDEN_STREAM_ART: &str = "https://discord.com/assets/4e2cd55089053b35.svg";

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
        let was_idle = self.vv.last_move.elapsed() >= IDLE || self.vv.forced_idle;
        self.vv.forced_idle = false;
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

    /// `onMouseLeave: onForceIdle` of the call's root (module 156652): the pointer left the call area, so it is idle at
    /// once unless something holds the idle off.
    pub(super) fn voice_view_left(&mut self, cx: &mut Context<Self>) {
        let hovered = !self.vv.hovered_controls.is_empty();
        self.vv.hovered_controls.clear();
        if !self.vv.forced_idle || hovered {
            self.vv.forced_idle = true;
            cx.notify();
        }
    }

    fn set_control_hover(&mut self, id: &'static str, inside: bool, cx: &mut Context<Self>) {
        let changed = if inside {
            self.vv.hovered_controls.insert(id)
        } else {
            self.vv.hovered_controls.remove(id)
        };
        if changed {
            if inside {
                self.vv.forced_idle = false;
                self.vv.last_move = Instant::now();
            }
            cx.notify();
        }
    }

    /// How far the idle fade has gone back in, 1 shown and 0 idle: one value for the header, the bar and the
    /// gradients, with their `.2s ease-in-out` (stylesheet 770583; none with reduced motion, `.full-motion` only).
    pub(super) fn call_idle_t(&self, shown: bool) -> f32 {
        let target = if shown { 1. } else { 0. };
        if super::motion::reduced() {
            return target;
        }
        super::motion::value(
            super::motion::key(&("call-idle-fade", self.vv.popout_render)),
            target,
            super::motion::css(200., super::motion::Ease::EaseInOut),
        )
    }

    /// The raw idle state of the call view (`idle` of module 461782): two seconds without a move or press in the area,
    /// or the pointer gone from it, unless a menu or a popout of the call holds it off (`onPreventIdle`).
    pub(super) fn call_idle(&self) -> bool {
        !self.controls_shown()
    }

    pub(super) fn controls_shown(&self) -> bool {
        // `dmcall controls`: a capture keeps the idle fade away, as the pointer resting in the area does.
        #[cfg(feature = "capture")]
        if self.dmc.pin_controls {
            return true;
        }
        // The zoom controls hold the idle off while hovered or pressed (`onPreventIdle("interact")`, 117029).
        (self.vv.last_move.elapsed() < IDLE && !self.vv.forced_idle)
            || !self.vv.hovered_controls.is_empty()
            || self.vv.menu.is_some()
            || self.audio_menu.as_ref().is_some_and(|menu| menu.call)
            || self.panel.popout.is_some()
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

    pub(super) fn user_of(&self, id: Id) -> User {
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
    pub(super) fn tile_color(&self, user: &User) -> Rgba {
        let (key, _) = super::avatar_source(user);
        let found = *self
            .vv
            .colors
            .borrow_mut()
            .entry(key.clone())
            .or_insert_with(|| {
                let picture = self.images.get(&key)?;
                let decoded = image::load_from_memory(picture.bytes()).ok()?;
                crate::palette::avatar_palette(decoded.to_rgba8().as_raw())
                    .first()
                    .copied()
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
                // Show My Own Camera off, or Disable Video of the person's menu (`isLocalVideoDisabled`).
                local_video_disabled: (is_me && hide_own)
                    || (!is_me && super::call_menus::video_disabled(state.user_id)),
                connected: false,
            };
            // Show My Screen Share off (module 666328): our own stream's tile stays, as the hidden stream of module
            // 568598 (`isSelfStreamHidden` makes it `HIDDEN_STREAM`).
            let hidden_stream = is_me && super::call_menus::own_stream_hidden(channel);
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
                    hidden: hidden_stream,
                    ended: false,
                });
            }
            out.push(Participant {
                user,
                mute,
                deaf,
                speaking,
                tile,
                hidden: false,
                ended: false,
            });
        }
        // A failed stream on screen keeps its owner's stream tile, drawn as module 768088.
        if let Some(key) = self.video.ended_focus
            && self.video.ended_failed
            && key.channel() == channel
            && let Some(person) = out.iter_mut().find(|person| {
                person.tile.kind == TileKind::Stream && person.tile.user == key.owner()
            })
        {
            person.ended = true;
        }
        // The focused stream that ended stays as its tile until closed, its owner still in the channel.
        if let Some(key) = self.video.ended_focus
            && key.channel() == channel
            && !out.iter().any(|person| {
                person.tile.kind == TileKind::Stream && person.tile.user == key.owner()
            })
            && let Some(index) = out
                .iter()
                .position(|person| person.tile.user == key.owner())
        {
            let owner = &out[index];
            let ended = Participant {
                user: owner.user.clone(),
                mute: owner.mute,
                deaf: owner.deaf,
                speaking: false,
                tile: Candidate {
                    kind: TileKind::Stream,
                    going_live: true,
                    connected: false,
                    ..owner.tile.clone()
                },
                hidden: false,
                ended: true,
            };
            out.insert(index, ended);
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
        // `showParticipants: participantsOpen && (!j || N)` of module 540462, `j` being the layouts that share the
        // column with the chat (`MINIMUM`, `NORMAL`): there the focused tile has the whole area to itself.
        let strip_shown = !self.vv.members_hidden
            && height > 384.
            && !others.is_empty()
            && self.call_layout_is_full();
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
        // `ed`: the bar's 72px less the room under the tile; the focused tile's action row keeps clear of it.
        self.vv
            .controls_bottom
            .set((72. - (height - main_height) / 2.).max(0.));
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
                            div()
                                .id("call-members-toggle-wrap")
                                .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| {
                                    cx.stop_propagation();
                                })
                                .on_click(|_, _, cx| cx.stop_propagation())
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
                                            .h(px(32.))
                                            .px(px(8.))
                                            .rounded(px(8.))
                                            .bg(gpui::hsla(0., 0., 0., 0.6))
                                            .flex()
                                            .items_center()
                                            .gap(px(4.))
                                            .cursor_pointer()
                                            .on_hover(cx.listener(|this, inside: &bool, _, cx| {
                                                this.set_control_hover(
                                                    "call-members-toggle",
                                                    *inside,
                                                    cx,
                                                );
                                            }))
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
                                                cx.stop_propagation();
                                                this.vv.members_hidden = !this.vv.members_hidden;
                                                cx.notify();
                                            })),
                                        cx,
                                    ),
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
            (self.combined_sidebar_width(), 32.)
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
                    // Module 156652: a press wakes the controls as a move does; the pointer leaving the area makes it idle at once.
                    .capture_any_mouse_down(cx.listener(|this, _: &gpui::MouseDownEvent, _, cx| {
                        this.voice_view_moved(cx);
                    }))
                    .on_hover(cx.listener(|this, inside: &bool, _, cx| {
                        if !*inside {
                            this.voice_view_left(cx);
                        }
                    }))
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
        let width = f32::from(size.width) - self.combined_sidebar_width();
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
            // Module 156652: a press wakes the controls as a move does; the pointer leaving the area makes it idle at once.
            .capture_any_mouse_down(cx.listener(|this, _: &gpui::MouseDownEvent, _, cx| {
                this.voice_view_moved(cx);
            }))
            .on_hover(cx.listener(|this, inside: &bool, _, cx| {
                if !*inside {
                    this.voice_view_left(cx);
                }
            }))
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
        // Another conversation's call, seen from outside, does not touch the focus of the call this client is in.
        if !self.vv.outside {
            self.vv.focus.keep_if(|user, kind| {
                people
                    .iter()
                    .any(|person| person.tile.user == user && person.tile.kind == kind)
            });
        }
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
        if people.iter().any(|person| person.hidden) {
            self.request_image(HIDDEN_STREAM_ART.to_owned(), HIDDEN_STREAM_ART.to_owned());
        }
        if people.iter().any(|person| person.ended) {
            self.request_image(ENDED_STREAM_ART.to_owned(), ENDED_STREAM_ART.to_owned());
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
        if !self.vv.outside {
            self.stream_zoom.follow(focused.and_then(|index| {
                let tile = &people[index].tile;
                (tile.kind == TileKind::Stream).then_some(tile.user)
            }));
        }
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
            // `disableGradients: !inCall`, and the bottom centre is the join buttons of the caller; tile context menus
            // still mount inside the observing call view (module 175203).
            return div()
                .absolute()
                .inset_0()
                .child(grid)
                .children(self.voice_menus(channel, width, height, cx))
                .into_any_element();
        }
        div()
            .absolute()
            .inset_0()
            .child(grid)
            .child(
                div()
                    .absolute()
                    .inset_0()
                    .opacity(self.call_idle_t(shown))
                    .child(fade(true))
                    .child(fade(false)),
            )
            .when(guild != 0, |this| {
                // `q` of module 416528: with the non-video participants hidden, the header counts the users.
                let members = (!show_non_video).then_some(user_count);
                this.child(self.call_header(channel, shown, focus_head, members, cx))
            })
            .children(activity_stage.then(|| self.activity_leave_button(cx)))
            .child(self.call_bar(guild, channel, shown, cx))
            // Module 394412 in `eventPromptsContainer_9bvEB0`, which fades with the controls.
            .children(self.event_prompts(guild, channel, self.call_idle_t(shown), cx))
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
        // `overlayTitle_7NOr2f.idle_7NOr2f{opacity:0}` and `overlayButtonContainer_7NOr2f.idle_7NOr2f{opacity:0}`
        // (stylesheet 655282): the name and the buttons go while the call is idle (module 175203 reads the idle context).
        let idle = self.call_idle() && !self.vv.outside;
        let pill = div()
            .h(px(32.))
            .flex()
            .items_center()
            .when(idle, |this| this.opacity(0.))
            .child(
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
            .when(!menu_open && !idle, |this| {
                this.opacity(0.)
                    .group_hover(group.clone(), |style| style.opacity(1.))
            })
            .when(!menu_open && idle, |this| this.opacity(0.))
            .capture_any_mouse_down(cx.listener(move |this, _: &gpui::MouseDownEvent, _, _| {
                this.vv.opener.set(Some(("tile-options", menu_open)));
            }))
            .on_click(cx.listener(move |this, event: &gpui::ClickEvent, _, cx| {
                cx.stop_propagation();
                // Module 175203: Options opens the menu with `minimal: true` at the press; a press on it while its
                // menu is open closes it.
                if this.opener_was_open("tile-options") {
                    this.vv.menu = None;
                    this.vv.tile_menu = None;
                    cx.notify();
                    return;
                }
                let subject = if stream_tile {
                    super::call_menus::Subject::Stream(user_id)
                } else {
                    super::call_menus::Subject::User(user_id)
                };
                this.open_tile_menu(subject, true, event.position(), cx);
            }))
            .child(icon("voice_view_more", 16., "--white"));
        let outside = self.vv.outside;
        let media = self.tile_media_for(person);
        let video = matches!(media, TileMedia::Video(_));
        let waiting = stream_tile && stream.is_none() && !person.hidden && !person.ended;
        let loading = if stream_tile {
            stream.is_some() && !video
        } else {
            video_tiles::shows_video(&person.tile) && !video
        };
        // A person's video still without a picture after 20 seconds: "Failed to start camera" (module 566566).
        let camera_failed = !stream_tile && !outside && {
            let (failed, began) =
                self.video_ready
                    .borrow_mut()
                    .observe(user_id, loading, std::time::Instant::now());
            if began {
                cx.spawn(async move |this, cx| {
                    cx.background_executor()
                        .timer(super::camera_failed::READY_TIMEOUT)
                        .await;
                    let _ = this.update(cx, |_, cx| cx.notify());
                })
                .detach();
            }
            failed
        };
        let background = if stream_tile || video_tiles::shows_video(&person.tile) {
            mid("--primary-700")
        } else {
            self.tile_color(&person.user)
        };
        // The focused stream's quality and LIVE sit in the header instead.
        let is_focus = self.vv.focus.tile == Some((user_id, kind));
        let zoom_controls = stream_tile && is_focus && video;
        // Module 555148's stream tile (not focused) and 83982's tile of our own camera carry the AV warning of
        // module 275731 / 289552 (`av_errors`).
        let own_camera_tile =
            !stream_tile && self.stores.current_user.as_ref().map(|me| me.id) == Some(user_id);
        let warning = if stream_tile && !is_focus {
            self.av_warning_icon(user_id, "tile", cx)
        } else if own_camera_tile {
            self.av_warning_icon(0, "tile", cx)
        } else {
            None
        };
        let has_indicators = stream.as_ref().filter(|_| !is_focus).is_some();
        let (mut warning, camera_warning) = if has_indicators {
            (warning, None)
        } else {
            (
                None,
                warning.map(|warning| {
                    div()
                        .absolute()
                        .top(px(8.))
                        .right(px(8.))
                        .child(warning)
                        .into_any_element()
                }),
            )
        };
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
                .items_center()
                .children(warning.take())
                .when_some(quality, |this, text| {
                    this.child(quality_pill(text).rounded_l(px(16.)))
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
            // Module 175203: a right click opens the tile's menu at the pointer, `minimal` on a stream, whether joined
            // or observing (`outside`).
            .on_mouse_up(
                gpui::MouseButton::Right,
                cx.listener(move |this, event: &gpui::MouseUpEvent, _, cx| {
                    cx.stop_propagation();
                    let subject = if stream_tile {
                        super::call_menus::Subject::Stream(user_id)
                    } else {
                        super::call_menus::Subject::User(user_id)
                    };
                    this.open_tile_menu(subject, stream_tile, event.position, cx);
                }),
            )
            .when(outside && stream_tile, |this| {
                // `handleSelectParticipant` on a stream from outside the call: join it and watch.
                let (guild, channel) = self.vv.shown_call;
                this.cursor_pointer()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.watch_stream_joining(guild, channel, user_id, cx);
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
            .when(!person.hidden && !person.ended, |this| {
                this.child(self.tile_media(media, person, layout))
            })
            .when(person.hidden, |this| {
                this.child(self.hidden_stream_content(layout, is_focus, cx))
            })
            .when(person.ended, |this| {
                this.child(if self.video.ended_failed {
                    self.stream_failed_content(layout.tile_width, cx)
                } else {
                    self.ended_stream_content(layout.tile_width, cx)
                })
            })
            .when(camera_failed, |this| {
                this.child(self.camera_failed_content(user_id, layout.tile_width, is_focus, cx))
            })
            .when(loading && !person.hidden && !camera_failed, |this| {
                this.child(spinner())
            })
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
            .when(
                stream_tile
                    && !waiting
                    && !self.vv.popout_render
                    && self.stores.current_user.as_ref().map(|me| me.id) == Some(user_id)
                    && super::voice_video::pause_own_preview_unfocused()
                    && super::voice_video::client_unfocused(cx),
                |this| this.child(own_preview_paused()),
            )
            .children(indicators)
            .children(camera_warning)
            .child(
                div()
                    .absolute()
                    .inset(px(12.))
                    .flex()
                    .items_end()
                    .justify_between()
                    .child(pill)
                    // Module 175203 `eM`: a focused tile's action row has no Options; a focused stream's has its zoom
                    // controls when in the call (`zoom-controls`, design/dom/popouts/stream-focused-darker-cozy.json);
                    // non-focused tiles show Options both in-call and while observing (`outside`).
                    .when(!outside && zoom_controls, |this| {
                        let image = match &media {
                            TileMedia::Video(key) => self.video.images.image(key).cloned(),
                            _ => None,
                        };
                        // `marginBottom: controlsBottom` on the action row (module 175203).
                        this.child(
                            div()
                                .mb(px(self.vv.controls_bottom.get()))
                                .child(self.stream_zoom_controls(image, cx)),
                        )
                    })
                    .when(!is_focus && !(!outside && zoom_controls), |this| {
                        this.child(options)
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

    /// Module 256195 over a hidden stream (`content_6DCDRW streamHidden_6DCDRW`: `--primary-700`, a column), through
    /// module 200749 (4px of padding): the art (`art_3rN_rc`, none under 400 wide, 88 high under 1000, else 134, 16
    /// under it), "This stream has been hidden." (`body-md/semibold`, white, 4 under it; not under 195 wide), then,
    /// unless the tile is selected, Watch Stream (`Watch` under 175, `sm` under 195; 8px above it from 195) and, while
    /// someone else streams, the eye that watches several streams (8px after it). Watch Stream has no handler of its
    /// own: a press goes to the tile.
    fn hidden_stream_content(
        &self,
        layout: GridLayout,
        selected: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let width = layout.tile_width;
        let small = width < 195.;
        let art_height = if width < 400. {
            None
        } else if width < 1000. {
            Some(88.)
        } else {
            Some(134.)
        };
        let art = self.images.get(HIDDEN_STREAM_ART).cloned();
        let me = self.stores.current_user.as_ref().map(|me| me.id);
        let (guild, channel) = self.vv.shown_call;
        let others_live = self
            .stores
            .voice_members(guild, channel)
            .iter()
            .any(|state| state.self_stream && Some(state.user_id) != me);
        let height = if small { 32. } else { 40. };
        let button = |id: &'static str| {
            div()
                .id(id)
                .h(px(height))
                .rounded(px(8.))
                .border_1()
                .border_color(mid("--control-secondary-border-default"))
                .bg(mid("--control-secondary-background-default"))
                .hover(|style| style.bg(mid("--control-secondary-background-hover")))
                .flex()
                .items_center()
                .justify_center()
                .cursor_pointer()
        };
        div()
            .absolute()
            .inset_0()
            .overflow_hidden()
            .bg(mid("--primary-700"))
            .p(px(4.))
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .children(art_height.map(|art_height| {
                div()
                    .w_full()
                    .h(px(art_height))
                    .mb(px(16.))
                    .flex()
                    .justify_center()
                    .children(art.map(|art| {
                        img(art)
                            .h(px(art_height))
                            .object_fit(gpui::ObjectFit::Contain)
                    }))
            }))
            .when(!small, |this| {
                this.child(
                    div()
                        .mb(px(4.))
                        .text_center()
                        .text_size(px(16.))
                        .line_height(px(20.))
                        .font_weight(FontWeight(600.))
                        .text_color(mid("--white"))
                        .child(dc_intl::t("epU4Ec")),
                )
            })
            .when(!selected, |this| {
                this.child(
                    div()
                        .when(!small, |this| this.pt(px(8.)))
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(
                            button("hidden-stream-watch")
                                .px(px(if small { 12. } else { 16. }))
                                .text_size(px(if small { 14. } else { 16. }))
                                .line_height(px(if small { 18. } else { 20. }))
                                .font_weight(FontWeight(500.))
                                .text_color(mid("--control-secondary-text-default"))
                                .child(if width < 175. {
                                    dc_intl::t("I6JG46")
                                } else {
                                    dc_intl::t("7Xq/nV")
                                }),
                        )
                        .when(others_live, |this| {
                            this.child(
                                div().ml(px(8.)).child(
                                    self.with_tip(
                                        "hidden-stream-multiple-tip",
                                        Tip::text(dc_intl::t("wCrzut"), Side::Top),
                                        button("hidden-stream-multiple")
                                            .w(px(height))
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                cx.stop_propagation();
                                                if let Some(me) = me {
                                                    this.watch_stream_with(me, true, cx);
                                                }
                                            }))
                                            .child(icon(
                                                "voice_view_eye_plus",
                                                20.,
                                                "--control-secondary-text-default",
                                            )),
                                        cx,
                                    ),
                                ),
                            )
                        }),
                )
            })
            .into_any_element()
    }

    /// Module 566331 over the focused stream its owner ended (`container_XFjmsP`, onyx, 4px of padding): the art
    /// (`art_XFjmsP`: none under 550 wide, 134 high under 1000, else 185, 48 under it), "This stream has ended."
    /// (`heading-sm/medium` under 550, else `heading-lg/medium`, `text-strong`, 24 under it) and the secondary Close
    /// Stream (`sm` under 550, else `md`).
    /// GAP: the container's radial over linear purple gradient is drawn as `--modal-background` alone.
    fn ended_stream_content(&self, width: f32, cx: &mut Context<Self>) -> AnyElement {
        use super::overlay::onyx;
        let small = width < 550.;
        let art_height = if small {
            None
        } else if width < 1000. {
            Some(134.)
        } else {
            Some(185.)
        };
        let art = self.images.get(ENDED_STREAM_ART).cloned();
        div()
            .absolute()
            .inset_0()
            .overflow_hidden()
            .bg(onyx("--modal-background"))
            .p(px(4.))
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .children(art_height.map(|art_height| {
                div().w_full().pb(px(48.)).flex().justify_center().child(
                    div()
                        .w_full()
                        .h(px(art_height))
                        .flex()
                        .justify_center()
                        .children(art.map(|art| {
                            img(art)
                                .h(px(art_height))
                                .object_fit(gpui::ObjectFit::Contain)
                        })),
                )
            }))
            .child(
                div()
                    .mb(px(24.))
                    .text_center()
                    .text_size(px(if small { 16. } else { 20. }))
                    .line_height(px(if small { 20. } else { 24. }))
                    .font_weight(FontWeight(500.))
                    .text_color(onyx("--text-strong"))
                    .child(dc_intl::t("Kb4Ukp")),
            )
            .child(
                div()
                    .id("ended-stream-close")
                    .h(px(if small { 32. } else { 40. }))
                    .px(px(if small { 12. } else { 16. }))
                    .rounded(px(8.))
                    .border_1()
                    .border_color(onyx("--control-secondary-border-default"))
                    .bg(onyx("--control-secondary-background-default"))
                    .hover(|style| style.bg(onyx("--control-secondary-background-hover")))
                    .flex()
                    .items_center()
                    .justify_center()
                    .cursor_pointer()
                    .text_size(px(if small { 14. } else { 16. }))
                    .line_height(px(if small { 18. } else { 20. }))
                    .font_weight(FontWeight(500.))
                    .text_color(onyx("--control-secondary-text-default"))
                    .child(dc_intl::t("4EGMWL"))
                    .on_click(cx.listener(|this, _, _, cx| {
                        cx.stop_propagation();
                        this.close_ended_stream(cx);
                    })),
            )
            .into_any_element()
    }

    /// `Watch Stream` over a stream nobody here watches yet (module 555148: a secondary button; `Watch` under 175px).
    fn watch_overlay(&self, owner: u64, width: f32, cx: &mut Context<Self>) -> AnyElement {
        // The call being drawn (`call_view` sets it first), so the popped-out window watches in its own call.
        let (guild, channel) = self.vv.shown_call;
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
                        this.watch_stream_joining(guild, channel, owner, cx);
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
        members: Option<usize>,
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
            // `topControls_rTsZa_` while idle: opacity 0 and 8px up, `.2s ease-in-out` (stylesheet 770583).
            .opacity(self.call_idle_t(shown))
            .mt(px(-8. * (1. - self.call_idle_t(shown))))
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
                    }))
                    // `ei` of module 681281 with nothing focused: the channel's running event (module 961314): the
                    // divider, the status line of 103355 with `liveText` "Live Event" (`body-md/semibold`, the calendar
                    // and the text in `--text-feedback-positive`), and the event's name (`body-md/normal`,
                    // `eventName_4__wLb`: 8 px on each side, one line).
                    .children(
                        focus
                            .is_none()
                            .then(|| self.call_live_event())
                            .flatten()
                            .map(|event_name| {
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(9.))
                                    .min_w(px(0.))
                                    .child(
                                        div()
                                            .size(px(4.))
                                            .rounded_full()
                                            .bg(mid("--interactive-icon-default")),
                                    )
                                    .child(
                                        div()
                                            .flex()
                                            .items_center()
                                            .gap(px(8.))
                                            .font_weight(FontWeight(600.))
                                            .text_color(mid("--text-feedback-positive"))
                                            .child(icon("events", 20., "--text-feedback-positive"))
                                            .child(dc_intl::t("kM6e4G")),
                                    )
                                    .child(
                                        div()
                                            .mx(px(8.))
                                            .min_w(px(0.))
                                            .overflow_hidden()
                                            .whitespace_nowrap()
                                            .text_ellipsis()
                                            .font_weight(FontWeight(400.))
                                            .text_color(mid("--text-default"))
                                            .child(event_name),
                                    )
                            }),
                    ),
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
            .children(members.map(|count| self.call_participants_button(count, cx)))
            // Module 416528: the warning of the focused stream (275731) or our focused camera (289552) before LIVE.
            .children(self.vv.focus.tile.and_then(|(owner, kind)| {
                let me = self.stores.current_user.as_ref().map(|me| me.id);
                let warning = match kind {
                    TileKind::Stream => self.av_warning_icon(owner, "header", cx),
                    TileKind::User if Some(owner) == me => self.av_warning_icon(0, "header", cx),
                    TileKind::User => None,
                };
                warning.map(|warning| div().mr(px(16.)).child(warning))
            }))
            .children(
                focus
                    .filter(|crumb| crumb.stream)
                    .map(|crumb| div().mr(px(16.)).child(live_indicator(crumb.quality))),
            )
            // Module 977851 (`V`): drawn only while the chat is closed (`er` of 416528 and `!c` of 482877 both require
            // it), so its label is always Show Chat; "Hide Chat" and "Chat in the popout is coming soon." need the chat
            // open or an `inPopout` that no caller passes (nothing dispatches SHOW_TEXT_IN_VOICE_POPOUT_COMING_SOON_TIP).
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

    /// The header's Participants button (module 772475: the 24px group icon and the count, `body-md/normal` 4px after
    /// it, 24 high) and, while it is open, its popout (module 195007, `root_2NnKyB`: 240 wide, at most 260 high, on
    /// `--background-surface-high` with 4px corners; the `Participants — N` header in 14px medium
    /// `--interactive-text-default`, 16px around and 9 under; the voice user list in a scroller padded 16 with 8 at the
    /// start and none on top). The popout opens under the button (`position: bottom`, 8px away), centred on it.
    fn call_participants_button(&self, count: usize, cx: &mut Context<Self>) -> AnyElement {
        const KEY: &str = "call-participants";
        let open = self.vv.participants_open;
        let button = div()
            .id("call-participants-button")
            .h(px(24.))
            .flex()
            .flex_row()
            .items_center()
            .cursor_pointer()
            .text_color(mid(if open {
                "--interactive-icon-active"
            } else {
                "--interactive-icon-default"
            }))
            .on_click(cx.listener(|this, _, _, cx| {
                this.vv.participants_open = !this.vv.participants_open;
                cx.notify();
            }))
            .child(icon(
                "members",
                24.,
                if open {
                    "--interactive-icon-active"
                } else {
                    "--interactive-icon-default"
                },
            ))
            .child(
                div()
                    .ml(px(4.))
                    .text_size(px(16.))
                    .line_height(px(24.))
                    .child(count.to_string()),
            );
        let tip = self.with_tip(
            KEY,
            Tip::text(dc_intl::t("jNqDh/"), Side::Bottom),
            button,
            cx,
        );
        let popout = open.then(|| {
            let (guild, channel) = self.vv.shown_call;
            let rows = self.voice_members_in(guild, channel, true, cx);
            let total = self.stores.voice_members(guild, channel).len();
            // A point under the button's centre, 8px below it; the popout hangs from it, kept inside the window.
            div()
                .absolute()
                .top(px(32.))
                .left(gpui::relative(0.5))
                .size_0()
                .child(
                    deferred(
                        gpui::anchored()
                            .position_mode(gpui::AnchoredPositionMode::Local)
                            .anchor(gpui::Anchor::TopCenter)
                            .snap_to_window_with_margin(px(8.))
                            .child(
                                div()
                                    .id("call-participants-popout")
                                    .occlude()
                                    .w(px(240.))
                                    .max_h(px(260.))
                                    .rounded(px(4.))
                                    .bg(color("--background-surface-high"))
                                    .flex()
                                    .flex_col()
                                    .on_mouse_down_out(cx.listener(
                                        |this, event: &gpui::MouseDownEvent, _, cx| {
                                            // A press on the button itself is its own toggle.
                                            let on_button =
                                                this.tip_anchors.borrow().get(KEY).is_some_and(
                                                    |bounds| bounds.contains(&event.position),
                                                );
                                            if !on_button {
                                                this.vv.participants_open = false;
                                                cx.notify();
                                            }
                                        },
                                    ))
                                    .child(
                                        div()
                                            .p(px(16.))
                                            .pb(px(9.))
                                            .text_size(px(14.))
                                            .font_weight(FontWeight(500.))
                                            .text_color(color("--interactive-text-default"))
                                            .child(dc_intl::format(
                                                "e95u3C",
                                                &[("count", (total as i64).into())],
                                            )),
                                    )
                                    .child(
                                        div()
                                            .id("call-participants-scroller")
                                            .overflow_y_scroll()
                                            .pt(px(0.))
                                            .pr(px(16.))
                                            .pb(px(16.))
                                            .pl(px(8.))
                                            .flex()
                                            .flex_col()
                                            .children(rows),
                                    ),
                            ),
                    )
                    .with_priority(2),
                )
        });
        div()
            .relative()
            .flex_none()
            .mr(px(16.))
            .child(tip)
            .children(popout)
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
        .on_click(cx.listener(|this, _, _, cx| {
            this.clear_tip();
            if this.opener_was_open("call-mic-chevron") {
                this.audio_menu = None;
                cx.notify();
            } else if !this.audio_menu.as_ref().is_some_and(|menu| menu.call) {
                this.toggle_call_audio_menu(cx);
            }
        }));
        let mic_nub = self.bar_opener(mic_nub, "call-mic-chevron", mic_open, cx);
        let mic = div()
            .flex()
            .gap(px(1.))
            .child(self.with_tip(
                "call-mic-tip",
                Tip::text(
                    dc_intl::tr(if muted { "Unmute" } else { "Mute" }),
                    Side::Top,
                ),
                mic_main,
                cx,
            ))
            .child(self.bar_tip(
                "call-mic-chevron-tip",
                Tip::text(dc_intl::tr("More Microphone Options"), Side::Top),
                mic_nub,
                mic_open,
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
                    dc_intl::tr(if camera_on {
                        "Turn Off Camera"
                    } else {
                        "Turn On Camera"
                    }),
                    Side::Top,
                ),
                camera_button.on_click(cx.listener(|this, _, _, cx| this.voice_camera_clicked(cx))),
                cx,
            ))
            .child({
                let open = self.vv.menu == Some(Menu::Camera);
                self.bar_tip(
                    "call-camera-chevron-tip",
                    Tip::text(dc_intl::tr("More Camera Options"), Side::Top),
                    self.bar_opener(
                        nub(
                            "call-camera-chevron",
                            if camera_on { active_text } else { "--white" },
                        ),
                        "call-camera-chevron",
                        open,
                        cx,
                    )
                    .when(camera_on, |this| this.bg(active_fill))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.toggle_bar_menu("call-camera-chevron", Menu::Camera, cx);
                    })),
                    open,
                    cx,
                )
            });
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
        let more = self
            .bar_opener(
                primary("call-more", "voice_view_more"),
                "call-more",
                more_open,
                cx,
            )
            .relative()
            .child(self.anchor_probe("call-more"))
            .when(more_open, |this| this.bg(gpui::hsla(0., 0., 1., 0.1)))
            .on_click(cx.listener(|this, _, _, cx| {
                this.toggle_bar_menu("call-more", Menu::More, cx);
            }));
        let soundboard_open = self.panel.popout == Some(super::voice_panel::Popout::Soundboard);
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
            self.bar_opener(this, "call-soundboard", soundboard_open, cx)
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.clear_tip();
                    let open = this.panel.popout == Some(super::voice_panel::Popout::Soundboard);
                    if this.opener_was_open("call-soundboard") {
                        if open {
                            this.close_panel_popout(cx);
                        }
                        return;
                    }
                    if !open {
                        this.toggle_soundboard(window, cx);
                    }
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
                                Tip::text(dc_intl::tr("Stop Streaming"), Side::Top),
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
                        .child({
                            let open = self.vv.menu == Some(Menu::Share);
                            self.bar_tip(
                                "call-screen-chevron-tip",
                                Tip::text("More Stream Options", Side::Top),
                                self.bar_opener(
                                    nub("call-screen-chevron", "--icon-voice-connected"),
                                    "call-screen-chevron",
                                    open,
                                    cx,
                                )
                                .bg(active_fill)
                                .on_click(cx.listener(
                                    |this, _, _, cx| {
                                        this.vv.quality_open = false;
                                        this.toggle_bar_menu(
                                            "call-screen-chevron",
                                            Menu::Share,
                                            cx,
                                        );
                                    },
                                )),
                                open,
                                cx,
                            )
                        })
                        .into_any_element()
                } else {
                    self.with_tip(
                        "call-screen-tip",
                        Tip::text(dc_intl::tr("Share Your Screen"), Side::Top),
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
                    Tip::text(dc_intl::tr("Start An Activity"), Side::Top),
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
                    .child(self.bar_tip(
                        "call-soundboard-tip",
                        Tip::text(
                            dc_intl::tr(soundboard_reason.unwrap_or("Open Soundboard")),
                            Side::Top,
                        ),
                        soundboard,
                        soundboard_open,
                        cx,
                    ))
                    .into_any_element(),
                self.bar_tip(
                    "call-more-tip",
                    Tip::text(dc_intl::tr("More Options"), Side::Top),
                    more,
                    more_open,
                    cx,
                ),
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
                // Module 984624: Disconnect gets the chevron of Disconnect Actions while a running event of this
                // channel can be managed by this account.
                let exit_event = (!stream).then(|| self.call_manageable_event()).flatten();
                let nub_shown = others > usize::from(stream) || exit_event.is_some();
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
                            this.disconnect_from_call(cx);
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
                        let menu = if exit_event.is_some() {
                            Menu::ExitOptions
                        } else {
                            Menu::Watched
                        };
                        let open = self.vv.menu == Some(menu);
                        this.child(
                            self.bar_opener(
                                div().id("call-end-chevron"),
                                "call-end-chevron",
                                open,
                                cx,
                            )
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
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.toggle_bar_menu("call-end-chevron", menu, cx);
                            }))
                            .child(icon(
                                "voice_view_caret",
                                16.,
                                "--white",
                            )),
                        )
                    })
                    .child(self.with_tip(
                        "call-disconnect-tip",
                        Tip::text(
                            dc_intl::tr(if stream {
                                "Stop Watching"
                            } else {
                                "Disconnect"
                            }),
                            Side::Top,
                        ),
                        button,
                        cx,
                    ))
            });
        // `bottomControls_rTsZa_` while idle: opacity 0 and 8px down, `.2s ease-in-out`; it keeps taking presses
        // (`controlSection_rTsZa_{pointer-events:all}`).
        let idle_t = self.call_idle_t(shown);
        div()
            .absolute()
            .left(px(16.))
            .right(px(16.))
            .bottom(px(16. - 8. * (1. - idle_t)))
            .opacity(idle_t)
            .h(px(50.))
            .flex()
            .items_center()
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
                            div()
                                .id("call-bar-left")
                                .capture_any_mouse_down(cx.listener(
                                    |this, event: &gpui::MouseDownEvent, _, _| {
                                        this.vv.bar_press.set(Some(event.position));
                                    },
                                ))
                                .on_hover(cx.listener(|this, inside: &bool, _, cx| {
                                    this.set_control_hover("call-bar-left", *inside, cx);
                                }))
                                .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| {
                                    cx.stop_propagation();
                                })
                                .on_click(|_, _, cx| cx.stop_propagation())
                                .child(
                                    self.with_tip(
                                        "call-chat-tip",
                                        Tip::text(
                                            dc_intl::tr(if hidden {
                                                "Show chat"
                                            } else {
                                                "Hide chat"
                                            }),
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
                                        .on_click(
                                            cx.listener(|this, _, _, cx| {
                                                this.tip = None;
                                                this.toggle_dm_call_chat(cx);
                                            }),
                                        ),
                                        cx,
                                    ),
                                ),
                        )
                    })
                    .when(guild != 0, |this| {
                        this.child(
                            div()
                                .id("call-bar-left")
                                .capture_any_mouse_down(cx.listener(
                                    |this, event: &gpui::MouseDownEvent, _, _| {
                                        this.vv.bar_press.set(Some(event.position));
                                    },
                                ))
                                .on_hover(cx.listener(|this, inside: &bool, _, cx| {
                                    this.set_control_hover("call-bar-left", *inside, cx);
                                }))
                                .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| {
                                    cx.stop_propagation();
                                })
                                .on_click(|_, _, cx| cx.stop_propagation())
                                .child(self.with_tip(
                                    "call-invite-tip",
                                    Tip::text(dc_intl::tr("Invite"), Side::Top),
                                    tray("call-invite", "voice_view_invite").on_click(
                                        cx.listener(|this, _, _, cx| this.open_invite(cx)),
                                    ),
                                    cx,
                                )),
                        )
                    }),
            )
            .child(
                div()
                    .id("call-bar-centre")
                    .flex_none()
                    .capture_any_mouse_down(cx.listener(
                        |this, event: &gpui::MouseDownEvent, _, _| {
                            this.vv.bar_press.set(Some(event.position));
                        },
                    ))
                    .on_hover(cx.listener(|this, inside: &bool, _, cx| {
                        this.set_control_hover("call-bar-centre", *inside, cx);
                    }))
                    .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| {
                        cx.stop_propagation();
                    })
                    .on_click(|_, _, cx| cx.stop_propagation())
                    .child(centre),
            )
            .child(
                div().flex_1().flex().items_center().justify_end().child(
                    div()
                        .id("call-bar-right")
                        .flex()
                        .items_center()
                        .gap(px(20.))
                        .capture_any_mouse_down(cx.listener(
                            |this, event: &gpui::MouseDownEvent, _, _| {
                                this.vv.bar_press.set(Some(event.position));
                            },
                        ))
                        .on_hover(cx.listener(|this, inside: &bool, _, cx| {
                            this.set_control_hover("call-bar-right", *inside, cx);
                        }))
                        .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| {
                            cx.stop_propagation();
                        })
                        .on_click(|_, _, cx| cx.stop_propagation())
                        .children(self.call_volume_control(cx))
                        // `canStayOnTop`: in the popped-out window (module 619344).
                        .when(self.vv.popout_render, |this| {
                            let on = self.vv.popout_on_top;
                            this.child(
                                self.with_tip(
                                    "call-stay-on-top-tip",
                                    Tip::text(
                                        dc_intl::tr(if on {
                                            "Remove From Top"
                                        } else {
                                            "Stay On Top"
                                        }),
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
                                    .on_click(cx.listener(
                                        |this, _, _, cx| this.toggle_call_popout_on_top(cx),
                                    )),
                                    cx,
                                ),
                            )
                        })
                        // Module 540462: Pop Out and Full Screen only exist in video mode (`mode === VIDEO && inCall`),
                        // so a DM's voice call has neither (golden `dm-call/in-call-alone`).
                        .when(guild != 0 || self.dmc.video, |this| {
                            this.child(self.with_tip(
                                "call-popout-tip",
                                Tip::text(dc_intl::tr("Pop Out"), Side::Top),
                                tray("call-popout", "voice_view_popout").on_click(cx.listener(
                                    |this, _, window, cx| this.toggle_call_popout(window, cx),
                                )),
                                cx,
                            ))
                            .child(self.with_tip(
                                "call-fullscreen-tip",
                                Tip::text(
                                    dc_intl::tr(if self.vv.fullscreen {
                                        "Exit Full Screen"
                                    } else {
                                        "Full Screen"
                                    }),
                                    Side::Top,
                                ),
                                tray("call-fullscreen", "voice_view_fullscreen").on_click(
                                    cx.listener(|this, _, window, cx| {
                                        this.toggle_call_fullscreen(window, cx)
                                    }),
                                ),
                                cx,
                            ))
                        }),
                ),
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
        let _ = (channel, width, height);
        match menu {
            Menu::More => {
                let Some(anchor) = self.tip_anchors.borrow().get("call-more").copied() else {
                    return Vec::new();
                };
                // Module 592982 `more-settings-context`: Invite to Voice (834857: a server's voice channel where we may
                // invite); Grid View (973937, not on a Stage), Show My Own Camera (575079), Show Non-Video Participants
                // (854182, not on a Stage), Show New Message Previews (956747, a server's voice channel or Stage); Voice &
                // Video Settings (470743); Report Problem (74329) while a stream is focused.
                let (guild, call_channel) = self.vv.shown_call;
                let stage = guild != 0
                    && self.stores.guild(guild).is_some_and(|known| {
                        known.channels.iter().any(|found| {
                            found.id == call_channel
                                && found.kind == dc_state::ChannelKind::Other(13)
                        })
                    });
                let can_invite = guild != 0
                    && !stage
                    && self.stores.guild(guild).is_some_and(|known| {
                        known.channels.iter().any(|found| {
                            found.id == call_channel
                                && self.can_in_channel(
                                    found,
                                    dc_state::permissions::CREATE_INSTANT_INVITE,
                                )
                        })
                    });
                let focused_stream = matches!(self.vv.focus.tile, Some((_, TileKind::Stream)));
                let mut rows = Vec::new();
                let mut rows_height = 16.;
                if can_invite {
                    rows.push(self.vmenu_row(
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
                    ));
                    rows.push(Self::menu_separator(true));
                    rows_height += 36. + 17.;
                }
                if !stage {
                    rows.push(self.vmenu_row(
                        "vmenu-grid",
                        None,
                        "Grid View",
                        Some(self.vv.focus.tile.is_none()),
                        true,
                        cx,
                        |this, _, _| {
                            // Checked: the grid shows. Unchecking it focuses the first tile (`AUTO`; a stream sorts
                            // first).
                            if this.vv.focus.tile.is_some() {
                                this.vv.focus.grid();
                            } else {
                                let (guild, channel) = this.vv.shown_call;
                                if let Some(first) =
                                    this.participants(guild, channel).into_iter().next()
                                {
                                    this.vv.focus.click(first.tile.user, first.tile.kind);
                                }
                            }
                            set_call_setting(GRID_VIEW, this.vv.focus.tile.is_none());
                        },
                    ));
                    rows_height += 36.;
                }
                rows.push(self.vmenu_row(
                    "vmenu-own-camera",
                    None,
                    "Show My Own Camera",
                    Some(call_setting(SHOW_OWN_CAMERA)),
                    true,
                    cx,
                    |_, _, _| toggle_call_setting(SHOW_OWN_CAMERA),
                ));
                rows_height += 36.;
                if !stage {
                    rows.push(self.vmenu_row(
                        "vmenu-non-video",
                        None,
                        "Show Non-Video Participants",
                        Some(call_setting(SHOW_NON_VIDEO)),
                        true,
                        cx,
                        |_, _, _| toggle_call_setting(SHOW_NON_VIDEO),
                    ));
                    rows_height += 36.;
                }
                if guild != 0 {
                    rows.push(self.vmenu_row(
                        "vmenu-previews",
                        None,
                        "Show New Message Previews",
                        Some(call_setting(MESSAGE_PREVIEWS)),
                        true,
                        cx,
                        |_, _, _| toggle_call_setting(MESSAGE_PREVIEWS),
                    ));
                    rows_height += 36.;
                }
                rows.push(Self::menu_separator(true));
                rows.push(self.vmenu_row(
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
                ));
                rows_height += 17. + 36.;
                if focused_stream {
                    // The report form of module 74329 has no counterpart here: the row is shown disabled.
                    rows.push(Self::menu_separator(true));
                    rows.push(
                        div()
                            .opacity(0.5)
                            .child(self.vmenu_row_ex(
                                "vmenu-report-stream",
                                Some("shop_circle_error"),
                                dc_intl::tr("Report Problem"),
                                None,
                                true,
                                true,
                                false,
                                cx,
                                |_, _, _| {},
                            ))
                            .into_any_element(),
                    );
                    rows_height += 17. + 36.;
                }
                // Right edges meet, 16px above the button.
                let (ox, oy) = self.call_origin();
                let left = f32::from(anchor.right()) - MORE_WIDTH - ox;
                // Over the button when there is room, under it in a call area at the top of the window.
                let top = super::audio_menu::popout_top(
                    f32::from(anchor.top()),
                    f32::from(anchor.bottom()),
                    rows_height,
                    16.,
                    self.viewport.1,
                ) - oy;
                vec![
                    deferred(
                        div().absolute().left(px(left)).top(px(top)).child(
                            self.menu_panel(MORE_WIDTH, true)
                                .id("voice-more-menu")
                                .popout("voice-more-menu", PopKind::Fade, PopSide::Top)
                                .on_mouse_down_out(cx.listener(
                                    |this, event: &gpui::MouseDownEvent, _, cx| {
                                        this.close_call_popup(event, cx);
                                    },
                                ))
                                .children(rows),
                        ),
                    )
                    .with_priority(2)
                    .into_any_element(),
                ]
            }
            Menu::Tile(..) => self.tile_menu_layer(cx).into_iter().collect(),
            Menu::Camera => self.camera_menu(cx),
            Menu::Share => self.share_menu(cx),
            Menu::Watched => self.watched_menu(cx),
            Menu::ExitOptions => self.exit_options_menu(cx),
            Menu::Stream(..) => self.tile_menu_layer(cx).into_iter().collect(),
        }
    }

    /// `L.Qs(channel)`: the name of the call channel's running event.
    fn call_live_event(&self) -> Option<String> {
        let call = self.call.as_ref()?;
        self.stores
            .scheduled_events
            .active(call.guild)
            .into_iter()
            .find(|event| event.channel_id == Some(call.channel))
            .map(|event| event.name.clone())
    }

    /// `f.Qs(channelId)` with `canManageGuildEvent`: the running event of the call's channel, when this account may
    /// end it.
    fn call_manageable_event(&self) -> Option<(Id, Id)> {
        let call = self.call.as_ref()?;
        let event = self
            .stores
            .scheduled_events
            .active(call.guild)
            .into_iter()
            .find(|event| event.channel_id == Some(call.channel))?;
        let manage = self
            .stores
            .guild(call.guild)
            .zip(self.stores.me())
            .is_some_and(|(guild, me)| super::guild_events::can_manage_event(guild, me, event));
        manage.then_some((call.guild, event.id))
    }

    /// The `exit-options` menu ("Disconnect Actions"): End Event (danger, the circled X) ends the event at once
    /// (`endEvent`).
    fn exit_options_menu(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let Some((guild, event)) = self.call_manageable_event() else {
            return Vec::new();
        };
        let row = self.vmenu_row_ex(
            "end-voice-event",
            Some("standing_circle_x"),
            dc_intl::t("qaYzPA"),
            None,
            true,
            true,
            false,
            cx,
            move |this, _, cx| {
                this.vv.menu = None;
                this.end_guild_event(guild, event, cx);
            },
        );
        let width = 220.;
        let Some((left, top)) = self.menu_at("call-end-group", width, 36. + 16.) else {
            return Vec::new();
        };
        vec![
            deferred(
                div().absolute().left(px(left)).top(px(top)).child(
                    self.menu_panel(width, true)
                        .id("voice-exit-options")
                        .popout("voice-exit-options", PopKind::Fade, PopSide::Top)
                        .on_mouse_down_out(cx.listener(
                            |this, event: &gpui::MouseDownEvent, _, cx| {
                                this.close_call_popup(event, cx);
                            },
                        ))
                        .child(row),
                ),
            )
            .with_priority(2)
            .into_any_element(),
        ]
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
                        .when(!super::motion::reduced(), |ring_box| {
                            ring_box.child(
                                canvas(
                                    |_, _, _| (),
                                    move |bounds, _, window, cx| {
                                        let progress = (since.elapsed().as_secs_f32()
                                            / INVITE_ROW_TIMEOUT.as_secs_f32())
                                        .clamp(0., 1.);
                                        paint_invite_ring(bounds, progress, window);
                                        if progress < 1.
                                            && window.is_window_active()
                                            && !cx.reduce_motion()
                                        {
                                            window.request_animation_frame();
                                        }
                                    },
                                )
                                .absolute()
                                .inset_0()
                                .size_full(),
                            )
                        })
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
                        .group_hover("voice-invite-row", |style| {
                            style.text_color(color("--interactive-text-hover"))
                        })
                        .child("Invite to Voice"),
                )
                .child(
                    div()
                        .id("voice-invite-dismiss")
                        .relative()
                        .size(px(16.))
                        .child(
                            div()
                                .group_hover("voice-invite-row", |style| style.opacity(0.))
                                .child(icon("menu_caret", 16., "--channels-default")),
                        )
                        .child(
                            div()
                                .absolute()
                                .inset_0()
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
        div()
            .absolute()
            .inset_0()
            .child(
                img(self.disconnected_background(width, height))
                    .absolute()
                    .inset_0()
                    .size_full(),
            )
            .child(self.call_header(channel, true, None, None, cx))
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

/// Remaining ring fraction of `.timer_MRjIbW .progress_MRjIbW`: a 24pi dash walking clockwise off a 22pi circle.
fn invite_ring_fraction(progress: f32) -> f32 {
    ((1. - progress.clamp(0., 1.)) * (24. / 22.)).clamp(0., 1.)
}

fn paint_ring_circle(cx: Pixels, cy: Pixels, radius: f32, stroke: Rgba, window: &mut Window) {
    const SEGMENTS: usize = 64;
    let mut path = PathBuilder::stroke(px(2.));
    for step in 0..SEGMENTS {
        let angle = (step as f32 / SEGMENTS as f32) * std::f32::consts::TAU;
        let (sin, cos) = angle.sin_cos();
        let pt = point(cx + px(radius * cos), cy + px(radius * sin));
        if step == 0 {
            path.move_to(pt);
        } else {
            path.line_to(pt);
        }
    }
    path.close();
    if let Ok(built) = path.build() {
        window.paint_path(built, stroke);
    }
}

/// `timer_MRjIbW`: 2px ring (`r = 11`) of `--border-normal` with `--border-strong` walking clockwise from 12 o'clock.
fn paint_invite_ring(bounds: Bounds<Pixels>, progress: f32, window: &mut Window) {
    const RADIUS: f32 = 11.;
    let cx = bounds.origin.x + bounds.size.width * 0.5;
    let cy = bounds.origin.y + bounds.size.height * 0.5;
    paint_ring_circle(cx, cy, RADIUS, color("--border-normal"), window);
    let remaining = invite_ring_fraction(progress);
    if remaining >= 0.9999 {
        paint_ring_circle(cx, cy, RADIUS, color("--border-strong"), window);
    } else if remaining > 0.001 {
        let segments = ((remaining * 64.).ceil() as usize).max(2);
        let start = -std::f32::consts::FRAC_PI_2;
        let sweep = remaining * std::f32::consts::TAU;
        let mut path = PathBuilder::stroke(px(2.));
        for step in 0..=segments {
            let angle = start + sweep * (step as f32 / segments as f32);
            let (sin, cos) = angle.sin_cos();
            let pt = point(cx + px(RADIUS * cos), cy + px(RADIUS * sin));
            if step == 0 {
                path.move_to(pt);
            } else {
                path.line_to(pt);
            }
        }
        if let Ok(built) = path.build() {
            window.paint_path(built, color("--border-strong"));
        }
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

    #[test]
    fn invite_ring_walks_from_full_circle_to_zero_over_ten_seconds() {
        assert_eq!(invite_ring_fraction(0.), 1.);
        assert_eq!(invite_ring_fraction(-0.2), 1.);
        let mid = invite_ring_fraction(0.5);
        assert!(mid > 0.5 && mid < 0.6);
        assert!(invite_ring_fraction(0.75) < mid);
        assert_eq!(invite_ring_fraction(1.), 0.);
        assert_eq!(invite_ring_fraction(1.5), 0.);
    }
}
