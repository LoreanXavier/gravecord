//! The soundboard wheel over the overlay (module 177303, shown by `eQ` of module 427830 while
//! `SoundboardOverlayStore` is enabled; the rules are `dc_overlay::wheel`).
//!
//! - The Open Soundboard (Hold) keybind (`SOUNDBOARD_HOLD`, module 596339) with a game in front: down shows the wheel
//!   (`hK` of module 944775, `keepOpen: false`), up hides it (`Lk`), and hiding a held wheel plays the sound the pointer
//!   was on (the unmount effect of `k`). The Soundboard keybind (`SOUNDBOARD`) opens it to stay and closes it again.
//!   Only in a voice channel the soundboard may be used in (`lI` of module 123973: a call, or a server voice channel
//!   with Use Soundboard and Speak).
//! - In the app with no game in front the keybind opens a popout at the pointer only under the user experiment
//!   `2026-05-global-soundboard-wheel-popout`, off by default; this client does nothing there.
//! - The wheel is a clickable region of the overlay, drawn locked or unlocked. A click on the selected sound plays it
//!   and closes the wheel; the X in the middle (a held wheel) closes it.
//!
//! In a harness run a sound the wheel would play is only recorded (`overlay wheel state`): nothing reaches the speaker.

use super::Root;
use super::overlay::onyx;
use dc_overlay::wheel;
use gpui::{AnyElement, Context, IntoElement, div, prelude::*, px, rgba, svg};

/// `USE_SOUNDBOARD` and `SPEAK` (module 818348).
const USE_SOUNDBOARD: u64 = 1 << 42;
const SPEAK: u64 = 1 << 21;

/// The open wheel.
#[derive(Clone, Debug, Default)]
pub(super) struct Wheel {
    pub keep_open: bool,
    pub page: usize,
    /// The slot of the page under the pointer's ray (`activeItem`).
    pub selected: Option<usize>,
    /// The pointer in the wheel's own pixels, while over it.
    pub pointer: Option<(f32, f32)>,
}

impl Root {
    /// `lI` of module 123973: the voice channel we are in lets us use the soundboard.
    pub(super) fn soundboard_usable(&self) -> bool {
        let Some(call) = self.call.as_ref() else {
            return false;
        };
        if call.guild == 0 {
            return true;
        }
        let (Some(guild), Some(me)) = (self.stores.guild(call.guild), self.stores.me()) else {
            return false;
        };
        let Some(channel) = guild.channels.iter().find(|known| known.id == call.channel) else {
            return false;
        };
        dc_state::permissions::can_in_channel(guild, channel, me, USE_SOUNDBOARD)
            && dc_state::permissions::can_in_channel(guild, channel, me, SPEAK)
    }

    /// `SOUNDBOARD_HOLD` going down or up.
    pub(super) fn soundboard_hold_keybind(&mut self, down: bool, cx: &mut Context<Self>) {
        if self.overlay_target().is_some() {
            if down {
                if self.soundboard_usable() {
                    self.overlay_wheel_open(false);
                }
            } else {
                self.overlay_wheel_close(cx);
            }
            cx.notify();
            return;
        }
        // In the app: the popout of `Ax` needs `2026-05-global-soundboard-wheel-popout` (default `enabled: false`).
        tracing::debug!(
            "soundboard wheel: no game in front; the app popout is behind an experiment that is off"
        );
    }

    /// `SOUNDBOARD` (on the key going up): the wheel opens to stay, or closes.
    pub(super) fn soundboard_toggle_keybind(&mut self, cx: &mut Context<Self>) {
        if self.overlay_target().is_none() {
            return;
        }
        match wheel::toggle(self.overlay.wheel.is_some()) {
            Some(shown) => {
                if self.soundboard_usable() {
                    self.overlay_wheel_open(shown.keep_open);
                }
            }
            None => self.overlay_wheel_close(cx),
        }
        cx.notify();
    }

    /// `hK`: `SOUNDBOARD_SET_OVERLAY_ENABLED {enabled: true}`; the sounds are asked for (`f.E7()`, `loadIfNecessary`).
    pub(super) fn overlay_wheel_open(&mut self, keep_open: bool) {
        self.soundboard_load_for_wheel();
        self.overlay.wheel = Some(Wheel {
            keep_open,
            ..Wheel::default()
        });
        tracing::info!("soundboard wheel shown (keep open {keep_open})");
    }

