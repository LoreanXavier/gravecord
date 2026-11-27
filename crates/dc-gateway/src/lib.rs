//! Gateway connection. Port of `GatewaySocket` in module 868: ETF over zstd-stream, QOS heartbeats, resume, backoff.
//! The reconnect shape follows Serein's `discord-gateway` (MIT/Apache-2.0); the protocol follows the pinned bundle.

/// The backoff class of module 158390, shared with the voice connection.
pub use dc_proto::backoff;
mod compression;

use backoff::Backoff;
use compression::Inflater;
use dc_proto::identity::{IdentifyState, Identity};
use dc_proto::op::Op;
use dc_proto::secrets::Token;
use dc_proto::{etf, pin};
use futures_util::{SinkExt, StreamExt};
use rand::Rng;
use serde_json::{Value, json};
use std::collections::VecDeque;
use std::time::{Duration, Instant};
use tokio::sync::mpsc;
use tokio_tungstenite::WebSocketStream;
use tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode;
use tokio_tungstenite::tungstenite::protocol::{CloseFrame, WebSocketConfig};
use tokio_tungstenite::tungstenite::{self, Message};
use tracing::{debug, info, warn};

/// `tu` in module 868: the `helloTimeout` set as `_connect` begins, so it covers the socket's connect and OP_HELLO.
const HELLO_TIMEOUT: Duration = Duration::from_secs(30);
/// The most the socket's TCP connect and TLS handshake may take, as the HTTP client's connect timeout: a connect that
/// hangs (one routed to a dead address) is given up and tried again on a new one, with a new name lookup, after the
/// backoff, instead of holding the start until the HELLO deadline.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
/// `t_` in module 868: past this since the last ack, a session is not worth resuming.
const RESUME_WINDOW: Duration = Duration::from_secs(180);
const AUTHENTICATION_FAILED: u16 = 4004;
const CLOSE_RECONNECT: u16 = 4000;
const MAX_FAILS_BEFORE_RESET: u32 = 4;
const MAX_PENDING_SENDS: usize = 64;

type Socket = WebSocketStream<dc_http::Upgraded>;

#[derive(Debug)]
pub enum Event {
    Dispatch {
        name: String,
        data: Value,
    },
    Connected,
    Reconnecting {
        delay: Duration,
    },
    Closed {
        code: u16,
        reason: String,
    },
    AuthenticationFailed,
    /// `_reset` (module 868): the session is gone (four failed reconnects, or 4004) and the next connection
    /// identifies. The official client dispatches `CONNECTION_CLOSED` here, which ends the voice connection.
    Reset,
}

/// What the operating system reported, as module 225123 hands it to the socket.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SystemEvent {
    /// `networkStateChange(5e3, "network detected online.")`.
    NetworkOnline,
    /// `networkStateChange(15e3, "network detected offline.", !1)`.
    NetworkOffline,
    /// `expeditedHeartbeat(5e3, "power monitor resumed")`.
    PowerResumed,
    /// `APP_STATE_UPDATE` to active on the desktop client: `resetBackoff("App state is active")`.
    AppActive,
}

/// `expeditedHeartbeat(timeout, reason, reset, closeUnestablished)`, or `None` for `resetBackoff(reason, true)`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Expedite {
    pub timeout: Duration,
    /// Not connected: reset the backoff and reconnect at once.
    pub reset: bool,
    /// With `reset`, a socket still connecting is closed and opened again (`resetBackoff`'s second argument).
    pub close_unestablished: bool,
}

impl SystemEvent {
    pub fn expedite(self) -> Option<Expedite> {
        match self {
            Self::NetworkOnline => Some(Expedite {
                timeout: Duration::from_secs(5),
                reset: true,
                close_unestablished: false,
            }),
            Self::NetworkOffline => Some(Expedite {
                timeout: Duration::from_secs(15),
                reset: false,
                close_unestablished: false,
            }),
            Self::PowerResumed => Some(Expedite {
                timeout: Duration::from_secs(5),
                reset: true,
                close_unestablished: true,
            }),
            Self::AppActive => None,
        }
    }
}

