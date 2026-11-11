//! Calls in direct messages and group DMs: the requests (`CallActions` of module 387755), the gateway effects of the
//! call store (module 470710), and the start of a call.
//!
//! Sources: official modules 387755 (`call`, `ring`, `stopRinging`), 470710 (call store), 597798
//! (`selectVoiceChannel`) and 868 (op 4 and op 13 payloads); `vendor/serein` (`ring_call` in `discord-api`, the
//! `dm_ringing` state of `client-core`) for the request shapes, adapted with attribution.
//!
//! Safety: in capture runs `net.rs` refuses ring and stop-ringing unless `GRAVECORD_ALLOW_RING=1`.

use super::Root;
use crate::net::Request;
use dc_proto::op::Op;
use dc_state::{Id, calls::Effect, calls::Recipients};
use gpui::Context;
use serde_json::{Value, json};
use tracing::info;

/// Body of `POST /channels/<id>/call/ring`: `{recipients, analytics_location}`; an undefined field is left out of
/// the JSON, as `JSON.stringify` does (module 387755 `ring`).
pub fn ring_body(recipients: Option<&[Id]>, location: Option<&str>) -> Value {
    let mut body = serde_json::Map::new();
    if let Some(recipients) = recipients {
        body.insert("recipients".into(), ids(recipients));
    }
    if let Some(location) = location {
        body.insert("analytics_location".into(), json!(location));
    }
    Value::Object(body)
}

/// Body of `POST /channels/<id>/call/stop-ringing`: `{recipients}` (module 387755 `stopRinging`).
pub fn stop_ringing_body(recipients: &[Id]) -> Value {
    json!({ "recipients": ids(recipients) })
}

/// Data of gateway op 13 `CALL_CONNECT` (`callConnect` in module 868).
pub fn call_connect_data(channel: Id) -> Value {
    json!({ "channel_id": channel.to_string() })
}

fn ids(list: &[Id]) -> Value {
    Value::Array(list.iter().map(|id| json!(id.to_string())).collect())
}

pub fn ring_route(channel: Id) -> String {
    format!("/channels/{channel}/call/ring")
}

pub fn stop_ringing_route(channel: Id) -> String {
    format!("/channels/{channel}/call/stop-ringing")
}

