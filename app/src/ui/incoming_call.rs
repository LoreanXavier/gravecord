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

/// `{width / 2 - 116, height / 2 - 157.5}` plus `10 * index`.
pub fn card_origin(width: f32, height: f32, offset: f32) -> (f32, f32) {
    (width / 2. - 116. + offset, height / 2. - 157.5 + offset)
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
        if let Some(name) = want {
            self.current = crate::sounds::play_event(name).map(|handle| (name, handle));
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
        if let Some(me) = self.stores.current_user.as_ref().map(|user| user.id) {
            self.stop_ringing(channel, &[me]);
        }
        cx.notify();
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
        let calls: Vec<_> = self.stores.calls.get_incoming_calls(gated).to_vec();
        let mut cards = Vec::new();
        for call in calls {
            let open = self.guild.is_none() && self.channel == Some(call.channel);
            if open {
                continue;
            }
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
            let (left, top) = card_origin(width, height, call.offset);
            let id = call.channel;
            let person = super::dm_call_view::Person {
                user: sender.id,
                ringing: true,
                mute: false,
                deaf: false,
                speaking: false,
                video: false,
            };
            let name = self.private_channel_name(&channel);
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
                                    .child("Incoming Call..."),
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
                                    this.select_home(Some(id));
                                    this.join_voice(0, id, cx);
                                }),
                            )),
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
        assert_eq!(card_origin(1280., 720., 0.), (524., 202.5));
        assert_eq!(card_origin(1280., 720., 10.), (534., 212.5));
    }
}
