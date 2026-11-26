//! The call area of a DM or group DM: the avatars of the people in the call or being rung, with the call tray.
//!
//! Sources (official modules, read in `design/snapshot/modules`):
//! - 289287 (channel view): the call shows when the channel's participants are not empty; the area sits between the
//!   header and the chat.
//! - 156652 / CSS 297468 `wrapper_hbKzX7`: `.minimum {background-color: var(--background-surface-high); height: 240px}`,
//!   `.minimum.video {height: 284px}`, `.normal {height: 50%}`.
//! - 493387 / CSS `P3YRS` `QKgWuD`: `VoiceCallParticipants`, avatars SIZE_80 (SIZE_40 when `(80 + 16) * count >
//!   width`), `participant {margin: 0 8px}`, `root {align-items: center; display: flex; height: 100%;
//!   justify-content: center; width: 100%}`, `voiceCallWrapper {padding-bottom: 24px}`.
//! - 85448 / 479464 (CSS `401425...css`): `CallAvatar`, the ringing pulse and the muted/deafened badge.
//! - 778712: avatar sizes (`SIZE_80 {status: 16, offset: 4}`, `SIZE_40 {status: 12, offset: 0}`).

use super::tooltip::{Side, Tip};
use super::{Root, avatar_source, color, icon};
use crate::parity::Parity;
use dc_state::{Id, User};
use gpui::{
    AnyElement, Context, InteractiveElement, IntoElement, MouseMoveEvent, ParentElement, Styled,
    Window, div, img, prelude::*, px,
};
use std::time::Duration;

/// `CallAvatar` `ringingType`: the person is being rung by us (outgoing) or we are rung (incoming).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RingKind {
    Outgoing,
    Incoming,
}

/// The keyframes of one property: (percent, value).
type Stops = &'static [(f64, f64)];

/// `ringingOutgoing_NdapwK:after` (2.5s, `ease-out`, infinite): the opacity keyframes.
const OUTGOING_OPACITY: Stops = &[
    (0., 0.),
    (11.56, 1.),
    (39.02, 0.),
    (39.04, 1.),
    (66.48, 0.),
    (100., 0.),
];
/// Its transform keyframes: `scaleX(1)` and `scale3d(1.1, 1.1, 1)`.
const OUTGOING_SCALE: Stops = &[
    (0., 1.),
    (39.02, 1.1),
    (39.03, 1.),
    (66.48, 1.1),
    (100., 1.),
];
/// `ringingIncoming_NdapwK:after` (5.407s, `ease-out`, infinite).
const INCOMING_OPACITY: Stops = &[
    (0., 0.),
    (0.7, 1.),
    (25.54, 0.),
    (25.56, 1.),
    (49.98, 0.),
    (50., 1.),
    (74.69, 0.),
    (74.71, 1.),
    (100., 0.),
];
const INCOMING_SCALE: Stops = &[
    (0., 1.),
    (25.54, 1.1),
    (25.55, 1.),
    (49.98, 1.1),
    (49.99, 1.),
    (74.69, 1.1),
    (74.70, 1.),
    (100., 1.1),
];

/// `--custom-call-avatar-outgoing-duration` and `--custom-call-avatar-incoming-duration`, in milliseconds.
pub const OUTGOING_MS: f64 = 2500.;
pub const INCOMING_MS: f64 = 5407.;

/// One property of the keyframes at `percent`: between two keyframes the value follows `ease-out`.
fn along(stops: Stops, percent: f64) -> f64 {
    let ease = super::motion::Ease::EaseOut;
    for pair in stops.windows(2) {
        let ((from_at, from), (to_at, to)) = (pair[0], pair[1]);
        if percent <= to_at {
            let span = to_at - from_at;
            let x = if span <= 0. {
                1.
            } else {
                (percent - from_at) / span
            };
            return from + (to - from) * ease.apply(x);
        }
    }
    stops.last().map_or(0., |last| last.1)
}

