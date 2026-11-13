//! One simulated voice server: the control socket (websocket, voice gateway version 8) and the media socket (UDP),
//! acting as Discord's SFU for every room on it.
//!
//! The control socket answers what `crates/dc-voice` sends, in the order it expects: HELLO on connect, READY to
//! identify (op 0 -> op 2), the session description to select protocol (op 1 -> op 4, with DAVE protocol version 1),
//! heartbeat acks, op 16 versions, and the room's announcements (op 11 client connect, op 12 sources, op 5 speaking,
//! op 13 client disconnect). DAVE runs through `mls::Group` (ops 22 to 31). Op numbers and fields are those of
//! `design/snapshot/dumps/VOICE.md` and the client's parsers (`dc-voice` `signal.rs`).
//!
//! The media socket answers IP discovery and forwards every participant's RTP to the others in its room, re-sealed with
//! each receiver's key and numbered on each receiver's own transport-wide sequence, video only to those whose op 15
//! asks for video. It keeps the SSRCs it assigned in READY (they are unique per server, so nothing needs rewriting).
//! Keyframe requests and NACKs from a receiver are relayed to the sender of the source they name; sender reports go to
//! the receivers. What the server itself tells a sender is selectable, because Discord's choice is not known from the
//! client: transport-wide feedback (every 100 ms, built by the receiving end `dc-voice` runs), receiver reports (every
//! second), REMB (every second; see [`Feedback`]).
use crate::link::{Delivery, Direction, Network, Shape};
use crate::mls::{self, Group};
use crate::wire::{self, Crypto, Mode, Opened, Rtcp, SourceStats};
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use std::collections::{HashMap, VecDeque};
use std::net::{SocketAddr, UdpSocket};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::net::TcpListener;
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::{
    Message,
    protocol::{CloseFrame, frame::coding::CloseCode},
};

/// What the server sends a media sender about its stream.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Feedback {
    pub transport_cc: bool,
    pub receiver_reports: bool,
    /// REMB with the server's estimate: the smaller of 90% of the sender's uplink capacity and of each video
    /// receiver's downlink capacity where a capacity is set, else one and a half times what arrives plus 500 kb/s.
    pub remb: bool,
}

impl Default for Feedback {
    fn default() -> Self {
        Self {
            transport_cc: true,
            receiver_reports: true,
            remb: false,
        }
    }
}

/// Who a token admits: the voice state it was issued for.
#[derive(Clone, Debug)]
pub struct Ticket {
    pub user: u64,
    pub session: String,
    pub server_id: u64,
    pub channel_id: u64,
    /// The participant's name in the simulator (its link shape follows it).
    pub label: String,
}

/// Seconds of per-second counters kept.
const SERIES: usize = 300;

/// Per-second totals of one connection, oldest first; the last entry is the second in progress.
#[derive(Clone, Debug, Default)]
pub struct Series {
    pub video_in: VecDeque<u64>,
    pub audio_in: VecDeque<u64>,
    pub video_out: VecDeque<u64>,
    pub audio_out: VecDeque<u64>,
}

impl Series {
    fn roll(&mut self) {
        for series in [
            &mut self.video_in,
            &mut self.audio_in,
            &mut self.video_out,
            &mut self.audio_out,
        ] {
            series.push_back(0);
            if series.len() > SERIES {
                series.pop_front();
            }
        }
    }
    fn add(series: &mut VecDeque<u64>, bytes: usize) {
        if series.is_empty() {
            series.push_back(0);
        }
        if let Some(last) = series.back_mut() {
            *last += bytes as u64;
        }
    }
}

/// What the server saw of one connection.
#[derive(Clone, Debug, Default)]
pub struct ConnReport {
    pub label: String,
    pub user: u64,
    pub stream: bool,
    pub audio_ssrc: u32,
    pub video: bool,
    pub wants_video: bool,
    pub series: Series,
    /// Keyframe requests and NACKs this connection sent, and those relayed to it (about its own sources).
    pub plis_from: u64,
    pub nacks_from: u64,
    pub plis_to: u64,
    pub nacks_to: u64,
    pub transport_feedback_from: u64,
    pub transport_feedback_to: u64,
    pub receiver_reports_to: u64,
    pub rembs_to: u64,
    pub last_remb_bps: u64,
    /// Datagrams of its uplink and downlink: sent into the path, lost, dropped at the queue.
    pub up: (u64, u64, u64),
    pub down: (u64, u64, u64),
    pub connected_for_ms: u64,
}

