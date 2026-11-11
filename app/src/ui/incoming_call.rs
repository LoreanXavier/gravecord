//! The incoming call card and the ring sound.
//!
//! Sources (official modules, read in `design/snapshot/modules`; every value here is UNCONFIRMED against a real
//! incoming call until one has been captured):
//! - 596339 `IncomingCallStore`: the cards, empty while the status is Do Not Disturb or quiet mode is on; the first card
//!   sits at `{width / 2 - 116, height / 2 - 157.5}`, each later one `10px` further down and right.
//! - 760571 / CSS 890596: the card, `min-width: 232px`, padding 16, radius 8, `--background-surface-high`,
//!   `--shadow-border` with `--shadow-high`; 232 x 267 for a voice call.
//! - 693409: the round buttons, 40 x 40, radius 8, icon 20; Join Call is `--green-360` with a white phone, Dismiss the
//!   critical colour with a white X.
//! - 85448 / 479464: the avatar and its `ringingIncoming` pulse (5.407s).
//! - Ring sounds: `call_ringing` while a call rings us, `call_calling` while we ring somebody who has not joined.

use super::dm_call_view::RingKind;
use super::{Root, color, icon};
use gpui::{
    AnyElement, Context, InteractiveElement, IntoElement, ParentElement, Styled, Window, div,
    point, prelude::*, px,
};
use tracing::info;

/// What decides the ring sound.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RingInputs {
    /// A call rings us and its card is not gated away.
    pub incoming: bool,
    /// We are in a DM call and somebody is being rung while nobody else has joined.
    pub outgoing: bool,
    /// Streamer mode switches the sounds off.
    pub streamer_sounds_off: bool,
}

/// The sound to loop, or `None` for silence. An incoming call wins over an outgoing one.
pub fn ring_sound(inputs: RingInputs) -> Option<&'static str> {
    if inputs.streamer_sounds_off {
        return None;
    }
    if inputs.incoming {
        Some("call_ringing")
    } else if inputs.outgoing {
        Some("call_calling")
    } else {
        None
    }
}

/// `{width / 2 - 116, height / 2 - 157.5}` plus `10 * index`, or the last place the card was dragged to while it still
/// fits (596339 `iJ`: the stored position when `x + 232 < width && y + 315 < height`, else the centre).
pub fn card_origin(width: f32, height: f32, offset: f32, stored: Option<(f32, f32)>) -> (f32, f32) {
    let (x, y) = match stored {
        Some((x, y)) if x + 232. < width && y + 315. < height => (x, y),
        _ => (width / 2. - 116., height / 2. - 157.5),
    };
    (x + offset, y + offset)
}

/// The ring of an incoming call: `call_ringing`, except one time in a thousand `call_ringing_beat` or
/// `call_ringing_snow_halation` (596339: a roll of 500 in `random(1, 1000)` picks a variant).
pub fn ring_variant(roll: u32, pick: u32) -> &'static str {
    if roll != 500 {
        "call_ringing"
    } else if pick.is_multiple_of(2) {
        "call_ringing_beat"
    } else {
        "call_ringing_snow_halation"
    }
}

/// A card being dragged (`dragAnywhere`): where the pointer and the card were at the start, and the card now.
#[derive(Clone, Copy, Debug)]
pub struct CardDrag {
    pub channel: dc_state::Id,
    pub pointer: gpui::Point<gpui::Pixels>,
    pub from: (f32, f32),
    pub now: (f32, f32),
}

/// The looping ring: the sound that plays now and its handle.
#[derive(Default)]
pub struct RingAudio {
    current: Option<(&'static str, dc_voice::sound::Handle)>,
}

impl RingAudio {
    /// Move to `want`: stop a different sound, start the wanted one, and start it again when it ran out.
    pub fn reconcile(&mut self, want: Option<&'static str>) {
        let finished = self
            .current
            .as_ref()
            .is_some_and(|(_, handle)| handle.is_finished());
        let same = self.current.as_ref().map(|(name, _)| *name) == want;
        if same && !finished {
            return;
        }
        if let Some((_, handle)) = self.current.take() {
            handle.stop();
        }
        if !same {
            info!(sound = ?want, "ring sound");
        }
        if let Some(name) = want {
            let nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|time| time.subsec_nanos())
                .unwrap_or(0);
            let played = if name == "call_ringing" {
                ring_variant(nanos % 1000 + 1, nanos / 1000)
            } else {
                name
            };
            self.current = crate::sounds::play_event(played).map(|handle| (name, handle));
        }
    }
}

impl Root {
    /// Whether the cards are gated away: Do Not Disturb or quiet mode.
    fn incoming_gated(&self) -> bool {
        let dnd = self
            .stores
            .current_user
            .as_ref()
            .is_some_and(|me| self.stores.status(me.id) == "dnd");
        (dnd && !self.dmc.shown_online) || self.stores.notification_quiet_mode()
    }