/// The ring's (opacity, scale) at `now_ms` of the animation clock.
pub fn pulse(kind: RingKind, now_ms: f64) -> (f32, f32) {
    let (duration, opacity, scale) = match kind {
        RingKind::Outgoing => (OUTGOING_MS, OUTGOING_OPACITY, OUTGOING_SCALE),
        RingKind::Incoming => (INCOMING_MS, INCOMING_OPACITY, INCOMING_SCALE),
    };
    let percent = (now_ms.rem_euclid(duration)) / duration * 100.;
    (along(opacity, percent) as f32, along(scale, percent) as f32)
}

/// What the call area shows for one person.
#[derive(Clone, Debug, PartialEq)]
pub struct Person {
    pub user: Id,
    pub ringing: bool,
    /// The mute and deafen marks (module 493387 passes `muted: isVoiceMuted()`, `deafen: isVoiceDeafened()`, and
    /// `CallAvatar` reads `isLocalMute`).
    pub marks: super::voice_marks::Marks,
    pub speaking: bool,
    pub video: bool,
}

/// Where the dragged height of the call area is kept (`callHeaderHeight` of the unsynced settings).
const CALL_HEIGHT_KEY: &str = "call_header_height";

/// A dragged height held to the official limits: at least 230px, at most the column less 200px.
pub(super) fn clamp_call_height(wanted: f32, column: f32) -> f32 {
    wanted.min(column - 200.).max(230.)
}

#[derive(Default)]
pub struct State {
    /// The call takes the whole column and the chat is hidden (`NO_CHAT`, "Hide chat" of the header).
    pub no_chat: bool,
    /// The height the call area was dragged to (`callHeaderHeight`), read from storage once.
    resized: Option<f32>,
    resized_read: bool,
    /// A drag of the bottom edge: where the pointer was and how high the area was when it began.
    resize_drag: Option<(f32, f32)>,
    /// A task redraws about thirty times a second while a ring pulses.
    pub ticking: bool,
    /// Something pulsed in the last frame.
    pub pulsing: bool,
    /// Capture only: the incoming card ignores the fixture account's Do Not Disturb.
    pub shown_online: bool,
    /// Where the incoming card was last dropped (`INCOMING_CALL_MOVE`), and a drag in progress.
    pub incoming_origin: Option<(f32, f32)>,
    pub incoming_drag: Option<super::incoming_call::CardDrag>,
    /// Capture only: the stop-ringing requests asked for (the harness refuses to send them).
    #[cfg(feature = "capture")]
    pub stops: Vec<String>,
    /// The call asked for through `GET /channels/{id}/call` is for this person (not a friend).
    pub asking: Option<dc_state::Id>,
    /// The "Start Call" prompt is up for this person.
    pub not_friend: Option<dc_state::Id>,
    /// The incoming card of this channel shows the camera preview (760571 `u`).
    pub incoming_preview: Option<dc_state::Id>,
    /// A call rings this account (its card is up).
    pub incoming: bool,
    /// Somebody is being rung by us in the current DM call.
    pub outgoing: bool,
    /// Active 180-second alone-in-DM-call timeout `(channel, generation)`.
    pub idle_timer: Option<(dc_state::Id, u64)>,
    /// Monotonic token for DM call idle timers so stale tasks never fire.
    pub idle_generation: u64,
    /// Capture only: the call controls stay shown (the idle fade would hide them before the frame is taken).
    #[cfg(feature = "capture")]
    pub pin_controls: bool,
    /// The area shows a video call (set while it is built, read by the call tray).
    pub video: bool,
}

impl Root {
    /// The people of the open DM's call: those with a voice state, then those being rung (`getUserParticipants` of
    /// module 568598: a voice state or `call.ringing.includes(user)`). Empty while the call is unavailable.
    pub(super) fn dm_call_people(&self, channel: Id) -> Vec<Person> {
        if self.stores.calls.is_call_unavailable(channel) {
            return Vec::new();
        }
        let mut out: Vec<Person> = self
            .stores
            .voice_members(0, channel)
            .into_iter()
            .map(|state| Person {
                user: state.user_id,
                ringing: false,
                marks: super::voice_marks::Marks::of(
                    state,
                    self.context
                        .mix
                        .get(&state.user_id)
                        .is_some_and(|mix| mix.1),
                ),
                speaking: self.speaking.contains(&state.user_id),
                video: state.self_video || state.self_stream,
            })
            .collect();
        if let Some(call) = self.stores.calls.get_call(channel) {
            for user in &call.ringing {
                if !out.iter().any(|person| person.user == *user) {
                    out.push(Person {
                        user: *user,
                        ringing: true,
                        marks: super::voice_marks::Marks::default(),
                        speaking: false,
                        video: false,
                    });
                }
            }
        }
        out
    }