    /// `Lk`: the wheel goes; a held one plays its selected sound as it goes.
    pub(super) fn overlay_wheel_close(&mut self, cx: &mut Context<Self>) {
        let Some(open) = self.overlay.wheel.take() else {
            return;
        };
        if !open.keep_open
            && let Some(sound) = self.overlay_wheel_selected_sound(&open)
        {
            self.overlay_wheel_play(sound, cx);
        }
        tracing::info!("soundboard wheel hidden");
    }

    fn overlay_wheel_selected_sound(&self, open: &Wheel) -> Option<super::soundboard::Sound> {
        let slot = open.selected?;
        self.soundboard_wheel_sounds()
            .get(open.page * wheel::PLACES.len() + slot)
            .cloned()
    }

    /// `p.Ak`: the sound goes to the channel and plays here (in a harness run it is only recorded).
    fn overlay_wheel_play(&mut self, sound: super::soundboard::Sound, cx: &mut Context<Self>) {
        tracing::info!("soundboard wheel plays a sound");
        if crate::storage::is_capture_run() {
            self.overlay.wheel_played = Some(sound.name.clone());
            return;
        }
        self.soundboard_click_sound(&sound, cx);
    }

    /// The pointer moved over the wheel (`K`, throttled to 16 ms in the official client).
    fn overlay_wheel_pointer(&mut self, at: (f32, f32)) {
        let count = self.soundboard_wheel_sounds().len();
        let Some(open) = self.overlay.wheel.as_mut() else {
            return;
        };
        let on_page = count
            .saturating_sub(open.page * wheel::PLACES.len())
            .min(wheel::PLACES.len());
        open.pointer = Some(at);
        open.selected = wheel::select(at, (wheel::SIZE, wheel::SIZE), on_page);
    }

    /// The wheel's scroll (`B`): the next or the previous page.
    fn overlay_wheel_scroll(&mut self, delta_y: f32) {
        let count = self.soundboard_wheel_sounds().len();
        if let Some(open) = self.overlay.wheel.as_mut() {
            let (page, selected) = wheel::scroll(open.page, delta_y, count, open.selected);
            open.page = page;
            open.selected = selected;
        }
    }

