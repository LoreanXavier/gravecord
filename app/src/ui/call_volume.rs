//! The volume control at the right of the call tray (`renderBottomRight` of module 540462, modules 821747 and 20504):
//! shown while a tile of someone else is focused. The speaker mutes and unmutes that person, or their stream when
//! the tile is a stream; while the pointer is over it a vertical bar above sets the level, 0 to 200 percent, and
//! hides 150 ms after the pointer leaves.
//!
//! `volumeButtonSlider_QlL4f-` sits 16px above the button and `volumeSlider_hbKzX7` lifts it 16px more; the bar is
//! the media bar of the voice message player (`mediaBarInteractionVolume_s1cr3c`: 14 by 88, an 8px padding round a
//! 6 by 72 rail).

use super::audio_menu::{slider_to_volume, volume_to_slider};
use super::video_tiles::TileKind;
use super::voice_message::position_of;
use super::{Root, color, icon, motion};
use gpui::{
    AnyElement, Bounds, Context, DispatchPhase, IntoElement, MouseButton, MouseDownEvent,
    MouseMoveEvent, MouseUpEvent, Pixels, canvas, div, fill, point, prelude::*, px, size,
};
use std::cell::Cell;
use std::rc::Rc;

/// `maxValue` on the desktop client (200), or more with the VolumeBooster plugin (`plugin_hooks.rs`).
fn max() -> f32 {
    super::plugin_hooks::user_volume_max() as f32
}

#[derive(Default)]
pub(super) struct State {
    /// The pointer is over the button or the bar.
    over: bool,
    /// When the bar hides after the pointer left (`_hoverTimeout`, 150 ms).
    hide_at: Option<f64>,
    dragging: bool,
    /// A stream's level while its bar is dragged (the owner, the percent, muted): stored once, when the drag ends.
    live: Option<(u64, f32, bool)>,
    /// Where the rail was painted.
    track: Rc<Cell<Bounds<Pixels>>>,
}

/// The glyph of module 20504: `VoiceX` at the minimum or muted, `VoiceLow` under half, `VoiceNormal` above.
fn glyph(position: f32, muted: bool) -> &'static str {
    if muted || position <= 0. {
        "vm_voice_x"
    } else if position < 0.5 {
        "vm_voice_low"
    } else {
        "sound_speaker"
    }
}

impl Root {
    /// The focused tile of someone else: the user and whether the tile is their stream.
    fn call_volume_target(&self) -> Option<(u64, bool)> {
        let (user, kind) = self.vv.focus.tile?;
        let me = self.stores.current_user.as_ref().map(|me| me.id);
        (Some(user) != me).then_some((user, kind == TileKind::Stream))
    }

    /// The level in percent (100 is unity) and whether the target is muted here.
    fn call_volume_level(&self, user: u64, stream: bool) -> (f32, bool) {
        if stream {
            match self.call_volume.live {
                Some((owner, percent, muted)) if owner == user => (percent, muted),
                _ => (
                    super::stream_volume::percent(user),
                    super::stream_volume::muted(user),
                ),
            }
        } else {
            let (factor, muted) = self.context.mix.get(&user).copied().unwrap_or((1., false));
            (factor * 100., muted)
        }
    }

    /// A drag that ended, or was cut short: its last level is stored.
    pub(super) fn call_volume_drag_end(&mut self) {
        self.call_volume.dragging = false;
        if let Some((owner, percent, muted)) = self.call_volume.live.take() {
            super::stream_volume::set_percent(owner, percent);
            if super::stream_volume::muted(owner) != muted {
                super::stream_volume::set_muted(owner, muted);
            }
            self.apply_stream_volume(owner);
        }
    }

    /// The drags of the call view end with the button, wherever it was let go.
    pub(super) fn call_drags_end(&mut self) {
        if self.call_volume.dragging || self.call_volume.live.is_some() {
            self.call_volume_drag_end();
        }
        self.stream_zoom.release();
    }

    fn call_volume_set(&mut self, user: u64, stream: bool, percent: f32, muted: bool) {
        if stream && self.call_volume.dragging {
            // Storage is written once at the end of the drag; the mix follows the pointer.
            self.call_volume.live = Some((user, percent, muted));
            if let Some(call) = &self.call {
                call.controls
                    .hub()
                    .set_stream_volume(user, if muted { 0. } else { percent / 100. });
            }
            return;
        }
        if stream {
            super::stream_volume::set_percent(user, percent);
            super::stream_volume::set_muted(user, muted);
            self.apply_stream_volume(user);
        } else {
            self.set_user_mix(user, percent / 100., muted);
        }
    }

    /// A press or drag on the bar at `y`: the level under the pointer; a level above zero unmutes.
    fn call_volume_drag_to(&mut self, y: Pixels, cx: &mut Context<Self>) {
        let Some((user, stream)) = self.call_volume_target() else {
            return;
        };
        let position = position_of(y, self.call_volume.track.get());
        let percent = slider_to_volume(position * max());
        let (_, muted) = self.call_volume_level(user, stream);
        self.call_volume_set(user, stream, percent, muted && percent <= 0.);
        cx.notify();
    }

