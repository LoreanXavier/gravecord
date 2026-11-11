//! Calls in direct messages and group DMs: `CallStore` (module 470710), `IncomingCallStore` (module 596339 `i1`) and the
//! ring queue of `CallActions` (module 387755).
//!
//! The gateway names a call by its channel. `CALL_CREATE`, `CALL_UPDATE` and `CALL_DELETE` are the whole protocol
//! (`eB(["CALL_CREATE"], ...)` in module 225123): `{channel_id, message_id, region, ongoing_rings, voice_states}`,
//! `{channel_id, message_id, region, ongoing_rings}` and `{channel_id, unavailable}`. `ongoing_rings` maps the user ids
//! being rung to the id of whoever rang them (`Object.keys(ongoing_rings)` is the ringing list, `ongoing_rings[me]`
//! the sender of an incoming call). Older payloads carry a plain `ringing` list of ids instead; both are read.
//!
//! The pieces here only keep state. What the app does about it (the ring request, the sounds, the modal) reads the
//! getters and drains [`CallStore::take_effects`].

use crate::Id;
use serde_json::Value;
use std::collections::HashMap;

/// A user and, when the payload names it, who rang them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ring {
    pub user: Id,
    pub sender: Option<Id>,
}

/// One call of `CallStore`: `{channelId, messageId, region, ringing, unavailable, regionUpdated}`.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Call {
    pub channel_id: Id,
    /// The system message that announces the call (type 3). A call that was only asked for (`CALL_CONNECT`) has none.
    pub message_id: Option<Id>,
    pub region: Option<String>,
    /// `Object.keys(ongoing_rings)`: who is being rung right now.
    pub ringing: Vec<Id>,
    /// The senders of those rings, by the user rung.
    pub rings: Vec<Ring>,
    pub unavailable: bool,
    /// The region changed since the call began (`regionUpdated`).
    pub region_updated: bool,
}

/// Who a queued ring is for (`CALL_ENQUEUE_RING` `recipients`): everyone, or these users.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Recipients {
    All,
    Users(Vec<Id>),
}

/// What the app owes the service after a state change.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Effect {
    /// `POST /channels/<id>/call/ring` with `{recipients}`: the call a queued ring waited for now exists.
    Ring {
        channel: Id,
        recipients: Option<Vec<Id>>,
    },
    /// Op 13 `CALL_CONNECT`: ask the gateway for the call of this channel (`callConnect` in module 868).
    CallConnect { channel: Id },
}

/// The fields of the three call dispatches.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct CallPayload {
    pub channel_id: Id,
    pub message_id: Option<Id>,
    pub region: Option<String>,
    pub rings: Vec<Ring>,
    pub unavailable: bool,
}

fn id_of(value: &Value) -> Option<Id> {
    match value {
        Value::String(text) => text.parse().ok(),
        Value::Number(number) => number.as_u64(),
        _ => None,
    }
}

/// Reads `ongoing_rings` (an object of sender by ringing user, or a list of ringing users) and the older `ringing`
/// list.
pub fn parse_rings(data: &Value) -> Vec<Ring> {
    let mut out = Vec::new();
    match &data["ongoing_rings"] {
        Value::Object(map) => {
            for (user, sender) in map {
                if let Ok(user) = user.parse::<Id>() {
                    out.push(Ring {
                        user,
                        sender: id_of(sender),
                    });
                }
            }
            // `Object.keys` lists integer-like keys in ascending order.
            out.sort_by_key(|ring| ring.user);
        }
        Value::Array(list) => {
            out.extend(
                list.iter()
                    .filter_map(id_of)
                    .map(|user| Ring { user, sender: None }),
            );
        }
        _ => {}
    }
    if out.is_empty()
        && let Some(list) = data["ringing"].as_array()
    {
        out.extend(
            list.iter()
                .filter_map(id_of)
                .map(|user| Ring { user, sender: None }),
        );
    }
    out
}

