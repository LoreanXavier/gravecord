//! The voice channel's call view (`callContainer_hbKzX7`, `rTsZa_`, `9bvEB0`, `K9Xq13` in the captured build): the
//! tile grid on black, the header and control bar that fade after two seconds without the pointer (module 540462,
//! `timeout: 2e3`), the More Options and tile Options menus, the disconnected state and the docked chat.
//! Geometry is measured in `design/dom/channel/voice-view-*-darker-cozy.json`; the grid sizing is module 511205.

use super::tooltip::{Side, Tip};
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

fn toggle_call_setting(key: &str) {
    storage::set(key, serde_json::json!(!call_setting(key)));
}

/// `midnight`: the call container carries `theme-midnight`, whatever the app's own theme is.
fn mid(name: &str) -> Rgba {
    rgba(Theme::Midnight.color(name))
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Menu {
    More,
    /// A tile's Options: whose, and where the pointer was (the menu's top-left corner).
    Tile(Id, Point<Pixels>),
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
    /// Dominant colour of an avatar by its image key, decoded once.
    colors: RefCell<HashMap<String, Option<[u8; 3]>>>,
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
            colors: RefCell::new(HashMap::new()),
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

/// What a tile shows behind its name. Only the avatar is drawn: video needs the media pipeline.
#[derive(Clone, Copy)]
pub(super) enum TileMedia {
    Avatar,
}

struct Participant {
    user: User,
    mute: bool,
    deaf: bool,
    speaking: bool,
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
    fn voice_view_moved(&mut self, cx: &mut Context<Self>) {
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

    fn controls_shown(&self) -> bool {
        self.vv.last_move.elapsed() < IDLE || self.vv.menu.is_some()
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

    fn participants(&self, guild: Id, channel: Id) -> Vec<Participant> {
        self.stores
            .voice_members(guild, channel)
            .into_iter()
            .map(|state| Participant {
                user: self.user_of(state.user_id),
                mute: state.self_mute || state.mute,
                deaf: state.self_deaf || state.deaf,
                speaking: self.speaking.contains(&state.user_id)
                    || self.soundboard.user_playing(state.user_id),
            })
            .collect()
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
        let body = if in_call {
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
            .on_mouse_move(cx.listener(|this, _: &MouseMoveEvent, _, cx| {
                this.voice_view_moved(cx);
            }))
            .child(body);
        if chat {
            view = view.child(self.voice_chat_panel(guild, channel, cx));
        }
        view.into_any_element()
    }

    fn call_view(
        &mut self,
        guild: Id,
        channel: Id,
        width: f32,
        height: f32,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let shown = self.controls_shown();
        let mut people = self.participants(guild, channel);
        if !call_setting(SHOW_NON_VIDEO) {
            // Nobody has video yet, so every tile is a non-video one.
            people.clear();
        }
        let can_invite = self.current_channel().is_some_and(|channel| {
            self.can_in_channel(channel, dc_state::permissions::CREATE_INSTANT_INVITE)
        });
        // Alone in the call with the right to invite: the invite card keeps the user's tile company (686502 `D`).
        let card = people.len() == 1 && can_invite;
        let count = people.len() + usize::from(card);
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
            rows.push(
                div()
                    .flex()
                    .flex_none()
                    .w_full()
                    .justify_center()
                    .gap(px(TILE_GAP))
                    .children(row)
                    .into_any_element(),
            );
        }
        let grid = div()
            .absolute()
            .inset_0()
            .px(px(8.))
            .pt(px(layout.pad_top))
            .pb(px(layout.pad_bottom.max(0.)))
            .flex()
            .flex_col()
            .gap(px(TILE_GAP))
            .overflow_hidden()
            .children(rows);
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
            .child(self.call_header(channel, shown, cx))
            .child(self.call_bar(guild, channel, shown, cx))
            .children(self.voice_menus(channel, width, height, cx))
            .into_any_element()
    }

    fn tile(&self, person: &Participant, layout: GridLayout, cx: &mut Context<Self>) -> AnyElement {
        let group = SharedString::from(format!("vtile-{}", person.user.id));
        let user_id = person.user.id;
        let menu_open = matches!(self.vv.menu, Some(Menu::Tile(id, _)) if id == user_id);
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
                .when(person.mute, |this| {
                    this.child(icon("voice_view_mic_slash", 16., "--white"))
                })
                .when(person.deaf && !person.mute, |this| {
                    this.child(icon("headphones_deafened", 16., "--white"))
                })
                .child(person.user.display_name().to_owned()),
        );
        let options = div()
            .id(SharedString::from(format!("vtile-options-{user_id}")))
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
                this.vv.menu = Some(Menu::Tile(user_id, event.position()));
                cx.notify();
            }))
            .child(icon("voice_view_more", 16., "--white"));
        div()
            .id(SharedString::from(format!("vtile-{user_id}")))
            .group(group)
            .relative()
            .flex_none()
            .w(px(layout.tile_width))
            .h(px(layout.tile_height))
            .rounded(px(8.))
            .overflow_hidden()
            .bg(self.tile_color(&person.user))
            .child(self.tile_media(TileMedia::Avatar, person))
            .child(
                div()
                    .absolute()
                    .inset(px(12.))
                    .flex()
                    .items_end()
                    .justify_between()
                    .child(pill)
                    .child(options),
            )
            .when(person.speaking, |this| {
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

    /// The one place a tile's picture is drawn: today the 80px avatar centred in it.
    fn tile_media(&self, media: TileMedia, person: &Participant) -> AnyElement {
        match media {
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
        }
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
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.voice_activity_clicked(cx);
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
    fn call_header(&self, channel: Id, shown: bool, cx: &mut Context<Self>) -> AnyElement {
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
                    .child(name),
            )
            .child(div().flex_1())
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
    fn call_bar(&self, guild: Id, channel: Id, shown: bool, cx: &mut Context<Self>) -> AnyElement {
        let muted = self.self_mute || self.self_deaf;
        let soundboard_reason = self.soundboard_disabled();
        let _ = (guild, channel);
        let tray = |id: &'static str, glyph: &'static str| {
            div().id(id).size(px(24.)).cursor_pointer().child(icon(
                glyph,
                24.,
                "--interactive-icon-default",
            ))
        };
        let primary = |id: &'static str, glyph: &'static str| {
            div()
                .id(id)
                .size(px(40.))
                .rounded(px(8.))
                .flex()
                .items_center()
                .justify_center()
                .cursor_pointer()
                .hover(|style| style.bg(gpui::hsla(0., 0., 1., 0.1)))
                .child(icon(glyph, 20., "--white"))
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
            .when(muted, |this| {
                this.bg(mid("--opacity-red-12"))
                    .hover(|style| style.bg(mid("--opacity-red-16")))
            })
            .when(!muted, |this| {
                this.hover(|style| style.bg(gpui::hsla(0., 0., 1., 0.1)))
            })
            .child(icon(
                if muted { "mic_muted" } else { "mic" },
                20.,
                if muted { "--red-400" } else { "--white" },
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
        let camera = div()
            .flex()
            .gap(px(1.))
            .child(
                self.with_tip(
                    "call-camera-tip",
                    Tip::text("Turn On Camera", Side::Top),
                    primary("call-camera", "voice_camera")
                        .on_click(cx.listener(|this, _, _, cx| this.voice_camera_clicked(cx))),
                    cx,
                ),
            )
            .child(
                self.with_tip(
                    "call-camera-chevron-tip",
                    Tip::text("More Camera Options", Side::Top),
                    nub("call-camera-chevron", "--white")
                        .on_click(cx.listener(|this, _, _, cx| this.voice_camera_clicked(cx))),
                    cx,
                ),
            );
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
        let soundboard = primary("call-soundboard", "voice_soundboard")
            .when(soundboard_reason.is_some(), |this| this.opacity(0.3))
            .when(soundboard_reason.is_none(), |this| {
                this.on_click(cx.listener(|this, _, window, cx| this.toggle_soundboard(window, cx)))
            });
        let centre = div()
            .flex()
            .flex_none()
            .items_center()
            .gap(px(12.))
            .child(section(vec![
                mic.into_any_element(),
                camera.into_any_element(),
            ]))
            .child(section(vec![
                self.with_tip(
                    "call-screen-tip",
                    Tip::text("Share Your Screen", Side::Top),
                    primary("call-screen", "voice_screen")
                        .on_click(cx.listener(|this, _, _, cx| this.voice_screen_clicked(cx))),
                    cx,
                )
                .into_any_element(),
                self.with_tip(
                    "call-activity-tip",
                    Tip::text("Start An Activity", Side::Top),
                    primary("call-activity", "voice_activity")
                        .on_click(cx.listener(|this, _, _, cx| this.voice_activity_clicked(cx))),
                    cx,
                )
                .into_any_element(),
                self.with_tip(
                    "call-soundboard-tip",
                    Tip::text(soundboard_reason.unwrap_or("Open Soundboard"), Side::Top),
                    soundboard,
                    cx,
                )
                .into_any_element(),
                self.with_tip(
                    "call-more-tip",
                    Tip::text("More Options", Side::Top),
                    more,
                    cx,
                )
                .into_any_element(),
            ]))
            .child(
                self.with_tip(
                    "call-disconnect-tip",
                    Tip::text("Disconnect", Side::Top),
                    div()
                        .id("call-disconnect")
                        .w(px(66.))
                        .h(px(50.))
                        .rounded(px(12.))
                        .bg(mid("--control-critical-primary-background-default"))
                        .hover(|style| style.bg(mid("--control-critical-primary-background-hover")))
                        .active(|style| {
                            style.bg(mid("--control-critical-primary-background-active"))
                        })
                        .flex()
                        .items_center()
                        .justify_center()
                        .cursor_pointer()
                        .on_click(cx.listener(|this, _, _, cx| this.leave_voice(cx)))
                        .child(icon("voice_disconnect", 20., "--white")),
                    cx,
                ),
            );
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
                div().flex_1().flex().items_center().child(
                    self.with_tip(
                        "call-invite-tip",
                        Tip::text("Invite", Side::Top),
                        tray("call-invite", "voice_view_invite")
                            .on_click(cx.listener(|this, _, _, cx| this.open_invite(cx))),
                        cx,
                    ),
                ),
            )
            .child(centre)
            .child(
                div()
                    .flex_1()
                    .flex()
                    .items_center()
                    .justify_end()
                    .gap(px(20.))
                    .child(self.with_tip(
                        "call-popout-tip",
                        Tip::text("Pop Out", Side::Top),
                        tray("call-popout", "voice_view_popout"),
                        cx,
                    ))
                    .child(self.with_tip(
                        "call-fullscreen-tip",
                        Tip::text("Full Screen", Side::Top),
                        tray("call-fullscreen", "voice_view_fullscreen").on_click(cx.listener(
                            |_, _, window, _| {
                                window.toggle_fullscreen();
                            },
                        )),
                        cx,
                    )),
            )
            .into_any_element()
    }

    // ---- menus ----

    fn menu_panel(&self, width: f32, theme_mid: bool) -> gpui::Div {
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

    fn menu_separator(theme_mid: bool) -> AnyElement {
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
    fn vmenu_row(
        &self,
        id: &'static str,
        glyph: Option<&'static str>,
        label: &'static str,
        checked: Option<bool>,
        theme_mid: bool,
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
                    .child(label),
            )
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
    fn voice_menus(
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
                        Some(call_setting(GRID_VIEW)),
                        true,
                        cx,
                        |_, _, _| toggle_call_setting(GRID_VIEW),
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
                let left = f32::from(anchor.right()) - MORE_WIDTH - 375.;
                let top = f32::from(anchor.top()) - 16. - 268. - 32.;
                vec![
                    deferred(
                        div().absolute().left(px(left)).top(px(top)).child(
                            self.menu_panel(MORE_WIDTH, true)
                                .id("voice-more-menu")
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
                let window_w = width + 375.;
                let window_h = height + 32.;
                let left = f32::from(at.x).min(window_w - 16. - TILE_MENU_WIDTH);
                let top = f32::from(at.y).min(window_h - 16. - rows_height);
                vec![
                    deferred(
                        div()
                            .absolute()
                            .left(px(left - 375.))
                            .top(px(top - 32.))
                            .child(
                                self.menu_panel(TILE_MENU_WIDTH, false)
                                    .id("voice-tile-menu")
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
            .child(self.call_header(channel, shown, cx))
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