#[derive(Debug)]
pub enum Command {
    Send {
        op: Op,
        data: Value,
    },
    SetActive(bool),
    /// The account's token was renewed: the next identify or resume uses it.
    SetToken(Token),
    /// The network came or went, the machine woke, the app became active.
    System(SystemEvent),
    Shutdown,
}

pub struct Config {
    pub token: Token,
    pub identity: Identity,
    pub state: IdentifyState,
}

#[derive(Debug, PartialEq)]
enum Next {
    Reconnect,
    ReconnectNow,
    Stop,
}

struct Session {
    config: Config,
    events: mpsc::UnboundedSender<Event>,
    commands: mpsc::UnboundedReceiver<Command>,
    backoff: Backoff,
    seq: u64,
    session_id: Option<String>,
    resume_url: Option<String>,
    last_ack: Option<Instant>,
    active: bool,
    identified: bool,
    pending_sends: VecDeque<(Op, Value)>,
    /// `nextReconnectIsImmediate`: set by a backoff reset, the next reconnect does not wait.
    immediate: bool,
    /// READY or RESUMED came on this socket (`SESSION_ESTABLISHED`).
    established: bool,
    /// The last op 4 that put this account in a voice channel or call, until one takes it out. A shutdown sends
    /// the leave itself: a call left open at exit stays open on the service for minutes (a DM call's message then
    /// reads "lasted 2 minutes" for a call that ended at once).
    voice: Option<serde_json::Value>,
}

pub fn gateway_url(base: &str) -> String {
    format!(
        "{}/?encoding=etf&v={}&compress=zstd-stream",
        base.trim_end_matches('/'),
        pin::API_VERSION
    )
}

/// Runs on the current tokio runtime until `Shutdown` or an authentication failure.
pub fn spawn(
    config: Config,
) -> (
    mpsc::UnboundedSender<Command>,
    mpsc::UnboundedReceiver<Event>,
) {
    let (event_tx, event_rx) = mpsc::unbounded_channel();
    let (command_tx, command_rx) = mpsc::unbounded_channel();
    let session = Session {
        active: config.state.active,
        identified: false,
        pending_sends: VecDeque::new(),
        immediate: false,
        established: false,
        config,
        events: event_tx,
        commands: command_rx,
        backoff: Backoff::new(Duration::from_secs(1), Duration::from_secs(60)),
        seq: 0,
        session_id: None,
        resume_url: None,
        last_ack: None,
        voice: None,
    };
    tokio::spawn(session.run());
    (command_tx, event_rx)
}

impl Session {
    fn emit(&self, event: Event) {
        let _ = self.events.send(event);
    }

    async fn run(mut self) {
        loop {
            let attempt = tokio::time::Instant::now();
            let next = match tokio::time::timeout(CONNECT_TIMEOUT, self.connect()).await {
                Ok(Ok(socket)) => self.drive(socket, attempt + HELLO_TIMEOUT).await,
                Ok(Err(error)) => {
                    warn!("gateway connect failed: {error}");
                    Next::Reconnect
                }
                Err(_) => {
                    warn!(
                        "gateway connect timed out after {} ms",
                        CONNECT_TIMEOUT.as_millis()
                    );
                    // As the HELLO timeout does (`setResumeUrl(null)`): the next attempt goes to the main endpoint.
                    self.resume_url = None;
                    Next::Reconnect
                }
            };
            match next {
                Next::Stop => return,
                Next::ReconnectNow => {}
                Next::Reconnect if std::mem::take(&mut self.immediate) => {
                    info!("gateway reconnecting immediately after a backoff reset");
                }
                Next::Reconnect => {
                    let delay = self.backoff.fail(&mut rand::rng());
                    if self.backoff.fails() > MAX_FAILS_BEFORE_RESET {
                        self.reset();
                    }
                    self.emit(Event::Reconnecting { delay });
                    if self.wait(delay).await == Next::Stop {
                        return;
                    }
                }
            }
        }
    }