/// What the DM header shows for its two call buttons.
pub struct CallButtons {
    pub voice: Option<(&'static str, bool)>,
    pub video: Option<(&'static str, bool)>,
}

impl Root {
    /// After every dispatch: what the call store now owes the service.
    pub(super) fn calls_after_dispatch(&mut self, name: &str, cx: &mut Context<Self>) {
        // `CONNECTION_OPEN` and `CONNECTION_RESUMED`: the open DM asks for its call again.
        if matches!(name, "READY" | "RESUMED")
            && self.guild.is_none()
            && let Some(channel) = self.channel
        {
            let private = self
                .stores
                .private_channels
                .iter()
                .any(|dm| dm.id == channel);
            self.stores.calls.connect_channel(channel, private, true);
        }
        self.calls_after_dispatch_effects();
        self.ring_reconcile(cx);
        cx.notify();
    }

    pub(super) fn calls_after_dispatch_effects(&mut self) {
        for effect in self.stores.calls.take_effects() {
            match effect {
                Effect::Ring {
                    channel,
                    recipients,
                } => self.send_ring(channel, recipients.as_deref(), None),
                Effect::CallConnect { channel } => {
                    let _ = self.requests.send(Request::StreamOp {
                        op: Op::CallConnect,
                        data: call_connect_data(channel),
                    });
                }
            }
        }
    }

    /// `ring(channel, recipients, location)`: ring now when the call exists, otherwise queue the ring for its
    /// `CALL_CREATE`.
    pub(super) fn ring_call(
        &mut self,
        channel: Id,
        recipients: Option<Vec<Id>>,
        location: Option<&str>,
    ) {
        if !self
            .stores
            .private_channels
            .iter()
            .any(|dm| dm.id == channel)
        {
            return;
        }
        if self.stores.calls.call_ringable_now(channel) {
            self.send_ring(channel, recipients.as_deref(), location);
        } else {
            let wanted = match recipients {
                Some(users) => Recipients::Users(users),
                None => Recipients::All,
            };
            self.stores.calls.enqueue_ring(channel, wanted);
        }
    }

    fn send_ring(&mut self, channel: Id, recipients: Option<&[Id]>, location: Option<&str>) {
        info!("ringing a call");
        let _ = self.requests.send(Request::Rest {
            key: None,
            method: reqwest::Method::POST,
            route: ring_route(channel),
            body: Some(ring_body(recipients, location)),
        });
    }

    /// `stopRinging(channel, recipients)`.
    pub(super) fn stop_ringing(&mut self, channel: Id, recipients: &[Id]) {
        let _ = self.requests.send(Request::Rest {
            key: None,
            method: reqwest::Method::POST,
            route: stop_ringing_route(channel),
            body: Some(stop_ringing_body(recipients)),
        });
    }

    /// The header's two call buttons of a DM (module 289287): each is `(tooltip, disabled)`, or `None` when hidden.
    /// The voice button hides in a call; the video button also hides while the call is a voice call.
    pub(super) fn dm_call_buttons(&self, channel: Id, other: Option<Id>) -> CallButtons {
        let in_call = self
            .call
            .as_ref()
            .is_some_and(|call| call.guild == 0 && call.channel == channel);
        let active = self.stores.calls.is_call_active(channel, None);
        let unavailable = self.stores.calls.is_call_unavailable(channel);
        let video_call = active
            && self
                .stores
                .voice_members(0, channel)
                .iter()
                .any(|state| state.self_video || state.self_stream);
        let blocked = other.is_some_and(|user| self.is_blocked(user));
        // `rF7lN5` Call Unavailable, `PHzjvX` blocked.
        let disabled_tip = |normal: &'static str| {
            if unavailable {
                ("Call Unavailable", true)
            } else if blocked {
                ("You cannot call a user you have blocked", true)
            } else {
                (normal, false)
            }
        };
        CallButtons {
            voice: (!in_call).then(|| {
                disabled_tip(if active {
                    "Join Voice Call"
                } else {
                    "Start Voice Call"
                })
            }),
            video: (!in_call && !(active && !video_call)).then(|| {
                disabled_tip(if video_call {
                    "Join Video Call"
                } else {
                    "Start Video Call"
                })
            }),
        }
    }

    /// The header buttons: join an active call, else start one that rings (`!notFriend && !managed && !shift`).
    pub(super) fn dm_call_clicked(
        &mut self,
        channel: Id,
        video: bool,
        shift: bool,
        cx: &mut Context<Self>,
    ) {
        let active = self.stores.calls.is_call_active(channel, None);
        self.start_call(channel, video, !active && !shift, cx);
    }

    /// `CallActions.call(channelId, video, ring)`: select the voice channel, then ring the others. A DM or group DM
    /// the account is not already in a call of opens its call.
    pub(super) fn start_call(
        &mut self,
        channel: Id,
        video: bool,
        ring: bool,
        cx: &mut Context<Self>,
    ) {
        self.join_voice(0, channel, cx);
        if video {
            // `selectVoiceChannel(channel, true)`: the camera is on from the start.
            self.voice_camera_start(cx);
        }
        if ring {
            self.ring_call(channel, None, None);
        }
    }
}

/// The test DM of the harness: the test account's DM with the owner's own account. The only channel a script may
/// call in.
#[cfg(feature = "capture")]
pub const TEST_DM: Id = 1_568_693_333_864_480_939;

#[cfg(feature = "capture")]
impl Root {
    /// The `dmcall` steps of the harness. `open`, `start voice|video`, `leave`, `state`, and `inject ...`, which
    /// feeds official-shaped `CALL_CREATE` / `CALL_UPDATE` / `CALL_DELETE` dispatches to the stores: nothing is sent
    /// anywhere, so a real incoming call is never produced by a script.
    pub(crate) fn script_dmcall(&mut self, words: &[&str], cx: &mut Context<Self>) -> String {
        let channel = TEST_DM;
        let me = self.stores.current_user.as_ref().map(|user| user.id);
        let other = self
            .stores
            .private_channels
            .iter()
            .find(|dm| dm.id == channel)
            .and_then(|dm| self.stores.dm_recipient(dm))
            .map(|user| user.id);
        let (Some(me), Some(other)) = (me, other) else {
            return "the test DM is not loaded".to_owned();
        };
        let state = |user: Id, video: bool| {
            json!({
                "channel_id": channel.to_string(),
                "user_id": user.to_string(),
                "session_id": "harness",
                "self_mute": false,
                "self_deaf": false,
                "self_video": video,
                "mute": false,
                "deaf": false,
            })
        };
        let rings = |from: Id, to: Id| json!({ to.to_string(): from.to_string() });
        match words {
            ["open"] => {
                self.select_home(Some(channel));
                self.calls_after_dispatch_effects();
                cx.notify();
                "opened the test DM".to_owned()
            }
            ["leave"] => {
                self.leave_voice(cx);
                "left".to_owned()
            }
            ["online"] => {
                // The fixture account is Do Not Disturb, which gates the incoming card away; show it as online.
                self.dmc.shown_online = true;
                cx.notify();
                "the incoming card ignores Do Not Disturb here; nothing was sent".to_owned()
            }
            ["state"] => {
                let call = self.stores.calls.get_call(channel);
                format!(
                    "call {:?}; incoming {}; in voice {}",
                    call.map(|call| (call.message_id, call.ringing.clone(), call.unavailable)),
                    self.stores.calls.get_incoming_calls(false).len(),
                    self.call.is_some()
                )
            }
            ["fake-join"] => {
                self.select_home(Some(channel));
                self.stores.dispatch("VOICE_STATE_UPDATE", &state(me, false));
                self.fake_dm_call(channel, cx);
                "this account is shown in the call; nothing was sent".to_owned()
            }
            ["fake-leave"] => {
                self.call = None;
                let mut gone = state(me, false);
                gone["channel_id"] = Value::Null;
                self.stores.dispatch("VOICE_STATE_UPDATE", &gone);
                cx.notify();
                "the call is closed here; nothing was sent".to_owned()
            }
            ["inject", "callee"] => self.inject_call(
                "CALL_CREATE",
                json!({
                    "channel_id": channel.to_string(),
                    "message_id": "1568700000000000001",
                    "region": "us-east",
                    "ongoing_rings": {},
                    "voice_states": [state(other, false)],
                }),
                cx,
            ),
            ["inject", "incoming"] => {
                self.inject_call(
                    "CALL_CREATE",
                    json!({
                        "channel_id": channel.to_string(),
                        "message_id": "1568700000000000001",
                        "region": "us-east",
                        "ongoing_rings": rings(other, me),
                        "voice_states": [state(other, false)],
                    }),
                    cx,
                )
            }
            ["inject", "outgoing"] => {
                self.inject_call(
                    "CALL_CREATE",
                    json!({
                        "channel_id": channel.to_string(),
                        "message_id": "1568700000000000001",
                        "region": "us-east",
                        "ongoing_rings": rings(me, other),
                        "voice_states": [state(me, false)],
                    }),
                    cx,
                )
            }
            ["inject", "answered"] => self.inject_call(
                "CALL_UPDATE",
                json!({
                    "channel_id": channel.to_string(),
                    "message_id": "1568700000000000001",
                    "region": "us-east",
                    "ongoing_rings": {},
                }),
                cx,
            ),
            ["inject", "delete"] => self.inject_call(
                "CALL_DELETE",
                json!({ "channel_id": channel.to_string() }),
                cx,
            ),
            _ => "usage: dmcall open | start voice|video | leave | state | fake-join | fake-leave | online | inject incoming|outgoing|callee|answered|delete"
                .to_owned(),
        }
    }