struct Conn {
    label: String,
    user: u64,
    room: (u64, u64),
    stream: bool,
    ws: mpsc::UnboundedSender<Message>,
    seq: i64,
    audio_ssrc: u32,
    video_ssrc: u32,
    rtx_ssrc: u32,
    key: [u8; 32],
    crypto: Option<Crypto>,
    addr: Option<SocketAddr>,
    described: bool,
    video: Option<Value>,
    wants_video: bool,
    up: Direction,
    down: Direction,
    twcc: dc_voice::sim::TransportFeedback,
    out_sequence: u16,
    sources: HashMap<u32, SourceStats>,
    next_report: Instant,
    next_remb: Instant,
    report: ConnReport,
    since: Instant,
}

impl Conn {
    fn send(&mut self, op: u8, d: Value) {
        self.seq += 1;
        let text = json!({"op": op, "seq": self.seq, "d": d}).to_string();
        let _ = self.ws.send(Message::Text(text.into()));
    }
    fn send_binary(&mut self, op: u8, payload: &[u8]) {
        self.seq += 1;
        let mut frame = (self.seq as u16).to_be_bytes().to_vec();
        frame.push(op);
        frame.extend_from_slice(payload);
        let _ = self.ws.send(Message::Binary(frame.into()));
    }
    fn owns(&self, ssrc: u32) -> bool {
        ssrc != 0 && (ssrc == self.audio_ssrc || ssrc == self.video_ssrc || ssrc == self.rtx_ssrc)
    }
}

struct State {
    tickets: HashMap<String, Ticket>,
    conns: HashMap<u64, Conn>,
    by_addr: HashMap<SocketAddr, u64>,
    groups: mls::Groups,
    rooms: HashMap<(u64, u64), Vec<u64>>,
    next_conn: u64,
    next_ssrc: u32,
    feedback: Feedback,
    keyframe_interval_ms: Option<u64>,
    shapes: HashMap<String, (Shape, Shape)>,
    log: Vec<String>,
    started: Instant,
    /// Datagrams that matched no connection or did not open.
    strays: u64,
    udp_port: u16,
}

impl State {
    fn note(&mut self, line: String) {
        let at = self.started.elapsed().as_millis();
        self.log.push(format!("[{at:>7} ms] {line}"));
        if self.log.len() > 2000 {
            self.log.drain(..500);
        }
    }

    fn room_of(&self, conn: u64) -> Vec<u64> {
        self.conns
            .get(&conn)
            .and_then(|c| self.rooms.get(&c.room))
            .cloned()
            .unwrap_or_default()
    }

    fn apply_mls(&mut self, sends: Vec<mls::Send>) {
        for send in sends {
            match send {
                mls::Send::Binary(to, op, payload) => {
                    if let Some(conn) = self.conns.get_mut(&to) {
                        conn.send_binary(op, &payload);
                    }
                }
                mls::Send::Text(to, op, d) => {
                    if let Some(conn) = self.conns.get_mut(&to) {
                        conn.send(op, d);
                    }
                }
            }
        }
    }
}

struct Shared {
    state: Mutex<State>,
    network: Network,
}

impl Shared {
    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Hands a datagram to `conn`'s downlink.
    fn send_to(&self, conn: &mut Conn, bytes: Vec<u8>) {
        let Some(addr) = conn.addr else { return };
        if let Some(at) = conn.down.admit(bytes.len(), Instant::now()) {
            self.network.schedule(at, Delivery::Out(addr), bytes);
        }
    }
}

/// A running voice server. Dropping it stops accepting; connections end with the runtime.
pub struct VoiceServer {
    pub ws_port: u16,
    pub udp_port: u16,
    shared: Arc<Shared>,
}