    /// `_reset`: forget the session; the app hears it once.
    fn reset(&mut self) {
        let had_session = self.session_id.is_some();
        self.session_id = None;
        self.seq = 0;
        if had_session {
            self.emit(Event::Reset);
        }
    }

    /// The backoff wait between connections. Commands still arrive: sends queue for the next session, and a system
    /// event that resets the backoff ends the wait (`resetBackoff` while `willReconnect()`: `_connect()` at once).
    async fn wait(&mut self, delay: Duration) -> Next {
        let until = tokio::time::Instant::now() + delay;
        loop {
            tokio::select! {
                () = tokio::time::sleep_until(until) => return Next::Reconnect,
                command = self.commands.recv() => match command {
                    None | Some(Command::Shutdown) => return Next::Stop,
                    Some(Command::SetActive(active)) => self.active = active,
                    Some(Command::SetToken(token)) => self.config.token = token,
                    Some(Command::Send { op, data }) => {
                        if op == Op::VoiceStateUpdate {
                            self.voice = (!data["channel_id"].is_null()).then(|| data.clone());
                        }
                        enqueue_pending_send(&mut self.pending_sends, op, data);
                    }
                    Some(Command::System(event)) => {
                        if event.expedite().is_none_or(|expedite| expedite.reset) {
                            info!(?event, "gateway backoff reset; reconnecting now");
                            self.backoff.succeed();
                            return Next::ReconnectNow;
                        }
                    }
                },
            }
        }
    }

    /// The socket the web app's page opens: Chromium's handshake and upgrade headers (`dc_http::chromium`), from
    /// the page's origin, with its user agent and language.
    async fn connect(&self) -> Result<Socket, dc_http::chromium::SocketError> {
        let base = self.resume_url.as_deref().unwrap_or(pin::GATEWAY_ENDPOINT);
        let url = gateway_url(base);
        info!("gateway connecting to {base}");
        dc_proto::replay::record_gateway_send("connect", None, &Value::Null);
        let url = dc_proto::replay::rewrite(&url);
        let config = WebSocketConfig::default()
            .max_message_size(None)
            .max_frame_size(None);
        dc_http::chromium::websocket(
            url.as_ref(),
            pin::WEBAPP_ORIGIN,
            &self.config.identity.system.locale,
            config,
        )
        .await
    }

    async fn send(
        &self,
        socket: &mut Socket,
        op: Op,
        data: Value,
    ) -> Result<(), tungstenite::Error> {
        let bytes = etf::pack(&json!({"op": op as u8, "d": data}));
        if dc_proto::replay::recording() {
            dc_proto::replay::record_gateway_send("send", Some(op as u64), &data);
        }
        let sent = socket.send(Message::Binary(bytes.into())).await;
        if op == Op::Identify {
            dc_proto::replay::stage("gw_identify_sent", Duration::ZERO);
        }
        sent
    }

    async fn flush_pending_sends(&mut self, socket: &mut Socket) -> Result<(), tungstenite::Error> {
        while let Some((op, data)) = self.pending_sends.pop_front() {
            if let Err(error) = self.send(socket, op, data.clone()).await {
                self.pending_sends.push_front((op, data));
                return Err(error);
            }
        }
        Ok(())
    }

    fn resumable(&self) -> bool {
        self.session_id.is_some()
            && self
                .last_ack
                .is_none_or(|ack| ack.elapsed() <= RESUME_WINDOW)
    }

    async fn resume_or_identify(&mut self, socket: &mut Socket) -> Result<(), tungstenite::Error> {
        if self.resumable() {
            let session_id = self.session_id.clone();
            info!("gateway resuming at seq {}", self.seq);
            let data = json!({"token": self.config.token.expose(), "session_id": session_id, "seq": self.seq});
            self.send(socket, Op::Resume, data).await?;
        } else {
            self.identify(socket).await?;
        }
        self.last_ack = Some(Instant::now());
        Ok(())
    }