    /// The sound the current calls ask for.
    fn ring_inputs(&self) -> RingInputs {
        let gated = self.incoming_gated();
        let user = self.stores.current_user.as_ref().map(|user| user.id);
        let streamer = crate::notifications::switch_value("streaming_enable_streamer_mode", user)
            .unwrap_or(false)
            && crate::notifications::switch_value("streamer_mode_disable_sounds", user)
                .unwrap_or(false);
        let outgoing = self.call.as_ref().is_some_and(|call| {
            call.guild == 0
                && self
                    .stores
                    .calls
                    .get_call(call.channel)
                    .is_some_and(|ring| !ring.ringing.is_empty())
                && self.stores.voice_members(0, call.channel).len() < 2
        });
        RingInputs {
            incoming: self.stores.calls.has_incoming_calls(gated),
            outgoing,
            streamer_sounds_off: streamer,
        }
    }

    /// Run after every change of the calls and on every tick while something rings.
    pub(super) fn ring_reconcile(&mut self, cx: &mut Context<Self>) {
        let inputs = self.ring_inputs();
        self.ring_audio.reconcile(ring_sound(inputs));
        self.dmc.incoming = inputs.incoming;
        if inputs.incoming || inputs.outgoing {
            self.start_dm_call_tick(cx);
        }
    }

    /// `stopRinging` for this account: the Dismiss button.
    fn dismiss_incoming(&mut self, channel: dc_state::Id, cx: &mut Context<Self>) {
        // 710801 and the Escape handler of 760571: `stopRinging(channel.id)`, no recipients.
        self.stop_ringing(channel, None);
        cx.notify();
    }

    /// The cards that are up: every call that rings us, hidden while that DM is the open channel (596339 `UH`).
    fn incoming_visible(&self) -> Vec<dc_state::calls::IncomingCall> {
        let gated = self.incoming_gated();
        self.stores
            .calls
            .get_incoming_calls(gated)
            .iter()
            .filter(|call| !(self.guild.is_none() && self.channel == Some(call.channel)))
            .cloned()
            .collect()
    }

    /// Escape while a card is up (760571: a window `keydown` listener calls `stopRinging(channel.id)` for each card).
    /// Returns whether a card took the key.
    pub(super) fn incoming_escape(&mut self, cx: &mut Context<Self>) -> bool {
        let cards = self.incoming_visible();
        for card in &cards {
            self.stop_ringing(card.channel, None);
        }
        if !cards.is_empty() {
            cx.notify();
        }
        !cards.is_empty()
    }

    /// The cards of every call that rings us, hidden while that DM is the open channel.
    pub(super) fn incoming_call_layer(
        &mut self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let gated = self.incoming_gated();
        let size = window.viewport_size();
        let (width, height) = (f32::from(size.width), f32::from(size.height));
        let _ = gated;
        let calls = self.incoming_visible();
        let mut cards = Vec::new();
        for call in calls {
            let Some(channel) = self
                .stores
                .private_channels
                .iter()
                .find(|dm| dm.id == call.channel)
                .cloned()
            else {
                continue;
            };
            let Some(sender) = call
                .sender
                .and_then(|id| self.stores.users.get(&id).cloned())
                .or_else(|| self.stores.dm_recipient(&channel).cloned())
            else {
                continue;
            };
            let (mut left, mut top) =
                card_origin(width, height, call.offset, self.dmc.incoming_origin);
            if let Some(drag) = self
                .dmc
                .incoming_drag
                .filter(|drag| drag.channel == call.channel)
            {
                (left, top) = drag.now;
            }
            let id = call.channel;
            let person = super::dm_call_view::Person {
                user: sender.id,
                // `ringing: isFocused()`: the pulse runs only while the window has focus.
                ringing: cfg!(feature = "capture") || self.notification_focused,
                mute: false,
                deaf: false,
                speaking: false,
                video: false,
            };
            let name = self.private_channel_name(&channel);
            // Js8cK3 "Incoming Call..." for a voice call, KcnWCO "Incoming Video Call..." for a video one.
            let subtitle = if self
                .stores
                .voice_members(0, id)
                .iter()
                .any(|state| state.self_video || state.self_stream)
            {
                "Incoming Video Call..."
            } else {
                "Incoming Call..."
            };
            let button = |key: &'static str, fill: &'static str, glyph: &'static str| {
                div()
                    .id(key)
                    .size(px(40.))
                    .rounded(px(8.))
                    .bg(color(fill))
                    .flex()
                    .items_center()
                    .justify_center()
                    .cursor_pointer()
                    .child(icon(glyph, 20., "--white"))
            };
            cards.push(
                div()
                    .id(gpui::SharedString::from(format!("incoming-call-{id}")))
                    .absolute()
                    .left(px(left))
                    .top(px(top))
                    .w(px(232.))
                    .h(px(267.))
                    .p(px(16.))
                    .rounded(px(8.))
                    .bg(color("--background-surface-high"))
                    .shadow(card_shadow())
                    .on_mouse_down(
                        gpui::MouseButton::Left,
                        cx.listener(move |this, event: &gpui::MouseDownEvent, _, cx| {
                            this.dmc.incoming_drag = Some(CardDrag {
                                channel: id,
                                pointer: event.position,
                                from: (left, top),
                                now: (left, top),
                            });
                            cx.notify();
                        }),
                    )
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .items_center()
                            .child(div().mt(px(16.)).child(self.call_avatar_kind(
                                &sender,
                                &person,
                                80.,
                                RingKind::Incoming,
                            )))
                            .child(
                                div()
                                    .mt(px(16.))
                                    .text_size(px(16.))
                                    .line_height(px(22.))
                                    .font_weight(gpui::FontWeight(600.))
                                    .text_color(color("--text-strong"))
                                    .child(name),
                            )
                            .child(
                                div()
                                    .mt(px(4.))
                                    .text_size(px(14.))
                                    .line_height(px(18.))
                                    .text_color(color("--text-default"))
                                    .child(subtitle),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .gap(px(8.))
                            .child(
                                button(
                                    "incoming-dismiss",
                                    "--control-critical-primary-background-default",
                                    "close",
                                )
                                .on_click(
                                    cx.listener(move |this, _, _, cx| {
                                        this.dismiss_incoming(id, cx)
                                    }),
                                ),
                            )
                            .child(button("incoming-join", "--green-360", "dm_call").on_click(
                                cx.listener(move |this, _, _, cx| {
                                    this.join_dm_call(id, false, cx);
                                }),
                            )),
                    )
                    .into_any_element(),
            );
        }
        if self.dmc.incoming_drag.is_some() {
            // A transparent sheet follows the pointer so a fast drag cannot leave the card behind.
            cards.push(
                div()
                    .id("incoming-drag-sheet")
                    .absolute()
                    .inset_0()
                    .on_mouse_move(cx.listener(
                        move |this, event: &gpui::MouseMoveEvent, window, cx| {
                            let size = window.viewport_size();
                            if let Some(drag) = this.dmc.incoming_drag.as_mut() {
                                let delta = event.position - drag.pointer;
                                // `maxX` / `maxY` of the draggable are the window's size.
                                let x = (drag.from.0 + f32::from(delta.x))
                                    .clamp(0., (f32::from(size.width) - 232.).max(0.));
                                let y = (drag.from.1 + f32::from(delta.y))
                                    .clamp(0., (f32::from(size.height) - 267.).max(0.));
                                drag.now = (x, y);
                                cx.notify();
                            }
                        },
                    ))
                    .on_mouse_up(
                        gpui::MouseButton::Left,
                        cx.listener(|this, _: &gpui::MouseUpEvent, _, cx| {
                            // `onDragEnd` -> `INCOMING_CALL_MOVE {x: left, y: top}`.
                            if let Some(drag) = this.dmc.incoming_drag.take() {
                                this.dmc.incoming_origin = Some(drag.now);
                            }
                            cx.notify();
                        }),
                    )
                    .into_any_element(),
            );
        }
        cards
    }
}