impl VoiceServer {
    /// Binds both sockets on 127.0.0.1 and starts serving on the current tokio runtime.
    pub async fn start() -> std::io::Result<Arc<Self>> {
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let ws_port = listener.local_addr()?.port();
        let socket = UdpSocket::bind("127.0.0.1:0")?;
        let udp_port = socket.local_addr()?.port();
        // Windows reports an ICMP "port unreachable" on the next receive as an error; it is ignored below.
        let receive = socket.try_clone()?;
        let shared_slot: Arc<Mutex<Option<std::sync::Weak<Shared>>>> = Arc::new(Mutex::new(None));
        let slot = shared_slot.clone();
        let ingress: Arc<dyn Fn(SocketAddr, Vec<u8>) + Send + Sync> =
            Arc::new(move |from, bytes| {
                let shared = slot.lock().unwrap_or_else(|e| e.into_inner()).clone();
                if let Some(shared) = shared.and_then(|weak| weak.upgrade()) {
                    media(&shared, from, &bytes);
                }
            });
        let network = Network::start(socket, ingress)?;
        let shared = Arc::new(Shared {
            state: Mutex::new(State {
                tickets: HashMap::new(),
                conns: HashMap::new(),
                by_addr: HashMap::new(),
                groups: mls::Groups::default(),
                rooms: HashMap::new(),
                next_conn: 1,
                next_ssrc: 0x0001_0000 + (rand::random::<u32>() % 0x0100_0000),
                feedback: Feedback::default(),
                keyframe_interval_ms: None,
                shapes: HashMap::new(),
                log: Vec::new(),
                started: Instant::now(),
                strays: 0,
                udp_port,
            }),
            network,
        });
        *shared_slot.lock().unwrap_or_else(|e| e.into_inner()) = Some(Arc::downgrade(&shared));

        // Media in: straight to the server, or through the sender's uplink when it has a shape.
        let weak = Arc::downgrade(&shared);
        std::thread::Builder::new()
            .name("dc-callsim-udp".into())
            .spawn(move || {
                let mut buffer = vec![0u8; 8192];
                loop {
                    let Some(shared) = weak.upgrade() else { return };
                    let _ = receive.set_read_timeout(Some(Duration::from_millis(200)));
                    match receive.recv_from(&mut buffer) {
                        Ok((length, from)) => {
                            let bytes = &buffer[..length];
                            let shaped = {
                                let mut state = shared.lock();
                                let id = state.by_addr.get(&from).copied();
                                match id.and_then(|id| state.conns.get_mut(&id)) {
                                    Some(conn) if conn.up.shape != Shape::default() => {
                                        Some(conn.up.admit(bytes.len(), Instant::now()))
                                    }
                                    _ => None,
                                }
                            };
                            match shaped {
                                Some(Some(at)) => {
                                    shared
                                        .network
                                        .schedule(at, Delivery::In(from), bytes.to_vec())
                                }
                                Some(None) => {}
                                None => media(&shared, from, bytes),
                            }
                        }
                        Err(error)
                            if error.kind() == std::io::ErrorKind::WouldBlock
                                || error.kind() == std::io::ErrorKind::TimedOut
                                || error.kind() == std::io::ErrorKind::ConnectionReset => {}
                        Err(_) => std::thread::sleep(Duration::from_millis(5)),
                    }
                }
            })?;

        // The clock of the server's own feedback and of DAVE transitions that waited too long.
        let ticker = Arc::downgrade(&shared);
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_millis(20));
            let mut second = Instant::now();
            loop {
                interval.tick().await;
                let Some(shared) = ticker.upgrade() else {
                    return;
                };
                tick(&shared, &mut second);
            }
        });

        let accept = Arc::downgrade(&shared);
        tokio::spawn(async move {
            while let Ok((stream, _)) = listener.accept().await {
                let Some(shared) = accept.upgrade() else {
                    return;
                };
                tokio::spawn(async move {
                    let _ = stream.set_nodelay(true);
                    if let Ok(ws) = tokio_tungstenite::accept_async(stream).await {
                        connection(shared, ws).await;
                    }
                });
            }
        });
        Ok(Arc::new(Self {
            ws_port,
            udp_port,
            shared,
        }))
    }

    /// The endpoint string of `VOICE_SERVER_UPDATE` / `STREAM_SERVER_UPDATE`.
    pub fn endpoint(&self) -> String {
        format!("127.0.0.1:{}", self.ws_port)
    }

    /// A fresh token for this voice state.
    pub fn issue(&self, ticket: Ticket) -> String {
        let token = format!("{:032x}", rand::random::<u128>());
        let mut state = self.shared.lock();
        state.note(format!(
            "ticket for {} in room {}:{}",
            ticket.label, ticket.server_id, ticket.channel_id
        ));
        state.tickets.insert(token.clone(), ticket);
        token
    }

    pub fn set_feedback(&self, feedback: Feedback) {
        let mut state = self.shared.lock();
        state.feedback = feedback;
        state.note(format!("feedback {feedback:?}"));
    }

    pub fn feedback(&self) -> Feedback {
        self.shared.lock().feedback
    }

    pub fn set_keyframe_interval(&self, ms: Option<u64>) {
        self.shared.lock().keyframe_interval_ms = ms;
    }

    /// The path of every connection labelled `label`: `up` toward the server, `down` from it.
    pub fn set_shape(&self, label: &str, up: Shape, down: Shape) {
        let mut state = self.shared.lock();
        state.shapes.insert(label.to_owned(), (up, down));
        for conn in state.conns.values_mut().filter(|conn| conn.label == label) {
            conn.up.shape = up;
            conn.down.shape = down;
        }
    }

    pub fn shapes(&self) -> Vec<(String, Shape, Shape)> {
        self.shared
            .lock()
            .shapes
            .iter()
            .map(|(label, (up, down))| (label.clone(), *up, *down))
            .collect()
    }

    /// Closes the control sockets of `label`'s connections (`stream` picks the call or the stream ones) with `code`.
    pub fn close(&self, label: &str, stream: bool, code: u16) -> usize {
        let mut state = self.shared.lock();
        let ids: Vec<u64> = state
            .conns
            .iter()
            .filter(|(_, conn)| conn.label == label && conn.stream == stream)
            .map(|(id, _)| *id)
            .collect();
        for id in &ids {
            if let Some(conn) = state.conns.get(id) {
                let _ = conn.ws.send(Message::Close(Some(CloseFrame {
                    code: CloseCode::from(code),
                    reason: "".into(),
                })));
            }
        }
        state.note(format!(
            "closed {} connection(s) of {label} with {code}",
            ids.len()
        ));
        ids.len()
    }

    /// Every connection that is up, by label.
    pub fn reports(&self) -> Vec<ConnReport> {
        let state = self.shared.lock();
        let mut out: Vec<ConnReport> = state
            .conns
            .values()
            .map(|conn| {
                let mut report = conn.report.clone();
                report.up = (conn.up.sent, conn.up.lost, conn.up.dropped);
                report.down = (conn.down.sent, conn.down.lost, conn.down.dropped);
                report.connected_for_ms = conn.since.elapsed().as_millis() as u64;
                report.wants_video = conn.wants_video;
                report.video = conn.video.is_some();
                report
            })
            .collect();
        out.sort_by(|a, b| (a.stream, &a.label).cmp(&(b.stream, &b.label)));
        out
    }

    /// Connections now, and what the DAVE groups say.
    pub fn describe(&self) -> String {
        let state = self.shared.lock();
        let mut rooms: Vec<String> = state
            .rooms
            .iter()
            .filter(|(_, members)| !members.is_empty())
            .map(|(room, members)| {
                let labels: Vec<String> = members
                    .iter()
                    .filter_map(|id| state.conns.get(id).map(|conn| conn.label.clone()))
                    .collect();
                format!(
                    "room {}:{} [{}] dave {}",
                    room.0,
                    room.1,
                    labels.join(","),
                    state
                        .groups
                        .0
                        .get(room)
                        .map_or("-".to_owned(), Group::describe)
                )
            })
            .collect();
        rooms.sort();
        format!(
            "connections {} strays {} {}",
            state.conns.len(),
            state.strays,
            rooms.join("; ")
        )
    }

    pub fn connection_count(&self) -> usize {
        self.shared.lock().conns.len()
    }

    /// The server's own log, newest last.
    pub fn log(&self) -> Vec<String> {
        self.shared.lock().log.clone()
    }
}