    async fn identify(&mut self, socket: &mut Socket) -> Result<(), tungstenite::Error> {
        self.seq = 0;
        self.session_id = None;
        info!("gateway identifying");
        let data = self
            .config
            .identity
            .identify(&self.config.token, &self.config.state);
        self.send(socket, Op::Identify, data).await
    }

    async fn heartbeat(&self, socket: &mut Socket) -> Result<(), tungstenite::Error> {
        let qos = json!({"active": self.active, "ver": pin::QOS_HEARTBEAT_VERSION, "reasons": []});
        self.send(
            socket,
            Op::QosHeartbeat,
            json!({"seq": self.seq, "qos": qos}),
        )
        .await
    }

    fn on_dispatch(&mut self, name: &str, data: &Value) {
        match name {
            "READY" => {
                self.session_id = data["session_id"].as_str().map(str::to_owned);
                self.resume_url = data["resume_gateway_url"]
                    .as_str()
                    .map(|url| url.trim_end_matches('/').to_owned());
                self.config.state.qos_derived = data["qos_token"].as_str().map(str::to_owned);
                self.backoff.succeed();
            }
            "READY_SUPPLEMENTAL" | "RESUMED" => self.backoff.succeed(),
            _ => {}
        }
        if matches!(name, "READY" | "RESUMED") {
            self.established = true;
        }
    }

