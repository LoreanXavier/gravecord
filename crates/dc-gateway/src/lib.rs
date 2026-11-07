//! Gateway connection. Port of `GatewaySocket` in module 868: ETF over zstd-stream, QOS heartbeats, resume, backoff.
//! The reconnect shape follows Serein's `discord-gateway` (MIT/Apache-2.0); the protocol follows the pinned bundle.

mod backoff;
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
use std::time::{Duration, Instant};
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::HeaderValue;
use tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode;
use tokio_tungstenite::tungstenite::protocol::{CloseFrame, WebSocketConfig};
use tokio_tungstenite::tungstenite::{self, Message};
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, connect_async_with_config};
use tracing::{debug, info, warn};

const HELLO_TIMEOUT: Duration = Duration::from_secs(30);
/// `t_` in module 868: past this since the last ack, a session is not worth resuming.
const RESUME_WINDOW: Duration = Duration::from_secs(180);
const AUTHENTICATION_FAILED: u16 = 4004;
const CLOSE_RECONNECT: u16 = 4000;
const MAX_FAILS_BEFORE_RESET: u32 = 4;

type Socket = WebSocketStream<MaybeTlsStream<TcpStream>>;

#[derive(Debug)]
pub enum Event {
    Dispatch { name: String, data: Value },
    Connected,
    Reconnecting { delay: Duration },
    Closed { code: u16, reason: String },
    AuthenticationFailed,
}

#[derive(Debug)]
pub enum Command {
    Send { op: Op, data: Value },
    SetActive(bool),
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
    // Dependencies pull in more than one rustls backend, so it has to be chosen. Fails harmlessly if already set.
    let _ = rustls::crypto::ring::default_provider().install_default();
    let (event_tx, event_rx) = mpsc::unbounded_channel();
    let (command_tx, command_rx) = mpsc::unbounded_channel();
    let session = Session {
        active: config.state.active,
        config,
        events: event_tx,
        commands: command_rx,
        backoff: Backoff::new(Duration::from_secs(1), Duration::from_secs(60)),
        seq: 0,
        session_id: None,
        resume_url: None,
        last_ack: None,
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
            let next = match self.connect().await {
                Ok(socket) => self.drive(socket).await,
                Err(error) => {
                    warn!("gateway connect failed: {error}");
                    Next::Reconnect
                }
            };
            match next {
                Next::Stop => return,
                Next::ReconnectNow => {}
                Next::Reconnect => {
                    let delay = self.backoff.fail(&mut rand::rng());
                    if self.backoff.fails() > MAX_FAILS_BEFORE_RESET {
                        self.session_id = None;
                        self.seq = 0;
                    }
                    self.emit(Event::Reconnecting { delay });
                    tokio::time::sleep(delay).await;
                }
            }
        }
    }

    async fn connect(&self) -> Result<Socket, tungstenite::Error> {
        let base = self.resume_url.as_deref().unwrap_or(pin::GATEWAY_ENDPOINT);
        let url = gateway_url(base);
        info!("gateway connecting to {base}");
        let mut request = url.as_str().into_client_request()?;
        let headers = request.headers_mut();
        headers.insert("Origin", HeaderValue::from_static(pin::WEBAPP_ORIGIN));
        headers.insert("User-Agent", HeaderValue::from_static(pin::USER_AGENT));
        headers.insert("Cache-Control", HeaderValue::from_static("no-cache"));
        headers.insert("Pragma", HeaderValue::from_static("no-cache"));
        let config = WebSocketConfig::default()
            .max_message_size(None)
            .max_frame_size(None);
        let (socket, _) = connect_async_with_config(request, Some(config), false).await?;
        Ok(socket)
    }

    async fn send(
        &self,
        socket: &mut Socket,
        op: Op,
        data: Value,
    ) -> Result<(), tungstenite::Error> {
        let bytes = etf::pack(&json!({"op": op as u8, "d": data}));
        socket.send(Message::Binary(bytes.into())).await
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
    }

    async fn drive(&mut self, mut socket: Socket) -> Next {
        let mut inflater = match Inflater::new() {
            Ok(inflater) => inflater,
            Err(error) => {
                warn!("zstd init failed: {error}");
                return Next::Reconnect;
            }
        };
        let hello_deadline = tokio::time::sleep(HELLO_TIMEOUT);
        tokio::pin!(hello_deadline);
        let mut hello_seen = false;
        let mut heartbeat_interval = Duration::from_secs(u64::MAX / 4);
        let heartbeat = tokio::time::sleep(heartbeat_interval);
        tokio::pin!(heartbeat);
        let mut acked = true;

        loop {
            tokio::select! {
                () = &mut hello_deadline, if !hello_seen => {
                    warn!("gateway timed out waiting for HELLO");
                    self.resume_url = None;
                    return Next::Reconnect;
                }
                () = &mut heartbeat => {
                    if !acked {
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
                        let _ = close(&mut socket, 1000).await;
                        return Next::Stop;
                    }
                    Some(Command::SetActive(active)) => self.active = active,
                    Some(Command::Send { op, data }) => {
                        if self.send(&mut socket, op, data).await.is_err() {
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
                    let payload = match inflater.feed(&bytes).map(|raw| etf::unpack(&raw)) {
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
                        }
                        Some(Op::HeartbeatAck) => {
                            acked = true;
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
                            let sent = if resumable {
                                self.resume_or_identify(&mut socket).await
                            } else {
                                self.identify(&mut socket).await
                            };
                            if sent.is_err() {
                                return Next::Reconnect;
                            }
                        }
                        Some(Op::Dispatch) => {
                            if let Some(seq) = payload["s"].as_u64() {
                                self.seq = seq;
                            }
                            let name = payload["t"].as_str().unwrap_or_default().to_owned();
                            let data = payload.get("d").cloned().unwrap_or(Value::Null);
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
            self.session_id = None;
            self.seq = 0;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn url_matches_the_recorded_connection() {
        assert_eq!(
            gateway_url("wss://gateway-us-east1-d.discord.gg/"),
            "wss://gateway-us-east1-d.discord.gg/?encoding=etf&v=9&compress=zstd-stream"
        );
    }
}