impl CallPayload {
    pub fn from_value(data: &Value) -> Option<Self> {
        Some(Self {
            channel_id: id_of(&data["channel_id"])?,
            message_id: id_of(&data["message_id"]),
            region: data["region"].as_str().map(str::to_owned),
            rings: parse_rings(data),
            unavailable: data["unavailable"].as_bool().unwrap_or(false),
        })
    }
}

/// An incoming call of `IncomingCallStore`: `{channel, senderId, x, y}`.
#[derive(Clone, Debug, PartialEq)]
pub struct IncomingCall {
    pub channel: Id,
    pub sender: Option<Id>,
    /// `10 * (calls already showing)`: later cards sit a little further down and right of the first.
    pub offset: f32,
}

/// `CallStore` and `IncomingCallStore` with the ring queue.
#[derive(Default)]
pub struct CallStore {
    me: Option<Id>,
    calls: HashMap<Id, Call>,
    /// `enqueuedRings`: rings asked for before the call existed.
    enqueued: HashMap<Id, Vec<Recipients>>,
    incoming: Vec<IncomingCall>,
    effects: Vec<Effect>,
}

impl CallStore {
    pub fn set_current_user(&mut self, user: Option<Id>) {
        self.me = user;
    }

    // ---- CallStore getters (module 470710) ----

    pub fn get_call(&self, channel: Id) -> Option<&Call> {
        self.calls.get(&channel)
    }

    pub fn get_calls(&self) -> Vec<&Call> {
        let mut calls: Vec<&Call> = self.calls.values().collect();
        calls.sort_by_key(|call| call.channel_id);
        calls
    }

    pub fn get_message_id(&self, channel: Id) -> Option<Id> {
        self.get_call(channel).and_then(|call| call.message_id)
    }

    /// `isCallActive(channel, messageId)`: there is a call, it is available, and it has `message_id` (when asked) or
    /// a region.
    pub fn is_call_active(&self, channel: Id, message_id: Option<Id>) -> bool {
        self.calls.get(&channel).is_some_and(|call| {
            !call.unavailable
                && match message_id {
                    Some(message) => call.message_id == Some(message),
                    None => call.region.is_some(),
                }
        })
    }

    pub fn is_call_unavailable(&self, channel: Id) -> bool {
        self.calls
            .get(&channel)
            .is_some_and(|call| call.unavailable)
    }

    /// `getInternalState().enqueuedRings`.
    pub fn enqueued_rings(&self, channel: Id) -> Option<&[Recipients]> {
        self.enqueued.get(&channel).map(Vec::as_slice)
    }

    /// `p(channel)` of module 325909: a callable channel whose call has its message and is not unavailable. A ring
    /// is sent at once for such a channel and queued otherwise.
    pub fn call_ringable_now(&self, channel: Id) -> bool {
        self.calls
            .get(&channel)
            .is_some_and(|call| call.message_id.is_some() && !call.unavailable)
    }

    /// Whether `user` is being rung in `channel`.
    pub fn is_ringing(&self, channel: Id, user: Id) -> bool {
        self.calls
            .get(&channel)
            .is_some_and(|call| call.ringing.contains(&user))
    }

    // ---- IncomingCallStore getters (module 596339 `i1`) ----

    /// `getIncomingCalls()`: nothing while `gated` (Do Not Disturb or quiet mode, `i0`).
    pub fn get_incoming_calls(&self, gated: bool) -> &[IncomingCall] {
        if gated { &[] } else { &self.incoming }
    }

    pub fn has_incoming_calls(&self, gated: bool) -> bool {
        !gated && !self.incoming.is_empty()
    }

    pub fn get_first_incoming_call_id(&self, gated: bool) -> Option<Id> {
        if gated {
            None
        } else {
            self.incoming.first().map(|call| call.channel)
        }
    }

    /// `isIncomingCall` of module 620982: this account is among the ringing.
    pub fn is_incoming_call(&self, channel: Id) -> bool {
        self.me.is_some_and(|me| self.is_ringing(channel, me))
    }

    /// `isOngoingCall` of module 620982: a call that is active and does not ring this account.
    pub fn is_ongoing_call(&self, channel: Id) -> bool {
        self.is_call_active(channel, None) && !self.is_incoming_call(channel)
    }

