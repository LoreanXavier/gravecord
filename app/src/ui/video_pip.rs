//! The video picture-in-picture window: the call's video in a small window over the app while another channel is on
//! screen.
//!
//! - When (`Pc` of module 596339, the PiP manager): in a call that is not popped out, with a stream we watch or send
//!   or someone's camera on, while User Settings or another full layer is not open and the selected channel is not
//!   the call's own.
//! - Who (`getSpeaker` of module 94247, `VideoSpeakerStore`): the focused tile, else the stream we watch or send,
//!   else the camera of whoever speaks, else the first camera of someone else, else our own.
//! - The window (module 806911 on the frame of module 271195, CSS of module 967481): 320 by 180 (`fG` of module
//!   806931, 16:9) on `--primary-800`, 80px from the right and 132px from the bottom of the window (`I` of module
//!   457864, the default `BOTTOM_RIGHT`), its corners 8px. Over the picture, the controls on a black gradient (black,
//!   clear from 30% to 60%, black): the back arrow and the channel's name (Open in Theater: back to the call's
//!   channel; so does a double click), the switcher between watched streams (module 6155: "Switch to {username}'s
//!   Stream" for every other stream, when there are two or more), and at the bottom the person's name (`body-md`)
//!   with, on the right 16px apart, the stream's settings (our own stream), Pop Out, Stop Streaming or Stop Watching
//!   (a stream), or Disconnect (a camera). The controls fade out 1.8 seconds after the pointer stopped over the
//!   window (`idle_qOjco6`).
//! - Moved and resized as module 457864 does it: a press anywhere starts a drag once the pointer has gone over 3 px
//!   (the squared distance above 9); the window follows the pointer inside the app's window; on release the corner is
//!   the quarter the pointer would reach in 200 ms at its speed (`calculateDecayingPosition`), and the window docks
//!   there at the corner's edge offsets (`PICTURE_IN_PICTURE_MOVE`). The 20 px handle at the corner facing the
//!   window's middle (stylesheet 155819 `resizeHandle*`, 7 px outside the window) resizes it sideways, between 200 and
//!   980 px while held and snapped to 40 px (200 to 960) on release (`T`, `PICTURE_IN_PICTURE_RESIZE`), never wider
//!   than the window allows (`ensureWidth`). Corner and width are kept as `PictureInPictureStore` keeps them
//!   (`openPosition`, `pipWidths`).

use super::lottie_icon::{self, Anim};
use super::tooltip::{Side, Tip};
use super::video_tiles::TileKind;
use super::{Root, avatar_source, color, icon};
use dc_state::Id;
use dc_voice::VideoSource;
use gpui::{
    AnyElement, Context, FontWeight, IntoElement, SharedString, Window, deferred, div, img,
    prelude::*, px,
};
use std::time::{Duration, Instant};

/// `fG[VIDEO]` (module 806931), the default width; the height is the 16:9 of `pictureInPictureVideo_qOjco6`.
const WIDTH: f32 = 320.;
/// `mn[VIDEO]` and `cF[VIDEO]` (module 806931): the resize limits.
const MIN_WIDTH: f32 = 200.;
const MAX_WIDTH: f32 = 960.;
/// `I` of module 457864: the edge offsets of the corners.
const TOP: f32 = 80.;
const BOTTOM: f32 = 132.;
const LEFT: f32 = 80.;
const RIGHT: f32 = 80.;
/// `resizeHandle_69M0s9`: 20 px, 7 px outside the window.
const HANDLE: f32 = 20.;
const HANDLE_OUT: f32 = 7.;
/// `timeout: 1800` of the idle wrapper.
const IDLE: Duration = Duration::from_millis(1800);
/// `persistKey` of `PictureInPictureStore` (module 334463).
const STORE: &str = "PictureInPictureStore";

/// `CUs` of module 652215: where the window docks.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub(super) enum Corner {
    TopLeft,
    TopRight,
    BottomLeft,
    #[default]
    BottomRight,
}

impl Corner {
    fn wire(self) -> &'static str {
        match self {
            Corner::TopLeft => "top-left",
            Corner::TopRight => "top-right",
            Corner::BottomLeft => "bottom-left",
            Corner::BottomRight => "bottom-right",
        }
    }

    fn from_wire(value: &str) -> Option<Self> {
        Some(match value {
            "top-left" => Corner::TopLeft,
            "top-right" => Corner::TopRight,
            "bottom-left" => Corner::BottomLeft,
            "bottom-right" => Corner::BottomRight,
            _ => return None,
        })
    }

    fn right(self) -> bool {
        matches!(self, Corner::TopRight | Corner::BottomRight)
    }

    fn top(self) -> bool {
        matches!(self, Corner::TopLeft | Corner::TopRight)
    }
}

