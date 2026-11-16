//! The simulated call: what Discord's gateway would answer the app's voice requests with (`VOICE_STATE_UPDATE`,
//! `VOICE_SERVER_UPDATE`, `CALL_*`, `STREAM_*`), the voice servers they point at, and the headless participants.
//!
//! Dispatch shapes are the ones the app's stores read (`dc-state` `VoiceState::from_value`, `calls.rs`,
//! `dc-voice` `streams::parse_dispatch`), which follow modules 225123, 470710 and 652896 of the pinned build. Their
//! order and the delays between them are assumptions (no voice traffic is in `design/snapshot/TRAFFIC`); see
//! `DECISIONS.md`.
use crate::server::{ConnReport, Feedback, Ticket, VoiceServer};
use crate::stats::{Board, Summary};
use dc_voice::streams::{Role, StreamJoin, StreamKey};
use serde_json::{Value, json};
use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex, Weak, atomic::AtomicBool};
use std::time::{Duration, Instant};
use tokio::runtime::{Handle, Runtime};
use tokio::sync::mpsc;

/// The account this simulation calls from, and where.
#[derive(Clone, Debug)]
pub struct Setup {
    pub me: u64,
    pub my_session: String,
    /// 0 for a DM or group DM call.
    pub guild: u64,
    pub channel: u64,
    /// Users the participants are, in order (a DM's recipient first). More participants get made-up ids.
    pub people: Vec<u64>,
}

/// Where dispatches for the app go.
pub type Sink = Arc<dyn Fn(String, Value) + Send + Sync>;

#[derive(Clone, Debug)]
struct VoiceStateRecord {
    user: u64,
    session: String,
    channel: Option<u64>,
    self_mute: bool,
    self_deaf: bool,
    self_video: bool,
    self_stream: bool,
}

struct StreamRecord {
    owner: u64,
    rtc: u64,
    viewers: Vec<u64>,
    paused: bool,
}

/// One headless participant.
struct Peer {
    user: u64,
    session: String,
    controls: dc_voice::Controls,
    board: Board,
    events: Arc<Mutex<Vec<String>>>,
    connected: Arc<std::sync::atomic::AtomicBool>,
    own: Option<(StreamKey, dc_voice::StreamControls)>,
    watching: HashMap<u64, dc_voice::StreamControls>,
    snapshots: Arc<Mutex<HashMap<(u64, &'static str), dc_media::Picture>>>,
    plan: dc_voice::sim::Plan,
}

struct World {
    setup: Setup,
    states: Vec<VoiceStateRecord>,
    call_exists: bool,
    ringing: Vec<u64>,
    streams: HashMap<StreamKey, StreamRecord>,
    next_rtc: u64,
    peers: BTreeMap<String, Peer>,
    next_person: usize,
    log: Vec<String>,
}

pub(crate) struct Inner {
    handle: Handle,
    world: Mutex<World>,
    call_server: Mutex<Arc<VoiceServer>>,
    stream_server: Arc<VoiceServer>,
    retired: Mutex<Vec<Arc<VoiceServer>>>,
    dispatch: mpsc::UnboundedSender<(Instant, String, Value)>,
    started: Instant,
    /// What our own client's hub delivered (the app registers the tap).
    pub(crate) my_board: Board,
}

/// A running simulation. Dropping it stops its runtime, its servers and its participants.
pub struct Sim {
    runtime: Option<Runtime>,
    inner: Arc<Inner>,
}

/// The tone a participant's voice carries, by its place in the call; the app listens for these.
pub const VOICE_TONES: [f32; 4] = [450.0, 650.0, 850.0, 1050.0];
/// The tones of participants' stream audio.
pub const STREAM_TONES: [f32; 4] = [1250.0, 1450.0, 1650.0, 1850.0];

fn kind(source: dc_voice::VideoSource) -> &'static str {
    match source {
        dc_voice::VideoSource::Camera => "camera",
        dc_voice::VideoSource::Stream(_) => "stream",
    }
}

impl Sim {
    /// Turns the process into a simulated one (`dc_voice::sim::activate`) and starts the two voice servers (the call's
    /// and the streams') on 127.0.0.1.
    pub fn start(setup: Setup, sink: Sink) -> std::io::Result<Self> {
        dc_voice::sim::activate();
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(3)
            .thread_name("dc-callsim")
            .enable_all()
            .build()?;
        let handle = runtime.handle().clone();
        let (call_server, stream_server) = runtime.block_on(async {
            Ok::<_, std::io::Error>((VoiceServer::start().await?, VoiceServer::start().await?))
        })?;
        let (dispatch, mut queue) = mpsc::unbounded_channel::<(Instant, String, Value)>();
        // Dispatches leave in order, each not before its time.
        runtime.spawn(async move {
            while let Some((due, name, data)) = queue.recv().await {
                tokio::time::sleep_until(due.into()).await;
                sink(name, data);
            }
        });
        let inner = Arc::new(Inner {
            handle,
            world: Mutex::new(World {
                setup,
                states: Vec::new(),
                call_exists: false,
                ringing: Vec::new(),
                streams: HashMap::new(),
                next_rtc: 1_580_000_000_000_000_001,
                peers: BTreeMap::new(),
                next_person: 0,
                log: Vec::new(),
            }),
            call_server: Mutex::new(call_server),
            stream_server,
            retired: Mutex::new(Vec::new()),
            dispatch,
            started: Instant::now(),
            my_board: Board::default(),
        });
        Ok(Self {
            runtime: Some(runtime),
            inner,
        })
    }

    pub fn handle(&self) -> Handle {
        self.inner.handle.clone()
    }

    /// The board our own client's pictures are recorded on (feed it from the app's hub tap).
    pub fn my_board(&self) -> Board {
        self.inner.my_board.clone()
    }

    pub fn setup(&self) -> Setup {
        self.inner.lock().setup.clone()
    }

    // ----- the app's requests -----

    /// Op 4 from the app. Answered with our voice state, the call's creation in a DM, and a voice server on joining.
    pub fn voice_state(&self, guild: u64, channel: Option<u64>, flags: (bool, bool, bool)) {
        self.inner.voice_state(guild, channel, flags);
    }

    /// Op 13, 18, 19, 20, 21 or 22 from the app.
    pub fn stream_op(&self, op: u8, data: &Value) {
        self.inner.stream_op(op, data);
    }