fn op_of(text: &str) -> Option<(u64, Value)> {
    let mut value: Value = serde_json::from_str(text).ok()?;
    Some((value["op"].as_u64()?, value["d"].take()))
}

async fn connection(
    shared: Arc<Shared>,
    ws: tokio_tungstenite::WebSocketStream<tokio::net::TcpStream>,
) {
    let (mut sink, mut stream) = ws.split();
    let (out, mut outgoing) = mpsc::unbounded_channel::<Message>();
    let writer = tokio::spawn(async move {
        while let Some(message) = outgoing.recv().await {
            let closing = matches!(message, Message::Close(_));
            if sink.send(message).await.is_err() || closing {
                break;
            }
        }
        let _ = sink.close().await;
    });
    // HELLO first, as the voice gateway does (version 8: binary frames carry a sequence number).
    let _ = out.send(Message::Text(
        json!({"op": 8, "d": {"v": 8, "heartbeat_interval": 13750.0}})
            .to_string()
            .into(),
    ));
    let mut id: Option<u64> = None;
    while let Some(Ok(message)) = stream.next().await {
        match message {
            Message::Text(text) => {
                let Some((op, d)) = op_of(&text) else {
                    continue;
                };
                if let Some(close) = text_op(&shared, &mut id, &out, op, d) {
                    let _ = out.send(Message::Close(Some(CloseFrame {
                        code: CloseCode::from(close),
                        reason: "".into(),
                    })));
                    break;
                }
            }
            Message::Binary(bytes) => {
                if let (Some(conn), Some((&op, payload))) = (id, bytes.split_first()) {
                    binary_op(&shared, conn, op, payload);
                }
            }
            Message::Close(_) => break,
            _ => {}
        }
    }
    if let Some(conn) = id {
        gone(&shared, conn);
    }
    drop(out);
    let _ = writer.await;
}