    fn inject_call(&mut self, name: &str, data: Value, cx: &mut Context<Self>) -> String {
        if name == "CALL_CREATE"
            && let Some(states) = data["voice_states"].as_array()
        {
            for state in states {
                self.stores.dispatch("VOICE_STATE_UPDATE", state);
            }
        }
        self.stores.dispatch(name, &data);
        self.calls_after_dispatch(name, cx);
        format!("injected {name}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ring_body_leaves_undefined_fields_out() {
        assert_eq!(ring_body(None, None), json!({}));
        assert_eq!(
            ring_body(Some(&[7]), Some("user_context_menu")),
            json!({"recipients": ["7"], "analytics_location": "user_context_menu"})
        );
    }

    #[test]
    fn stop_ringing_names_recipients() {
        assert_eq!(
            stop_ringing_body(&[7, 8]),
            json!({"recipients": ["7", "8"]})
        );
        assert_eq!(stop_ringing_route(5), "/channels/5/call/stop-ringing");
        assert_eq!(ring_route(5), "/channels/5/call/ring");
    }

    #[test]
    fn call_connect_is_op_13_with_a_channel_id() {
        assert_eq!(call_connect_data(9), json!({"channel_id": "9"}));
        assert_eq!(Op::CallConnect as u8, 13);
    }
}
