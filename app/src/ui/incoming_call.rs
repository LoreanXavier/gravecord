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
use super::tooltip::{Side, Tip};
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
    /// The pointer left the press point: only then is it a drag (a press that stays is a click on a button).
    pub moved: bool,
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
            self.current = crate::sounds::play_ring_loop(played).map(|handle| (name, handle));
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
                .unwrap_or(true);
        let incoming = self
            .stores
            .calls
            .get_incoming_calls(gated)
            .iter()
            .any(|call| !self.dm_call_in(Some(call.channel)));
        let outgoing = self.call.as_ref().is_some_and(|call| {
            call.guild == 0
                && self
                    .stores
                    .calls
                    .get_call(call.channel)
                    .is_some_and(|ring| ring.ringing.iter().any(|id| Some(*id) != user))
                && self.stores.voice_members(0, call.channel).len() < 2
        });
        RingInputs {
            incoming,
            outgoing,
            streamer_sounds_off: streamer,
        }
    }

    /// Run after every change of the calls and on every tick while something rings.
    pub(super) fn ring_reconcile(&mut self, cx: &mut Context<Self>) {
        crate::sounds::set_current_user(self.stores.current_user.as_ref().map(|user| user.id));
        crate::sounds::prefetch_call_sounds();
        let inputs = self.ring_inputs();
        self.ring_audio.reconcile(ring_sound(inputs));
        self.dmc.incoming = inputs.incoming;
        self.dmc.outgoing = inputs.outgoing;
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

    /// Starts the camera behind the card's preview: the same worker as Test Video. A capture run shows the synthetic
    /// pattern instead of a real camera.
    fn incoming_preview_start(&mut self, cx: &mut Context<Self>) {
        #[cfg(feature = "capture")]
        {
            self.camera_test_synthetic(cx);
        }
        #[cfg(not(feature = "capture"))]
        {
            self.camera_test_start(cx);
        }
    }

    /// The preview's newest picture, once the camera delivered one.
    fn camera_preview_picture(&self) -> Option<std::sync::Arc<gpui::RenderImage>> {
        self.camera_test_image()
    }

    fn dm_banner_covered(&self) -> bool {
        self.settings.is_some()
            || self.server_settings.is_some()
            || self.channel_settings.is_some()
            || self.profile_editor.is_some()
            || self.boost_page.is_some()
            || self.lightbox.is_some()
            || self.vv.fullscreen
            || self.nitro_open
            || self.discover_active()
            || self.quests_active()
            || self.shop_active()
            || self.message_requests_active()
            || (self.guild.is_some() && (self.guild_guide_open || self.channel_browser_open))
            || self.voice_view_open()
    }

    /// The cards that are up: every call that rings us, hidden only while that DM's call area is uncovered on screen
    /// (`596339` `UH`).
    fn incoming_visible(&self) -> Vec<dc_state::calls::IncomingCall> {
        let gated = self.incoming_gated();
        let viewing_uncovered = self.guild.is_none() && !self.dm_banner_covered();
        self.stores
            .calls
            .get_incoming_calls(gated)
            .iter()
            .filter(|call| {
                !self.dm_call_in(Some(call.channel))
                    && !(viewing_uncovered && self.channel == Some(call.channel))
            })
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
                marks: super::voice_marks::Marks::default(),
                speaking: false,
                video: false,
            };
            let name = self.private_channel_name(&channel);
            // Js8cK3 "Incoming Call..." for a voice call, KcnWCO "Incoming Video Call..." for a video one.
            let video_mode = self
                .stores
                .voice_members(0, id)
                .iter()
                .any(|state| state.self_video || state.self_stream);
            let subtitle = if video_mode {
                dc_intl::t!("KcnWCO")
            } else {
                dc_intl::t!("Js8cK3")
            };
            // 760571 `T`: the video card needs video support and at least one camera. A capture run has the
            // synthetic source standing in for one.
            let can_video =
                video_mode && (cfg!(feature = "capture") || !dc_voice::camera_devices().is_empty());
            // `u`: "Preview my camera" was pressed; the card widens to 475 x 267 around the live picture.
            let preview = can_video && self.dmc.incoming_preview == Some(id);
            let (card_w, card_h) = if preview {
                (475., 267.)
            } else if can_video {
                (232., 315.)
            } else {
                (232., 267.)
            };
            // `translateX: [0, -|475 - 232| / 2]` while the card widens.
            let left = if preview {
                (left - (475. - 232.) / 2.).clamp(0., (width - 475.).max(0.))
            } else {
                left
            };
            let picture = preview.then(|| self.camera_preview_picture()).flatten();
            let button = |this: &Self,
                          key: &'static str,
                          fill: &'static str,
                          hover: &'static str,
                          glyph: &'static str,
                          tip: &'static str,
                          cx: &mut Context<Self>| {
                this.with_tip(
                    format!("{key}-{id}"),
                    Tip::text(tip, Side::Top),
                    div()
                        .id(key)
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
            let dismiss_btn = button(
                self,
                "incoming-dismiss",
                "--control-critical-primary-background-default",
                "--control-critical-primary-background-hover",
                "close",
                dc_intl::t!("WAI6xu"),
                cx,
            );
            let video_btn = can_video.then(|| {
                button(
                    self,
                    "incoming-join-video",
                    "--green-360",
                    "--green-360",
                    "dm_video",
                    dc_intl::t!("YbjQQ/"),
                    cx,
                )
            });
            let join_btn = button(
                self,
                "incoming-join",
                "--green-360",
                "--green-360",
                "dm_call",
                dc_intl::t!("0D/6Rz"),
                cx,
            );
            let header = if preview {
                // `previewChannelHeader_dcNks8`: 16px from top/left, horizontal 40px avatar + 16px gap + text.
                div()
                    .absolute()
                    .top(px(16.))
                    .left_0()
                    .right_0()
                    .px(px(16.))
                    .flex()
                    .flex_row()
                    .items_center()
                    .child(div().flex_none().mr(px(16.)).child(self.call_avatar_kind(
                        &sender,
                        &person,
                        40.,
                        RingKind::Incoming,
                        "--background-mod-normal",
                    )))
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .flex()
                            .flex_col()
                            .child(
                                div()
                                    .truncate()
                                    .text_size(px(14.))
                                    .line_height(px(18.))
                                    .font_weight(gpui::FontWeight(600.))
                                    .text_color(color("--white"))
                                    .child(name),
                            )
                            .child(
                                div()
                                    .truncate()
                                    .text_size(px(12.))
                                    .line_height(px(16.))
                                    .text_color(color("--white"))
                                    .child(subtitle),
                            ),
                    )
                    .into_any_element()
            } else {
                div()
                    .flex()
                    .flex_col()
                    .items_center()
                    .w_full()
                    .child(div().my(px(16.)).child(self.call_avatar_kind(
                        &sender,
                        &person,
                        80.,
                        RingKind::Incoming,
                        "--background-surface-high",
                    )))
                    .child(
                        div()
                            .w_full()
                            .mb(px(2.))
                            .truncate()
                            .text_center()
                            .text_size(px(16.))
                            .line_height(px(20.))
                            .font_weight(gpui::FontWeight(600.))
                            .text_color(color("--text-strong"))
                            .child(name),
                    )
                    .child(
                        div()
                            .w_full()
                            .mb(px(24.))
                            .truncate()
                            .text_center()
                            .text_size(px(14.))
                            .line_height(px(18.))
                            .text_color(color("--text-default"))
                            .child(subtitle),
                    )
                    .into_any_element()
            };
            cards.push(
                gpui::deferred(
                    div()
                        .id(gpui::SharedString::from(format!("incoming-call-{id}")))
                        .absolute()
                        .left(px(left))
                        .top(px(top))
                        .w(px(card_w))
                        .h(px(card_h))
                        .p(px(16.))
                        .rounded(px(8.))
                        .overflow_hidden()
                        .bg(color(if preview {
                            "--background-mod-normal"
                        } else {
                            "--background-surface-high"
                        }))
                        .shadow(card_shadow())
                        .children(preview.then(|| {
                            div()
                                .absolute()
                                .inset_0()
                                .flex()
                                .items_center()
                                .justify_center()
                                .child(super::voice_view::spinner())
                        }))
                        .children(picture.map(|image| {
                            div().absolute().inset_0().child(
                                gpui::img(image)
                                    .size_full()
                                    .object_fit(gpui::ObjectFit::Cover),
                            )
                        }))
                        .children(preview.then(|| {
                            div()
                                .absolute()
                                .top_0()
                                .left_0()
                                .right_0()
                                .h(px(84.))
                                .bg(gpui::linear_gradient(
                                    180.,
                                    gpui::linear_color_stop(gpui::rgba(0x18191cff), 0.),
                                    gpui::linear_color_stop(gpui::rgba(0x18191c00), 1.),
                                ))
                        }))
                        .children(preview.then(|| {
                            div()
                                .absolute()
                                .bottom_0()
                                .left_0()
                                .right_0()
                                .h(px(84.))
                                .bg(gpui::linear_gradient(
                                    180.,
                                    gpui::linear_color_stop(gpui::rgba(0x18191c00), 0.),
                                    gpui::linear_color_stop(gpui::rgba(0x18191cff), 1.),
                                ))
                        }))
                        .on_mouse_down(
                            gpui::MouseButton::Left,
                            cx.listener(move |this, event: &gpui::MouseDownEvent, _, cx| {
                                this.dmc.incoming_drag = Some(CardDrag {
                                    channel: id,
                                    pointer: event.position,
                                    from: (left, top),
                                    now: (left, top),
                                    moved: false,
                                });
                                cx.notify();
                            }),
                        )
                        .on_mouse_move(
                            cx.listener(move |this, event: &gpui::MouseMoveEvent, _, cx| {
                                if let Some(drag) = this.dmc.incoming_drag.as_mut()
                                    && drag.channel == id
                                    && !drag.moved
                                    && event.dragging()
                                {
                                    let delta = event.position - drag.pointer;
                                    if f32::from(delta.x).abs() + f32::from(delta.y).abs() > 3. {
                                        drag.moved = true;
                                        cx.notify();
                                    }
                                }
                            }),
                        )
                        .on_mouse_up(
                            gpui::MouseButton::Left,
                            cx.listener(|this, _: &gpui::MouseUpEvent, _, _| {
                                if this.dmc.incoming_drag.is_some_and(|drag| !drag.moved) {
                                    this.dmc.incoming_drag = None;
                                }
                            }),
                        )
                        .flex()
                        .flex_col()
                        .items_center()
                        .map(|card| {
                            if preview {
                                card.justify_end()
                            } else {
                                card.justify_between()
                            }
                        })
                        .child(header)
                        .child(
                            div()
                                .relative()
                                .flex()
                                .gap(px(8.))
                                .child(
                                    dismiss_btn
                                        .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| {
                                            cx.stop_propagation();
                                        })
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            this.tip = None;
                                            if this.dmc.incoming_preview.take().is_some() {
                                                this.camera_test_stop(cx);
                                            }
                                            this.dismiss_incoming(id, cx)
                                        })),
                                )
                                .children(video_btn.map(|btn| {
                                    // `K.A` with `join` and `color: "green"`: joins with the camera on.
                                    btn.on_mouse_down(gpui::MouseButton::Left, |_, _, cx| {
                                        cx.stop_propagation();
                                    })
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.tip = None;
                                        this.dmc.incoming_preview = None;
                                        this.join_dm_call(id, true, cx);
                                    }))
                                }))
                                .child(
                                    join_btn
                                        .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| {
                                            cx.stop_propagation();
                                        })
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            this.tip = None;
                                            if this.dmc.incoming_preview.take().is_some() {
                                                this.camera_test_stop(cx);
                                            }
                                            this.join_dm_call(id, false, cx);
                                        })),
                                ),
                        )
                        // 760571: "Preview my camera" (8B0UDr), a full-width small secondary button under the row.
                        .children((can_video && !preview).then(|| {
                            div()
                                .id("incoming-preview")
                                .w_full()
                                .h(px(32.))
                                .mt(px(16.))
                                .rounded(px(8.))
                                .bg(color("--control-secondary-background-default"))
                                .hover(|style| {
                                    style.bg(color("--control-secondary-background-hover"))
                                })
                                .flex()
                                .items_center()
                                .justify_center()
                                .cursor_pointer()
                                .text_size(px(14.))
                                .line_height(px(18.))
                                .font_weight(gpui::FontWeight(500.))
                                .text_color(color("--control-secondary-text-default"))
                                .child(dc_intl::t!("8B0UDr"))
                                .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| {
                                    cx.stop_propagation();
                                })
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.dmc.incoming_preview = Some(id);
                                    this.incoming_preview_start(cx);
                                }))
                        })),
                )
                .with_priority(6)
                .into_any_element(),
            );
        }
        if self.dmc.incoming_drag.is_some_and(|drag| drag.moved) {
            // A transparent sheet follows the pointer so a fast drag cannot leave the card behind.
            cards.push(
                gpui::deferred(
                    div()
                        .id("incoming-drag-sheet")
                        .absolute()
                        .inset_0()
                        .on_mouse_move(cx.listener(
                            move |this, event: &gpui::MouseMoveEvent, window, cx| {
                                let size = window.viewport_size();
                                if let Some(drag) = this.dmc.incoming_drag.as_mut() {
                                    let delta = event.position - drag.pointer;
                                    let card_w =
                                        if this.dmc.incoming_preview == Some(drag.channel) {
                                            475.
                                        } else {
                                            232.
                                        };
                                    // `maxX` / `maxY` of the draggable are the window's size.
                                    let x = (drag.from.0 + f32::from(delta.x))
                                        .clamp(0., (f32::from(size.width) - card_w).max(0.));
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
                        ),
                )
                .with_priority(6)
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
