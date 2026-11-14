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
/// `body: {recipients: t}`: an absent `t` is dropped by `JSON.stringify`, leaving `{}`.
pub fn stop_ringing_body(recipients: Option<&[Id]>) -> Value {
    match recipients {
        Some(list) => json!({ "recipients": ids(list) }),
        None => json!({}),
    }
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

const NOT_FRIEND_KEY: &str = "dm-call-ringable:";

/// The key of the `GET /channels/{id}/call` that decides a call to someone who is not a friend.
pub fn not_friend_key(channel: Id, video: bool, ring: bool) -> String {
    format!("{NOT_FRIEND_KEY}{channel}:{}:{}", video as u8, ring as u8)
}

pub fn parse_not_friend_key(key: &str) -> Option<(Id, bool, bool)> {
    let mut parts = key.strip_prefix(NOT_FRIEND_KEY)?.split(':');
    let channel = parts.next()?.parse().ok()?;
    let video = parts.next()? == "1";
    let ring = parts.next()? == "1";
    Some((channel, video, ring))
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
        if name == "READY" {
            // `CONNECTION_OPEN` of `ApplicationStreamPreviewStore`.
            self.stream_previews.clear();
        }
        if matches!(name, "VOICE_STATE_UPDATE" | "CALL_CREATE" | "CALL_DELETE") {
            self.stream_pending_watch(cx);
        }
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

    /// The header's voice and video call buttons (`lw` in module 289287, the same for a DM and a group DM).
    pub(super) fn header_call_buttons(
        &self,
        channel: Id,
        other: Option<Id>,
        tool: &dyn Fn(&str) -> gpui::Div,
        cx: &mut Context<Self>,
    ) -> Vec<gpui::AnyElement> {
        use super::tooltip::{Side, Tip};
        use gpui::{IntoElement, StatefulInteractiveElement, Styled};
        let buttons = self.dm_call_buttons(channel, other);
        let one = |id: &'static str,
                   glyph: &str,
                   video: bool,
                   state: Option<(&'static str, bool)>,
                   cx: &mut Context<Self>| {
            state.map(|(tip, disabled)| {
                let button = self.header_stop(
                    self.with_tip(id, Tip::text(tip, Side::Bottom), tool(glyph), cx),
                    id,
                    cx,
                );
                if disabled {
                    button.into_any_element()
                } else {
                    button
                        .cursor_pointer()
                        .on_click(cx.listener(move |this, event: &gpui::ClickEvent, _, cx| {
                            this.tip = None;
                            let shift = event.modifiers().shift;
                            this.dm_call_clicked(channel, video, shift, cx);
                        }))
                        .into_any_element()
                }
            })
        };
        [
            one("dm-call", "dm_call", false, buttons.voice, cx),
            one("dm-video", "dm_video", true, buttons.video, cx),
        ]
        .into_iter()
        .flatten()
        .collect()
    }

    /// Resolves the private call channel for module 313998 (`Ring` / `Stop Ringing`): in a popped-out call window or
    /// full-screen overlay `shown_call.1` names the call's DM; otherwise the open private channel is used, falling back
    /// to `shown_call.1` when nonzero and private.
    pub(super) fn resolved_dm_call_channel(&self) -> Option<Id> {
        let is_private =
            |id: Id| id != 0 && self.stores.private_channels.iter().any(|dm| dm.id == id);
        if !self.vv.popout_render
            && self.guild.is_none()
            && let Some(channel) = self.channel.filter(|id| is_private(*id))
        {
            return Some(channel);
        }
        if self.vv.shown_call.0 == 0 && is_private(self.vv.shown_call.1) {
            return Some(self.vv.shown_call.1);
        }
        self.channel.filter(|id| is_private(*id))
    }

    /// Module 313998 (and 325909 `o`): a person's menu in a DM or group DM offers "Ring" when they are not in the
    /// call and "Stop Ringing" while the call rings them. The person must be a friend, not this account, not a bot,
    /// and the call must have participants. `Some(false)` is Ring, `Some(true)` is Stop Ringing.
    pub(super) fn ring_row_for(&self, user: Id, guild: Option<Id>) -> Option<bool> {
        if guild.is_some() {
            return None;
        }
        let channel = self.resolved_dm_call_channel()?;
        let other = self.stores.users.get(&user)?;
        let me = self.stores.current_user.as_ref().map(|me| me.id);
        if other.bot || Some(user) == me || self.stores.relationships.get(&user) != Some(&1) {
            return None;
        }
        let people = self.dm_call_people(channel);
        if people.is_empty() {
            return None;
        }
        match people.iter().find(|person| person.user == user) {
            None => Some(false),
            Some(person) if person.ringing => Some(true),
            Some(_) => None,
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
    pub(super) fn stop_ringing(&mut self, channel: Id, recipients: Option<&[Id]>) {
        info!("stopping a ring");
        #[cfg(feature = "capture")]
        self.dmc
            .stops
            .push(format!("{channel} {}", stop_ringing_body(recipients)));
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
                (dc_intl::tr("Call Unavailable"), true)
            } else if blocked {
                (dc_intl::tr("You cannot call a user you have blocked"), true)
            } else {
                (normal, false)
            }
        };
        CallButtons {
            voice: (!in_call).then(|| {
                disabled_tip(if active {
                    dc_intl::tr("Join Voice Call")
                } else {
                    dc_intl::tr("Start Voice Call")
                })
            }),
            video: (!in_call && !(active && !video_call)).then(|| {
                disabled_tip(if video_call {
                    dc_intl::tr("Join Video Call")
                } else {
                    dc_intl::tr("Start Video Call")
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
        let ring = !active && !shift;
        // Module 387755 `call(channel, video, ring, otherUserId)`: for a DM with someone who is not a friend, ask
        // `GET /channels/{id}/call` first. A blocked person: nothing happens.
        let other = self
            .stores
            .private_channels
            .iter()
            .find(|dm| dm.id == channel)
            .and_then(|dm| self.stores.dm_recipient(dm))
            .map(|user| user.id);
        if let Some(user) = other
            && self.stores.relationships.get(&user) != Some(&1)
        {
            if self.is_blocked(user) {
                return;
            }
            let _ = self.requests.send(Request::Rest {
                key: Some(not_friend_key(channel, video, ring)),
                method: reqwest::Method::GET,
                route: format!("/channels/{channel}/call"),
                body: None,
            });
            self.dmc.asking = Some(user);
            return;
        }
        self.start_call(channel, video, ring, cx);
    }

    /// The answer of `GET /channels/{id}/call`: a success selects the voice channel and rings only when the call is
    /// `ringable`; a failure opens the "Start Call" prompt (My50nf, IdKo2z, PMsq/b, BddRzS).
    pub(super) fn dm_call_ringable(&mut self, key: &str, body: &Value, cx: &mut Context<Self>) {
        let Some((channel, video, ring)) = parse_not_friend_key(key) else {
            return;
        };
        let asked = self.dmc.asking.take();
        if body["ok"].as_bool() == Some(true) {
            let ringable = body["body"]["ringable"].as_bool().unwrap_or(false);
            self.start_call(channel, video, ring && ringable, cx);
        } else if let Some(user) = asked {
            self.dmc.not_friend = Some(user);
        }
        cx.notify();
    }

    /// The "Start Call" prompt for someone who is not a friend (module 387755).
    pub(super) fn not_friend_modal(&self, cx: &mut Context<Self>) -> Option<gpui::AnyElement> {
        use gpui::{IntoElement, ParentElement, div};
        let user = self.dmc.not_friend?;
        let name = self
            .stores
            .users
            .get(&user)
            .map(|known| known.username.clone())
            .unwrap_or_default();
        let prefix = "You need to be friends with ";
        let text = gpui::StyledText::new(format!("{prefix}{name} to start a call"))
            .with_highlights(Some((
                prefix.len()..prefix.len() + name.len(),
                gpui::HighlightStyle {
                    font_weight: Some(gpui::FontWeight(600.)),
                    ..Default::default()
                },
            )));
        let spec = super::modal::Spec {
            id: "not-friend-call-modal",
            title: dc_intl::tr("Start Call").into(),
            subtitle: Some(div().child(text).into_any_element()),
            body: None,
            buttons: vec![
                super::modal::button(
                    "not-friend-call-cancel",
                    dc_intl::tr("Okay"),
                    super::modal::Tone::Secondary,
                    |this, _, _| this.dmc.not_friend = None,
                ),
                super::modal::button(
                    "not-friend-call-confirm",
                    dc_intl::tr("Send Friend Request"),
                    super::modal::Tone::Primary,
                    move |this, _, cx| {
                        this.dmc.not_friend = None;
                        this.set_relationship(user, Some(1), cx);
                    },
                ),
            ],
            close: Box::new(|this, _, _| this.dmc.not_friend = None),
        };
        Some(super::modal::render_small(spec, cx))
    }

    /// Join Call / Join with Video on a call that is already running: select its voice channel and open the DM
    /// (module 760571 `selectVoiceChannel(id, video)` then `transitionTo`), without ringing anyone.
    pub(super) fn join_dm_call(&mut self, channel: Id, video: bool, cx: &mut Context<Self>) {
        self.select_home(Some(channel));
        self.start_call(channel, video, false, cx);
        cx.notify();
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

/// The local group DM of the harness (`dmcall group`): never sent anywhere.
#[cfg(feature = "capture")]
pub(super) const GROUP_DM: Id = 1_570_000_000_000_000_100;

#[cfg(feature = "capture")]
impl Root {
    /// The `dmcall` steps of the harness. `open`, `start voice|video`, `leave`, `state`, and `inject ...`, which
    /// feeds official-shaped `CALL_CREATE` / `CALL_UPDATE` / `CALL_DELETE` dispatches to the stores: nothing is sent
    /// anywhere, so a real incoming call is never produced by a script.
    pub(crate) fn script_dmcall(&mut self, words: &[&str], cx: &mut Context<Self>) -> String {
        if let ["group-manage", rest @ ..] = words {
            return self.script_group_manage(rest, cx);
        }
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
        // This account's own state carries this connection's session, as a real one does; the others a made-up one.
        let own_session = self
            .stores
            .session_id
            .clone()
            .unwrap_or_else(|| "harness".to_owned());
        let state = |user: Id, video: bool| {
            json!({
                "channel_id": channel.to_string(),
                "user_id": user.to_string(),
                "session_id": if user == me { own_session.as_str() } else { "harness" },
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
            // `activity` / `activity linked`: the test person plays a made-up game whose application has a
            // connection entry point; its token lookup answers with no token (or with one), and the global feed holds a
            // live first-session entry for it. Nothing is sent.
            ["activity"] | ["activity", "linked"] => {
                let linked = words.len() > 1;
                let app = "1570000000000000200";
                let now = chrono::Utc::now().timestamp_millis();
                self.stores.dispatch(
                    "PRESENCE_UPDATE",
                    &json!({
                        "user": {"id": other.to_string()},
                        "status": "online",
                        "activities": [{
                            "type": 0, "name": "Fixture Game", "application_id": app,
                            "created_at": now,
                            "timestamps": {"start": now - 5_406_000},
                        }],
                    }),
                );
                self.fetched.insert(
                    format!("presence-app:{app}"),
                    json!([{"id": app, "name": "Fixture Game",
                            "connection_entrypoint_url": "https://example.com/link"}]),
                );
                self.fetched.insert(
                    format!("oauth-token:{app}"),
                    if linked {
                        json!([{"id": "1", "application": {"id": app}}])
                    } else {
                        json!([])
                    },
                );
                self.stores.content_inventory.set_feed(
                    dc_state::content_inventory::GLOBAL_FEED,
                    &json!({"entries": [{"content": {
                        "id": "1570000000000000300", "author_id": other.to_string(),
                        "author_type": 1, "content_type": 1,
                        "traits": [{"type": 3, "is_live": true}, {"type": 1, "first_time": true}],
                        "extra": {"application_id": app, "game_name": "Fixture Game"},
                    }}]}),
                    now,
                );
                cx.notify();
                "a fixture game activity with a connection entry point".to_owned()
            }
            ["online"] => {
                // The fixture account is Do Not Disturb, which gates the incoming card away; show it as online.
                self.dmc.shown_online = true;
                cx.notify();
                "the incoming card ignores Do Not Disturb here; nothing was sent".to_owned()
            }
            ["group"] => {
                // A local-only group DM of the test person and two made-up people, to see the group header.
                let group: Id = GROUP_DM;
                self.stores.dispatch(
                    "CHANNEL_CREATE",
                    &json!({
                        "id": group.to_string(),
                        "type": 3,
                        "name": null,
                        "recipients": [
                            {"id": other.to_string(), "username": "loreanxavier", "discriminator": "0"},
                            {"id": "1570000000000000201", "username": "sample-one", "discriminator": "0"},
                            {"id": "1570000000000000202", "username": "sample-two", "discriminator": "0"},
                        ],
                        "last_message_id": null,
                    }),
                );
                self.select_home(Some(group));
                cx.notify();
                "opened a local group DM; nothing was sent".to_owned()
            }
            ["notfriend"] => {
                // The prompt a failed `GET /channels/{id}/call` opens, without asking the service.
                self.dmc.not_friend = Some(other);
                cx.notify();
                "the Start Call prompt is up; nothing was sent".to_owned()
            }
            ["state"] => {
                let call = self.stores.calls.get_call(channel);
                format!(
                    "stop-ringing asked {:?}; call {:?}; incoming {}; in voice {}; controls shown {}; {}",
                    self.dmc.stops,
                    call.map(|call| (call.message_id, call.ringing.clone(), call.unavailable)),
                    self.stores.calls.get_incoming_calls(false).len(),
                    self.call.is_some(),
                    self.controls_shown(),
                    self.call_view_state()
                )
            }
            ["controls"] => {
                self.dmc.pin_controls = true;
                cx.notify();
                "the call controls stay shown".to_owned()
            }
            // `fake-join video`: this account and the test person both have their cameras on.
            ["fake-join", "video"] => {
                self.select_home(Some(channel));
                self.stores.dispatch("VOICE_STATE_UPDATE", &state(me, true));
                self.stores.dispatch("VOICE_STATE_UPDATE", &state(other, true));
                self.fake_dm_call(channel, cx);
                if let Some(call) = self.call.as_mut() {
                    call.camera = true;
                }
                cx.notify();
                "both accounts are shown with video; nothing was sent".to_owned()
            }
            // Group DM calls, all local: `group join <n>` / `group leave <n>` put member n (0 = the test person,
            // 1 and 2 the made-up ones, 3 = this account) in or out of the call; `group ring <n>` rings member n from
            // this account (the request is refused by the harness, the ring shows through a local CALL_UPDATE).
            // `menu <n>`: the person's context menu in the open channel (0 = the test person), where Ring and Stop Ringing show.
            ["menu", who] => {
                let user = match *who {
                    "0" => other,
                    "1" => 1_570_000_000_000_000_201,
                    "2" => 1_570_000_000_000_000_202,
                    _ => return "member is 0..2".to_owned(),
                };
                self.open_context_menu(
                    super::context_menu::Target::User { user, guild: None },
                    gpui::point(gpui::px(640.), gpui::px(300.)),
                    cx,
                );
                format!("menu of member {who}")
            }
            ["group", verb, who] => {
                let group: Id = GROUP_DM;
                let member = |n: &str| -> Option<Id> {
                    match n {
                        "0" => Some(other),
                        "1" => Some(1_570_000_000_000_000_201),
                        "2" => Some(1_570_000_000_000_000_202),
                        "3" => Some(me),
                        _ => None,
                    }
                };
                let Some(user) = member(who) else {
                    return "member is 0..3".to_owned();
                };
                let voice = |user: Id, here: bool| {
                    json!({
                        "channel_id": if here { json!(group.to_string()) } else { Value::Null },
                        "user_id": user.to_string(), "session_id": "harness",
                        "self_mute": false, "self_deaf": false, "self_video": false,
                        "mute": false, "deaf": false,
                    })
                };
                match *verb {
                    "join" | "leave" => {
                        let here = *verb == "join";
                        if self.stores.calls.get_call(group).is_none() {
                            self.inject_call(
                                "CALL_CREATE",
                                json!({"channel_id": group.to_string(),
                                    "message_id": "1570000000000000002", "region": "us-east",
                                    "ongoing_rings": {}, "voice_states": []}),
                                cx,
                            );
                        }
                        self.stores.dispatch("VOICE_STATE_UPDATE", &voice(user, here));
                        if user == me {
                            if here {
                                self.fake_dm_call(group, cx);
                            } else {
                                self.call = None;
                            }
                        }
                        cx.notify();
                        format!("member {who} {verb}s the group call; nothing was sent")
                    }
                    "ring" => {
                        self.ring_call(group, Some(vec![user]), Some("script"));
                        let ringing = self
                            .stores
                            .calls
                            .get_call(group)
                            .map(|call| call.ringing.clone())
                            .unwrap_or_default();
                        let mut rung: serde_json::Map<String, Value> = ringing
                            .iter()
                            .map(|id| (id.to_string(), json!(me.to_string())))
                            .collect();
                        rung.insert(user.to_string(), json!(me.to_string()));
                        self.inject_call(
                            "CALL_UPDATE",
                            json!({"channel_id": group.to_string(),
                                "message_id": "1570000000000000002", "region": "us-east",
                                "ongoing_rings": Value::Object(rung)}),
                            cx,
                        );
                        format!("rang member {who} locally; the request was refused")
                    }
                    _ => "group join|leave|ring 0..3".to_owned(),
                }
            }
            // `fake-stream`: the test person is in the call and going live; this account is not in it.
            ["fake-stream"] => {
                self.select_home(Some(channel));
                let mut live = state(other, false);
                live["self_stream"] = json!(true);
                self.stores.dispatch("VOICE_STATE_UPDATE", &live);
                cx.notify();
                "the test person is shown going live in the call; nothing was sent".to_owned()
            }
            // `fake-live own|watch`: a stream of this account, or one of the test person that this client watches and
            // has focused, shown without a connection.
            ["fake-live", "own"] => self
                .fake_stream(me, false, cx)
                .to_owned(),
            ["fake-live", "watch"] => {
                let mut live = state(other, false);
                live["self_stream"] = json!(true);
                self.stores.dispatch("VOICE_STATE_UPDATE", &live);
                let said = self.fake_stream(other, true, cx);
                self.vv
                    .focus
                    .click(other, super::video_tiles::TileKind::Stream);
                said.to_owned()
            }
            ["fake-live", "grid"] => {
                self.vv.focus.grid();
                cx.notify();
                "back to the grid".to_owned()
            }
            // `fake-flags`: the test person's client announced that clips are enabled (voice op 18).
            ["fake-flags"] => {
                if let Some(call) = self.call.as_mut() {
                    call.flags.insert(other, 1);
                }
                cx.notify();
                "the test person is shown with clips enabled; nothing was sent".to_owned()
            }
            // `fake-unfocused`: the client draws as if none of its windows had the focus.
            ["fake-unfocused"] => {
                super::voice_video::FAKE_UNFOCUSED.store(true, std::sync::atomic::Ordering::Relaxed);
                cx.notify();
                "drawn as if no window of the client were focused".to_owned()
            }
            // `fake-preview`: the test person's stream has a preview, their avatar's picture standing in for it.
            ["fake-preview"] => {
                let url = self
                    .stores
                    .users
                    .get(&other)
                    .map(|user| super::avatar_source(user).1)
                    .unwrap_or_default();
                let key = super::stream_previews::key_of(0, channel, other).to_string();
                self.stream_preview_fetched(
                    &format!("{}{key}", super::stream_previews::KEY_PREFIX),
                    &json!({ "url": url }),
                    cx,
                );
                "the stream has a preview picture; nothing was sent".to_owned()
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
            ["inject", "callee"] => {
                // The call's announcement (type 3, no `ended_timestamp`), which carries the "Join the call" link.
                let (author, global) = self
                    .stores
                    .users
                    .get(&other)
                    .map(|user| (user.username.clone(), user.global_name.clone()))
                    .unwrap_or_default();
                self.stores.dispatch(
                    "MESSAGE_CREATE",
                    &json!({
                        "id": "1570000000000000001",
                        "channel_id": channel.to_string(),
                        "type": 3,
                        "content": "",
                        "timestamp": chrono::Utc::now().to_rfc3339(),
                        "author": {"id": other.to_string(), "username": author, "global_name": global, "discriminator": "0"},
                        "call": {"participants": [other.to_string()]},
                    }),
                );
                self.inject_call(
                "CALL_CREATE",
                json!({
                    "channel_id": channel.to_string(),
                    "message_id": "1570000000000000001",
                    "region": "us-east",
                    "ongoing_rings": {},
                    // The official client, not rung and not connected, holds no voice state of the call
                    // (module 568598 takes participants from voice states and `call.ringing` only).
                    "voice_states": [],
                }),
                cx,
            )
            }
            ["inject", "incoming"] => {
                self.inject_call(
                    "CALL_CREATE",
                    json!({
                        "channel_id": channel.to_string(),
                        "message_id": "1570000000000000001",
                        "region": "us-east",
                        "ongoing_rings": rings(other, me),
                        "voice_states": [state(other, false)],
                    }),
                    cx,
                )
            }
            ["inject", "incoming", mode] => {
                self.inject_call(
                    "CALL_CREATE",
                    json!({
                        "channel_id": channel.to_string(),
                        "message_id": "1570000000000000001",
                        "region": "us-east",
                        "ongoing_rings": rings(other, me),
                        "voice_states": [state(other, *mode == "video")],
                    }),
                    cx,
                )
            }
            ["inject", "outgoing"] => {
                self.inject_call(
                    "CALL_CREATE",
                    json!({
                        "channel_id": channel.to_string(),
                        "message_id": "1570000000000000001",
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
                    "message_id": "1570000000000000001",
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
            _ => "usage: dmcall open | start voice|video | leave | state | fake-join | fake-leave | online | notfriend | group | inject incoming|outgoing|callee|answered|delete"
                .to_owned(),
        }
    }

    fn inject_call(&mut self, name: &str, data: Value, cx: &mut Context<Self>) -> String {
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
    fn the_not_friend_key_round_trips() {
        let key = not_friend_key(42, true, false);
        assert_eq!(parse_not_friend_key(&key), Some((42, true, false)));
        assert_eq!(parse_not_friend_key("other"), None);
    }

    #[test]
    fn stop_ringing_names_recipients() {
        assert_eq!(
            stop_ringing_body(Some(&[7, 8])),
            json!({"recipients": ["7", "8"]})
        );
        assert_eq!(stop_ringing_body(None), json!({}));
        assert_eq!(stop_ringing_route(5), "/channels/5/call/stop-ringing");
        assert_eq!(ring_route(5), "/channels/5/call/ring");
    }

    #[test]
    fn call_connect_is_op_13_with_a_channel_id() {
        assert_eq!(call_connect_data(9), json!({"channel_id": "9"}));
        assert_eq!(Op::CallConnect as u8, 13);
    }
}