/// `T` of module 457864: a width snapped to 40 px inside the limits.
pub(super) fn snap_width(width: f32) -> f32 {
    ((width / 40.).round() * 40.).clamp(MIN_WIDTH, MAX_WIDTH)
}

/// `ensureWidth`: the widest the window may be in an app window `max_x` wide.
pub(super) fn widest(max_x: f32) -> f32 {
    snap_width(max_x - (LEFT + RIGHT))
}

/// `getPosition`: the window's top left corner docked at a corner of a `max_x` by `max_y` area.
pub(super) fn docked(corner: Corner, size: (f32, f32), max: (f32, f32)) -> (f32, f32) {
    let top = TOP.round();
    let bottom = (max.1 - BOTTOM - size.1).round();
    let left = LEFT.round();
    let right = (max.0 - RIGHT - size.0).round();
    match corner {
        Corner::TopLeft => (left, top),
        Corner::BottomLeft => (left, bottom),
        Corner::TopRight => (right, top),
        Corner::BottomRight => (right, bottom),
    }
}

/// `translate` of the draggable: the window kept inside the area.
pub(super) fn kept_inside(at: (f32, f32), size: (f32, f32), max: (f32, f32)) -> (f32, f32) {
    let x = if at.0 < 0. {
        0.
    } else if at.0 > max.0 - size.0 {
        max.0 - size.0
    } else {
        at.0
    };
    let y = if at.1 < 0. {
        0.
    } else if at.1 > max.1 - size.1 {
        max.1 - size.1
    } else {
        at.1
    };
    (x, y)
}

/// `handleDragEnd`: the corner of the quarter the pointer reaches in 200 ms at its speed (px per ms).
pub(super) fn settle(release: (f32, f32), velocity: (f32, f32), max: (f32, f32)) -> Corner {
    let x = release.0 + 200. * velocity.0;
    let y = release.1 + 200. * velocity.1;
    match (y <= max.1 / 2., x <= max.0 / 2.) {
        (true, true) => Corner::TopLeft,
        (true, false) => Corner::TopRight,
        (false, true) => Corner::BottomLeft,
        (false, false) => Corner::BottomRight,
    }
}

/// The width while the handle is held (module 761929 with `maxDimension: maxWidth + 20`): a right corner's handle is
/// on the window's left, so moving left widens it.
pub(super) fn resized(corner: Corner, start_width: f32, start_x: f32, x: f32) -> f32 {
    let delta = if corner.right() {
        start_x - x
    } else {
        x - start_x
    };
    (start_width + delta).clamp(MIN_WIDTH, MAX_WIDTH + 20.)
}

/// A press on the window, until it is let go.
#[derive(Clone, Copy, Debug)]
struct Drag {
    start: (f32, f32),
    /// The pointer's offset from the window's corner.
    offset: (f32, f32),
    /// Where the window is drawn while dragged.
    at: (f32, f32),
    /// Past the 3 px threshold.
    dragging: bool,
    last: (f32, f32, Instant),
    velocity: (f32, f32),
}

/// The resize handle held.
#[derive(Clone, Copy, Debug)]
struct Resize {
    start_x: f32,
    start_width: f32,
    width: f32,
}

/// The window's state between frames.
#[derive(Default)]
pub(super) struct VideoPip {
    /// When the pointer last moved over the window.
    moved: Option<Instant>,
    /// The switcher menu of module 6155 is open.
    switcher: bool,
    /// An idle check is waiting.
    idle_check: bool,
    /// `openPosition` and `pipWidths[VIDEO]`, read from storage the first time.
    place: Option<(Corner, f32)>,
    drag: Option<Drag>,
    resize: Option<Resize>,
}

/// What the window shows (`participantOnScreen`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum OnScreen {
    Stream(Id),
    Camera(Id),
}