    /// A REST request of a call (ring, stop ringing, the ringable check): the answer body, if the route is one.
    pub fn rest(&self, method: &str, route: &str, body: Option<&Value>) -> Option<Value> {
        self.inner.rest(method, route, body)
    }

    // ----- the participants -----

    /// A participant joins the call. `same_account` makes it this account on another device.
    pub fn peer_add(&self, name: &str, same_account: bool) -> String {
        Inner::peer_add(&self.inner, name, same_account)
    }

    pub fn peer_leave(&self, name: &str) -> String {
        self.inner.peer_leave(name)
    }

    pub fn peer_camera(&self, name: &str, on: bool) -> String {
        self.inner.peer_camera(name, on)
    }

    /// `content` is motion, static or text.
    pub fn peer_stream_start(
        &self,
        name: &str,
        width: u32,
        height: u32,
        fps: u32,
        content: dc_media::sim::Content,
    ) -> String {
        Inner::peer_stream_start(&self.inner, name, width, height, fps, content)
    }

    pub fn peer_stream_stop(&self, name: &str) -> String {
        self.inner.peer_stream_stop(name)
    }

    /// `name` watches the stream of `owner`: another participant's name, or "me".
    pub fn peer_watch(&self, name: &str, owner: &str) -> String {
        Inner::peer_watch(&self.inner, name, owner)
    }

    pub fn peer_unwatch(&self, name: &str, owner: &str) -> String {
        self.inner.peer_unwatch(name, owner)
    }

    /// Saves the newest picture `name` received of `owner`'s `kind` ("camera" or "stream") as PNG.
    pub fn peer_shot(&self, name: &str, owner: &str, kind: &str, path: &std::path::Path) -> String {
        self.inner.peer_shot(name, owner, kind, path)
    }

    pub fn peer_summary(
        &self,
        name: &str,
        owner: &str,
        kind: &str,
        window: Duration,
    ) -> Option<Summary> {
        self.inner.peer_summary(name, owner, kind, window)
    }

    pub fn peer_connected(&self, name: &str) -> bool {
        self.inner
            .lock()
            .peers
            .get(name)
            .is_some_and(|peer| peer.connected.load(std::sync::atomic::Ordering::Acquire))
    }

    pub fn peer_session(&self, name: &str) -> Option<String> {
        self.inner
            .lock()
            .peers
            .get(name)
            .map(|peer| peer.session.clone())
    }

    pub fn peer_user(&self, name: &str) -> Option<u64> {
        self.inner.lock().peers.get(name).map(|peer| peer.user)
    }

    pub fn peer_events(&self, name: &str) -> Vec<String> {
        self.inner
            .lock()
            .peers
            .get(name)
            .map(|peer| {
                peer.events
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .clone()
            })
            .unwrap_or_default()
    }

    // ----- the network and the servers -----

    /// The path of `name` ("me" for our client), both directions.
    pub fn set_link(&self, name: &str, shape: crate::link::Shape) {
        self.inner.call_server().set_shape(name, shape, shape);
        self.inner.stream_server.set_shape(name, shape, shape);
        self.inner
            .note(format!("link {name}: {}", shape.describe()));
    }

    pub fn set_feedback(&self, feedback: Feedback) {
        self.inner.call_server().set_feedback(feedback);
        self.inner.stream_server.set_feedback(feedback);
        self.inner.note(format!("server feedback {feedback:?}"));
    }

    pub fn feedback(&self) -> Feedback {
        self.inner.stream_server.feedback()
    }

    /// The call moves to another voice server: our client gets `VOICE_SERVER_UPDATE`, the participants reconnect.
    pub fn server_move(&self) -> String {
        Inner::server_move(&self.inner)
    }

    /// The stream of `owner` that our client watches or sends is named again by `STREAM_SERVER_UPDATE` with a new
    /// token, and the stream server drops our old connection to it (a stream server reassignment).
    pub fn stream_move(&self, owner: u64) -> String {
        self.inner.stream_move(owner)
    }

    /// The stream server drops our connections to its streams with `code`, and nothing is dispatched: a transient
    /// loss the client has to recover from by itself.
    pub fn stream_drop(&self, code: u16) -> String {
        let dropped = self.inner.stream_server.close("me", true, code)
            + self.inner.stream_server.close("me", false, code);
        format!("{dropped} stream connection(s) of ours dropped with {code}")
    }

    /// Close code 4014 on our call connection; then `after`: "stay" (nothing), "moved" (a voice server update
    /// follows), "gone" (our voice state leaves the channel).
    pub fn kick(&self, after: &str) -> String {
        self.inner.kick(after)
    }

    /// What the servers saw of `label`'s call connection, or (`stream`) of its stream connection, the one it sends
    /// on before the ones it watches with.
    pub fn conn_report(&self, label: &str, stream: bool) -> Option<ConnReport> {
        let mut reports: Vec<ConnReport> = self
            .inner
            .call_server()
            .reports()
            .into_iter()
            .chain(self.inner.stream_server.reports())
            .filter(|report| report.label == label && report.stream == stream)
            .collect();
        reports.sort_by_key(|report| !report.video);
        reports.into_iter().next()
    }

    /// Connections open on all servers (the call's, retired ones, the streams').
    pub fn connections(&self) -> usize {
        self.inner.call_server().connection_count()
            + self.inner.stream_server.connection_count()
            + self
                .inner
                .retired
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .iter()
                .map(|server| server.connection_count())
                .sum::<usize>()
    }

    /// Everything measured, as text for the step log.
    pub fn stats(&self, window: Duration) -> String {
        self.inner.stats(window)
    }

    pub fn log(&self) -> Vec<String> {
        let mut lines = self.inner.lock().log.clone();
        lines.extend(
            self.inner
                .call_server()
                .log()
                .into_iter()
                .map(|line| format!("call server {line}")),
        );
        lines.extend(
            self.inner
                .stream_server
                .log()
                .into_iter()
                .map(|line| format!("stream server {line}")),
        );
        lines
    }
}

impl Drop for Sim {
    fn drop(&mut self) {
        let peers: Vec<String> = self.inner.lock().peers.keys().cloned().collect();
        for name in peers {
            self.inner.peer_leave(&name);
        }
        if let Some(runtime) = self.runtime.take() {
            runtime.shutdown_timeout(Duration::from_secs(2));
        }
    }
}