/// One JSON op; `Some(code)` closes the socket with that code.
fn text_op(
    shared: &Shared,
    id: &mut Option<u64>,
    out: &mpsc::UnboundedSender<Message>,
    op: u64,
    d: Value,
) -> Option<u16> {
    let mut state = shared.lock();
    match op {
        // Identify.
        0 => {
            let token = d["token"].as_str().unwrap_or_default();
            let Some(ticket) = state.tickets.get(token).cloned() else {
                state.note("identify with an unknown token: 4004".to_owned());
                return Some(4004);
            };
            let parse = |key: &str| d[key].as_str().and_then(|v| v.parse::<u64>().ok());
            if parse("user_id") != Some(ticket.user)
                || parse("server_id") != Some(ticket.server_id)
                || parse("channel_id") != Some(ticket.channel_id)
                || d["session_id"].as_str() != Some(ticket.session.as_str())
            {
                state.note(format!(
                    "identify of {} does not match its ticket: 4004",
                    ticket.label
                ));
                return Some(4004);
            }
            let stream = d["streams"][0]["type"] == "screen";
            let conn_id = state.next_conn;
            state.next_conn += 1;
            let ssrc = state.next_ssrc;
            state.next_ssrc = state.next_ssrc.wrapping_add(3);
            let room = (ticket.server_id, ticket.channel_id);
            let (up, down) = state.shapes.get(&ticket.label).copied().unwrap_or_default();
            let mut up_direction = Direction::new(ssrc ^ 0x5bd1_e995);
            up_direction.shape = up;
            let mut down_direction = Direction::new(ssrc ^ 0x1b87_3593);
            down_direction.shape = down;
            let now = Instant::now();
            let mut conn = Conn {
                label: ticket.label.clone(),
                user: ticket.user,
                room,
                stream,
                ws: out.clone(),
                seq: 0,
                audio_ssrc: ssrc,
                video_ssrc: ssrc + 1,
                rtx_ssrc: ssrc + 2,
                key: rand::random(),
                crypto: None,
                addr: None,
                described: false,
                video: None,
                wants_video: false,
                up: up_direction,
                down: down_direction,
                twcc: dc_voice::sim::TransportFeedback::new(now),
                out_sequence: rand::random(),
                sources: HashMap::new(),
                next_report: now + Duration::from_secs(1),
                next_remb: now + Duration::from_secs(1),
                report: ConnReport {
                    label: ticket.label.clone(),
                    user: ticket.user,
                    stream,
                    audio_ssrc: ssrc,
                    ..ConnReport::default()
                },
                since: now,
            };
            let kind = if stream { "screen" } else { "video" };
            let port = state.udp_port;
            conn.send(
                2,
                json!({
                    "ssrc": ssrc,
                    "ip": "127.0.0.1",
                    "port": port,
                    "modes": ["aead_aes256_gcm_rtpsize", "aead_xchacha20_poly1305_rtpsize"],
                    "experiments": [],
                    "streams": [{"type": kind, "rid": "100", "ssrc": ssrc + 1, "rtx_ssrc": ssrc + 2,
                        "active": false, "quality": 100}],
                }),
            );
            state.note(format!(
                "identify {} ({}) in room {}:{}: ssrc {ssrc}",
                ticket.label,
                if stream { "stream" } else { "call" },
                room.0,
                room.1
            ));
            state.conns.insert(conn_id, conn);
            state
                .groups
                .0
                .entry(room)
                .or_insert_with(|| Group::new(room.1));
            *id = Some(conn_id);
            None
        }
        // Select protocol.
        1 => {
            let conn_id = (*id)?;
            let mode = d["data"]["mode"].as_str().and_then(Mode::parse);
            let Some(mode) = mode else {
                state.note("select protocol with an unknown mode: 4016".to_owned());
                return Some(4016);
            };
            let interval = state.keyframe_interval_ms;
            let room = {
                let conn = state.conns.get_mut(&conn_id)?;
                conn.crypto = Some(Crypto::new(mode, &conn.key));
                let mut description = json!({
                    "mode": mode.name(),
                    "secret_key": conn.key.to_vec(),
                    "audio_codec": "opus",
                    "video_codec": "H264",
                    "media_session_id": format!("{:032x}", rand::random::<u128>()),
                    "dave_protocol_version": 1,
                });
                if let Some(ms) = interval {
                    description["keyframe_interval"] = json!(ms);
                }
                conn.send(4, description);
                conn.room
            };
            let external = state.groups.0.get(&room).map(Group::external_sender);
            let others: Vec<u64> = state.rooms.get(&room).cloned().unwrap_or_default();
            let newcomer = {
                let conn = state.conns.get_mut(&conn_id)?;
                if let Some(external) = external {
                    conn.send_binary(25, &external);
                }
                conn.described = true;
                (conn.user, conn.audio_ssrc)
            };
            let other_users: Vec<(u64, u32, Option<Value>)> = others
                .iter()
                .filter_map(|other| state.conns.get(other))
                .filter(|other| other.described)
                .map(|other| (other.user, other.audio_ssrc, other.video.clone()))
                .collect();
            if let Some(conn) = state.conns.get_mut(&conn_id) {
                if !other_users.is_empty() {
                    let ids: Vec<String> = other_users
                        .iter()
                        .map(|(user, ..)| user.to_string())
                        .collect();
                    conn.send(11, json!({"user_ids": ids}));
                }
                for (user, audio, video) in &other_users {
                    conn.send(12, announcement(*user, *audio, video.as_ref()));
                }
            }
            for other in &others {
                if let Some(conn) = state.conns.get_mut(other).filter(|c| c.described) {
                    conn.send(11, json!({"user_ids": [newcomer.0.to_string()]}));
                    conn.send(12, announcement(newcomer.0, newcomer.1, None));
                }
            }
            state.rooms.entry(room).or_default().push(conn_id);
            None
        }
        // Heartbeat.
        3 => {
            let nonce = if d.is_object() { d["t"].clone() } else { d };
            let conn = state.conns.get_mut(&(*id)?)?;
            conn.send(6, json!({"t": nonce}));
            None
        }
        // Speaking.
        5 => {
            let conn_id = (*id)?;
            let (user, ssrc) = state.conns.get(&conn_id).map(|c| (c.user, c.audio_ssrc))?;
            let speaking = d["speaking"].as_u64().unwrap_or(0);
            for other in state.room_of(conn_id) {
                if other != conn_id
                    && let Some(conn) = state.conns.get_mut(&other)
                {
                    conn.send(
                        5,
                        json!({"user_id": user.to_string(), "ssrc": ssrc, "speaking": speaking}),
                    );
                }
            }
            None
        }
        // Resume: this server never loses a session it did not close, so a resume finds none.
        7 => {
            state.note("resume asked: no session to resume, 4006".to_owned());
            Some(4006)
        }
        // A source on or off.
        12 => {
            let conn_id = (*id)?;
            let video_ssrc = d["video_ssrc"].as_u64().unwrap_or(0);
            let (user, label) = {
                let conn = state.conns.get_mut(&conn_id)?;
                conn.video = (video_ssrc != 0).then(|| d.clone());
                (conn.user, conn.label.clone())
            };
            state.note(format!(
                "{label}: video {}",
                if video_ssrc != 0 { "announced" } else { "off" }
            ));
            let mut forwarded = d;
            forwarded["user_id"] = json!(user.to_string());
            for other in state.room_of(conn_id) {
                if other != conn_id
                    && let Some(conn) = state.conns.get_mut(&other)
                {
                    conn.send(12, forwarded.clone());
                }
            }
            None
        }
        // Media sink wants.
        15 => {
            let conn_id = (*id)?;
            let wants = d["any"].as_u64().unwrap_or(0) > 0;
            let conn = state.conns.get_mut(&conn_id)?;
            conn.wants_video = wants;
            None
        }
        16 => {
            let conn = state.conns.get_mut(&(*id)?)?;
            conn.send(16, json!({"voice": "callsim", "rtc_worker": "callsim"}));
            None
        }
        // DAVE transition ready, invalid commit or welcome.
        23 | 31 => {
            let conn_id = (*id)?;
            let room = state.conns.get(&conn_id)?.room;
            let transition = d["transition_id"].as_u64().unwrap_or(0) as u16;
            let sends = {
                let group = state.groups.0.get_mut(&room)?;
                if op == 23 {
                    group.ready(conn_id, transition)
                } else {
                    group.invalid(conn_id)
                }
            };
            if op == 31 {
                state.note(format!(
                    "invalid commit or welcome from connection {conn_id}"
                ));
            }
            state.apply_mls(sends);
            None
        }
        other => {
            state.note(format!("op {other} ignored"));
            None
        }
    }
}