    /// Who rang this account in `channel`, when the payload said.
    pub fn incoming_sender(&self, channel: Id) -> Option<Id> {
        self.incoming
            .iter()
            .find(|call| call.channel == channel)
            .and_then(|call| call.sender)
    }

    pub fn take_effects(&mut self) -> Vec<Effect> {
        std::mem::take(&mut self.effects)
    }

    // ---- actions ----

    /// `CONNECTION_CLOSED`.
    pub fn connection_closed(&mut self) {
        self.calls.clear();
        self.enqueued.clear();
        self.incoming.clear();
    }

    /// `A(force, channel)` of module 470710, run for `CONNECTION_OPEN`, `CONNECTION_RESUMED` and `CHANNEL_SELECT`:
    /// a private channel with no call on record gets a placeholder and a `CALL_CONNECT`.
    pub fn connect_channel(&mut self, channel: Id, is_private: bool, force: bool) -> bool {
        if !is_private || (self.calls.contains_key(&channel) && !force) {
            return false;
        }
        self.calls.entry(channel).or_insert_with(|| Call {
            channel_id: channel,
            ..Call::default()
        });
        self.effects.push(Effect::CallConnect { channel });
        true
    }

    pub fn call_create(&mut self, payload: &CallPayload) {
        let channel = payload.channel_id;
        self.calls.insert(
            channel,
            Call {
                channel_id: channel,
                message_id: payload.message_id,
                region: payload.region.clone(),
                ringing: payload.rings.iter().map(|ring| ring.user).collect(),
                rings: payload.rings.clone(),
                unavailable: false,
                region_updated: false,
            },
        );
        if let Some(queue) = self.enqueued.remove(&channel) {
            // `1!==e.indexOf("all")&&(e=null)`: the union of the queue, `null` (everyone) unless "all" sits at
            // index 1 of it, which never happens for a queue that holds one entry.
            let all = queue.contains(&Recipients::All);
            let recipients = if all {
                None
            } else {
                let mut users: Vec<Id> = Vec::new();
                for entry in &queue {
                    if let Recipients::Users(list) = entry {
                        for user in list {
                            if !users.contains(user) {
                                users.push(*user);
                            }
                        }
                    }
                }
                Some(users)
            };
            self.effects.push(Effect::Ring {
                channel,
                recipients,
            });
        }
    }

    pub fn call_update(&mut self, payload: &CallPayload) {
        let channel = payload.channel_id;
        let previous = self.calls.get(&channel);
        let region_updated = previous.is_some_and(|call| {
            call.region_updated || call.region.as_deref() != payload.region.as_deref()
        });
        let base = previous.cloned().unwrap_or_default();
        self.calls.insert(
            channel,
            Call {
                channel_id: channel,
                message_id: payload.message_id,
                region: payload.region.clone(),
                ringing: payload.rings.iter().map(|ring| ring.user).collect(),
                rings: payload.rings.clone(),
                region_updated,
                ..base
            },
        );
    }

    pub fn call_delete(&mut self, channel: Id, unavailable: bool) {
        match self.calls.get(&channel).cloned() {
            Some(call) if unavailable => {
                self.calls.insert(
                    channel,
                    Call {
                        unavailable: true,
                        ..call
                    },
                );
            }
            _ => {
                self.calls.insert(
                    channel,
                    Call {
                        channel_id: channel,
                        unavailable,
                        ..Call::default()
                    },
                );
            }
        }
        self.enqueued.remove(&channel);
    }

    /// `CALL_ENQUEUE_RING`: `E[t] = union(E[t] ?? [], recipients ?? ["all"])`.
    pub fn enqueue_ring(&mut self, channel: Id, recipients: Recipients) {
        let queue = self.enqueued.entry(channel).or_default();
        if !queue.contains(&recipients) {
            queue.push(recipients);
        }
    }

    /// `VOICE_CHANNEL_SELECT` with no channel: the queue is dropped.
    pub fn voice_channel_left(&mut self) {
        self.enqueued.clear();
    }

    pub fn channel_delete(&mut self, channel: Id) {
        self.enqueued.remove(&channel);
        self.calls.remove(&channel);
        self.incoming.retain(|call| call.channel != channel);
    }