    /// The control, or nothing while no tile of someone else is focused.
    pub(super) fn call_volume_control(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let (user, stream) = self.call_volume_target()?;
        let (percent, muted) = self.call_volume_level(user, stream);
        let position = if muted {
            0.
        } else {
            (volume_to_slider(percent) / max()).clamp(0., 1.)
        };
        let state = &self.call_volume;
        let open =
            state.over || state.dragging || state.hide_at.is_some_and(|at| motion::now() < at);
        if open && !state.over && !state.dragging {
            // Keeps frames coming until the bar's time is up.
            cx.notify();
        }
        let hover = cx.listener(|this, inside: &bool, _, cx| {
            this.call_volume.over = *inside;
            this.call_volume.hide_at = (!*inside).then(|| motion::now() + 150.);
            cx.notify();
        });
        let button = div()
            .id("call-volume")
            .size(px(24.))
            .cursor_pointer()
            .on_click(cx.listener(move |this, _, _, cx| {
                let (percent, muted) = this.call_volume_level(user, stream);
                this.call_volume_set(user, stream, percent, !muted);
                cx.notify();
            }))
            .child(icon(
                glyph(position, muted),
                24.,
                "--interactive-icon-default",
            ));
        let weak = cx.entity().downgrade();
        let track = state.track.clone();
        let dragging = state.dragging;
        let bar = div()
            .id("call-volume-bar")
            .occlude()
            .absolute()
            // 16px above the 24px button, and the 16px of `volumeSlider_hbKzX7`.
            .bottom(px(24. + 16. + 16.))
            .left(px(5.))
            .w(px(14.))
            .h(px(88.))
            .rounded(px(8.))
            .bg(color("--opacity-black-68"))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, event: &MouseDownEvent, _, cx| {
                    this.call_volume.dragging = true;
                    this.call_volume_drag_to(event.position.y, cx);
                    cx.stop_propagation();
                }),
            )
            .child(
                canvas(
                    |_, _, _| {},
                    move |bounds, (), window, _| {
                        let rail = Bounds::new(
                            point(bounds.origin.x + px(4.), bounds.origin.y + px(8.)),
                            size(px(6.), bounds.size.height - px(16.)),
                        );
                        track.set(rail);
                        if dragging {
                            let moved = weak.clone();
                            window.on_mouse_event(move |event: &MouseMoveEvent, phase, _, cx| {
                                if phase != DispatchPhase::Capture {
                                    return;
                                }
                                let _ = moved.update(cx, |this, cx| {
                                    this.call_volume_drag_to(event.position.y, cx);
                                });
                            });
                            let released = weak.clone();
                            window.on_mouse_event(move |_: &MouseUpEvent, phase, _, cx| {
                                if phase != DispatchPhase::Capture {
                                    return;
                                }
                                let _ = released.update(cx, |this, cx| {
                                    this.call_volume_drag_end();
                                    if !this.call_volume.over {
                                        this.call_volume.hide_at = Some(motion::now() + 150.);
                                    }
                                    cx.notify();
                                });
                            });
                        }
                        // `mediaBarWrapperVolume_s1cr3c`: `--primary-300` at 30%; the progress is `--brand-500`.
                        let mut back = color("--primary-300");
                        back.a *= 0.3;
                        window.paint_quad(fill(rail, back).corner_radii(px(3.)));
                        let filled = rail.size.height * position;
                        let top = rail.origin.y + rail.size.height - filled;
                        window.paint_quad(
                            fill(
                                Bounds::new(
                                    point(rail.origin.x, top),
                                    size(rail.size.width, filled),
                                ),
                                color("--brand-500"),
                            )
                            .corner_radii(px(3.)),
                        );
                        if dragging {
                            window.paint_quad(
                                fill(
                                    Bounds::new(
                                        point(rail.origin.x - px(2.), top - px(5.)),
                                        size(px(10.), px(10.)),
                                    ),
                                    color("--brand-500"),
                                )
                                .corner_radii(px(5.)),
                            );
                        }
                    },
                )
                .size_full(),
            );
        Some(
            div()
                .id("call-volume-holder")
                .relative()
                .flex_none()
                .on_hover(hover)
                .child(button)
                .when(open, |this| {
                    // The gap between the button and the bar keeps the pointer inside the holder.
                    this.child(
                        div()
                            .absolute()
                            .bottom(px(24.))
                            .left_0()
                            .w(px(24.))
                            .h(px(32.)),
                    )
                    .child(bar)
                })
                .into_any_element(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_speaker_follows_the_level() {
        assert_eq!(glyph(0.8, false), "sound_speaker");
        assert_eq!(glyph(0.3, false), "vm_voice_low");
        assert_eq!(glyph(0., false), "vm_voice_x");
        assert_eq!(glyph(0.8, true), "vm_voice_x");
    }

    #[test]
    fn unity_sits_at_half_of_a_bar_that_runs_to_200_percent() {
        assert!((volume_to_slider(100.) / max() - 0.5).abs() < 1e-4);
        assert!((slider_to_volume(0.5 * max()) - 100.).abs() < 1e-2);
    }
}