/// `getSpeaker` of module 94247 over plain facts: the focused tile when it has video, else a stream (`getLastActiveStream`),
/// else whoever of the cameras of others speaks, else the first camera of someone else, else ourselves.
pub(super) fn speaker(
    focused: Option<(Id, TileKind)>,
    streams: &[Id],
    cameras: &[Id],
    speaking: impl Fn(Id) -> bool,
    me: Id,
) -> OnScreen {
    match focused {
        Some((owner, TileKind::Stream)) if streams.contains(&owner) => {
            return OnScreen::Stream(owner);
        }
        Some((user, TileKind::User)) if cameras.contains(&user) => return OnScreen::Camera(user),
        _ => {}
    }
    if let Some(owner) = streams.first() {
        return OnScreen::Stream(*owner);
    }
    let others: Vec<Id> = cameras.iter().copied().filter(|user| *user != me).collect();
    if let Some(user) = others.iter().find(|user| speaking(**user)) {
        return OnScreen::Camera(*user);
    }
    OnScreen::Camera(others.first().copied().unwrap_or(me))
}

impl Root {
    /// The call's channel when the window shows (`Pc`), with the streams and cameras it can show.
    fn video_pip_sources(&self) -> Option<(Id, Id, Vec<Id>, Vec<Id>)> {
        let call = self.call.as_ref()?;
        if !call.connected || self.call_popped_out() {
            return None;
        }
        // A full layer hides it (`m5.hasLayers()`), as does the call's own channel on screen.
        if self.settings.is_some()
            || self.profile_editor.is_some()
            || self.server_settings.is_some()
            || self.channel_settings.is_some()
            || self.channel == Some(call.channel)
        {
            return None;
        }
        let streams: Vec<Id> = self.video.held_owners();
        let cameras: Vec<Id> = self
            .stores
            .voice_members(call.guild, call.channel)
            .into_iter()
            .filter(|state| state.self_video && !super::call_menus::video_disabled(state.user_id))
            .map(|state| state.user_id)
            .collect();
        if streams.is_empty() && cameras.is_empty() {
            return None;
        }
        Some((call.guild, call.channel, streams, cameras))
    }

    /// Whether the window is up (for the activity window, which gives way to it, and the harness).
    pub(super) fn video_pip_shown(&self) -> bool {
        self.video_pip_sources().is_some()
    }

    fn video_pip_idle(&self) -> bool {
        self.video_pip
            .moved
            .is_none_or(|moved| moved.elapsed() >= IDLE)
    }

    /// The pointer moved over the window: the controls show, and fade 1.8 seconds later.
    fn video_pip_moved(&mut self, cx: &mut Context<Self>) {
        self.video_pip.moved = Some(Instant::now());
        if !self.video_pip.idle_check {
            self.video_pip.idle_check = true;
            cx.spawn(async move |this, cx| {
                loop {
                    cx.background_executor().timer(IDLE).await;
                    let again = this
                        .update(cx, |this, cx| {
                            cx.notify();
                            let waiting = !this.video_pip_idle();
                            if !waiting {
                                this.video_pip.idle_check = false;
                            }
                            waiting
                        })
                        .unwrap_or(false);
                    if !again {
                        break;
                    }
                }
            })
            .detach();
        }
        cx.notify();
    }

    /// `openPosition` and the video width of `PictureInPictureStore` (the defaults when nothing is stored).
    fn video_pip_place(&self) -> (Corner, f32) {
        if let Some(place) = self.video_pip.place {
            return place;
        }
        let stored = crate::storage::get(STORE).unwrap_or(serde_json::Value::Null);
        let corner = stored["openPosition"]
            .as_str()
            .and_then(Corner::from_wire)
            .unwrap_or_default();
        let width = stored["pipWidths"]["0"]
            .as_f64()
            .map_or(WIDTH, |w| w as f32);
        (corner, width)
    }

    fn video_pip_store(&mut self, corner: Corner, width: f32) {
        self.video_pip.place = Some((corner, width));
        let stored = crate::storage::get(STORE).unwrap_or(serde_json::Value::Null);
        let camera = stored["pipWidths"]["1"].as_f64().unwrap_or(160.);
        crate::storage::set(
            STORE,
            serde_json::json!({
                "openPosition": corner.wire(),
                "pipWidths": {"0": width, "1": camera},
            }),
        );
    }