    async fn drive(&mut self, mut socket: Socket, hello_by: tokio::time::Instant) -> Next {
        self.identified = false;
        self.established = false;
        self.immediate = false;
        let mut inflater = match Inflater::new() {
            Ok(inflater) => inflater,
            Err(error) => {
                warn!("zstd init failed: {error}");
                return Next::Reconnect;
            }
        };
        let hello_deadline = tokio::time::sleep_until(hello_by);
        tokio::pin!(hello_deadline);
        let mut hello_seen = false;
        let mut heartbeat_interval = Duration::from_secs(u64::MAX / 4);
        let heartbeat = tokio::time::sleep(heartbeat_interval);
        tokio::pin!(heartbeat);
        let mut acked = true;
        // `expeditedHeartbeatTimeout`.
        let far = tokio::time::Instant::now() + Duration::from_secs(u64::MAX / 8);
        let expedited = tokio::time::sleep_until(far);
        tokio::pin!(expedited);
        let mut expediting = false;

        loop {
            tokio::select! {
                () = &mut hello_deadline, if !hello_seen => {
                    warn!("gateway timed out waiting for HELLO");
                    self.resume_url = None;
                    return Next::Reconnect;
                }
                () = &mut expedited, if expediting => {
                    expediting = false;
                    if !acked {
                        warn!("gateway expedited heartbeat was not acknowledged");
                        let _ = close(&mut socket, CLOSE_RECONNECT).await;
                        return Next::Reconnect;
                    }
                }
                () = &mut heartbeat => {
                    if !acked {
                        // `_doHeartbeatInterval`: a pending expedited heartbeat has its own deadline.
                        if expediting {
                            heartbeat.as_mut().reset(tokio::time::Instant::now() + heartbeat_interval);
                            continue;
                        }
                        warn!("gateway heartbeat was not acknowledged");
                        let _ = close(&mut socket, CLOSE_RECONNECT).await;
                        return Next::Reconnect;
                    }
                    acked = false;
                    if self.heartbeat(&mut socket).await.is_err() {
                        return Next::Reconnect;
                    }
                    heartbeat.as_mut().reset(tokio::time::Instant::now() + heartbeat_interval);
                }
                command = self.commands.recv() => match command {
                    None | Some(Command::Shutdown) => {
                        if let Some(leave) = voice_leave(self.voice.as_ref()) {
                            let _ = self.send(&mut socket, Op::VoiceStateUpdate, leave).await;
                        }
                        let _ = close(&mut socket, 1000).await;
                        return Next::Stop;
                    }
                    Some(Command::SetActive(active)) => self.active = active,
                    Some(Command::SetToken(token)) => self.config.token = token,
                    Some(Command::System(event)) => match event.expedite() {
                        Some(expedite) if hello_seen => {
                            debug!(?event, "gateway expedited heartbeat");
                            acked = false;
                            if self.heartbeat(&mut socket).await.is_err() {
                                return Next::Reconnect;
                            }
                            expedited.as_mut().reset(tokio::time::Instant::now() + expedite.timeout);
                            expediting = true;
                        }
                        Some(Expedite { reset: false, .. }) => {}
                        // `resetBackoff`: the backoff starts over and the next reconnect is immediate; a socket that
                        // is not yet established is closed now when asked to.
                        other => {
                            info!(?event, "gateway backoff reset");
                            self.backoff.succeed();
                            self.immediate = true;
                            if !self.established && other.is_none_or(|e| e.close_unestablished) {
                                let _ = close(&mut socket, CLOSE_RECONNECT).await;
                                return Next::ReconnectNow;
                            }
                        }
                    },
                    Some(Command::Send { op, data }) => {
                        if op == Op::VoiceStateUpdate {
                            self.voice = (!data["channel_id"].is_null()).then(|| data.clone());
                        }
                        if !self.identified {
                            enqueue_pending_send(&mut self.pending_sends, op, data);
                        } else if self.send(&mut socket, op, data).await.is_err() {
                            return Next::Reconnect;
                        }
                    }
                },
                frame = socket.next() => {
                    let bytes = match frame {
                        Some(Ok(Message::Binary(bytes))) => bytes,
                        Some(Ok(Message::Close(frame))) => {
                            let (code, reason) = frame.map_or((1000, String::new()), |f| (u16::from(f.code), f.reason.to_string()));
                            return self.on_close(code, reason);
                        }
                        Some(Ok(_)) => continue,
                        Some(Err(error)) => {
                            warn!("gateway socket error: {error}");
                            return Next::Reconnect;
                        }
                        None => return self.on_close(1006, "stream ended".to_owned()),
                    };
                    let decoding = Instant::now();
                    let mut inflated = None;
                    let payload = match inflater.feed(&bytes).map(|raw| {
                        inflated = Some(Instant::now());
                        etf::unpack(&raw)
                    }) {
                        Ok(Ok(payload)) => payload,
                        Ok(Err(error)) => {
                            warn!("gateway payload did not decode: {error}");
                            return Next::Reconnect;
                        }
                        Err(error) => {
                            warn!("gateway decompression failed: {error}");
                            return Next::Reconnect;
                        }
                    };
                    dc_proto::replay::record_gateway_recv(decoding, &payload);
                    if payload["t"] == "READY" {
                        // Where the READY frame's time went: read off the socket, inflated, unpacked (`start_trace`).
                        dc_proto::replay::stage("gw_ready_read", decoding.elapsed());
                        if let Some(inflated) = inflated {
                            dc_proto::replay::stage("gw_ready_inflated", inflated.elapsed());
                        }
                        dc_proto::replay::stage("gw_ready_unpacked", Duration::ZERO);
                    }
                    let op = payload["op"].as_u64().and_then(Op::from_u64);
                    match op {
                        Some(Op::Hello) => {
                            hello_seen = true;
                            let millis = payload["d"]["heartbeat_interval"].as_u64().unwrap_or(41_250);
                            heartbeat_interval = Duration::from_millis(millis);
                            let first = rand::rng().random_range(0..millis.max(1));
                            heartbeat.as_mut().reset(tokio::time::Instant::now() + Duration::from_millis(first));
                            acked = true;
                            debug!("gateway hello, heartbeat every {millis} ms");
                            self.emit(Event::Connected);
                            if self.resume_or_identify(&mut socket).await.is_err() {
                                return Next::Reconnect;
                            }
                            self.identified = true;
                            if self.flush_pending_sends(&mut socket).await.is_err() {
                                return Next::Reconnect;
                            }
                        }
                        Some(Op::HeartbeatAck) => {
                            acked = true;
                            if std::mem::take(&mut expediting) {
                                debug!("gateway expedited heartbeat succeeded");
                            }
                            self.last_ack = Some(Instant::now());
                        }
                        Some(Op::Heartbeat) => {
                            if self.heartbeat(&mut socket).await.is_err() {
                                return Next::Reconnect;
                            }
                            heartbeat.as_mut().reset(tokio::time::Instant::now() + heartbeat_interval);
                        }
                        Some(Op::Reconnect) => {
                            let _ = close(&mut socket, CLOSE_RECONNECT).await;
                            return Next::ReconnectNow;
                        }
                        Some(Op::InvalidSession) => {
                            let resumable = payload["d"].as_bool().unwrap_or(false);
                            info!("gateway invalid session, resumable: {resumable}");
                            self.identified = false;
                            let sent = if resumable {
                                self.resume_or_identify(&mut socket).await
                            } else {
                                self.identify(&mut socket).await
                            };
                            if sent.is_err() {
                                return Next::Reconnect;
                            }
                            self.identified = true;
                            if self.flush_pending_sends(&mut socket).await.is_err() {
                                return Next::Reconnect;
                            }
                        }
                        Some(Op::Dispatch) => {
                            if let Some(seq) = payload["s"].as_u64() {
                                self.seq = seq;
                            }
                            let name = payload["t"].as_str().unwrap_or_default().to_owned();
                            let data = payload.get("d").cloned().unwrap_or(Value::Null);
                            let decode_ms = decoding.elapsed().as_millis();
                            if decode_ms >= 5 {
                                debug!(
                                    "gateway {name} decoded in {decode_ms} ms from {} compressed bytes",
                                    bytes.len()
                                );
                            }
                            self.on_dispatch(&name, &data);
                            self.emit(Event::Dispatch { name, data });
                        }
                        _ => debug!("gateway op {} ignored", payload["op"]),
                    }
                }
            }
        }
    }