    /// Whether the open DM shows its call area: someone is in the call or being rung.
    pub(super) fn dm_call_shown(&self) -> bool {
        self.guild.is_none()
            && self.channel.is_some_and(|channel| {
                self.current_dm().is_some() && !self.dm_call_people(channel).is_empty()
            })
    }

    /// Whether this account is in the call of `channel`.
    pub(super) fn dm_call_in(&self, channel: Option<Id>) -> bool {
        channel.is_some_and(|channel| {
            self.call
                .as_ref()
                .is_some_and(|call| call.guild == 0 && call.channel == channel)
        })
    }

    /// The call area of a DM, with the header inside it, or `None` when the call is not shown.
    /// Golden `design/dom/dm-call/in-call-alone`: `wrapper_hbKzX7 minimum_hbKzX7` at (375, 33) 905 x 240, its
    /// `root_rTsZa_` black, the header (`topControls_rTsZa_`) in the top 49px, one 80px avatar centred in the 240px
    /// box (the 24px bottom padding of `voiceCallWrapper` is outside the content box), the controls 16px from the
    /// bottom.
    pub(super) fn dm_call_area(
        &mut self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if self.guild.is_some() {
            return None;
        }
        let channel = self.channel?;
        self.current_dm()?;
        let people = self.dm_call_people(channel);
        if people.is_empty() {
            self.dmc.pulsing = false;
            return None;
        }
        let in_call = self
            .call
            .as_ref()
            .is_some_and(|call| call.guild == 0 && call.channel == channel);
        let video = people.iter().any(|person| person.video)
            || (in_call && self.call.as_ref().is_some_and(|call| call.camera));
        let size = window.viewport_size();
        let width = f32::from(size.width) - self.combined_sidebar_width();
        // The minimum layout keeps its controls (golden `in-call-alone` without hover shows them); the video
        // layouts hide them after the idle time.
        let shown = !video || self.controls_shown();
        self.dmc.video = video;
        self.dmc.pulsing = people.iter().any(|person| person.ringing);
        if self.dmc.pulsing {
            self.start_dm_call_tick(cx);
        }
        // `.minimum` is 240px high, `.normal` half the column and `.noChat` all of it (stylesheet of module 880162).
        // A height the bottom edge was dragged to replaces the first two, between 230px and the column less 200px
        // (module 540462: `minHeight: 230`, `maxHeight: height - 200`).
        let column = f32::from(size.height) - 32.;
        if !self.dmc.resized_read {
            self.dmc.resized_read = true;
            self.dmc.resized = crate::storage::get(CALL_HEIGHT_KEY)
                .and_then(|value| value.as_f64())
                .map(|height| height as f32);
        }
        let no_chat = video && in_call && self.dmc.no_chat;
        let target = if no_chat {
            column
        } else if video && in_call && self.call_popped_out() {
            // `.wrapper.poppedOut`.
            154.
        } else {
            match self.dmc.resized {
                Some(resized) => clamp_call_height(resized, column),
                None if video => column * 0.5,
                None => 240.,
            }
        };
        // `.wrapper.animated`: `transition: height .35s ease`. A drag follows the pointer at once.
        let height = super::motion::value(
            super::motion::key(&"dm-call-height"),
            target,
            super::motion::css(
                if self.dmc.resize_drag.is_some() {
                    0.
                } else {
                    350.
                },
                super::motion::Ease::Ease,
            ),
        );
        let body: AnyElement = if video && in_call && self.call_popped_out() {
            self.call_popped_out_message()
        } else if video && in_call && self.vv.fullscreen {
            // Full Screen draws the call in its own layer.
            div().into_any_element()
        } else if video && in_call {
            self.call_view(0, channel, width, height, cx)
        } else if video {
            // Module 874739 with `inCall` false and the mode `VIDEO`: the grid of everyone, a stream as a tile with
            // Watch Stream.
            self.vv.outside = true;
            let grid = self.call_view(0, channel, width, height, cx);
            self.vv.outside = false;
            grid
        } else {
            self.vv.shown_call = (0, channel);
            if in_call {
                // Module 540462: voice-only DM mode has no focused video/stream tile (`mode === VOICE`).
                // Clear any stale focus left behind when a stream ended so neither the bottom-right volume
                // control nor the header LIVE badge leaks into voice-only mode.
                self.vv.focus.grid();
                self.video.ended_focus = None;
            }
            self.dm_call_avatars(&people, width, cx)
        };
        let header = self.header_with(cx, true).into_any_element();
        let people_ring_me = self
            .stores
            .current_user
            .as_ref()
            .is_some_and(|me| self.stores.calls.is_ringing(channel, me.id));
        Some(
            div()
                .parity("dm-call")
                .id("dm-call-area")
                .relative()
                // Module 156652: a press wakes the controls as a move does; the pointer leaving the area makes it idle at once.
                .capture_any_mouse_down(cx.listener(|this, _: &gpui::MouseDownEvent, _, cx| {
                    this.voice_view_moved(cx);
                }))
                .on_hover(cx.listener(|this, inside: &bool, _, cx| {
                    if !*inside {
                        this.voice_view_left(cx);
                    }
                }))
                .flex_none()
                .w_full()
                .h(px(height))
                .overflow_hidden()
                // `root_rTsZa_` paints `rgb(0, 0, 0)` over the wrapper's `--background-surface-high`.
                .bg(color("--black"))
                .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, _, cx| {
                    if event.pressed_button.is_none() {
                        this.call_drags_end();
                    }
                    this.voice_view_moved(cx);
                }))
                .on_mouse_up(
                    gpui::MouseButton::Left,
                    cx.listener(|this, _, _, _| {
                        this.call_drags_end();
                    }),
                )
                .child(body)
                // The DM's header is the call's `topControls` (`renderExternalHeader`, module 540462): with video in the
                // call it fades out and 8px up when idle (`idle: !forceShowControls && inCall && layout != MINIMUM`).
                .child({
                    let t = self.call_idle_t(!(video && in_call) || self.controls_shown());
                    div()
                        .absolute()
                        .top(px(-8. * (1. - t)))
                        .left_0()
                        .right_0()
                        .opacity(t)
                        .child(header)
                })
                // `renderBottomCenter`: outside the call the tray of module 105225 is the join buttons, in the voice
                // and the video mode alike.
                .when(!in_call, |this| {
                    this.child(self.dm_join_tray(channel, people_ring_me, cx))
                })
                .when(in_call && !video, |this| {
                    this.child(self.call_bar(0, channel, shown, cx))
                })
                .when(!video, |this| {
                    this.children(self.voice_menus(channel, width, height, cx))
                })
                .when(!no_chat, |this| {
                    this.child(self.dm_call_resize_handle(height, cx))
                })
                .into_any_element(),
        )
    }

    /// `resizeHandle_poo5Jt` (module 587837): 4px across the bottom edge of the area with the `ns-resize` cursor. A
    /// press starts a drag; the pointer then sets the height until the button is let go, and the height is kept.
    fn dm_call_resize_handle(&self, height: f32, cx: &mut Context<Self>) -> AnyElement {
        let weak = cx.entity().downgrade();
        let dragging = self.dmc.resize_drag.is_some();
        div()
            .id("dm-call-resize")
            .absolute()
            .left_0()
            .right_0()
            .bottom_0()
            .h(px(4.))
            .cursor(gpui::CursorStyle::ResizeUpDown)
            .on_mouse_down(
                gpui::MouseButton::Left,
                cx.listener(move |this, event: &gpui::MouseDownEvent, _, cx| {
                    this.dmc.resize_drag = Some((f32::from(event.position.y), height));
                    cx.stop_propagation();
                    cx.notify();
                }),
            )
            .when(dragging, |handle| {
                handle.child(
                    gpui::canvas(
                        |_, _, _| (),
                        move |_, (), window, _| {
                            let moved = weak.clone();
                            window.on_mouse_event(
                                move |event: &MouseMoveEvent, phase, window, cx| {
                                    if phase != gpui::DispatchPhase::Capture {
                                        return;
                                    }
                                    let column = f32::from(window.viewport_size().height) - 32.;
                                    let _ = moved.update(cx, |this, cx| {
                                        if let Some((from, start)) = this.dmc.resize_drag {
                                            let wanted = start + f32::from(event.position.y) - from;
                                            this.dmc.resized =
                                                Some(clamp_call_height(wanted, column));
                                            cx.notify();
                                        }
                                    });
                                },
                            );
                            let released = weak.clone();
                            window.on_mouse_event(move |_: &gpui::MouseUpEvent, phase, _, cx| {
                                if phase != gpui::DispatchPhase::Capture {
                                    return;
                                }
                                let _ = released.update(cx, |this, cx| {
                                    if this.dmc.resize_drag.take().is_some() {
                                        if let Some(height) = this.dmc.resized {
                                            crate::storage::set(
                                                CALL_HEIGHT_KEY,
                                                serde_json::json!(height),
                                            );
                                        }
                                        cx.notify();
                                    }
                                });
                            });
                        },
                    )
                    .absolute()
                    .size_0(),
                )
            })
            .into_any_element()
    }

    /// "Hide chat" and "Show chat" of a direct message video call (`handleToggleLayout`: `NORMAL` and `NO_CHAT`).
    pub(super) fn toggle_dm_call_chat(&mut self, cx: &mut Context<Self>) {
        self.dmc.no_chat = !self.dmc.no_chat;
        cx.notify();
    }

    /// Whether the header of the open direct message offers "Hide chat" (`canChangeLayout`: a private channel, in
    /// the call, and the call has video or a stream).
    pub(super) fn dm_call_can_hide_chat(&self) -> bool {
        self.dmc.video && self.dm_call_in(self.channel)
    }

    pub(super) fn dm_call_chat_hidden(&self) -> bool {
        self.dmc.no_chat && self.dm_call_can_hide_chat()
    }

    /// The tray of a call this account has not joined (`eb` in module 105225): join with video, Join Call and, while
    /// the call rings this account, Decline. `wrapper_9bvEB0` is a row with a 12px gap; each button is a
    /// `centerButton_K9Xq13.fullRegionButton_K9Xq13` (66 x 50, radius 12, `--border-muted` border, 20px icon) coloured
    /// `join` (`--green-360`, white icon) or `disconnect` (`--control-critical-primary-background-*`).
    fn dm_join_tray(
        &mut self,
        channel: Id,
        ringing_me: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let button = |key: &'static str,
                      fill: &'static str,
                      hover: &'static str,
                      glyph: &'static str,
                      tip: &'static str,
                      cx: &mut Context<Self>| {
            self.with_tip(
                key,
                Tip::text(tip, Side::Top),
                div()
                    .w(px(66.))
                    .h(px(50.))
                    .rounded(px(12.))
                    .border_1()
                    .border_color(color("--border-muted"))
                    .bg(color(fill))
                    .hover(|style| style.bg(color(hover)))
                    .flex()
                    .items_center()
                    .justify_center()
                    .cursor_pointer()
                    .child(icon(glyph, 20., "--white")),
                cx,
            )
        };
        let join_video = button(
            "dm-join-video",
            "--green-360",
            "--green-360",
            "dm_video",
            dc_intl::t!("YbjQQ/"),
            cx,
        );
        let join_call = button(
            "dm-join-call",
            "--green-360",
            "--green-360",
            "dm_call",
            dc_intl::t!("0D/6Rz"),
            cx,
        );
        let decline = ringing_me.then(|| {
            button(
                "dm-decline-call",
                "--control-critical-primary-background-default",
                "--control-critical-primary-background-hover",
                "close",
                dc_intl::t!("BVN4pL"),
                cx,
            )
        });
        div()
            .absolute()
            .left_0()
            .right_0()
            .bottom(px(16.))
            .h(px(50.))
            .flex()
            .items_center()
            .justify_center()
            .gap(px(12.))
            .child(join_video.on_click(cx.listener(move |this, _, _, cx| {
                this.tip = None;
                this.join_dm_call(channel, true, cx);
            })))
            .child(join_call.on_click(cx.listener(move |this, _, _, cx| {
                this.tip = None;
                this.join_dm_call(channel, false, cx);
            })))
            .children(decline.map(|button| {
                button.on_click(cx.listener(move |this, _, _, cx| {
                    this.tip = None;
                    // `710801`: `stopRinging(channel.id)`, no recipients.
                    this.stop_ringing(channel, None);
                    cx.notify();
                }))
            }))
            .into_any_element()
    }

    /// `VoiceCallParticipants` (module 874739 -> 493387): the avatars in a row, centred; right-click opens the call
    /// user context menu (left-click has no `onClick`).
    fn dm_call_avatars(
        &mut self,
        people: &[Person],
        width: f32,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let small = (80. + 16.) * people.len() as f32 > width;
        let size = if small { 40. } else { 80. };
        let avatars: Vec<AnyElement> = people
            .iter()
            .map(|person| {
                let id = person.user;
                let user = self
                    .stores
                    .users
                    .get(&id)
                    .cloned()
                    .or_else(|| self.stores.current_user.clone().filter(|me| me.id == id))
                    .unwrap_or_default();
                let (key, url) = avatar_source(&user);
                self.request_image(key, url);
                let face = self.call_avatar(&user, person, size);
                // Not in the pinned client: Discord's native client names the person above the avatar on hover
                // (`dm-call-avatar-item`), with the app's ordinary tooltip.
                let name = user.display_name().to_owned();
                let avatar = div()
                    .id(gpui::SharedString::from(format!("dm-call-avatar-{id}")))
                    .mx(px(8.))
                    .on_mouse_up(
                        gpui::MouseButton::Right,
                        cx.listener(move |this, event: &gpui::MouseUpEvent, _, cx| {
                            cx.stop_propagation();
                            this.open_tile_menu(
                                super::call_menus::Subject::User(id),
                                false,
                                event.position,
                                cx,
                            );
                        }),
                    )
                    .child(face);
                self.with_tip(
                    format!("dm-call-avatar-tip-{id}"),
                    Tip::body(super::tooltip::Body::Text(name.into()), Side::Top),
                    avatar,
                    cx,
                )
                .into_any_element()
            })
            .collect();
        div()
            .absolute()
            .inset_0()
            .flex()
            .items_center()
            .justify_center()
            .children(avatars)
            .into_any_element()
    }

    /// `CallAvatar` (module 85448).
    fn call_avatar(&self, user: &User, person: &Person, size: f32) -> AnyElement {
        // The avatars sit on the call area's black (`root_rTsZa_`).
        self.call_avatar_kind(user, person, size, RingKind::Outgoing, "--black")
    }

    /// `backdrop`: the colour behind the avatar, which the hole around a mute or deafen badge shows.
    pub(super) fn call_avatar_kind(
        &self,
        user: &User,
        person: &Person,
        size: f32,
        kind: RingKind,
        backdrop: &'static str,
    ) -> AnyElement {
        let picture = self.images.get(&avatar_source(user).0).cloned();
        let ringing = person.ringing;
        // `.voiceAvatar.ringingOutgoing {opacity: .3}`.
        let face = div()
            .size(px(size))
            .rounded_full()
            .overflow_hidden()
            .bg(color("--background-mod-strong"))
            .when(ringing && kind == RingKind::Outgoing, |this| {
                this.opacity(0.3)
            })
            .children(picture.map(|picture| img(picture).size(px(size)).rounded_full()));
        // `getAvatarSizeInfo`: SIZE_80 {status 16, offset 4}, SIZE_40 {status 12, offset 0}.
        let (status, offset) = super::voice_marks::call_status_size(size);
        let mark = person.marks.call_avatar();
        // `statusContainer_NdapwK` (red-400 disk, bottom/end 0) with the icon in white at the status size.
        let badge = mark.map(|mark| {
            div()
                .absolute()
                .bottom_0()
                .right_0()
                .size(px(status + 2. * offset))
                .rounded_full()
                .bg(color("--red-400"))
                .flex()
                .items_center()
                .justify_center()
                .child(icon(mark, status, "--white"))
        });
        // `svg-mask-avatar-call-status-*`: with a mark the avatar is masked with a hole around the badge, so the
        // backdrop shows in a ring between the two. The hole is painted in the backdrop's colour.
        let hole = mark.map(|_| {
            let (centre, radius) = super::voice_marks::call_status_hole(size);
            div()
                .absolute()
                .left(px(centre - radius))
                .top(px(centre - radius))
                .size(px(2. * radius))
                .rounded_full()
                .bg(color(backdrop))
        });
        // `U = !muted && !deafen && !isLocalMute`: the speaking border is drawn only without a mark.
        let (inner, backing) = super::motion::messages::speaking_rings(
            person.user,
            person.speaking && !person.marks.any(),
        );
        let ring = ringing.then(|| {
            let (opacity, scale) = pulse(kind, super::motion::now());
            // `:after {border: 3px solid var(--interactive-text-active); border-radius: 100%; inset: -3px}`
            // under `transform: scale()` about the centre; 1px at inset -1px for the 24px and 32px sizes.
            let (border, gap) = if size <= 32. { (1., 1.) } else { (3., 3.) };
            let side = (size + 2. * gap) * scale;
            let shift = (side - size) / 2.;
            div()
                .absolute()
                .left(px(-shift))
                .top(px(-shift))
                .size(px(side))
                .rounded_full()
                .border(px(border))
                .border_color(color("--interactive-text-active"))
                .opacity(opacity)
        });
        div()
            .relative()
            .size(px(size))
            .child(face)
            .child(
                div()
                    .absolute()
                    .inset_0()
                    .child(
                        div()
                            .absolute()
                            .inset_0()
                            .rounded_full()
                            .border(px(backing))
                            .border_color(color("--background-base-lower")),
                    )
                    .child(
                        div()
                            .absolute()
                            .inset_0()
                            .rounded_full()
                            .border(px(inner))
                            .border_color(color("--status-speaking")),
                    ),
            )
            .children(ring)
            .children(hole)
            .children(badge)
            .into_any_element()
    }

    /// About thirty redraws a second while a ring pulses.
    pub(super) fn start_dm_call_tick(&mut self, cx: &mut Context<Self>) {
        if self.dmc.ticking {
            return;
        }
        self.dmc.ticking = true;
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(33))
                    .await;
                let Ok(keep) = this.update(cx, |this, cx| {
                    this.ring_reconcile(cx);
                    let keep = this.dmc.pulsing || this.dmc.incoming || this.dmc.outgoing;
                    if keep {
                        cx.notify();
                    } else {
                        this.dmc.ticking = false;
                    }
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_outgoing_ring_starts_invisible_and_peaks_at_11_56_percent() {
        let (opacity, scale) = pulse(RingKind::Outgoing, 0.);
        assert_eq!((opacity, scale), (0., 1.));
        let (opacity, _) = pulse(RingKind::Outgoing, OUTGOING_MS * 0.1156);
        assert!((opacity - 1.).abs() < 1e-3, "{opacity}");
    }

    #[test]
    fn the_outgoing_ring_fades_out_growing_to_1_1_and_pulses_twice() {
        let (opacity, scale) = pulse(RingKind::Outgoing, OUTGOING_MS * 0.3902);
        assert!(
            opacity < 1e-3 && (scale - 1.1).abs() < 1e-3,
            "{opacity} {scale}"
        );
        // The second pulse starts at 39.04 %.
        let (opacity, scale) = pulse(RingKind::Outgoing, OUTGOING_MS * 0.3904);
        assert!(
            (opacity - 1.).abs() < 1e-3 && scale < 1.01,
            "{opacity} {scale}"
        );
        let (opacity, _) = pulse(RingKind::Outgoing, OUTGOING_MS * 0.9);
        assert_eq!(opacity, 0.);
    }

    #[test]
    fn the_incoming_ring_pulses_three_times_in_5_407_ms() {
        let peaks = [0.007, 0.2556, 0.5, 0.7471];
        for at in peaks {
            let (opacity, _) = pulse(RingKind::Incoming, INCOMING_MS * at);
            assert!((opacity - 1.).abs() < 1e-3, "{at}: {opacity}");
        }
        let (opacity, scale) = pulse(RingKind::Incoming, INCOMING_MS * 0.9998);
        assert!(opacity < 0.01 && scale > 1.09, "{opacity} {scale}");
    }

    #[test]
    fn the_clock_wraps() {
        assert_eq!(
            pulse(RingKind::Outgoing, 100.),
            pulse(RingKind::Outgoing, 100. + OUTGOING_MS)
        );
    }
    /// Every icon name used by the call views, the incoming card and the DM header buttons exists as an asset, and is
    /// a plain mask: a full-size `<rect>` in the svg (the Lottie exports) draws as a white square when tinted.
    #[test]
    fn the_icons_used_by_the_call_views_resolve() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/icons");
        let mut names = vec!["dm_call", "dm_video", "close", "more_horizontal"];
        for file in [
            "dm_call_view.rs",
            "incoming_call.rs",
            "dm_call.rs",
            "live_activity.rs",
            "voice_view.rs",
            "voice.rs",
            "voice_panel.rs",
        ] {
            let text = std::fs::read_to_string(
                std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("src/ui")
                    .join(file),
            )
            .unwrap();
            let text = text
                .split("#[cfg(test)]")
                .next()
                .unwrap_or_default()
                .to_owned();
            for part in text.split("icon(").skip(1) {
                let part = part.trim_start();
                if let Some(rest) = part.strip_prefix('"')
                    && let Some(end) = rest.find('"')
                    && rest[..end]
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || c == '_')
                {
                    names.push(Box::leak(rest[..end].to_owned().into_boxed_str()));
                }
            }
        }
        for name in names {
            let svg = std::fs::read_to_string(dir.join(format!("{name}.svg")))
                .unwrap_or_else(|_| panic!("icon {name} has no asset"));
            // Rects inside defs, clip paths and masks only shape the glyph; a bare one of the full viewBox size is a
            // filled square once the svg is used as a mask.
            let mut bare = svg.clone();
            for (open, close) in [
                ("<defs>", "</defs>"),
                ("<clipPath", "</clipPath>"),
                ("<mask", "</mask>"),
            ] {
                while let (Some(from), Some(to)) = (bare.find(open), bare.find(close)) {
                    if to < from {
                        break;
                    }
                    bare.replace_range(from..to + close.len(), "");
                }
            }
            let square = bare.split("<rect").skip(1).any(|rect| {
                let tag = rect.split('>').next().unwrap_or_default();
                !tag.contains("mask=")
                    && tag.contains("width=\"24\"")
                    && tag.contains("height=\"24\"")
            });
            assert!(
                !square,
                "icon {name} has a full-size filled rect and draws as a square"
            );
        }
    }
}

#[cfg(test)]
mod resize_tests {
    use super::clamp_call_height;

    #[test]
    fn a_dragged_call_height_stays_between_230_and_the_column_less_200() {
        assert_eq!(clamp_call_height(100., 688.), 230.);
        assert_eq!(clamp_call_height(400., 688.), 400.);
        assert_eq!(clamp_call_height(900., 688.), 488.);
        // A column too short for both limits keeps the lower one.
        assert_eq!(clamp_call_height(300., 400.), 230.);
    }
}