/// Op 12 as the room hears it: a participant's audio SSRC and, while it sends one, its video source.
fn announcement(user: u64, audio_ssrc: u32, video: Option<&Value>) -> Value {
    match video {
        Some(video) => {
            let mut d = video.clone();
            d["user_id"] = json!(user.to_string());
            d
        }
        None => json!({"user_id": user.to_string(), "audio_ssrc": audio_ssrc, "video_ssrc": 0,
            "rtx_ssrc": 0, "streams": []}),
    }
}

fn binary_op(shared: &Shared, conn_id: u64, op: u8, payload: &[u8]) {
    let mut state = shared.lock();
    let Some(room) = state.conns.get(&conn_id).map(|conn| conn.room) else {
        return;
    };
    let result = {
        let Some(group) = state.groups.0.get_mut(&room) else {
            return;
        };
        match op {
            26 => group.key_package(conn_id, payload),
            28 => group.commit(conn_id, payload),
            _ => Err("unsupported DAVE op from a client"),
        }
    };
    match result {
        Ok(sends) => state.apply_mls(sends),
        Err(error) => state.note(format!("DAVE op {op} from connection {conn_id}: {error}")),
    }
}

/// A connection's socket closed.
fn gone(shared: &Shared, conn_id: u64) {
    let mut state = shared.lock();
    let Some(conn) = state.conns.remove(&conn_id) else {
        return;
    };
    if let Some(addr) = conn.addr {
        state.by_addr.remove(&addr);
    }
    let room = conn.room;
    let members = state.rooms.entry(room).or_default();
    members.retain(|id| *id != conn_id);
    let remaining = members.clone();
    state.note(format!("{} left room {}:{}", conn.label, room.0, room.1));
    for other in &remaining {
        if let Some(other) = state.conns.get_mut(other) {
            other.send(13, json!({"user_id": conn.user.to_string()}));
        }
    }
    let sends = state
        .groups
        .0
        .get_mut(&room)
        .map(|group| group.left(conn_id, remaining.len()))
        .unwrap_or_default();
    state.apply_mls(sends);
}