    /// The window's size and top left corner in an app window of `max`.
    fn video_pip_rect(&self, max: (f32, f32)) -> ((f32, f32), (f32, f32)) {
        let (corner, stored) = self.video_pip_place();
        let width = match self.video_pip.resize {
            Some(resize) => resize.width,
            None => stored.min(widest(max.0)),
        };
        let size = (width, (width * 9. / 16.).round());
        let at = match self.video_pip.drag.filter(|drag| drag.dragging) {
            Some(drag) => drag.at,
            None => docked(corner, size, max),
        };
        (size, at)
    }

    /// The press on the window (`handleMouseDown` with `dragAnywhere`).
    fn video_pip_press(&mut self, pointer: (f32, f32), max: (f32, f32)) {
        let (_, at) = self.video_pip_rect(max);
        self.video_pip.drag = Some(Drag {
            start: pointer,
            offset: (pointer.0 - at.0, pointer.1 - at.1),
            at,
            dragging: false,
            last: (pointer.0, pointer.1, Instant::now()),
            velocity: (0., 0.),
        });
    }

    /// A move with the button held: the drag starts past 3 px, then the window follows; the handle resizes.
    fn video_pip_drag_to(&mut self, pointer: (f32, f32), max: (f32, f32)) -> bool {
        let (corner, _) = self.video_pip_place();
        if let Some(resize) = self.video_pip.resize.as_mut() {
            resize.width = resized(corner, resize.start_width, resize.start_x, pointer.0);
            return true;
        }
        let (size, _) = self.video_pip_rect(max);
        let Some(drag) = self.video_pip.drag.as_mut() else {
            return false;
        };
        let distance = (pointer.0 - drag.start.0).powi(2) + (pointer.1 - drag.start.1).powi(2);
        if !drag.dragging && distance > 9. {
            drag.dragging = true;
            drag.velocity = (0., 0.);
            drag.last = (pointer.0, pointer.1, Instant::now());
        }
        if !drag.dragging {
            return false;
        }
        // `handleDrag`: the speed between two moves, in px per ms.
        let now = Instant::now();
        let elapsed = now.duration_since(drag.last.2).as_secs_f32() * 1000.;
        if elapsed > 0. {
            drag.velocity = (
                (pointer.0 - drag.last.0) / elapsed,
                (pointer.1 - drag.last.1) / elapsed,
            );
            drag.last = (pointer.0, pointer.1, now);
        }
        // GAP: the official window follows on a spring (tension 80, friction 8) and glides into its corner; here it
        // follows the pointer directly and docks at once.
        drag.at = kept_inside(
            (pointer.0 - drag.offset.0, pointer.1 - drag.offset.1),
            size,
            max,
        );
        true
    }

    /// The button let go: a drag docks at its corner, a resize snaps to 40 px.
    fn video_pip_release(&mut self, pointer: (f32, f32), max: (f32, f32)) {
        let (corner, width) = self.video_pip_place();
        if let Some(resize) = self.video_pip.resize.take() {
            let width = snap_width(resize.width).min(widest(max.0));
            self.video_pip_store(corner, width);
            return;
        }
        if let Some(drag) = self.video_pip.drag.take()
            && drag.dragging
        {
            let next = settle(pointer, drag.velocity, max);
            self.video_pip_store(next, width);
        }
    }

    /// Open in Theater (`handleJumpToChannel`): the call's channel, with the stream on screen selected.
    fn video_pip_jump(&mut self, on_screen: OnScreen, cx: &mut Context<Self>) {
        let Some(call) = self.call.as_ref() else {
            return;
        };
        let (guild, channel) = (call.guild, call.channel);
        if let OnScreen::Stream(owner) = on_screen {
            self.vv.focus.tile = Some((owner, TileKind::Stream));
        }
        self.video_pip.switcher = false;
        if guild == 0 {
            self.select_home(Some(channel));
        } else {
            self.open_voice_view(guild, channel, cx);
        }
        cx.notify();
    }

    fn video_pip_button(
        &self,
        id: &'static str,
        label: String,
        child: AnyElement,
        run: impl Fn(&mut Root, &mut Window, &mut Context<Root>) + 'static,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        // `rightTrayIcon_ScIiLS`: 16px from the one before; the icon is `md` (24px) in `currentColor`.
        self.with_tip(
            format!("{id}-tip"),
            Tip::body(super::tooltip::Body::Text(label.into()), Side::Top),
            div()
                .id(id)
                .ml(px(16.))
                .size(px(24.))
                .flex()
                .items_center()
                .justify_center()
                .cursor_pointer()
                .child(child)
                .on_click(cx.listener(move |this, _, window, cx| {
                    run(this, window, cx);
                    cx.notify();
                })),
            cx,
        )
        .into_any_element()
    }

