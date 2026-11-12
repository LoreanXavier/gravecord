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
    pub mute: bool,
    pub deaf: bool,
    pub speaking: bool,
    pub video: bool,
}

#[derive(Default)]
pub struct State {
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
                mute: state.self_mute || state.mute,
                deaf: state.self_deaf || state.deaf,
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
                        mute: false,
                        deaf: false,
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
        let width = f32::from(size.width) - 375.;
        // The minimum layout keeps its controls (golden `in-call-alone` without hover shows them); the video
        // layouts hide them after the idle time.
        let shown = !video || self.controls_shown();
        self.dmc.video = video;
        self.dmc.pulsing = people.iter().any(|person| person.ringing);
        if self.dmc.pulsing {
            self.start_dm_call_tick(cx);
        }
        // `.minimum` is 240px high, `.normal` half the column (module 156652).
        let height = if video {
            (f32::from(size.height) - 32.) * 0.5
        } else {
            240.
        };
        let body: AnyElement = if video && in_call && self.vv.fullscreen {
            // Full Screen draws the call in its own layer.
            div().into_any_element()
        } else if video && in_call {
            self.call_view(0, channel, width, height, cx)
        } else {
            self.dm_call_avatars(&people, width)
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
                .flex_none()
                .w_full()
                .h(px(height))
                .overflow_hidden()
                // `root_rTsZa_` paints `rgb(0, 0, 0)` over the wrapper's `--background-surface-high`.
                .bg(color("--black"))
                .on_mouse_move(cx.listener(|this, _: &MouseMoveEvent, _, cx| {
                    this.voice_view_moved(cx);
                }))
                .child(body)
                .child(div().absolute().top_0().left_0().right_0().child(header))
                .when(!in_call && !video, |this| {
                    this.child(self.dm_join_tray(channel, people_ring_me, cx))
                })
                .when(in_call && !video, |this| {
                    this.child(self.call_bar(0, channel, shown, cx))
                        .children(self.voice_menus(channel, width, height, cx))
                })
                .into_any_element(),
        )
    }

    /// The tray of a call this account has not joined (`eb` in module 105225): join with video, Join Call and, while
    /// the call rings this account, Decline. `wrapper_9bvEB0` is a row with a 12px gap; each button is a
    /// `centerButton_K9Xq13` (radius 8, padding 10, a 20px icon) coloured `join` (`--green-360`, white icon) or
    /// `disconnect` (`--control-critical-primary-background-*`). Unconfirmed until a real incoming call is captured.
    fn dm_join_tray(
        &mut self,
        channel: Id,
        ringing_me: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let button =
            |key: &'static str, fill: &'static str, hover: &'static str, glyph: &'static str| {
                div()
                    .id(key)
                    .size(px(40.))
                    .rounded(px(8.))
                    .bg(color(fill))
                    .hover(|style| style.bg(color(hover)))
                    .flex()
                    .items_center()
                    .justify_center()
                    .cursor_pointer()
                    .child(icon(glyph, 20., "--white"))
            };
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
            .child(
                button("dm-join-video", "--green-360", "--green-360", "dm_video").on_click(
                    cx.listener(move |this, _, _, cx| this.join_dm_call(channel, true, cx)),
                ),
            )
            .child(
                button("dm-join-call", "--green-360", "--green-360", "dm_call").on_click(
                    cx.listener(move |this, _, _, cx| this.join_dm_call(channel, false, cx)),
                ),
            )
            .when(ringing_me, |this| {
                this.child(
                    button(
                        "dm-decline-call",
                        "--control-critical-primary-background-default",
                        "--control-critical-primary-background-hover",
                        "close",
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        // `710801`: `stopRinging(channel.id)`, no recipients.
                        this.stop_ringing(channel, None);
                        cx.notify();
                    })),
                )
            })
            .into_any_element()
    }

    /// `VoiceCallParticipants`: the avatars in a row, centred.
    fn dm_call_avatars(&mut self, people: &[Person], width: f32) -> AnyElement {
        let small = (80. + 16.) * people.len() as f32 > width;
        let size = if small { 40. } else { 80. };
        let avatars: Vec<AnyElement> = people
            .iter()
            .map(|person| {
                let user = self
                    .stores
                    .users
                    .get(&person.user)
                    .cloned()
                    .or_else(|| {
                        self.stores
                            .current_user
                            .clone()
                            .filter(|me| me.id == person.user)
                    })
                    .unwrap_or_default();
                let (key, url) = avatar_source(&user);
                self.request_image(key, url);
                self.call_avatar(&user, person, size)
            })
            .collect();
        div()
            .absolute()
            .inset_0()
            .flex()
            .items_center()
            .justify_center()
            .children(
                avatars
                    .into_iter()
                    .map(|avatar| div().mx(px(8.)).child(avatar)),
            )
            .into_any_element()
    }

    /// `CallAvatar` (module 85448).
    fn call_avatar(&self, user: &User, person: &Person, size: f32) -> AnyElement {
        self.call_avatar_kind(user, person, size, RingKind::Outgoing)
    }

    pub(super) fn call_avatar_kind(
        &self,
        user: &User,
        person: &Person,
        size: f32,
        kind: RingKind,
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
        let (status, offset) = if size >= 80. { (16., 4.) } else { (12., 0.) };
        let badge = (person.mute || person.deaf).then(|| {
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
                .child(icon(
                    if person.deaf {
                        "headphones_deafened"
                    } else {
                        "mic_muted"
                    },
                    status,
                    "--white",
                ))
        });
        let (inner, backing) =
            super::motion::messages::speaking_rings(person.user, person.speaking);
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
                    let keep = this.dmc.pulsing || this.dmc.incoming;
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