/// One datagram that reached the server (after its uplink).
fn media(shared: &Shared, from: SocketAddr, bytes: &[u8]) {
    let mut state = shared.lock();
    if let Some(ssrc) = wire::discovery_ssrc(bytes) {
        let found = state
            .conns
            .iter()
            .find(|(_, conn)| conn.audio_ssrc == ssrc)
            .map(|(id, _)| *id);
        if let Some(id) = found {
            if let Some(old) = state.conns.get(&id).and_then(|conn| conn.addr) {
                state.by_addr.remove(&old);
            }
            state.by_addr.insert(from, id);
            if let Some(conn) = state.conns.get_mut(&id) {
                conn.addr = Some(from);
                // The reply goes at once: discovery is not media.
                shared.network.schedule(
                    Instant::now(),
                    Delivery::Out(from),
                    wire::discovery_reply(ssrc, from).to_vec(),
                );
            }
        } else {
            state.strays += 1;
        }
        return;
    }
    if wire::is_keepalive(bytes) {
        return;
    }
    let Some(sender_id) = state.by_addr.get(&from).copied() else {
        state.strays += 1;
        return;
    };
    let now = Instant::now();
    if wire::is_rtcp(bytes) {
        rtcp(shared, &mut state, sender_id, bytes, now);
        return;
    }
    let opened = state
        .conns
        .get(&sender_id)
        .and_then(|conn| conn.crypto.as_ref())
        .and_then(|crypto| crypto.open_rtp(bytes));
    let Some(opened) = opened else {
        state.strays += 1;
        return;
    };
    let video = opened.payload_type != wire::OPUS;
    {
        let Some(sender) = state.conns.get_mut(&sender_id) else {
            return;
        };
        if !sender.owns(opened.ssrc) {
            return;
        }
        if let Some(sequence) = opened.transport_sequence {
            sender.twcc.receive(opened.ssrc, sequence, now);
        }
        let clock = if video { 90_000 } else { 48_000 };
        if opened.payload_type != wire::H264_RTX {
            sender
                .sources
                .entry(opened.ssrc)
                .or_insert_with(|| SourceStats::new(clock))
                .observe(opened.sequence, opened.timestamp, now);
        }
        let series = if video {
            &mut sender.report.series.video_in
        } else {
            &mut sender.report.series.audio_in
        };
        Series::add(series, bytes.len());
    }
    forward(shared, &mut state, sender_id, &opened, video);
}

fn forward(shared: &Shared, state: &mut State, sender_id: u64, opened: &Opened, video: bool) {
    for receiver_id in state.room_of(sender_id) {
        if receiver_id == sender_id {
            continue;
        }
        let Some(receiver) = state.conns.get_mut(&receiver_id) else {
            continue;
        };
        if !receiver.described || (video && !receiver.wants_video) {
            continue;
        }
        receiver.out_sequence = receiver.out_sequence.wrapping_add(1);
        let sequence = receiver.out_sequence;
        let Some(sealed) = receiver
            .crypto
            .as_mut()
            .and_then(|crypto| crypto.seal_rtp(opened, Some(sequence)))
        else {
            continue;
        };
        let series = if video {
            &mut receiver.report.series.video_out
        } else {
            &mut receiver.report.series.audio_out
        };
        Series::add(series, sealed.len());
        shared.send_to(receiver, sealed);
    }
}