fn state_json(guild: u64, state: &VoiceStateRecord) -> Value {
    json!({
        "guild_id": (guild != 0).then(|| guild.to_string()),
        "channel_id": state.channel.map(|c| c.to_string()),
        "user_id": state.user.to_string(),
        "session_id": state.session,
        "self_mute": state.self_mute,
        "self_deaf": state.self_deaf,
        "self_video": state.self_video,
        "self_stream": state.self_stream,
        "mute": false,
        "deaf": false,
        "suppress": false,
        "request_to_speak_timestamp": null,
    })
}

impl Inner {
    fn lock(&self) -> std::sync::MutexGuard<'_, World> {
        self.world.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn call_server(&self) -> Arc<VoiceServer> {
        self.call_server
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    fn note(&self, line: String) {
        let at = self.started.elapsed().as_millis();
        let mut world = self.lock();
        world.log.push(format!("[{at:>7} ms] {line}"));
        if world.log.len() > 2000 {
            world.log.drain(..500);
        }
    }

    /// `name` with `data` to the app, `after` from now (in order after everything already queued).
    fn emit(&self, after: Duration, name: &str, data: Value) {
        let summary = match name {
            "VOICE_STATE_UPDATE" => format!(
                "{name} channel {} video {} stream {}",
                data["channel_id"].as_str().map_or("none", |_| "set"),
                data["self_video"],
                data["self_stream"]
            ),
            _ => name.to_owned(),
        };
        self.note(format!("dispatch {summary} (+{} ms)", after.as_millis()));
        let _ = self
            .dispatch
            .send((Instant::now() + after, name.to_owned(), data));
    }

    fn place(&self) -> (u64, u64) {
        let world = self.lock();
        (world.setup.guild, world.setup.channel)
    }

    fn key_of(&self, owner: u64) -> StreamKey {
        let (guild, channel) = self.place();
        if guild != 0 {
            StreamKey::Guild {
                guild,
                channel,
                owner,
            }
        } else {
            StreamKey::Call { channel, owner }
        }
    }

    /// Sets the voice state of `(user, session)` and dispatches it.
    fn set_state(&self, after: Duration, state: VoiceStateRecord) {
        let guild = {
            let mut world = self.lock();
            world
                .states
                .retain(|known| !(known.user == state.user && known.session == state.session));
            if state.channel.is_some() {
                // One voice state per user: another session of the same account leaves.
                world.states.retain(|known| known.user != state.user);
                world.states.push(state.clone());
            }
            world.setup.guild
        };
        self.emit(after, "VOICE_STATE_UPDATE", state_json(guild, &state));
    }

    fn state_of(&self, user: u64, session: &str) -> Option<VoiceStateRecord> {
        self.lock()
            .states
            .iter()
            .find(|state| state.user == user && state.session == session)
            .cloned()
    }

    fn call_create_if_needed(&self, after: Duration) {
        let (guild, channel, needed, states) = {
            let mut world = self.lock();
            let needed = world.setup.guild == 0 && !world.call_exists;
            world.call_exists |= needed;
            (
                world.setup.guild,
                world.setup.channel,
                needed,
                world.states.clone(),
            )
        };
        if needed {
            let states: Vec<Value> = states
                .iter()
                .filter(|state| state.channel == Some(channel))
                .map(|state| state_json(guild, state))
                .collect();
            self.emit(
                after,
                "CALL_CREATE",
                json!({"channel_id": channel.to_string(), "message_id": "1580000000000000001",
                    "region": "us-east", "ongoing_rings": {}, "voice_states": states}),
            );
        }
    }

    fn voice_server_update(&self, after: Duration, ticket: Ticket) {
        let server = self.call_server();
        let (guild, channel) = (ticket.server_id, ticket.channel_id);
        let dm = self.place().0 == 0;
        let token = server.issue(ticket);
        self.emit(
            after,
            "VOICE_SERVER_UPDATE",
            json!({
                "token": token,
                "guild_id": (!dm).then(|| guild.to_string()),
                "channel_id": dm.then(|| channel.to_string()),
                "endpoint": server.endpoint(),
            }),
        );
    }

    fn stream_move(&self, owner: u64) -> String {
        let (found, me, session) = {
            let world = self.lock();
            let me = world.setup.me;
            let found = world
                .streams
                .iter()
                .find(|(_, stream)| {
                    stream.owner == owner && (owner == me || stream.viewers.contains(&me))
                })
                .map(|(key, stream)| (*key, stream.rtc));
            (found, me, world.setup.my_session.clone())
        };
        let Some((key, rtc)) = found else {
            return "our client neither sends nor watches that stream".to_owned();
        };
        let dropped = self.stream_server.close("me", true, 1001)
            + self.stream_server.close("me", false, 1001);
        let token = self.stream_server.issue(Ticket {
            user: me,
            session,
            server_id: rtc,
            channel_id: rtc - 1,
            label: "me".to_owned(),
        });
        self.note(format!(
            "stream {key} moves: {dropped} old connection(s) dropped"
        ));
        self.emit(
            Duration::from_millis(30),
            "STREAM_SERVER_UPDATE",
            json!({"stream_key": key.to_string(), "endpoint": self.stream_server.endpoint(), "token": token,
                "guild_id": key.guild().map(|g| g.to_string())}),
        );
        format!("stream moved; {dropped} old connection(s) dropped")
    }

    fn my_ticket(&self) -> Ticket {
        let world = self.lock();
        let setup = &world.setup;
        Ticket {
            user: setup.me,
            session: setup.my_session.clone(),
            server_id: if setup.guild != 0 {
                setup.guild
            } else {
                setup.channel
            },
            channel_id: setup.channel,
            label: "me".to_owned(),
        }
    }

    fn voice_state(&self, guild: u64, channel: Option<u64>, flags: (bool, bool, bool)) {
        let (place_guild, place_channel, me, session) = {
            let world = self.lock();
            (
                world.setup.guild,
                world.setup.channel,
                world.setup.me,
                world.setup.my_session.clone(),
            )
        };
        if guild != place_guild || channel.is_some_and(|c| c != place_channel) {
            self.note("op 4 for another channel: not simulated, nothing answered".to_owned());
            return;
        }
        let before = self.state_of(me, &session);
        let streaming = before.as_ref().is_some_and(|state| state.self_stream);
        let state = VoiceStateRecord {
            user: me,
            session,
            channel,
            self_mute: flags.0,
            self_deaf: flags.1,
            self_video: flags.2,
            self_stream: streaming && channel.is_some(),
        };
        self.note(format!(
            "op 4: {} mute {} deaf {} video {}",
            if channel.is_some() {
                "in the channel"
            } else {
                "leave"
            },
            flags.0,
            flags.1,
            flags.2
        ));
        self.set_state(Duration::from_millis(40), state);
        if channel.is_some() {
            self.call_create_if_needed(Duration::from_millis(30));
            if before.is_none() {
                self.voice_server_update(Duration::from_millis(80), self.my_ticket());
            }
        } else {
            // Our streams end, and a call nobody is in any more ends.
            let ours: Vec<StreamKey> = {
                let world = self.lock();
                world
                    .streams
                    .iter()
                    .filter(|(_, s)| s.owner == me)
                    .map(|(k, _)| *k)
                    .collect()
            };
            for key in ours {
                self.end_stream(key, "user_requested");
            }
            let mut world = self.lock();
            for stream in world.streams.values_mut() {
                stream.viewers.retain(|viewer| *viewer != me);
            }
            let empty = !world
                .states
                .iter()
                .any(|s| s.channel == Some(place_channel));
            let ends = world.setup.guild == 0 && world.call_exists && empty;
            if ends {
                world.call_exists = false;
                world.ringing.clear();
            }
            drop(world);
            if ends {
                self.emit(
                    Duration::from_millis(200),
                    "CALL_DELETE",
                    json!({"channel_id": place_channel.to_string()}),
                );
            }
        }
    }

    fn stream_key(data: &Value) -> Option<StreamKey> {
        StreamKey::parse(data["stream_key"].as_str()?)
    }

    fn stream_op(&self, op: u8, data: &Value) {
        let me = self.lock().setup.me;
        match op {
            13 => {
                self.note("op 13 call connect".to_owned());
                let exists = self.lock().call_exists;
                if exists {
                    let (guild, channel, states, ringing) = {
                        let world = self.lock();
                        (
                            world.setup.guild,
                            world.setup.channel,
                            world.states.clone(),
                            world.ringing.clone(),
                        )
                    };
                    let states: Vec<Value> = states
                        .iter()
                        .filter(|state| state.channel == Some(channel))
                        .map(|state| state_json(guild, state))
                        .collect();
                    let rings: serde_json::Map<String, Value> = ringing
                        .iter()
                        .map(|user| (user.to_string(), json!(me.to_string())))
                        .collect();
                    self.emit(
                        Duration::from_millis(40),
                        "CALL_CREATE",
                        json!({"channel_id": channel.to_string(), "message_id": "1580000000000000001",
                            "region": "us-east", "ongoing_rings": rings, "voice_states": states}),
                    );
                }
            }
            18 => {
                let key = self.key_of(me);
                let session = self.lock().setup.my_session.clone();
                let Some(mut state) = self.state_of(me, &session) else {
                    self.note("op 18 outside the call: refused".to_owned());
                    return;
                };
                let rtc = {
                    let mut world = self.lock();
                    world.next_rtc += 2;
                    let rtc = world.next_rtc;
                    world.streams.insert(
                        key,
                        StreamRecord {
                            owner: me,
                            rtc,
                            viewers: Vec::new(),
                            paused: false,
                        },
                    );
                    rtc
                };
                self.note(format!("op 18: our stream {key}"));
                state.self_stream = true;
                self.set_state(Duration::from_millis(30), state);
                self.stream_halves(key, rtc, me, session, Vec::new(), false);
            }
            19 => {
                let Some(key) = Self::stream_key(data) else {
                    return;
                };
                self.note(format!("op 19 for {key}"));
                if key.owner() == me {
                    self.end_stream(key, "user_requested");
                } else {
                    let mut world = self.lock();
                    if let Some(stream) = world.streams.get_mut(&key) {
                        stream.viewers.retain(|viewer| *viewer != me);
                    }
                }
            }
            20 => {
                let Some(key) = Self::stream_key(data) else {
                    return;
                };
                self.note(format!("op 20: watch {key}"));
                let session = self.lock().setup.my_session.clone();
                let found = {
                    let mut world = self.lock();
                    world.streams.get_mut(&key).map(|stream| {
                        if !stream.viewers.contains(&me) {
                            stream.viewers.push(me);
                        }
                        (stream.rtc, stream.viewers.clone(), stream.paused)
                    })
                };
                match found {
                    Some((rtc, viewers, paused)) => {
                        self.stream_halves(key, rtc, me, session, viewers, paused)
                    }
                    None => self.emit(
                        Duration::from_millis(40),
                        "STREAM_DELETE",
                        json!({"stream_key": key.to_string(), "reason": "unauthorized", "unavailable": false}),
                    ),
                }
            }
            22 => {
                let Some(key) = Self::stream_key(data) else {
                    return;
                };
                let paused = data["paused"].as_bool().unwrap_or(false);
                let viewers = {
                    let mut world = self.lock();
                    world.streams.get_mut(&key).map(|stream| {
                        stream.paused = paused;
                        stream.viewers.clone()
                    })
                };
                if let Some(viewers) = viewers {
                    self.stream_update(key, viewers, paused);
                }
            }
            21 => {}
            other => self.note(format!("gateway op {other} ignored")),
        }
    }

    /// `STREAM_CREATE` and `STREAM_SERVER_UPDATE` of `key` for our client.
    fn stream_halves(
        &self,
        key: StreamKey,
        rtc: u64,
        me: u64,
        session: String,
        viewers: Vec<u64>,
        paused: bool,
    ) {
        let ids: Vec<String> = viewers.iter().map(u64::to_string).collect();
        self.emit(
            Duration::from_millis(60),
            "STREAM_CREATE",
            json!({"stream_key": key.to_string(), "rtc_server_id": rtc.to_string(), "region": "us-east",
                "viewer_ids": ids, "paused": paused}),
        );
        let token = self.stream_server.issue(Ticket {
            user: me,
            session,
            server_id: rtc,
            channel_id: rtc - 1,
            label: "me".to_owned(),
        });
        self.emit(
            Duration::from_millis(30),
            "STREAM_SERVER_UPDATE",
            json!({"stream_key": key.to_string(), "endpoint": self.stream_server.endpoint(), "token": token,
                "guild_id": key.guild().map(|g| g.to_string())}),
        );
    }

    fn stream_update(&self, key: StreamKey, viewers: Vec<u64>, paused: bool) {
        let ids: Vec<String> = viewers.iter().map(u64::to_string).collect();
        self.emit(
            Duration::from_millis(40),
            "STREAM_UPDATE",
            json!({"stream_key": key.to_string(), "viewer_ids": ids, "paused": paused}),
        );
    }

    /// A stream ends: its owner's voice state, the dispatch to our client when it sends or watches it, and every
    /// participant watching it stops.
    fn end_stream(&self, key: StreamKey, reason: &str) {
        let (record, me, session) = {
            let mut world = self.lock();
            (
                world.streams.remove(&key),
                world.setup.me,
                world.setup.my_session.clone(),
            )
        };
        let Some(record) = record else { return };
        let owner_state = {
            let world = self.lock();
            world
                .states
                .iter()
                .find(|s| s.user == record.owner && s.channel.is_some())
                .cloned()
        };
        if let Some(mut state) = owner_state {
            state.self_stream = false;
            self.set_state(Duration::from_millis(20), state);
        }
        if record.owner == me || record.viewers.contains(&me) {
            self.emit(
                Duration::from_millis(30),
                "STREAM_DELETE",
                json!({"stream_key": key.to_string(), "reason": reason, "unavailable": false}),
            );
        }
        let _ = session;
        let mut world = self.lock();
        for peer in world.peers.values_mut() {
            if let Some(controls) = peer.watching.remove(&key.owner()) {
                controls.stop();
            }
        }
    }

    fn rest(&self, method: &str, route: &str, body: Option<&Value>) -> Option<Value> {
        let channel = self.place().1;
        let prefix = format!("/channels/{channel}/call");
        if !route.starts_with("/channels/") || !route.contains("/call") {
            return None;
        }
        if !route.starts_with(&prefix) {
            self.note(format!(
                "{method} on another channel's call: answered with nothing"
            ));
            return Some(json!({}));
        }
        let rest = &route[prefix.len()..];
        let me = self.lock().setup.me;
        match (method, rest) {
            ("GET", "") => Some(json!({"ringable": true})),
            ("POST", "/ring") | ("POST", "/stop-ringing") => {
                let ringing = rest == "/ring";
                let wanted: Vec<u64> = body
                    .and_then(|b| b["recipients"].as_array())
                    .map(|list| {
                        list.iter()
                            .filter_map(|id| id.as_str()?.parse().ok())
                            .collect()
                    })
                    .unwrap_or_else(|| self.lock().setup.people.iter().take(1).copied().collect());
                let rings = {
                    let mut world = self.lock();
                    for user in &wanted {
                        world.ringing.retain(|known| known != user);
                        if ringing {
                            world.ringing.push(*user);
                        }
                    }
                    world.ringing.clone()
                };
                self.note(format!("{method} {rest}: {} ringing", rings.len()));
                let map: serde_json::Map<String, Value> = rings
                    .iter()
                    .map(|user| (user.to_string(), json!(me.to_string())))
                    .collect();
                self.emit(
                    Duration::from_millis(50),
                    "CALL_UPDATE",
                    json!({"channel_id": channel.to_string(), "message_id": "1580000000000000001",
                        "region": "us-east", "ongoing_rings": map}),
                );
                Some(Value::Null)
            }
            _ => Some(json!({})),
        }
    }

    // ----- participants -----

    fn peer_add(this: &Arc<Self>, name: &str, same_account: bool) -> String {
        if this.lock().peers.contains_key(name) {
            return format!("{name} is in the call already");
        }
        let (user, index) = {
            let mut world = this.lock();
            let index = world.next_person;
            world.next_person += 1;
            let user = if same_account {
                world.setup.me
            } else {
                world
                    .setup
                    .people
                    .get(index)
                    .copied()
                    .unwrap_or(1_580_000_000_000_000_100 + index as u64)
            };
            (user, index)
        };
        let session = format!("{:032x}", rand::random::<u128>());
        let plan = dc_voice::sim::Plan {
            tone_hz: Some(VOICE_TONES[index % VOICE_TONES.len()]),
            level: 0.2,
            listen_hz: VOICE_TONES
                .iter()
                .chain(STREAM_TONES.iter())
                .copied()
                .collect(),
            share: None,
            share_tone_hz: Some(STREAM_TONES[index % STREAM_TONES.len()]),
        };
        dc_voice::sim::set_plan(&session, plan.clone());
        let controls = dc_voice::Controls::new();
        let board = Board::default();
        let snapshots: Arc<Mutex<HashMap<(u64, &'static str), dc_media::Picture>>> = Arc::default();
        let sink_board = board.clone();
        let sink_snapshots = snapshots.clone();
        controls.set_video_sink(Some(Arc::new(move |frame: dc_voice::RemoteFrame| {
            let kind = kind(frame.source);
            sink_board.record(frame.user, kind, frame.picture.width, frame.picture.height);
            let mut snapshots = sink_snapshots.lock().unwrap_or_else(|e| e.into_inner());
            snapshots.insert((frame.user, kind), frame.picture);
        })));
        let peer = Peer {
            user,
            session: session.clone(),
            controls,
            board,
            events: Arc::default(),
            connected: Arc::default(),
            own: None,
            watching: HashMap::new(),
            snapshots,
            plan,
        };
        let channel = this.place().1;
        this.lock().peers.insert(name.to_owned(), peer);
        this.note(format!(
            "{name} joins{}",
            if same_account {
                " as this account on another device"
            } else {
                ""
            }
        ));
        this.set_state(
            Duration::from_millis(20),
            VoiceStateRecord {
                user,
                session: session.clone(),
                channel: Some(channel),
                self_mute: false,
                self_deaf: false,
                self_video: false,
                self_stream: false,
            },
        );
        this.call_create_if_needed(Duration::from_millis(20));
        if same_account {
            // The voice server lets the old device's connection go (an assumption: the gateway's state alone
            // already takes the call away from it).
            let server = this.call_server();
            let weak = Arc::downgrade(this);
            this.handle.spawn(async move {
                tokio::time::sleep(Duration::from_millis(300)).await;
                let closed = server.close("me", false, 4014);
                if let Some(inner) = weak.upgrade() {
                    inner.note(format!(
                        "old device's call connection closed with 4014 ({closed})"
                    ));
                }
            });
        }
        Self::peer_connect(this, name);
        format!("{name} joins the call")
    }

    /// Opens `name`'s call connection to the current call server.
    fn peer_connect(this: &Arc<Self>, name: &str) {
        let (server_id, channel) = {
            let world = this.lock();
            let setup = &world.setup;
            (
                if setup.guild != 0 {
                    setup.guild
                } else {
                    setup.channel
                },
                setup.channel,
            )
        };
        let Some((user, session, controls, events, connected)) =
            this.lock().peers.get(name).map(|peer| {
                (
                    peer.user,
                    peer.session.clone(),
                    peer.controls.clone(),
                    peer.events.clone(),
                    peer.connected.clone(),
                )
            })
        else {
            return;
        };
        let server = this.call_server();
        let token = server.issue(Ticket {
            user,
            session: session.clone(),
            server_id,
            channel_id: channel,
            label: name.to_owned(),
        });
        let join = dc_voice::Join {
            endpoint: server.endpoint(),
            token,
            session_id: session.clone(),
            server_id,
            channel_id: channel,
            user_id: user,
        };
        let weak: Weak<Self> = Arc::downgrade(this);
        let name = name.to_owned();
        this.handle.spawn(async move {
            let started = Instant::now();
            let (task, mut receiver) = dc_voice::connect(join, &controls);
            while let Some(event) = receiver.recv().await {
                let line = format!("[{:>6} ms] {event:?}", started.elapsed().as_millis());
                events.lock().unwrap_or_else(|e| e.into_inner()).push(line);
                match &event {
                    dc_voice::Event::Connected => {
                        connected.store(true, std::sync::atomic::Ordering::Release)
                    }
                    dc_voice::Event::Disconnected { .. } => {
                        connected.store(false, std::sync::atomic::Ordering::Release)
                    }
                    dc_voice::Event::VideoState { camera } => {
                        if let Some(inner) = weak.upgrade()
                            && let Some(mut state) = inner.state_of(user, &session)
                        {
                            state.self_video = *camera;
                            inner.set_state(Duration::from_millis(20), state);
                            inner.note(format!(
                                "{name}: camera {}",
                                if *camera { "on" } else { "off" }
                            ));
                        }
                    }
                    _ => {}
                }
            }
            let _ = task.await;
        });
    }

    fn peer_leave(&self, name: &str) -> String {
        let Some(peer) = self.lock().peers.remove(name) else {
            return format!("no participant {name}");
        };
        peer.controls.disconnect();
        for controls in peer.watching.values() {
            controls.stop();
        }
        if let Some((key, controls)) = &peer.own {
            controls.stop();
            self.end_stream(*key, "user_requested");
        }
        self.note(format!("{name} leaves"));
        if let Some(mut state) = self.state_of(peer.user, &peer.session) {
            state.channel = None;
            state.self_stream = false;
            state.self_video = false;
            self.set_state(Duration::from_millis(30), state);
        }
        format!("{name} left")
    }

    fn peer_camera(&self, name: &str, on: bool) -> String {
        let world = self.lock();
        let Some(peer) = world.peers.get(name) else {
            return format!("no participant {name}");
        };
        // The id is never opened: the simulation turns every camera into the test picture.
        peer.controls.set_camera(on.then(|| "sim".to_owned()));
        format!(
            "{name}: camera {}",
            if on { "asked on" } else { "asked off" }
        )
    }

    fn peer_stream_start(
        this: &Arc<Self>,
        name: &str,
        width: u32,
        height: u32,
        fps: u32,
        content: dc_media::sim::Content,
    ) -> String {
        let Some((user, session, mut plan, hub, events)) =
            this.lock().peers.get(name).map(|peer| {
                (
                    peer.user,
                    peer.session.clone(),
                    peer.plan.clone(),
                    peer.controls.hub(),
                    peer.events.clone(),
                )
            })
        else {
            return format!("no participant {name}");
        };
        if this
            .lock()
            .peers
            .get(name)
            .is_some_and(|peer| peer.own.is_some())
        {
            return format!("{name} is live already");
        }
        let Some(mut state) = this.state_of(user, &session) else {
            return format!("{name} is not in the call");
        };
        plan.share = Some(content);
        dc_voice::sim::set_plan(&session, plan.clone());
        let key = this.key_of(user);
        let rtc = {
            let mut world = this.lock();
            world.next_rtc += 2;
            let rtc = world.next_rtc;
            world.streams.insert(
                key,
                StreamRecord {
                    owner: user,
                    rtc,
                    viewers: Vec::new(),
                    paused: false,
                },
            );
            rtc
        };
        state.self_stream = true;
        this.set_state(Duration::from_millis(30), state);
        let token = this.stream_server.issue(Ticket {
            user,
            session: session.clone(),
            server_id: rtc,
            channel_id: rtc - 1,
            label: name.to_owned(),
        });
        // The ceiling of the pinned build: 9 Mb/s above 720p30, 3.5 Mb/s at or below (DECISIONS.md, Go Live).
        let bitrate = if height <= 720 && fps <= 30 {
            3_500_000
        } else {
            9_000_000
        };
        let join = StreamJoin {
            key,
            role: Role::Send,
            endpoint: this.stream_server.endpoint(),
            token,
            session_id: session,
            server_id: rtc,
            channel_id: rtc - 1,
            user_id: user,
        };
        let controls = dc_voice::StreamControls::new();
        controls.set_previews(false);
        if let Some(peer) = this.lock().peers.get_mut(name) {
            peer.own = Some((key, controls.clone()));
            peer.plan = plan;
        }
        let source = dc_voice::StreamSource::Synthetic {
            width,
            height,
            fps,
            bitrate,
        };
        let label = name.to_owned();
        this.handle.spawn(async move {
            let started = Instant::now();
            let (task, mut receiver) =
                dc_voice::connect_stream(join, Some(source), &hub, &controls);
            while let Some(event) = receiver.recv().await {
                let line = format!(
                    "[{:>6} ms] own stream {event:?}",
                    started.elapsed().as_millis()
                );
                events.lock().unwrap_or_else(|e| e.into_inner()).push(line);
            }
            let _ = task.await;
        });
        this.note(format!(
            "{label} goes live: {width}x{height} {fps} fps {}",
            content.name()
        ));
        format!(
            "{label} goes live {width}x{height}@{fps} {}",
            content.name()
        )
    }

    fn peer_stream_stop(&self, name: &str) -> String {
        let own = self
            .lock()
            .peers
            .get_mut(name)
            .and_then(|peer| peer.own.take());
        let Some((key, controls)) = own else {
            return format!("{name} is not live");
        };
        controls.stop();
        self.end_stream(key, "user_requested");
        format!("{name} stopped streaming")
    }

    fn owner_of(&self, owner: &str) -> Option<u64> {
        let world = self.lock();
        if owner == "me" {
            Some(world.setup.me)
        } else {
            world.peers.get(owner).map(|peer| peer.user)
        }
    }

    fn peer_watch(this: &Arc<Self>, name: &str, owner: &str) -> String {
        let Some(owner_id) = this.owner_of(owner) else {
            return format!("no participant {owner}");
        };
        let key = this.key_of(owner_id);
        let Some((user, session, hub, board, events)) = this.lock().peers.get(name).map(|peer| {
            (
                peer.user,
                peer.session.clone(),
                peer.controls.hub(),
                peer.board.clone(),
                peer.events.clone(),
            )
        }) else {
            return format!("no participant {name}");
        };
        let found = {
            let mut world = this.lock();
            world.streams.get_mut(&key).map(|stream| {
                if !stream.viewers.contains(&user) {
                    stream.viewers.push(user);
                }
                (stream.rtc, stream.viewers.clone(), stream.paused)
            })
        };
        let Some((rtc, viewers, paused)) = found else {
            return format!("{owner} is not live");
        };
        let me = this.lock().setup.me;
        if owner_id == me {
            // Our client sends this stream: it hears of the new viewer.
            this.stream_update(key, viewers, paused);
        }
        board.arm(owner_id, "stream");
        let token = this.stream_server.issue(Ticket {
            user,
            session: session.clone(),
            server_id: rtc,
            channel_id: rtc - 1,
            label: name.to_owned(),
        });
        let join = StreamJoin {
            key,
            role: Role::Watch,
            endpoint: this.stream_server.endpoint(),
            token,
            session_id: session,
            server_id: rtc,
            channel_id: rtc - 1,
            user_id: user,
        };
        let controls = dc_voice::StreamControls::new();
        if let Some(peer) = this.lock().peers.get_mut(name)
            && let Some(old) = peer.watching.insert(owner_id, controls.clone())
        {
            old.stop();
        }
        let label = format!("watch of {owner}");
        this.handle.spawn(async move {
            let started = Instant::now();
            let (task, mut receiver) = dc_voice::connect_stream(join, None, &hub, &controls);
            while let Some(event) = receiver.recv().await {
                let line = format!(
                    "[{:>6} ms] {label} {event:?}",
                    started.elapsed().as_millis()
                );
                events.lock().unwrap_or_else(|e| e.into_inner()).push(line);
            }
            let _ = task.await;
        });
        this.note(format!("{name} watches {owner}"));
        format!("{name} watches {owner}")
    }

    fn peer_unwatch(&self, name: &str, owner: &str) -> String {
        let Some(owner_id) = self.owner_of(owner) else {
            return format!("no participant {owner}");
        };
        let key = self.key_of(owner_id);
        let (controls, user) = {
            let mut world = self.lock();
            let Some(peer) = world.peers.get_mut(name) else {
                return format!("no participant {name}");
            };
            (peer.watching.remove(&owner_id), peer.user)
        };
        if let Some(controls) = controls {
            controls.stop();
        }
        let viewers = {
            let mut world = self.lock();
            world.streams.get_mut(&key).map(|stream| {
                stream.viewers.retain(|viewer| *viewer != user);
                (stream.viewers.clone(), stream.paused)
            })
        };
        if let Some((viewers, paused)) = viewers
            && owner_id == self.lock().setup.me
        {
            self.stream_update(key, viewers, paused);
        }
        format!("{name} stopped watching {owner}")
    }

    fn peer_summary(
        &self,
        name: &str,
        owner: &str,
        kind: &str,
        window: Duration,
    ) -> Option<Summary> {
        let owner = self.owner_of(owner)?;
        let kind = if kind == "camera" { "camera" } else { "stream" };
        let board = self.lock().peers.get(name)?.board.clone();
        board.summary(owner, kind, window)
    }

    fn peer_shot(&self, name: &str, owner: &str, kind: &str, path: &std::path::Path) -> String {
        let Some(owner) = self.owner_of(owner) else {
            return "no such owner".to_owned();
        };
        let kind = if kind == "camera" { "camera" } else { "stream" };
        let picture = self.lock().peers.get(name).and_then(|peer| {
            peer.snapshots
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .get(&(owner, kind))
                .cloned()
        });
        let Some(picture) = picture else {
            return format!("{name} has no picture of that {kind}");
        };
        let Some(rgba) = picture.to_rgba() else {
            return "the picture does not convert".to_owned();
        };
        match image::RgbaImage::from_raw(picture.width, picture.height, rgba)
            .map(|image| image.save(path))
        {
            Some(Ok(())) => format!(
                "saved {}x{} to {}",
                picture.width,
                picture.height,
                path.display()
            ),
            Some(Err(error)) => format!("not saved: {error}"),
            None => "not saved: bad size".to_owned(),
        }
    }

    // ----- servers -----

    fn server_move(this: &Arc<Self>) -> String {
        let fresh = match this.handle.block_on_task(VoiceServer::start()) {
            Some(Ok(server)) => server,
            _ => return "a new voice server could not start".to_owned(),
        };
        let old_server = this.call_server();
        let feedback = old_server.feedback();
        fresh.set_feedback(feedback);
        for (label, up, down) in old_server.shapes() {
            fresh.set_shape(&label, up, down);
        }
        let old = std::mem::replace(
            &mut *this.call_server.lock().unwrap_or_else(|e| e.into_inner()),
            fresh,
        );
        this.retired
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(old);
        this.note("the call moves to another voice server".to_owned());
        let me_in = {
            let world = this.lock();
            world
                .states
                .iter()
                .any(|s| s.user == world.setup.me && s.session == world.setup.my_session)
        };
        if me_in {
            this.voice_server_update(Duration::from_millis(50), this.my_ticket());
        }
        let names: Vec<String> = this.lock().peers.keys().cloned().collect();
        for name in &names {
            let old = {
                let mut world = this.lock();
                let Some(peer) = world.peers.get_mut(name) else {
                    continue;
                };
                let fresh = peer.controls.rejoin();
                peer.connected = Arc::new(AtomicBool::new(false));
                std::mem::replace(&mut peer.controls, fresh)
            };
            old.disconnect();
            Self::peer_connect(this, name);
        }
        format!(
            "moved: our client and {} participant(s) get the new server",
            names.len()
        )
    }

    fn kick(&self, after: &str) -> String {
        let closed = self.call_server().close("me", false, 4014);
        match after {
            "moved" => self.voice_server_update(Duration::from_millis(500), self.my_ticket()),
            "gone" => {
                let world = self.lock();
                let mine = world
                    .states
                    .iter()
                    .find(|s| s.user == world.setup.me && s.session == world.setup.my_session)
                    .cloned();
                drop(world);
                if let Some(mut state) = mine {
                    state.channel = None;
                    self.set_state(Duration::from_millis(100), state);
                }
            }
            _ => {}
        }
        format!("closed {closed} call connection(s) with 4014, then {after}")
    }

    fn stats(&self, window: Duration) -> String {
        let mut out = String::new();
        let line = |out: &mut String, text: String| {
            out.push_str(&text);
            out.push('\n');
        };
        let (me, names, peers): (u64, Vec<String>, Vec<PeerRow>) = {
            let world = self.lock();
            let peers = world
                .peers
                .iter()
                .map(|(name, peer)| {
                    (
                        name.clone(),
                        peer.user,
                        peer.board.clone(),
                        peer.connected.load(std::sync::atomic::Ordering::Acquire),
                        peer.session.clone(),
                    )
                })
                .collect();
            (world.setup.me, world.peers.keys().cloned().collect(), peers)
        };
        let name_of = |user: u64| -> String {
            if user == me {
                "me".to_owned()
            } else {
                peers
                    .iter()
                    .find(|(_, id, ..)| *id == user)
                    .map_or("someone".to_owned(), |(name, ..)| name.clone())
            }
        };
        line(
            &mut out,
            format!(
                "window {} s; participants {}",
                window.as_secs(),
                names.join(",")
            ),
        );
        let my_session = self.lock().setup.my_session.clone();
        for (who, board, session, connected) in
            std::iter::once(("me".to_owned(), self.my_board.clone(), my_session, true)).chain(
                peers.iter().map(|(name, _, board, connected, session)| {
                    (name.clone(), board.clone(), session.clone(), *connected)
                }),
            )
        {
            line(&mut out, format!("{who}: connected {connected}"));
            for (user, kind) in board.keys() {
                if let Some(summary) = board.summary(user, kind, window) {
                    line(
                        &mut out,
                        format!(
                            "  {who} sees {}'s {kind}: {}",
                            name_of(user),
                            summary.line()
                        ),
                    );
                    line(&mut out, format!("    per second {:?}", summary.per_second));
                }
            }
            let audio = dc_voice::sim::report(&session);
            let tones: Vec<String> = audio
                .tones
                .iter()
                .filter(|(_, level)| *level > 0.005)
                .map(|(hz, level)| format!("{hz:.0} Hz {level:.3}"))
                .collect();
            line(
                &mut out,
                format!(
                    "  {who} audio: played {} frames ({} audible), longest gap {} ms, rms {:.3}, tones [{}], mic frames {}",
                    audio.frames,
                    audio.audible,
                    audio.longest_gap_ms,
                    audio.rms,
                    tones.join(", "),
                    audio.sent
                ),
            );
        }
        for (server, label) in [
            (self.call_server(), "call"),
            (self.stream_server.clone(), "stream"),
        ] {
            line(&mut out, format!("{label} server: {}", server.describe()));
            for report in server.reports() {
                let kbps = |series: &std::collections::VecDeque<u64>| -> Vec<u64> {
                    let take = window.as_secs() as usize;
                    let skip = series.len().saturating_sub(take + 1);
                    series
                        .iter()
                        .skip(skip)
                        .take(take)
                        .map(|bytes| bytes * 8 / 1000)
                        .collect()
                };
                line(
                    &mut out,
                    format!(
                        "  {} ({}): video in kb/s {:?} out {:?}; plis from {} to {}; nacks from {} to {}; twcc to {} from {}; rr to {}; remb to {} last {} kb/s; up {:?} down {:?}",
                        report.label,
                        if report.stream { "stream" } else { "call" },
                        kbps(&report.series.video_in),
                        kbps(&report.series.video_out),
                        report.plis_from,
                        report.plis_to,
                        report.nacks_from,
                        report.nacks_to,
                        report.transport_feedback_to,
                        report.transport_feedback_from,
                        report.receiver_reports_to,
                        report.rembs_to,
                        report.last_remb_bps / 1000,
                        report.up,
                        report.down
                    ),
                );
            }
        }
        let counts = dc_voice::sim::counts();
        line(
            &mut out,
            format!(
                "process: voice connections {} stream connections {} audio workers {}; endpoints refused {}",
                counts.calls,
                counts.streams,
                counts.audio_workers,
                dc_voice::sim::refused_endpoints()
            ),
        );
        out
    }
}

/// A participant as the stats see it: name, user, board, connected, session.
type PeerRow = (String, u64, Board, bool, String);

/// Running a future to completion from outside the runtime, or from one of its threads.
trait BlockOn {
    fn block_on_task<F: std::future::Future + Send + 'static>(
        &self,
        future: F,
    ) -> Option<F::Output>
    where
        F::Output: Send + 'static;
}

impl BlockOn for Handle {
    fn block_on_task<F: std::future::Future + Send + 'static>(&self, future: F) -> Option<F::Output>
    where
        F::Output: Send + 'static,
    {
        let (send, receive) = std::sync::mpsc::channel();
        self.spawn(async move {
            let _ = send.send(future.await);
        });
        receive.recv_timeout(Duration::from_secs(10)).ok()
    }
}