    /// `VOICE_CHANNEL_SELECT` for `channel` (we joined it): its incoming call is answered.
    pub fn voice_channel_selected(&mut self, channel: Id) {
        self.incoming.retain(|call| call.channel != channel);
    }

    /// `iJ` for `CALL_CREATE` and `CALL_UPDATE`: an incoming call appears while this account is among the ringing and
    /// the channel is known, and goes when it no longer is.
    fn incoming_changed(&mut self, payload: &CallPayload, channel_known: bool) {
        let channel = payload.channel_id;
        let me_ring = self
            .me
            .and_then(|me| payload.rings.iter().find(|ring| ring.user == me))
            .copied();
        let showing = self.incoming.iter().any(|call| call.channel == channel);
        if !showing && let Some(ring) = me_ring {
            if !channel_known {
                return;
            }
            let offset = 10. * self.incoming.len() as f32;
            self.incoming.push(IncomingCall {
                channel,
                sender: ring.sender,
                offset,
            });
        } else if showing && me_ring.is_none() {
            self.incoming.retain(|call| call.channel != channel);
        }
    }

    /// Everything the gateway says about calls. `channel_known` tells whether a channel is one of the account's
    /// private channels.
    pub fn dispatch(&mut self, name: &str, data: &Value, channel_known: &dyn Fn(Id) -> bool) {
        match name {
            "READY" => {
                self.me = id_of(&data["user"]["id"]).or(self.me);
                // `CONNECTION_OPEN`: nothing of the old connection is kept.
                self.connection_closed();
            }
            "CALL_CREATE" => {
                if let Some(payload) = CallPayload::from_value(data) {
                    self.call_create(&payload);
                    self.incoming_changed(&payload, channel_known(payload.channel_id));
                }
            }
            "CALL_UPDATE" => {
                if let Some(payload) = CallPayload::from_value(data) {
                    self.call_update(&payload);
                    self.incoming_changed(&payload, channel_known(payload.channel_id));
                }
            }
            "CALL_DELETE" => {
                if let Some(channel) = id_of(&data["channel_id"]) {
                    self.call_delete(channel, data["unavailable"].as_bool().unwrap_or(false));
                    self.incoming.retain(|call| call.channel != channel);
                }
            }
            "CHANNEL_DELETE" => {
                if let Some(channel) = id_of(&data["id"]) {
                    self.channel_delete(channel);
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const ME: Id = 10;
    const OTHER: Id = 20;
    const DM: Id = 500;

    fn store() -> CallStore {
        let mut store = CallStore::default();
        store.set_current_user(Some(ME));
        store
    }

    fn known(channel: Id) -> bool {
        channel == DM
    }

    #[test]
    fn call_create_records_the_call_with_its_rings() {
        let mut store = store();
        store.dispatch(
            "CALL_CREATE",
            &json!({
                "channel_id": "500", "message_id": "900", "region": "rotterdam",
                "ongoing_rings": {"10": "20"}, "voice_states": []
            }),
            &known,
        );
        let call = store.get_call(DM).unwrap();
        assert_eq!(call.message_id, Some(900));
        assert_eq!(call.region.as_deref(), Some("rotterdam"));
        assert_eq!(call.ringing, vec![ME]);
        assert_eq!(
            call.rings,
            vec![Ring {
                user: ME,
                sender: Some(OTHER)
            }]
        );
        assert!(store.is_call_active(DM, None));
        assert!(store.is_call_active(DM, Some(900)));
        assert!(!store.is_call_active(DM, Some(901)));
        assert_eq!(store.get_message_id(DM), Some(900));
        assert!(store.is_incoming_call(DM));
        assert!(!store.is_ongoing_call(DM));
    }

    #[test]
    fn the_older_ringing_list_is_read_too() {
        let mut store = store();
        store.dispatch(
            "CALL_CREATE",
            &json!({"channel_id": "500", "message_id": "900", "region": "x", "ringing": ["10", "30"]}),
            &known,
        );
        assert_eq!(store.get_call(DM).unwrap().ringing, vec![10, 30]);
        assert!(store.is_incoming_call(DM));
        assert_eq!(store.incoming_sender(DM), None);
    }

    #[test]
    fn a_call_that_rings_someone_else_is_ongoing_not_incoming() {
        let mut store = store();
        store.dispatch(
            "CALL_CREATE",
            &json!({"channel_id": "500", "message_id": "900", "region": "x", "ongoing_rings": {"20": "10"}}),
            &known,
        );
        assert!(!store.is_incoming_call(DM));
        assert!(store.is_ongoing_call(DM));
        assert!(store.get_incoming_calls(false).is_empty());
    }

    #[test]
    fn an_incoming_call_follows_update_and_delete() {
        let mut store = store();
        store.dispatch(
            "CALL_CREATE",
            &json!({"channel_id": "500", "message_id": "900", "region": "x", "ongoing_rings": {"10": "20"}}),
            &known,
        );
        assert_eq!(store.get_incoming_calls(false).len(), 1);
        assert_eq!(store.incoming_sender(DM), Some(OTHER));
        assert_eq!(store.get_first_incoming_call_id(false), Some(DM));
        // A second device answers: the ring list loses this account.
        store.dispatch(
            "CALL_UPDATE",
            &json!({"channel_id": "500", "message_id": "900", "region": "x", "ongoing_rings": {}}),
            &known,
        );
        assert!(store.get_incoming_calls(false).is_empty());
        assert!(store.is_call_active(DM, None));
        store.dispatch(
            "CALL_UPDATE",
            &json!({"channel_id": "500", "message_id": "900", "region": "x", "ongoing_rings": {"10": "20"}}),
            &known,
        );
        assert_eq!(store.get_incoming_calls(false).len(), 1);
        store.dispatch("CALL_DELETE", &json!({"channel_id": "500"}), &known);
        assert!(store.get_incoming_calls(false).is_empty());
        let call = store.get_call(DM).unwrap();
        assert_eq!(call.message_id, None);
        assert!(!store.is_call_active(DM, None));
        assert!(!store.is_call_unavailable(DM));
    }

    #[test]
    fn a_channel_the_account_does_not_have_never_rings() {
        let mut store = store();
        store.dispatch(
            "CALL_CREATE",
            &json!({"channel_id": "777", "message_id": "9", "region": "x", "ongoing_rings": {"10": "20"}}),
            &known,
        );
        assert!(store.get_incoming_calls(false).is_empty());
    }

    #[test]
    fn do_not_disturb_hides_incoming_calls_without_forgetting_them() {
        let mut store = store();
        store.dispatch(
            "CALL_CREATE",
            &json!({"channel_id": "500", "message_id": "900", "region": "x", "ongoing_rings": {"10": "20"}}),
            &known,
        );
        assert!(!store.has_incoming_calls(true));
        assert!(store.get_incoming_calls(true).is_empty());
        assert_eq!(store.get_first_incoming_call_id(true), None);
        assert!(store.has_incoming_calls(false));
    }

    #[test]
    fn incoming_cards_stack_by_ten_pixels() {
        let mut store = store();
        for channel in ["500", "501"] {
            store.dispatch(
                "CALL_CREATE",
                &json!({"channel_id": channel, "message_id": "9", "region": "x", "ongoing_rings": {"10": "20"}}),
                &|_| true,
            );
        }
        let offsets: Vec<f32> = store
            .get_incoming_calls(false)
            .iter()
            .map(|call| call.offset)
            .collect();
        assert_eq!(offsets, vec![0., 10.]);
    }

    #[test]
    fn joining_the_call_answers_it() {
        let mut store = store();
        store.dispatch(
            "CALL_CREATE",
            &json!({"channel_id": "500", "message_id": "900", "region": "x", "ongoing_rings": {"10": "20"}}),
            &known,
        );
        store.voice_channel_selected(DM);
        assert!(store.get_incoming_calls(false).is_empty());
        // The call itself is still there until the gateway says otherwise.
        assert!(store.is_call_active(DM, None));
    }

    #[test]
    fn update_remembers_a_region_change() {
        let mut store = store();
        store.dispatch(
            "CALL_CREATE",
            &json!({"channel_id": "500", "message_id": "900", "region": "a", "ongoing_rings": {}}),
            &known,
        );
        assert!(!store.get_call(DM).unwrap().region_updated);
        store.dispatch(
            "CALL_UPDATE",
            &json!({"channel_id": "500", "message_id": "900", "region": "b", "ongoing_rings": {}}),
            &known,
        );
        assert!(store.get_call(DM).unwrap().region_updated);
    }

    #[test]
    fn unavailable_delete_keeps_the_call_and_marks_it() {
        let mut store = store();
        store.dispatch(
            "CALL_CREATE",
            &json!({"channel_id": "500", "message_id": "900", "region": "a", "ongoing_rings": {}}),
            &known,
        );
        store.dispatch(
            "CALL_DELETE",
            &json!({"channel_id": "500", "unavailable": true}),
            &known,
        );
        assert!(store.is_call_unavailable(DM));
        assert!(!store.is_call_active(DM, None));
        assert_eq!(store.get_message_id(DM), Some(900));
    }

    #[test]
    fn a_queued_ring_is_sent_when_the_call_appears() {
        let mut store = store();
        assert!(!store.call_ringable_now(DM));
        store.enqueue_ring(DM, Recipients::All);
        store.enqueue_ring(DM, Recipients::All);
        assert_eq!(store.enqueued_rings(DM).unwrap().len(), 1);
        store.dispatch(
            "CALL_CREATE",
            &json!({"channel_id": "500", "message_id": "900", "region": "a", "ongoing_rings": {}}),
            &known,
        );
        assert_eq!(
            store.take_effects(),
            vec![Effect::Ring {
                channel: DM,
                recipients: None
            }]
        );
        assert!(store.enqueued_rings(DM).is_none());
        assert!(store.call_ringable_now(DM));
    }

    #[test]
    fn queued_rings_for_people_are_merged() {
        let mut store = store();
        store.enqueue_ring(DM, Recipients::Users(vec![1, 2]));
        store.enqueue_ring(DM, Recipients::Users(vec![2, 3]));
        store.dispatch(
            "CALL_CREATE",
            &json!({"channel_id": "500", "message_id": "9", "region": "a", "ongoing_rings": {}}),
            &known,
        );
        assert_eq!(
            store.take_effects(),
            vec![Effect::Ring {
                channel: DM,
                recipients: Some(vec![1, 2, 3])
            }]
        );
    }

    #[test]
    fn leaving_voice_drops_the_queue() {
        let mut store = store();
        store.enqueue_ring(DM, Recipients::All);
        store.voice_channel_left();
        assert!(store.enqueued_rings(DM).is_none());
    }

    #[test]
    fn a_private_channel_without_a_call_is_asked_about_once() {
        let mut store = store();
        assert!(store.connect_channel(DM, true, false));
        assert_eq!(
            store.take_effects(),
            vec![Effect::CallConnect { channel: DM }]
        );
        assert!(!store.connect_channel(DM, true, false));
        assert!(store.connect_channel(DM, true, true));
        assert!(!store.connect_channel(DM, false, true));
        // The placeholder is not an active call.
        assert!(!store.is_call_active(DM, None));
        assert!(store.get_call(DM).is_some());
    }

    #[test]
    fn channel_delete_and_connection_closed_clear_everything() {
        let mut store = store();
        store.dispatch(
            "CALL_CREATE",
            &json!({"channel_id": "500", "message_id": "9", "region": "a", "ongoing_rings": {"10": "20"}}),
            &known,
        );
        store.dispatch("CHANNEL_DELETE", &json!({"id": "500"}), &known);
        assert!(store.get_call(DM).is_none());
        assert!(store.get_incoming_calls(false).is_empty());
        store.dispatch(
            "CALL_CREATE",
            &json!({"channel_id": "500", "message_id": "9", "region": "a", "ongoing_rings": {}}),
            &known,
        );
        store.dispatch("READY", &json!({"user": {"id": "10"}}), &known);
        assert!(store.get_calls().is_empty());
    }
}