fn rtcp(shared: &Shared, state: &mut State, from_id: u64, bytes: &[u8], now: Instant) {
    let compound = state
        .conns
        .get(&from_id)
        .and_then(|conn| conn.crypto.as_ref())
        .and_then(|crypto| crypto.open_rtcp(bytes));
    let Some(compound) = compound else {
        state.strays += 1;
        return;
    };
    let Some(packets) = wire::parse_compound(&compound) else {
        return;
    };
    let room = state.room_of(from_id);
    for packet in packets {
        match packet {
            Rtcp::SenderReport {
                ssrc,
                ntp_middle,
                raw,
            } => {
                if let Some(conn) = state.conns.get_mut(&from_id)
                    && let Some(source) = conn.sources.get_mut(&ssrc)
                {
                    source.last_sender_report = Some((ntp_middle, now));
                }
                // Receivers echo sender reports in their own reports.
                for receiver in &room {
                    if *receiver == from_id {
                        continue;
                    }
                    if let Some(conn) = state.conns.get_mut(receiver)
                        && let Some(sealed) = conn.crypto.as_mut().and_then(|c| c.seal_rtcp(raw))
                    {
                        shared.send_to(conn, sealed);
                    }
                }
            }
            Rtcp::Pli { media, raw } | Rtcp::Nack { media, raw } => {
                let pli = raw[1] == 206;
                if let Some(conn) = state.conns.get_mut(&from_id) {
                    if pli {
                        conn.report.plis_from += 1;
                    } else {
                        conn.report.nacks_from += 1;
                    }
                }
                let owner = room
                    .iter()
                    .copied()
                    .find(|id| state.conns.get(id).is_some_and(|conn| conn.owns(media)));
                if let Some(owner) = owner
                    && let Some(conn) = state.conns.get_mut(&owner)
                    && let Some(sealed) = conn.crypto.as_mut().and_then(|c| c.seal_rtcp(raw))
                {
                    if pli {
                        conn.report.plis_to += 1;
                    } else {
                        conn.report.nacks_to += 1;
                    }
                    shared.send_to(conn, sealed);
                }
            }
            Rtcp::TransportFeedback => {
                if let Some(conn) = state.conns.get_mut(&from_id) {
                    conn.report.transport_feedback_from += 1;
                }
            }
            Rtcp::ReceiverReport | Rtcp::Remb | Rtcp::Other => {}
        }
    }
}

/// The server's own RTCP to senders, DAVE timeouts, and the per-second counters.
fn tick(shared: &Shared, second: &mut Instant) {
    let mut state = shared.lock();
    let now = Instant::now();
    let roll = now.duration_since(*second) >= Duration::from_secs(1);
    if roll {
        *second += Duration::from_secs(1);
        for conn in state.conns.values_mut() {
            conn.report.series.roll();
        }
    }
    let rooms: Vec<(u64, u64)> = state.groups.0.keys().copied().collect();
    for room in rooms {
        let sends = state
            .groups
            .0
            .get_mut(&room)
            .map(Group::tick)
            .unwrap_or_default();
        state.apply_mls(sends);
    }
    let feedback = state.feedback;
    // Capacities of each video receiver's downlink, for the REMB estimate.
    let ids: Vec<u64> = state.conns.keys().copied().collect();
    for id in ids {
        let room = state.room_of(id);
        let receivers_cap: Vec<u64> = room
            .iter()
            .filter(|other| **other != id)
            .filter_map(|other| state.conns.get(other))
            .filter(|other| other.wants_video)
            .filter_map(|other| other.down.shape.capacity_bps)
            .collect();
        let Some(conn) = state.conns.get_mut(&id) else {
            continue;
        };
        if conn.crypto.is_none() || conn.addr.is_none() {
            continue;
        }
        let server_ssrc = 1u32;
        let mut out: Vec<Vec<u8>> = Vec::new();
        if feedback.transport_cc {
            for (header, body) in conn.twcc.due(server_ssrc, now) {
                let mut compound = header.to_vec();
                compound.extend(body);
                out.push(compound);
                conn.report.transport_feedback_to += 1;
            }
        }
        if now >= conn.next_report {
            conn.next_report = now + Duration::from_secs(1);
            if feedback.receiver_reports {
                let blocks: Vec<wire::Block> = conn
                    .sources
                    .iter_mut()
                    .filter_map(|(ssrc, stats)| stats.block(*ssrc, now))
                    .collect();
                if !blocks.is_empty() {
                    out.push(wire::receiver_report(server_ssrc, &blocks));
                    conn.report.receiver_reports_to += 1;
                }
            }
        }
        if feedback.remb && conn.video.is_some() && now >= conn.next_remb {
            conn.next_remb = now + Duration::from_secs(1);
            let arriving = conn
                .report
                .series
                .video_in
                .iter()
                .rev()
                .nth(1)
                .copied()
                .unwrap_or(0)
                * 8;
            let mut estimate = arriving * 3 / 2 + 500_000;
            for capacity in conn.up.shape.capacity_bps.into_iter().chain(receivers_cap) {
                estimate = estimate.min(capacity * 9 / 10);
            }
            conn.report.last_remb_bps = estimate;
            conn.report.rembs_to += 1;
            out.push(wire::remb(server_ssrc, estimate, &[conn.video_ssrc]));
        }
        for compound in out {
            if let Some(sealed) = conn.crypto.as_mut().and_then(|c| c.seal_rtcp(&compound)) {
                shared.send_to(conn, sealed);
            }
        }
    }
}