/// `--shadow-border` then `--shadow-high`.
fn card_shadow() -> Vec<gpui::BoxShadow> {
    vec![
        gpui::BoxShadow {
            color: gpui::hsla(0., 0., 1., 0.08),
            offset: point(px(0.), px(0.)),
            blur_radius: px(0.),
            spread_radius: px(1.),
            inset: false,
        },
        gpui::BoxShadow {
            color: gpui::hsla(0., 0., 0., 0.24),
            offset: point(px(0.), px(12.)),
            blur_radius: px(24.),
            spread_radius: px(0.),
            inset: false,
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_incoming_call_rings_before_an_outgoing_one() {
        let both = RingInputs {
            incoming: true,
            outgoing: true,
            streamer_sounds_off: false,
        };
        assert_eq!(ring_sound(both), Some("call_ringing"));
        let out = RingInputs {
            incoming: false,
            outgoing: true,
            ..RingInputs::default()
        };
        assert_eq!(ring_sound(out), Some("call_calling"));
        assert_eq!(ring_sound(RingInputs::default()), None);
    }

    #[test]
    fn streamer_mode_silences_both_rings() {
        let inputs = RingInputs {
            incoming: true,
            outgoing: true,
            streamer_sounds_off: true,
        };
        assert_eq!(ring_sound(inputs), None);
    }

    #[test]
    fn the_first_card_is_centred_and_later_cards_step_down_right() {
        assert_eq!(card_origin(1280., 720., 0., None), (524., 202.5));
        assert_eq!(card_origin(1280., 720., 10., None), (534., 212.5));
    }

    #[test]
    fn a_dragged_card_returns_only_while_it_fits() {
        assert_eq!(card_origin(1280., 720., 0., Some((100., 50.))), (100., 50.));
        assert_eq!(
            card_origin(1280., 720., 10., Some((100., 50.))),
            (110., 60.)
        );
        // Off the right edge of a smaller window: back to the centre.
        assert_eq!(card_origin(300., 720., 0., Some((100., 50.))), (34., 202.5));
    }

    #[test]
    fn one_ring_in_a_thousand_is_a_variant() {
        assert_eq!(ring_variant(1, 0), "call_ringing");
        assert_eq!(ring_variant(1000, 7), "call_ringing");
        assert_eq!(ring_variant(500, 0), "call_ringing_beat");
        assert_eq!(ring_variant(500, 1), "call_ringing_snow_halation");
    }
}