    /// The wheel over the whole overlay (`container_kK3yeM`), while it is open in a voice channel.
    pub(super) fn overlay_wheel_layer(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let open = self.overlay.wheel.clone()?;
        self.call.as_ref()?;
        let sounds = self.soundboard_wheel_sounds();
        if sounds.is_empty() {
            return None;
        }
        let side = wheel::SIZE;
        let scale = side / wheel::VIEW;
        let pages = wheel::pages(sounds.len());
        let page: Vec<super::soundboard::Sound> = sounds
            .iter()
            .skip(open.page * wheel::PLACES.len())
            .take(wheel::PLACES.len())
            .cloned()
            .collect();
        let surface = onyx("--background-surface-high");
        let ring_outer = (wheel::RING_RADIUS + wheel::RING_STROKE / 2.) * scale;
        let dead = wheel::DEAD_ZONE * scale;
        // `chatWheelBackground`: the ring, `--background-surface-high` at 0.9.
        // GAP: the SVG's drop shadow (dy 8, blur 8, 16% black) is left out: a gpui shadow fills the ring's middle too.
        let ring = div()
            .absolute()
            .left(px(side / 2. - ring_outer))
            .top(px(side / 2. - ring_outer))
            .size(px(2. * ring_outer))
            .rounded_full()
            .border(px(wheel::RING_STROKE * scale))
            .border_color(surface)
            .opacity(0.9);
        // `chatWheelCenter`: the indicator, half the pointer's offset inside the ring, in SVG units.
        let offset = open
            .pointer
            .map(|at| wheel::indicator(at, (side, side)))
            .unwrap_or((0., 0.));
        let moved = offset.0.abs() + offset.1.abs() > 0.;
        let indicator = moved.then(|| {
            let mut fill = surface;
            fill.a *= 0.6;
            div()
                .absolute()
                .left(px(side / 2. + offset.0 * scale - dead))
                .top(px(side / 2. + offset.1 * scale - dead))
                .size(px(2. * dead))
                .rounded_full()
                .bg(fill)
        });
        // `chatWheelDeadZone`: only while held; transparent, 0.3 under the pointer.
        let dead_zone = (!open.keep_open).then(|| {
            div()
                .id("soundboard-wheel-dead-zone")
                .absolute()
                .left(px(side / 2. - dead))
                .top(px(side / 2. - dead))
                .size(px(2. * dead))
                .rounded_full()
                .hover(move |style| {
                    let mut fill = surface;
                    fill.a *= 0.3;
                    style.bg(fill)
                })
        });
        let items: Vec<AnyElement> = page
            .iter()
            .enumerate()
            .filter_map(|(index, sound)| {
                let (left, top, width, height) = wheel::slot(index, (side, side))?;
                Some(
                    div()
                        .absolute()
                        .left(px(left))
                        .top(px(top))
                        .w(px(width))
                        .h(px(height))
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(self.soundboard_wheel_item(sound, open.selected == Some(index)))
                        .into_any_element(),
                )
            })
            .collect();
        // `innerContent`: the close X (a held wheel) and "Scroll for more".
        let inner = div()
            .absolute()
            .left_0()
            .right_0()
            .bottom_0()
            .top(px(side / 2. - 16.))
            .flex()
            .flex_col()
            .items_center()
            .when(!open.keep_open, |this| {
                this.child(
                    div()
                        .id("soundboard-wheel-close")
                        .size(px(48.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .cursor_pointer()
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.overlay.wheel = None;
                            cx.notify();
                        }))
                        // `N`: the X drawn in a 48 px box (`chatWheelDeadZoneIcon`, white).
                        .child(
                            svg()
                                .path("icons/soundboard_wheel_close.svg")
                                .size(px(48.))
                                .text_color(onyx("--white")),
                        ),
                )
            })
            .when(pages > 1, |this| {
                this.child(
                    div()
                        .mt(px(8.))
                        .p(px(4.))
                        .rounded(px(4.))
                        .bg(rgba(0x00000047))
                        .text_size(px(12.))
                        .line_height(px(16.))
                        .text_color(onyx("--white"))
                        .child(dc_intl::t("Xy+S02")),
                )
            });
        let body = div()
            .id("soundboard-wheel")
            .relative()
            .size(px(side))
            .on_mouse_move(cx.listener(|this, event: &gpui::MouseMoveEvent, _, cx| {
                let bounds = this
                    .overlay
                    .bounds
                    .borrow()
                    .get("soundboard-wheel")
                    .copied();
                if let Some(bounds) = bounds {
                    let at = (
                        f32::from(event.position.x - bounds.origin.x),
                        f32::from(event.position.y - bounds.origin.y),
                    );
                    this.overlay_wheel_pointer(at);
                    cx.notify();
                }
            }))
            .on_scroll(cx.listener(|this, event: &gpui::ScrollEvent, _, cx| {
                let delta = f32::from(event.delta.pixel_delta(px(16.)).y);
                if delta != 0. {
                    // A wheel turned towards the user (content up) is the next page (`deltaY > 0`).
                    this.overlay_wheel_scroll(-delta);
                    cx.notify();
                }
            }))
            .on_click(cx.listener(|this, _, _, cx| {
                // `U`: a click with a selected sound plays it and closes the wheel.
                if let Some(open) = this.overlay.wheel.clone()
                    && let Some(sound) = this.overlay_wheel_selected_sound(&open)
                {
                    this.overlay.wheel = None;
                    this.overlay_wheel_play(sound, cx);
                    cx.notify();
                }
            }))
            .child(super::overlay_widgets::probe(
                &self.overlay.bounds,
                "soundboard-wheel".into(),
            ))
            .child(ring)
            .children(dead_zone)
            .children(indicator)
            .child(inner)
            .children(items);
        Some(
            div()
                .absolute()
                .inset_0()
                .flex()
                .items_center()
                .justify_center()
                .child(
                    div()
                        .relative()
                        .child(body)
                        // The wheel is a clickable region of a locked overlay.
                        .child(self.overlay_zone_probe()),
                )
                .into_any_element(),
        )
    }

    /// Capture only: `overlay wheel hold|release|toggle|move <x> <y>|scroll <dy>|state`.
    #[cfg(feature = "capture")]
    pub(super) fn script_overlay_wheel(
        &mut self,
        words: &[&str],
        cx: &mut Context<Self>,
    ) -> String {
        match words {
            ["hold"] => self.soundboard_hold_keybind(true, cx),
            ["release"] => self.soundboard_hold_keybind(false, cx),
            ["toggle"] => self.soundboard_toggle_keybind(cx),
            ["move", x, y] => {
                self.overlay_wheel_pointer((x.parse().unwrap_or(0.), y.parse().unwrap_or(0.)));
            }
            ["scroll", dy] => self.overlay_wheel_scroll(dy.parse().unwrap_or(1.)),
            ["state"] => {}
            _ => {
                return "usage: overlay wheel hold|release|toggle|move <x> <y>|scroll <dy>|state"
                    .to_owned();
            }
        }
        cx.notify();
        format!(
            "overlay wheel: open={:?} sounds={} played={:?}",
            self.overlay.wheel,
            self.soundboard_wheel_sounds().len(),
            self.overlay.wheel_played
        )
    }
}