    pub(super) fn video_pip_layer(
        &self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let (guild, channel, streams, cameras) = self.video_pip_sources()?;
        let me = self.stores.current_user.as_ref().map_or(0, |me| me.id);
        let on_screen = speaker(
            self.vv.focus.tile,
            &streams,
            &cameras,
            |user| self.speaking.contains(&user),
            me,
        );
        let (person, key) = match on_screen {
            OnScreen::Stream(owner) => (owner, (owner, VideoSource::Stream(owner))),
            OnScreen::Camera(user) => (user, (user, VideoSource::Camera)),
        };
        let picture = self.video.images.image(&key).cloned();
        let user = self.user_of(person);
        let avatar = {
            let (key, url) = avatar_source(&user);
            let image = self.images.get(&key).cloned();
            if image.is_none() {
                self.want_image(key, url, cx);
            }
            image
        };
        // `m1` of module 47167: the channel's name, a DM's by its people.
        let title = if guild == 0 {
            self.stores
                .private_channels
                .iter()
                .find(|known| known.id == channel)
                .map(|known| self.private_channel_name(known))
                .unwrap_or_default()
        } else {
            self.stores
                .guild(guild)
                .and_then(|known| known.channels.iter().find(|known| known.id == channel))
                .map(|known| known.name.clone())
                .unwrap_or_default()
        };
        let idle = self.video_pip_idle() && !self.video_pip.switcher;
        let own_stream = on_screen == OnScreen::Stream(me);
        let size = window.viewport_size();
        let max = (f32::from(size.width), f32::from(size.height));
        let ((width, height), (x, y)) = self.video_pip_rect(max);
        let (corner, _) = self.video_pip_place();
        // The picture, or the person's 80px avatar (module 806911 renders the video component's avatar fallback).
        let content = match picture {
            Some(picture) => img(picture)
                .size_full()
                .rounded(px(8.))
                .object_fit(gpui::ObjectFit::Contain)
                .into_any_element(),
            None => div()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .children(avatar.map(|avatar| img(avatar).size(px(80.)).rounded_full()))
                .into_any_element(),
        };
        // Module 6155: shown with two or more streams of others in the channel and a single window.
        let others: Vec<Id> = streams
            .iter()
            .copied()
            .filter(|owner| *owner != me)
            .collect();
        let switcher = (others.len() > 1).then(|| {
            div()
                .id("video-pip-switcher")
                .flex_none()
                .cursor_pointer()
                .child(icon("more_dots", 24., "--white"))
                .on_click(cx.listener(|this, _, _, cx| {
                    this.video_pip.switcher = !this.video_pip.switcher;
                    cx.notify();
                }))
        });
        let header = div()
            .flex()
            .justify_between()
            .overflow_hidden()
            .child(
                div()
                    .id("video-pip-title")
                    .group("video-pip-title")
                    .flex()
                    .items_center()
                    .overflow_hidden()
                    .border_b_1()
                    .border_color(gpui::transparent_black())
                    .hover(|style| style.border_color(color("--white")))
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _, _, cx| this.video_pip_jump(on_screen, cx)))
                    .child(div().flex_none().mr(px(2.)).child(icon(
                        "clips_arrow_small_left",
                        20.,
                        "--primary-300",
                    )))
                    .child(
                        div()
                            .flex_shrink(1.)
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .text_ellipsis()
                            .text_size(px(16.))
                            .line_height(px(20.))
                            .font_weight(FontWeight(500.))
                            .text_color(color("--white"))
                            .child(title),
                    ),
            )
            .children(switcher);
        let stop_label = if own_stream {
            dc_intl::t("S5anIc")
        } else {
            dc_intl::t("q3O3J8")
        };
        let mut right: Vec<AnyElement> = Vec::new();
        if let OnScreen::Stream(owner) = on_screen {
            // GAP: the viewers count of a stream (module 525788 compact) and our own stream's settings popout (module
            // 352018) are not drawn here.
            if !own_stream {
                right.push(self.video_pip_button(
                    "video-pip-popout",
                    dc_intl::t("JzWezz").to_owned(),
                    icon("voice_view_popout", 24., "--white").into_any_element(),
                    |this, window, cx| {
                        if !this.call_popped_out() {
                            this.toggle_call_popout(window, cx);
                        }
                    },
                    cx,
                ));
            }
            right.push(
                self.video_pip_button(
                    "video-pip-stop",
                    stop_label.to_owned(),
                    lottie_icon::icon(
                        "video-pip-stop-icon",
                        Anim::ScreenShare,
                        Some("disable"),
                        24.,
                        color("--white"),
                    )
                    .into_any_element(),
                    move |this, _, cx| {
                        if own_stream {
                            this.stream_stop(cx);
                        } else {
                            this.unwatch_stream_of(owner, cx);
                        }
                    },
                    cx,
                ),
            );
        } else {
            right.push(
                self.video_pip_button(
                    "video-pip-disconnect",
                    dc_intl::t("6vrfgt").to_owned(),
                    lottie_icon::icon(
                        "video-pip-disconnect-icon",
                        Anim::Disconnect,
                        None,
                        24.,
                        color("--white"),
                    )
                    .into_any_element(),
                    |this, _, cx| this.disconnect_from_call(cx),
                    cx,
                ),
            );
        }
        // `canSeeParticipantName`: a stream's or a camera's person, not in a DM.
        let name = (guild != 0).then(|| {
            div()
                .ml(px(8.))
                .overflow_hidden()
                .whitespace_nowrap()
                .text_ellipsis()
                .text_size(px(16.))
                .line_height(px(20.))
                .text_color(color("--white"))
                .child(user.display_name().to_owned())
        });
        let shift = if idle { 8. } else { 0. };
        let controls = div()
            .absolute()
            .inset_0()
            .p(px(8.))
            .flex()
            .flex_col()
            .justify_between()
            .opacity(if idle { 0. } else { 1. })
            .bg(gpui::linear_gradient(
                180.,
                gpui::linear_color_stop(gpui::hsla(0., 0., 0., 1.), 0.),
                gpui::linear_color_stop(gpui::hsla(0., 0., 0., 0.), 0.3),
            ))
            .child(
                div()
                    .absolute()
                    .left_0()
                    .right_0()
                    .bottom_0()
                    .h(px(height * 0.4))
                    .bg(gpui::linear_gradient(
                        180.,
                        gpui::linear_color_stop(gpui::hsla(0., 0., 0., 0.), 0.),
                        gpui::linear_color_stop(gpui::hsla(0., 0., 0., 1.), 1.),
                    )),
            )
            .child(div().relative().mt(px(-shift)).child(header))
            .child(
                div()
                    .relative()
                    .mb(px(-shift))
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(div().min_w(px(0.)).flex_1().children(name))
                    .child(div().flex().items_center().children(right)),
            );
        let menu = self.video_pip.switcher.then(|| {
            let rows: Vec<AnyElement> = others
                .iter()
                .copied()
                .filter(|owner| on_screen != OnScreen::Stream(*owner))
                .map(|owner| {
                    let name = self.user_of(owner).display_name().to_owned();
                    div()
                        .id(SharedString::from(format!("video-pip-switch-{owner}")))
                        .min_h(px(32.))
                        .px(px(8.))
                        .py(px(6.))
                        .rounded(px(4.))
                        .cursor_pointer()
                        .text_size(px(14.))
                        .line_height(px(18.))
                        .font_weight(FontWeight(500.))
                        .text_color(color("--text-strong"))
                        .hover(|style| style.bg(color("--interactive-background-hover")))
                        .child(dc_intl::format("sqmFRg", &[("username", name.into())]))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            // `selectParticipant`: the stream goes on screen.
                            this.vv.focus.tile = Some((owner, TileKind::Stream));
                            this.video_pip.switcher = false;
                            cx.notify();
                        }))
                        .into_any_element()
                })
                .collect();
            div()
                .id("video-pip-switch-menu")
                .occlude()
                .absolute()
                .left(px(x + width - 196.))
                .top(px(y + 36.))
                .w(px(188.))
                .p(px(8.))
                .rounded(px(8.))
                .border_1()
                .border_color(color("--border-subtle"))
                .bg(color("--background-surface-higher"))
                .children(rows)
        });
        let point = |event_position: gpui::Point<gpui::Pixels>| {
            (f32::from(event_position.x), f32::from(event_position.y))
        };
        // `resizeHandle*`: at the window's corner that faces the middle of the app's window.
        let handle = div()
            .id("video-pip-resize")
            .absolute()
            .size(px(HANDLE))
            .when(corner.top(), |this| this.bottom(px(-HANDLE_OUT)))
            .when(!corner.top(), |this| this.top(px(-HANDLE_OUT)))
            .when(corner.right(), |this| this.left(px(-HANDLE_OUT)))
            .when(!corner.right(), |this| this.right(px(-HANDLE_OUT)))
            .cursor(if corner.top() == corner.right() {
                gpui::CursorStyle::ResizeUpRightDownLeft
            } else {
                gpui::CursorStyle::ResizeUpLeftDownRight
            })
            .on_mouse_down(
                gpui::MouseButton::Left,
                cx.listener(move |this, event: &gpui::MouseDownEvent, _, cx| {
                    cx.stop_propagation();
                    this.video_pip.drag = None;
                    this.video_pip.resize = Some(Resize {
                        start_x: f32::from(event.position.x),
                        start_width: width,
                        width,
                    });
                    cx.notify();
                }),
            );
        // `.pictureInPicture.dragging`: while dragged or resized the layer covers the app's window and takes the
        // pointer, so the moves and the release reach it wherever they are.
        let held = self.video_pip.resize.is_some()
            || self.video_pip.drag.is_some_and(|drag| drag.dragging);
        let catcher = held.then(|| {
            div()
                .id("video-pip-dragging")
                .occlude()
                .absolute()
                .inset_0()
                .cursor(if self.video_pip.resize.is_some() {
                    gpui::CursorStyle::ResizeLeftRight
                } else {
                    gpui::CursorStyle::ClosedHand
                })
                .on_mouse_move(
                    cx.listener(move |this, event: &gpui::MouseMoveEvent, _, cx| {
                        if this.video_pip_drag_to(point(event.position), max) {
                            cx.notify();
                        }
                    }),
                )
                .on_mouse_up(
                    gpui::MouseButton::Left,
                    cx.listener(move |this, event: &gpui::MouseUpEvent, _, cx| {
                        this.video_pip_release(point(event.position), max);
                        cx.notify();
                    }),
                )
        });
        Some(
            deferred(
                div()
                    .absolute()
                    .inset_0()
                    .child(
                        div()
                            .absolute()
                            .left(px(x))
                            .top(px(y))
                            .w(px(width))
                            .h(px(height))
                            .child(
                                div()
                                    .id("video-pip")
                                    .occlude()
                                    .size_full()
                                    .rounded(px(8.))
                                    .overflow_hidden()
                                    .bg(color("--primary-800"))
                                    .cursor(gpui::CursorStyle::OpenHand)
                                    .on_mouse_down(
                                        gpui::MouseButton::Left,
                                        cx.listener(
                                            move |this, event: &gpui::MouseDownEvent, _, _| {
                                                this.video_pip_press(point(event.position), max);
                                            },
                                        ),
                                    )
                                    .on_mouse_move(cx.listener(
                                        move |this, event: &gpui::MouseMoveEvent, _, cx| {
                                            this.video_pip_moved(cx);
                                            if event.dragging()
                                                && this
                                                    .video_pip_drag_to(point(event.position), max)
                                            {
                                                cx.notify();
                                            }
                                        },
                                    ))
                                    .on_mouse_up(
                                        gpui::MouseButton::Left,
                                        cx.listener(
                                            move |this, event: &gpui::MouseUpEvent, _, cx| {
                                                this.video_pip_release(point(event.position), max);
                                                cx.notify();
                                            },
                                        ),
                                    )
                                    .on_click(cx.listener(
                                        move |this, event: &gpui::ClickEvent, _, cx| {
                                            if event.click_count() >= 2 {
                                                this.video_pip_jump(on_screen, cx);
                                            }
                                        },
                                    ))
                                    .child(content)
                                    .child(controls),
                            )
                            .child(handle),
                    )
                    .children(menu)
                    .children(catcher),
            )
            .into_any_element(),
        )
    }

    /// Capture only: `pip state` names what the window shows.
    #[cfg(feature = "capture")]
    pub(crate) fn script_pip(&mut self, words: &[&str], cx: &mut Context<Self>) -> String {
        match words {
            ["state"] => match self.video_pip_sources() {
                Some((_, _, streams, cameras)) => {
                    let me = self.stores.current_user.as_ref().map_or(0, |me| me.id);
                    let on = speaker(
                        self.vv.focus.tile,
                        &streams,
                        &cameras,
                        |user| self.speaking.contains(&user),
                        me,
                    );
                    format!(
                        "pip shown, streams {} cameras {} on screen {}",
                        streams.len(),
                        cameras.len(),
                        match on {
                            OnScreen::Stream(owner) if owner == me => "our stream",
                            OnScreen::Stream(_) => "a stream",
                            OnScreen::Camera(user) if user == me => "our camera",
                            OnScreen::Camera(_) => "a camera",
                        }
                    )
                }
                None => "pip hidden".to_owned(),
            },
            ["wake"] => {
                self.video_pip_moved(cx);
                "controls shown".to_owned()
            }
            // `pip place <w> <h>`: the corner, the stored width and the window's rect in an app window that size.
            ["place", w, h] => {
                let max = (w.parse().unwrap_or(1280.), h.parse().unwrap_or(720.));
                let (corner, width) = self.video_pip_place();
                let (size, at) = self.video_pip_rect(max);
                format!("pip place {corner:?} width {width} rect {at:?} {size:?}")
            }
            _ => "usage: pip state|wake|place <w> <h>".to_owned(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_speaker_is_the_focus_then_a_stream_then_a_speaking_camera() {
        let me = 1;
        let quiet = |_: Id| false;
        // The focused stream wins.
        assert_eq!(
            speaker(Some((7, TileKind::Stream)), &[5, 7], &[9], quiet, me),
            OnScreen::Stream(7)
        );
        // A focused tile without video does not.
        assert_eq!(
            speaker(Some((3, TileKind::User)), &[5], &[9], quiet, me),
            OnScreen::Stream(5)
        );
        // No stream: the camera of whoever speaks, else the first camera of someone else.
        assert_eq!(
            speaker(None, &[], &[1, 9, 4], |u| u == 4, me),
            OnScreen::Camera(4)
        );
        assert_eq!(
            speaker(None, &[], &[1, 9, 4], quiet, me),
            OnScreen::Camera(9)
        );
        // Only our own camera: ourselves.
        assert_eq!(speaker(None, &[], &[1], quiet, me), OnScreen::Camera(1));
    }

    #[test]
    fn the_window_docks_at_the_corner_its_throw_reaches() {
        let max = (1280., 720.);
        assert_eq!(docked(Corner::BottomRight, (320., 180.), max), (880., 408.));
        assert_eq!(docked(Corner::TopLeft, (320., 180.), max), (80., 80.));
        assert_eq!(docked(Corner::TopRight, (320., 180.), max), (880., 80.));
        assert_eq!(docked(Corner::BottomLeft, (320., 180.), max), (80., 408.));
        // Released left of the middle and still: the left; a fast throw to the right crosses it.
        assert_eq!(settle((300., 200.), (0., 0.), max), Corner::TopLeft);
        assert_eq!(settle((600., 200.), (0.5, 0.), max), Corner::TopRight);
        assert_eq!(settle((640., 360.), (0., 0.), max), Corner::TopLeft);
        assert_eq!(settle((900., 600.), (0., 0.), max), Corner::BottomRight);
        assert_eq!(settle((100., 500.), (0., -1.), max), Corner::TopLeft);
        assert_eq!(kept_inside((-5., 900.), (320., 180.), max), (0., 540.));
        assert_eq!(kept_inside((1100., 10.), (320., 180.), max), (960., 10.));
    }

    #[test]
    fn the_width_snaps_to_40_px_inside_its_limits() {
        assert_eq!(snap_width(339.), 320.);
        assert_eq!(snap_width(341.), 360.);
        assert_eq!(snap_width(150.), 200.);
        assert_eq!(snap_width(2000.), 960.);
        // Held, the handle goes to 980; a right corner's handle widens to the left.
        assert_eq!(resized(Corner::BottomRight, 320., 500., 400.), 420.);
        assert_eq!(resized(Corner::BottomLeft, 320., 500., 400.), 220.);
        assert_eq!(resized(Corner::TopLeft, 320., 0., 2000.), 980.);
        assert_eq!(resized(Corner::TopRight, 320., 0., 2000.), 200.);
        assert_eq!(widest(1280.), 960.);
        assert_eq!(widest(600.), 440.);
        assert_eq!(
            Corner::from_wire(Corner::TopRight.wire()),
            Some(Corner::TopRight)
        );
        assert_eq!(Corner::from_wire("x"), None);
    }
}