    fn on_close(&mut self, code: u16, reason: String) -> Next {
        info!("gateway closed: {code}");
        self.emit(Event::Closed { code, reason });
        if code == AUTHENTICATION_FAILED {
            self.reset();
            self.emit(Event::AuthenticationFailed);
            return Next::Stop;
        }
        Next::Reconnect
    }
}

async fn close(socket: &mut Socket, code: u16) -> Result<(), tungstenite::Error> {
    socket
        .close(Some(CloseFrame {
            code: CloseCode::from(code),
            reason: "".into(),
        }))
        .await
}

fn enqueue_pending_send(queue: &mut VecDeque<(Op, Value)>, op: Op, data: Value) {
    if queue.len() >= MAX_PENDING_SENDS {
        queue.pop_front();
    }
    queue.push_back((op, data));
}

/// The op 4 that undoes `last`: the same guild (null for a DM call) with a null channel, no camera.
pub fn voice_leave(last: Option<&serde_json::Value>) -> Option<serde_json::Value> {
    let mut leave = last?.clone();
    leave["channel_id"] = serde_json::Value::Null;
    leave["self_video"] = serde_json::Value::Bool(false);
    Some(leave)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn shutdown_leaves_a_dm_call_with_a_null_channel_and_null_guild() {
        let joined = json!({"guild_id": null, "channel_id": "5", "self_mute": false, "self_deaf": false, "self_video": true});
        let leave = voice_leave(Some(&joined)).expect("a call is open");
        assert!(leave["channel_id"].is_null());
        assert!(leave["guild_id"].is_null());
        assert_eq!(leave["self_video"], json!(false));
    }

    #[test]
    fn system_events_follow_module_225123() {
        let online = SystemEvent::NetworkOnline.expedite().unwrap();
        assert_eq!(online.timeout, Duration::from_secs(5));
        assert!(online.reset && !online.close_unestablished);
        let offline = SystemEvent::NetworkOffline.expedite().unwrap();
        assert_eq!(offline.timeout, Duration::from_secs(15));
        assert!(!offline.reset);
        let resumed = SystemEvent::PowerResumed.expedite().unwrap();
        assert_eq!(resumed.timeout, Duration::from_secs(5));
        assert!(resumed.reset && resumed.close_unestablished);
        assert_eq!(SystemEvent::AppActive.expedite(), None);
    }

    #[tokio::test]
    async fn a_backoff_wait_ends_on_a_reset_and_keeps_sends() {
        let (events, _rx) = mpsc::unbounded_channel();
        let (commands_tx, commands) = mpsc::unbounded_channel();
        let mut session = Session {
            config: Config {
                token: Token::new("t"),
                identity: Identity::new(),
                state: IdentifyState::default(),
            },
            events,
            commands,
            backoff: Backoff::new(Duration::from_secs(1), Duration::from_secs(60)),
            seq: 0,
            session_id: Some("s".into()),
            resume_url: None,
            last_ack: None,
            active: true,
            identified: false,
            pending_sends: VecDeque::new(),
            immediate: false,
            established: false,
            voice: None,
        };
        session.backoff.fail(&mut rand::rng());
        commands_tx
            .send(Command::Send {
                op: Op::GuildSubscriptions,
                data: json!({"n": 1}),
            })
            .unwrap();
        commands_tx
            .send(Command::System(SystemEvent::NetworkOffline))
            .unwrap();
        commands_tx
            .send(Command::System(SystemEvent::NetworkOnline))
            .unwrap();
        let started = Instant::now();
        assert_eq!(
            session.wait(Duration::from_secs(30)).await,
            Next::ReconnectNow
        );
        assert!(started.elapsed() < Duration::from_secs(5));
        assert_eq!(session.backoff.fails(), 0);
        assert_eq!(
            session.pending_sends.len(),
            1,
            "a send during the wait is kept"
        );
        commands_tx.send(Command::Shutdown).unwrap();
        assert_eq!(session.wait(Duration::from_secs(30)).await, Next::Stop);
    }

    #[test]
    fn a_reset_forgets_the_session_and_says_so_once() {
        let (events, mut rx) = mpsc::unbounded_channel();
        let (_tx, commands) = mpsc::unbounded_channel();
        let mut session = Session {
            config: Config {
                token: Token::new("t"),
                identity: Identity::new(),
                state: IdentifyState::default(),
            },
            events,
            commands,
            backoff: Backoff::new(Duration::from_secs(1), Duration::from_secs(60)),
            seq: 9,
            session_id: Some("s".into()),
            resume_url: None,
            last_ack: None,
            active: true,
            identified: false,
            pending_sends: VecDeque::new(),
            immediate: false,
            established: false,
            voice: None,
        };
        session.reset();
        session.reset();
        assert_eq!((session.session_id.clone(), session.seq), (None, 0));
        assert!(matches!(rx.try_recv(), Ok(Event::Reset)));
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn shutdown_has_nothing_to_leave_when_no_call_is_open() {
        assert!(voice_leave(None).is_none());
    }

    #[test]
    fn url_matches_the_recorded_connection() {
        assert_eq!(
            gateway_url("wss://gateway-us-east1-d.discord.gg/"),
            "wss://gateway-us-east1-d.discord.gg/?encoding=etf&v=9&compress=zstd-stream"
        );
    }

    #[test]
    fn pre_identify_queue_preserves_order_and_caps_at_limit() {
        let mut queue = VecDeque::new();
        for index in 0..(MAX_PENDING_SENDS + 5) {
            enqueue_pending_send(&mut queue, Op::GuildSubscriptions, json!({"n": index}));
        }
        assert_eq!(queue.len(), MAX_PENDING_SENDS);
        assert_eq!(queue.front().unwrap().1["n"], json!(5));
        assert_eq!(queue.back().unwrap().1["n"], json!(MAX_PENDING_SENDS + 4));
    }
}
