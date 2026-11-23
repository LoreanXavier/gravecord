//! One voice connection: control socket, UDP media, DAVE, Opus, mixer and audio devices.
//!
//! Adapted from Serein `crates/discord-voice/src/transport.rs` (`run_inner`, `send`,
//! `json_send`, `UdpFailures`, `transient_receive`, `negotiation_timeout`), MIT OR
//! Apache-2.0. Changes: cameras in the call (send with viewer RTCP honoured, receive with NACK/PLI
//! and one decoder thread per source; no diagnostics), protocol URL `v=9` with `channel_id` in IDENTIFY and RESUME, op 16 after
//! READY on server version 6+, a 20 s HELLO timeout, AES-256-GCM preferred over XChaCha20,
//! Origin/User-Agent of the pinned build on the socket, audio devices owned by the session,
//! and app-facing events in place of Serein's `Status`.
use crate::prefs;
use crate::{
    DisconnectReason, Event, Frame, RtcState,
    activity::{self, LOCAL_THRESHOLD_DB},
    audio::Audio,
    capture::CapturePacer,
    clock,
    dave::Dave,
    playout::CallPlayout,
    rtc_stats::{
        CameraStats, InboundAudioStats, InboundUserStats, InboundVideoStats, OutboundAudioStats,
        OutboundStats, OutboundVideoStats, RtcStatsSnapshot, RtcStatsStore, TransportStats,
    },
    rtp::{self, MAX_PACKET, Mode, Transport},
    signal::{self, CLOSE_DISCONNECTED, Context, DaveAt, Ids, MAX_SIGNAL},
    video::{
        RemoteFrame, VideoHub, VideoSource,
        receive::{self, Decoders, Receivers},
        sender::{CALL_START_BITRATE, VideoSender},
    },
};
use dc_media::{
    EncodedFrame,
    camera::{self, Camera, CameraFrame},
};
use dc_proto::voice_op::{VoiceOp, close};
use futures_util::{SinkExt, StreamExt};
use opus2::{Application, Bitrate, Channels, Encoder, SoftClip};
use std::{
    collections::{BTreeSet, HashMap},
    net::SocketAddr,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering},
        mpsc as std_mpsc,
    },
    time::Duration,
};
use tokio::{
    net::{TcpStream, UdpSocket},
    sync::{mpsc, watch},
    time::{Instant, timeout},
};
use tokio_tungstenite::{
    MaybeTlsStream, WebSocketStream,
    tungstenite::{
        Message,
        client::IntoClientRequest,
        http::HeaderValue,
        protocol::{WebSocketConfig, frame::coding::CloseCode},
    },
};
use tracing::{debug, warn};
use zeroize::Zeroizing;

pub(crate) type Socket = WebSocketStream<MaybeTlsStream<TcpStream>>;

/// Time a sole member gives the roster announcement before waiting for a peer (Serein).
pub(crate) const PEER_GRACE: Duration = Duration::from_millis(500);
/// HELLO timeout of the pinned build (`tools/voice-notes.md`).
pub(crate) const HELLO_TIMEOUT: Duration = Duration::from_secs(20);
/// Serein's bound for reaching a secured call, and for recovering one after a change.
pub(crate) const NEGOTIATION: Duration = Duration::from_secs(90);
pub(crate) const RENEGOTIATION: Duration = Duration::from_secs(30);
/// Sustained UDP send outage before attempting socket rebind / failing (`921747.js`: UDP is lossy
/// and transient routing/Wi-Fi blips must never drop an active call).
const UDP_OUTAGE: Duration = Duration::from_secs(30);
/// Minimum consecutive UDP send errors across [`UDP_OUTAGE`] so two isolated 5 s keepalives never kill a call.
const UDP_MIN_FAILURES: u32 = 10;
pub(crate) const UDP_KEEPALIVE: Duration = Duration::from_secs(5);
pub(crate) const DISCOVERY_TIMEOUT: Duration = Duration::from_secs(15);
/// Interval between UDP IP discovery retransmissions so a single lost packet never hangs discovery.
pub(crate) const DISCOVERY_RETRY: Duration = Duration::from_millis(400);
/// `RTCControlSocket` (`921747.js` `class eJ`): `backoff.fails > 3` allows 4 consecutive resume attempts.
pub(crate) const MAX_RESUMES: u8 = 4;
/// `RTCControlSocket` (`921747.js` `e$ = 1 minute`): maximum elapsed time since last healthy ACK for resume.
pub(crate) const RESUME_WINDOW: Duration = Duration::from_secs(60);
/// `reconnectInterval = 6e4` of module 31595, the transport option the official client hands its media engine (and
/// never changes): with no datagram received for this long, the UDP socket is rebound (`fastUdpReconnect`).
pub(crate) const UDP_RECONNECT_INTERVAL: Duration = Duration::from_secs(60);

/// What the heartbeat timer does at a wake (`startHeartbeater` and `expeditedHeartbeat` of module 921747).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Beat {
    /// Nothing is due.
    Wait,
    /// An interval passed while an expedited heartbeat is pending: the regular tick neither sends nor times out.
    Skip,
    /// An interval passed with the last heartbeat acknowledged: send the next.
    Send,
    /// The last heartbeat was not acknowledged by the next tick, or by the expedited deadline: the socket is dead
    /// (`handleHeartbeatTimeout`, then a resume after the backoff).
    TimedOut,
}

pub(crate) fn beat(
    now: Instant,
    next_tick: Instant,
    awaiting_ack: bool,
    expedited: Option<Instant>,
) -> Beat {
    if let Some(deadline) = expedited {
        if now >= deadline && awaiting_ack {
            return Beat::TimedOut;
        }
        return if now >= next_tick {
            Beat::Skip
        } else {
            Beat::Wait
        };
    }
    if now < next_tick {
        Beat::Wait
    } else if awaiting_ack {
        Beat::TimedOut
    } else {
        Beat::Send
    }
}

/// `RTCControlSocket` (`921747.js` `backoff = new Backoff(1000, 5000)`): jittered exponential delay
/// for control WebSocket resume retries (1s -> 2s -> 4s -> 5s).
pub(crate) fn ws_resume_backoff(attempt: u8) -> Duration {
    let base_ms = (1000u64 << attempt.min(3)).min(5000);
    let jitter = 0.75 + rand::random::<f64>() * 0.5;
    Duration::from_millis(((base_ms as f64) * jitter).round().clamp(500.0, 5000.0) as u64)
}

/// Close codes that allow resuming the existing media session in place instead of tearing down UDP and DAVE.
/// `RTCControlSocket.handleClose` (`921747.js`) stops resuming on `4004`, `4006`, `4011` and `4015` only. `4014`
/// is not in the bundle at all (the official client treats it as any other code and tries a resume); it is Discord's
/// documented "disconnected" (kicked, moved, channel deleted), and this client stops on it as Serein does, so the
/// app can tell a move from a kick. `4800` to `4805` are the client's own codes and never come from the server.
pub(crate) fn close_code_is_resumable(code: u16) -> bool {
    !matches!(
        code,
        close::AUTHENTICATION_FAILED
            | close::INVALID_SESSION
            | close::SERVER_NOT_FOUND
            | CLOSE_DISCONNECTED
            | close::SERVER_CRASH
            | 4800..=4805
    )
}

/// `fastUdpReconnect` (`921747.js` `_handleResuming`): rebinds and reconnects the local UDP socket
/// after a network/route blip while keeping the existing SRTP/AEAD transport key and DAVE epoch intact.
pub(crate) async fn rebind_udp(peer: SocketAddr) -> Option<UdpSocket> {
    let bind_addr = if peer.is_ipv4() {
        "0.0.0.0:0"
    } else {
        "[::]:0"
    };
    let socket = UdpSocket::bind(bind_addr).await.ok()?;
    socket.connect(peer).await.ok()?;
    Some(socket)
}

pub(crate) struct Credentials {
    pub endpoint: String,
    pub token: Zeroizing<String>,
    pub session_id: Zeroizing<String>,
    pub server_id: u64,
    pub channel_id: u64,
    pub user_id: u64,
    pub stage: bool,
    pub rtc_connection_id: String,
    pub report_no_route: bool,
}
impl Credentials {
    fn ids(&self) -> Ids<'_> {
        Ids {
            server_id: self.server_id,
            channel_id: self.channel_id,
            user_id: self.user_id,
            session_id: &self.session_id,
            token: &self.token,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct ControlState {
    pub self_mute: bool,
    pub self_deaf: bool,
    pub loopback: bool,
    pub disconnect: bool,
    /// Bumped when the camera wish or the video sink changed, so the loop looks at `Shared` again.
    pub video_request: u32,
    /// Bumped with each [`crate::SystemEvent`]; `system` is the latest.
    pub system_seq: u32,
    pub system: Option<crate::SystemEvent>,
}

/// What the app hands the call besides the copyable control state.
pub(crate) struct Shared {
    /// The camera wish: `Some(device id)`, empty for the default camera, `None` for off.
    pub camera: Mutex<Option<String>>,
    pub hub: VideoHub,
    /// Per-user volume (a factor, 1.0 is 100%) and local mute, as chosen in a user's menu.
    pub user_mix: Mutex<std::collections::HashMap<u64, (f32, bool)>>,
    /// Lock-free / device-pulled playout bus shared with the playback callback and watched streams.
    pub playout: Arc<CallPlayout>,
    /// Periodic WebRTC-shaped telemetry read by [`crate::Controls::rtc_stats`].
    pub rtc_stats: Arc<RtcStatsStore>,
}

impl Default for Shared {
    fn default() -> Self {
        let hub = VideoHub::new();
        let playout = Arc::new(CallPlayout::new(hub.sidechain_arc()));
        hub.attach_playout(&playout);
        Self {
            camera: Mutex::default(),
            hub,
            user_mix: Mutex::default(),
            playout,
            rtc_stats: Arc::default(),
        }
    }
}

/// Video senders announce this much; the camera encoder runs at `camera::BITRATE`.
const SINK_WANTS_INTERVAL: Duration = Duration::from_secs(5);
/// The lowest rate the camera is steered to (`videoBitrate.min` and `videoBitrateFloor` of module 731854, 150 kb/s).
const CAMERA_MIN_BITRATE: u32 = 150_000;
/// The camera's top rate (`videoBitrate.max` of module 731854, 2.5 Mb/s, at the full budget portion of the ladder):
/// `getVideoQuality` hands it as both `encodingVideoMaxBitRate` and `encodingVideoBitRate`, as a camera has no target.
/// The estimation still starts at the call's start rate; the camera encoder holds its own bounds.
const CAMERA_MAX_BITRATE: u32 = 2_500_000;

pub(crate) async fn send(ws: &mut Socket, message: Message) -> Result<(), &'static str> {
    timeout(Duration::from_secs(5), ws.send(message))
        .await
        .map_err(|_| "Voice signaling write timed out")?
        .map_err(|_| "Voice signaling disconnected")
}
pub(crate) async fn text(ws: &mut Socket, value: String) -> Result<(), &'static str> {
    send(ws, Message::Text(value.into())).await
}

/// Sends a WebSocket message if the control socket is open; when the media transport is already up,
/// a write error drops the socket and schedules an in-place resume (`921747.js`) instead of ending the call.
pub(crate) async fn send_or_resume(
    ws: &mut Option<Socket>,
    reconnect: &mut Option<&'static str>,
    has_transport: bool,
    message: Message,
) -> Result<(), &'static str> {
    let Some(sock) = ws.as_mut() else {
        return Ok(());
    };
    match send(sock, message).await {
        Ok(()) => Ok(()),
        Err(err) if has_transport => {
            *ws = None;
            if reconnect.is_none() {
                *reconnect = Some(err);
            }
            Ok(())
        }
        Err(err) => Err(err),
    }
}

pub(crate) async fn text_or_resume(
    ws: &mut Option<Socket>,
    reconnect: &mut Option<&'static str>,
    has_transport: bool,
    value: String,
) -> Result<(), &'static str> {
    send_or_resume(ws, reconnect, has_transport, Message::Text(value.into())).await
}

/// `disconnectForRefusedDaveDowngrade` (4804) and `disconnectForInvalidDaveMessage` (4805) of module 921747: the
/// control socket closes with the code and the connection ends with it.
pub(crate) async fn refuse(ws: &mut Option<Socket>, code: u16, at: &str) -> DisconnectReason {
    if code == close::DAVE_DOWNGRADE_REFUSED {
        warn!("refusing DAVE protocol downgrade to version 0 at {at}, disconnecting");
    } else {
        warn!("invalid DAVE message: {at}, disconnecting");
    }
    if let Some(mut sock) = ws.take() {
        let frame = tokio_tungstenite::tungstenite::protocol::CloseFrame {
            code: CloseCode::from(code),
            reason: "".into(),
        };
        let _ = timeout(Duration::from_secs(1), sock.close(Some(frame))).await;
    }
    DisconnectReason::Closed(code)
}

/// NO_ROUTE (`ConnectionStateChange` of module 921747): the state, op 32 on the first attempt of a reconnect cycle
/// (`0 === this._backoff.fails && this._handleNoRoute()`), and the end of this connection, which the app's backoff
/// reconnects (`_scheduleReconnect`).
pub(crate) async fn no_route(
    ws: &mut Option<Socket>,
    events: &mpsc::UnboundedSender<Event>,
    report: bool,
    at: &str,
) -> &'static str {
    warn!("voice media found no route ({at})");
    emit(events, Event::State(RtcState::NoRoute));
    if report && let Some(sock) = ws.as_mut() {
        let _ = text(sock, signal::no_route()).await;
    }
    "Discord voice found no route to the server; reconnecting"
}

pub(crate) struct OpenedSocket {
    pub ws: Socket,
    pub host: String,
    pub port: u16,
    pub tls_version: Option<&'static str>,
    pub opened_at: Instant,
}

fn ws_target(url: &str) -> Result<(String, u16), &'static str> {
    let parsed = url::Url::parse(url).map_err(|_| "Invalid Discord voice endpoint")?;
    if !parsed.username().is_empty() || parsed.password().is_some() {
        return Err("Invalid Discord voice endpoint");
    }
    match parsed.scheme() {
        "wss" => {
            let host = parsed.host_str().ok_or("Missing Discord voice host")?;
            let raw = match parsed.port() {
                Some(port) => format!("{host}:{port}"),
                None => host.to_string(),
            };
            signal::validate_endpoint_host(&raw)
        }
        #[cfg(feature = "sim")]
        "ws" if crate::sim::active() && parsed.host_str() == Some("127.0.0.1") => {
            let port = parsed
                .port()
                .filter(|p| *p != 0)
                .ok_or("Invalid Discord voice endpoint")?;
            Ok(("127.0.0.1".to_string(), port))
        }
        _ => Err("Invalid Discord voice endpoint"),
    }
}

pub(crate) fn sanitized_ws_error(err: &tokio_tungstenite::tungstenite::Error) -> String {
    use tokio_tungstenite::tungstenite::Error;
    match err {
        Error::Io(io_err) => format!("io:{:?}", io_err.kind()),
        Error::Tls(_) => "tls".to_string(),
        Error::Http(resp) => format!("http:{}", resp.status().as_u16()),
        Error::HttpFormat(_) => "http_format".to_string(),
        Error::Protocol(_) => "protocol".to_string(),
        Error::Url(_) => "url".to_string(),
        Error::Capacity(_) => "capacity".to_string(),
        Error::WriteBufferFull(_) => "write_buffer_full".to_string(),
        Error::Utf8(_) => "utf8".to_string(),
        Error::AttackAttempt => "attack_attempt".to_string(),
        Error::ConnectionClosed | Error::AlreadyClosed => "closed".to_string(),
    }
}

pub(crate) fn log_ws_stage_ok(
    kind: &'static str,
    stage: &'static str,
    host: &str,
    port: u16,
    elapsed_ms: u64,
    tls_version: Option<&'static str>,
) {
    debug!(
        kind,
        stage, host, port, elapsed_ms, tls_version, "voice transport stage completed"
    );
}

pub(crate) fn log_ws_stage_err(
    kind: &'static str,
    stage: &'static str,
    host: &str,
    port: u16,
    elapsed_ms: u64,
    error_kind: &str,
) {
    debug!(
        kind,
        stage, host, port, elapsed_ms, error_kind, "voice transport stage failed"
    );
}

pub(crate) fn log_ws_close(
    kind: &'static str,
    host: &str,
    port: u16,
    elapsed_ms: u64,
    close_code: u16,
    reason_len: usize,
) {
    debug!(
        kind,
        stage = "ws_close",
        host,
        port,
        elapsed_ms,
        close_code,
        reason_len,
        "voice websocket closed"
    );
}

pub(crate) async fn open_with_stages(
    kind: &'static str,
    url: &str,
) -> Result<OpenedSocket, &'static str> {
    let (host, port) = ws_target(url)?;
    let opened_at = Instant::now();

    let dns_start = Instant::now();
    let addrs: Vec<SocketAddr> = match timeout(
        Duration::from_secs(10),
        tokio::net::lookup_host((host.as_str(), port)),
    )
    .await
    {
        Ok(Ok(iter)) => {
            let list: Vec<_> = iter.collect();
            let elapsed_ms = dns_start.elapsed().as_millis() as u64;
            if list.is_empty() {
                log_ws_stage_err(kind, "dns", &host, port, elapsed_ms, "empty");
                return Err("Voice DNS resolution failed");
            }
            log_ws_stage_ok(kind, "dns", &host, port, elapsed_ms, None);
            list
        }
        Ok(Err(err)) => {
            log_ws_stage_err(
                kind,
                "dns",
                &host,
                port,
                dns_start.elapsed().as_millis() as u64,
                &format!("io:{:?}", err.kind()),
            );
            return Err("Voice DNS resolution failed");
        }
        Err(_) => {
            log_ws_stage_err(
                kind,
                "dns",
                &host,
                port,
                dns_start.elapsed().as_millis() as u64,
                "timed_out",
            );
            return Err("Voice connection timed out");
        }
    };

    let tcp_start = Instant::now();
    let tcp = match timeout(Duration::from_secs(10), TcpStream::connect(&addrs[..])).await {
        Ok(Ok(stream)) => {
            let _ = stream.set_nodelay(true);
            log_ws_stage_ok(
                kind,
                "tcp",
                &host,
                port,
                tcp_start.elapsed().as_millis() as u64,
                None,
            );
            stream
        }
        Ok(Err(err)) => {
            log_ws_stage_err(
                kind,
                "tcp",
                &host,
                port,
                tcp_start.elapsed().as_millis() as u64,
                &format!("io:{:?}", err.kind()),
            );
            return Err("Voice TLS connection failed");
        }
        Err(_) => {
            log_ws_stage_err(
                kind,
                "tcp",
                &host,
                port,
                tcp_start.elapsed().as_millis() as u64,
                "timed_out",
            );
            return Err("Voice connection timed out");
        }
    };

    let mut request = url
        .into_client_request()
        .map_err(|_| "Invalid Discord voice endpoint")?;
    let headers = request.headers_mut();
    headers.insert(
        "Origin",
        HeaderValue::from_static(dc_proto::pin::WEBAPP_ORIGIN),
    );
    headers.insert(
        "User-Agent",
        HeaderValue::from_static(dc_proto::pin::USER_AGENT),
    );
    let config = WebSocketConfig::default()
        .max_message_size(Some(MAX_SIGNAL))
        .max_frame_size(Some(MAX_SIGNAL))
        .write_buffer_size(0)
        .max_write_buffer_size(MAX_SIGNAL * 2);

    let tls_start = Instant::now();
    let (ws, _) = match timeout(
        Duration::from_secs(12),
        tokio_tungstenite::client_async_tls_with_config(request, tcp, Some(config), None),
    )
    .await
    {
        Ok(Ok(pair)) => pair,
        Ok(Err(err)) => {
            log_ws_stage_err(
                kind,
                "tls_ws",
                &host,
                port,
                tls_start.elapsed().as_millis() as u64,
                &sanitized_ws_error(&err),
            );
            return Err("Voice TLS connection failed");
        }
        Err(_) => {
            log_ws_stage_err(
                kind,
                "tls_ws",
                &host,
                port,
                tls_start.elapsed().as_millis() as u64,
                "timed_out",
            );
            return Err("Voice connection timed out");
        }
    };

    let tls_version = match ws.get_ref() {
        MaybeTlsStream::Rustls(tls) => match tls.get_ref().1.protocol_version() {
            Some(rustls::ProtocolVersion::TLSv1_3) => Some("TLSv1.3"),
            Some(rustls::ProtocolVersion::TLSv1_2) => Some("TLSv1.2"),
            Some(_) => Some("TLS"),
            None => None,
        },
        _ => None,
    };
    log_ws_stage_ok(
        kind,
        "tls_ws",
        &host,
        port,
        tls_start.elapsed().as_millis() as u64,
        tls_version,
    );
    Ok(OpenedSocket {
        ws,
        host,
        port,
        tls_version,
        opened_at,
    })
}

/// Media datagrams are lossy by design: a full buffer or a stray ICMP unreachable drops
/// one packet. Never wait for socket capacity, that would stall audio and signaling.
#[derive(Default)]
pub(crate) struct UdpFailures {
    since: Option<Instant>,
    consecutive: u32,
    pub sent_packets: u64,
    pub sent_bytes: u64,
}
impl UdpFailures {
    /// Best-effort datagram send (RTCP feedback, keepalives, discovery retransmits): never kills
    /// the call on transient routing/interface errors.
    pub(crate) fn send_best_effort(&mut self, socket: &UdpSocket, data: &[u8]) -> bool {
        if socket.try_send(data).is_ok() {
            self.since = None;
            self.consecutive = 0;
            self.sent_packets += 1;
            self.sent_bytes += data.len() as u64;
            true
        } else {
            false
        }
    }

    pub(crate) fn send(
        &mut self,
        socket: &UdpSocket,
        data: &[u8],
        outage: &'static str,
    ) -> Result<(), &'static str> {
        if self.send_best_effort(socket, data) {
            return Ok(());
        }
        let now = Instant::now();
        self.consecutive = self.consecutive.saturating_add(1);
        let started = *self.since.get_or_insert(now);
        if self.consecutive >= UDP_MIN_FAILURES && now.duration_since(started) >= UDP_OUTAGE {
            return Err(outage);
        }
        Ok(())
    }

    pub(crate) fn reset(&mut self) {
        self.since = None;
        self.consecutive = 0;
    }
}

/// Receive errors that describe one lost or rejected datagram or transient network blip rather than a dead socket.
pub(crate) fn transient_receive(error: &std::io::Error) -> bool {
    // Windows socket codes that fire during brief Wi-Fi/router/VPN blips or oversized datagrams.
    const WSAEMSGSIZE: i32 = 10040;
    const WSAENETDOWN: i32 = 10050;
    const WSAENETUNREACH: i32 = 10051;
    const WSAECONNABORTED: i32 = 10053;
    const WSAEHOSTUNREACH: i32 = 10065;
    matches!(
        error.kind(),
        std::io::ErrorKind::ConnectionReset
            | std::io::ErrorKind::ConnectionRefused
            | std::io::ErrorKind::ConnectionAborted
            | std::io::ErrorKind::Interrupted
            | std::io::ErrorKind::WouldBlock
            | std::io::ErrorKind::NetworkUnreachable
            | std::io::ErrorKind::HostUnreachable
            | std::io::ErrorKind::NetworkDown
    ) || (cfg!(windows)
        && matches!(
            error.raw_os_error(),
            Some(WSAEMSGSIZE | WSAENETDOWN | WSAENETUNREACH | WSAECONNABORTED | WSAEHOSTUNREACH)
        ))
}

fn negotiation_timeout(
    hello: bool,
    transport: bool,
    key: bool,
    dave: &Dave,
    resuming: bool,
) -> &'static str {
    if resuming {
        "Discord voice resume acknowledgement timed out; rejoin the call"
    } else if !hello {
        "Discord voice Hello timed out; rejoin the call"
    } else if !transport {
        "Discord voice Ready timed out; rejoin the call"
    } else if !key {
        "Discord voice protocol selection timed out; no transport key was received"
    } else if dave.session.is_ready() && dave.transitioning() {
        "Discord DAVE transition execution timed out; no audio was enabled"
    } else {
        "Discord DAVE group negotiation timed out; no accepted commit or welcome was received"
    }
}

/// The official client's default: a voice channel's 64 kb/s, variable, speech-tuned, with in-band FEC. Serein's profile
/// (dual-mono stereo at 510 kb/s CBR) sent packets the other members never played: the voice server forwards a
/// channel's bitrate, not five times it. Mono capture is still sent as dual-mono stereo, as the SDP asks.
/// `packetLossRate` of `getConnectionTransportOptions` (module 31595), as a percentage.
const EXPECTED_LOSS_PERCENT: i32 = 30;

fn encoder() -> Result<Encoder, &'static str> {
    let fail = |_| "Opus encoder configuration failed";
    let mut encoder = Encoder::new(48_000, Channels::Stereo, Application::Voip).map_err(fail)?;
    encoder.set_bitrate(Bitrate::Bits(64_000)).map_err(fail)?;
    encoder.set_vbr(true).map_err(fail)?;
    encoder.set_complexity(10).map_err(fail)?;
    encoder.set_signal(opus2::Signal::Voice).map_err(fail)?;
    encoder.set_inband_fec(true).map_err(fail)?;
    // `packetLossRate: .3` of the transport options (module 31595): the encoder plans its in-band FEC for 30 % loss.
    encoder
        .set_packet_loss_perc(EXPECTED_LOSS_PERCENT)
        .map_err(fail)?;
    encoder.set_dtx(false).map_err(fail)?;
    Ok(encoder)
}

/// While a call is up, global attenuation may hold other applications down; this is its end. Dropping it (the call
/// ended, an error, an abort) puts them back.
struct Presence;

impl Presence {
    fn begin() -> Self {
        dc_winaudio::attenuation().set_in_call(true);
        Self
    }
}

impl Drop for Presence {
    fn drop(&mut self) {
        dc_winaudio::attenuation().set_in_call(false);
    }
}

pub(crate) fn emit(events: &mpsc::UnboundedSender<Event>, event: Event) {
    // A dropped receiver is the app not listening; the call itself is ended through Controls.
    let _ = events.send(event);
}

struct SessionGuard {
    playout: Arc<CallPlayout>,
    rtc_stats: Arc<RtcStatsStore>,
}

impl Drop for SessionGuard {
    fn drop(&mut self) {
        self.playout.clear();
        self.rtc_stats
            .update(|snap| *snap = RtcStatsSnapshot::default());
    }
}

pub(crate) async fn run(
    credentials: Credentials,
    mut controls: watch::Receiver<ControlState>,
    events: mpsc::UnboundedSender<Event>,
    shared: Arc<Shared>,
) -> Result<DisconnectReason, &'static str> {
    #[cfg(feature = "sim")]
    let _live = crate::sim::Live::call();
    let url = signal::endpoint_url(&credentials.endpoint)?;
    if controls.borrow_and_update().disconnect {
        return Ok(DisconnectReason::Requested);
    }
    let mut system_seen = controls.borrow().system_seq;
    emit(&events, Event::State(RtcState::Connecting));
    let playout = Arc::clone(&shared.playout);
    let _guard = SessionGuard {
        playout: Arc::clone(&playout),
        rtc_stats: Arc::clone(&shared.rtc_stats),
    };
    playout.clear();
    playout.sync_user_mix_table(&shared.user_mix.lock().unwrap_or_else(|e| e.into_inner()));
    let own = credentials.user_id;
    let opened = open_with_stages("call", &url).await?;
    let mut ws: Option<Socket> = Some(opened.ws);
    let mut diag_host = opened.host;
    let mut diag_port = opened.port;
    let mut tls_version = opened.tls_version;
    let mut connect_started = opened.opened_at;
    // Only voice-scoped credentials go to this validated endpoint.
    text(
        ws.as_mut().expect("fresh voice socket"),
        signal::identify(&credentials.ids(), Context::Call),
    )
    .await?;
    emit(&events, Event::State(RtcState::Authenticating));

    let mut dave = Dave::new(own, credentials.channel_id)?;
    let mut mode: Option<Mode> = None;
    let mut transport: Option<Transport> = None;
    let mut secured_at: Option<Instant> = None;
    // Where packets go, for the debug log every five seconds: received, opened by the transport, from a known user,
    // decrypted by DAVE; and sent.
    let mut tally = [0u32; 5];
    let mut tally_at = Instant::now() + Duration::from_secs(5);
    let mut udp: Option<UdpSocket> = None;
    let mut udp_peer: Option<SocketAddr> = None;
    // Declared after `udp`, so the flow is taken out of qWAVE before the socket closes.
    let mut qos_flow: Option<dc_winaudio::QosFlow> = None;
    let mut qos_wanted: Option<bool> = None;
    let mut presence: Option<Presence> = None;
    let mut attenuating = (false, false);
    let mut input_reported: Option<bool> = None;
    let mut next_udp_ping = Instant::now();
    let mut udp_ping_sequence = 0u32;
    let mut udp_failures = UdpFailures::default();
    let mut discovering = false;
    let mut discovery_started = Instant::now();
    let mut discovery_retry_at = Instant::now();
    let mut discovery_deadline = Instant::now();
    let mut routing_time_ms: Option<u64> = None;
    let mut ssrc = 0u32;
    let mut media_session_id: Option<String> = None;
    let mut ping_ms: Option<u64> = None;
    let mut outbound_loss_percent = 0.0f32;
    let mut inbound_packets = 0u64;
    let mut inbound_bytes = 0u64;
    let mut inbound_audio_packets: HashMap<u32, u64> = HashMap::new();
    let mut inbound_audio_bytes: HashMap<u32, u64> = HashMap::new();
    let mut decryption_failures = 0u64;
    let mut outbound_audio_packets = 0u64;
    let mut outbound_audio_bytes = 0u64;
    let mut outbound_audio_frames = 0u64;
    let mut local_audio_encrypt_ok = 0u64;
    let mut seq_ack: i64 = -1;
    let mut server_version = 0u64;
    let mut heartbeat_ms: Option<u64> = None;
    let mut heartbeat_at = Instant::now();
    let mut hello_deadline = Instant::now() + HELLO_TIMEOUT;
    let mut awaiting_ack = None;
    let mut heartbeat_nonce = 0u64;
    let mut heartbeat_sent = Instant::now();
    // `expeditedHeartbeatTimeout`: the deadline of a heartbeat sent at once for a system event.
    let mut expedited: Option<Instant> = None;
    let mut last_inbound_at = Instant::now();
    // Set when the socket died or went silent; schedules a non-blocking in-place resume (`921747.js`).
    let mut reconnect: Option<&'static str> = None;
    let mut ws_resume_task: Option<tokio::task::JoinHandle<Result<OpenedSocket, &'static str>>> =
        None;
    let mut ws_resume_at: Option<Instant> = None;
    let mut last_healthy_at = Instant::now();
    let mut deadline = Some(Instant::now() + NEGOTIATION);
    let mut ready_announced = false;
    let mut waiting_announced = false;
    let mut connected = false;
    let mut resuming = false;
    let mut resume_attempts = 0u8;
    let mut speaking = false;
    let mut silence = 0u8;

    let mut encoder = encoder()?;
    let mut sequence: u16 = rand::random();
    let mut timestamp: u32 = rand::random();
    let mut packet = [0u8; MAX_PACKET + 1];
    let mut encoded = [0u8; 1275];
    let mut stereo = [0.0f32; 1920];
    let mut soft_clip = SoftClip::new(Channels::Stereo);
    let mut ticks = clock::ticks()?;
    // The pacer wakes on a high-resolution clock: tokio's timer would round each wait up to the system tick.
    let (pacer_clock, mut pacer_wakes) = clock::PacerClock::start()?;
    // The camera's encoded pictures that found the channel full: the chain needs a keyframe.
    let camera_overflow = Arc::new(AtomicBool::new(false));
    let camera_dims = Arc::new((AtomicU32::new(0), AtomicU32::new(0)));
    let camera_frames_captured = Arc::new(AtomicU64::new(0));
    let mut signal_window = Instant::now();
    let mut signal_count = 0u16;

    // Devices open only once the call is secured (or waiting alone), never before.
    let (capture_send, capture) = std_mpsc::sync_channel::<Frame>(8);
    let mut capture_sender_end = Some(capture_send);
    let mut audio: Option<Audio> = None;
    let mut capture_pacer = CapturePacer::default();
    let mut capture_at = Instant::now();
    let mut capture_enabled = false;
    let mut capture_reset = true;
    let mut was_playout_active = false;
    let mut local_activity = 0u8;
    let mut speakers_at = Instant::now();
    let mut speaking_buf: Vec<u64> = Vec::with_capacity(crate::dave::MAX_PARTICIPANTS);
    let mut last_speakers = BTreeSet::new();
    let mut clients = BTreeSet::new();
    let mut stats_at = Instant::now() + Duration::from_millis(250);
    let mut fps_at = Instant::now() + Duration::from_secs(1);
    let mut last_fps_at = Instant::now();
    let mut prev_camera_frames = 0u64;
    let mut camera_fps = 0u32;
    let mut prev_encoded_frames = 0u64;
    let mut encoded_fps = 0u32;
    let mut prev_decoded_frames: HashMap<u64, u64> = HashMap::new();
    let mut decoded_fps: HashMap<u64, u32> = HashMap::new();

    // Video: what arrives from the others, and our camera.
    let hub = shared.hub.clone();
    let mut receivers = Receivers::default();
    let mut decoders = Decoders::new(hub.clone(), |_| VideoSource::Camera);
    let mut wants_video = false;
    let mut wants_video_at = Instant::now();
    let mut video_request_seen = 0u32;
    let mut camera_wish: Option<String> = None;
    let mut camera: Option<Camera> = None;
    let mut camera_task: Option<tokio::task::JoinHandle<Result<Camera, String>>> = None;
    let (camera_send, mut camera_frames) = mpsc::channel::<EncodedFrame>(4);
    let mut video_sender: Option<VideoSender> = None;
    let mut video_ssrc = 0u32;
    let mut rtx_ssrc = 0u32;
    let mut video_announced = false;
    let mut secure = false;
    let mut was_secure = false;
    let mut camera_sent_keyframe = false;
    // The server's keyframe interval (op 4 or 14), for the camera sender.
    let mut keyframe_interval = 0u64;
    let mut video_tally = [0u32; 3];

    loop {
        if let Some(reason) = reconnect.take() {
            ws = None;
            if let Some(task) = ws_resume_task.take() {
                task.abort();
            }
            // `handleClose`: past `backoff.fails > 3` the socket gives up; the resume window is checked once the new
            // socket opens (`doResumeOrClose`).
            if transport.is_none() || resume_attempts >= MAX_RESUMES {
                return Err(reason);
            }
            let delay = ws_resume_backoff(resume_attempts);
            resume_attempts += 1;
            debug!(
                attempt = resume_attempts,
                delay_ms = delay.as_millis() as u64,
                "voice socket lost; scheduling in-place resume while keeping media alive"
            );
            resuming = true;
            ws_resume_at = Some(Instant::now() + delay);
            deadline = Some(Instant::now() + RENEGOTIATION + delay);
            heartbeat_ms = None;
            awaiting_ack = None;
            expedited = None;
        }
        pacer_clock.set(
            video_sender
                .as_ref()
                .and_then(VideoSender::deadline)
                .map(Instant::into_std),
        );
        tokio::select! {
            changed = controls.changed() => {
                if changed.is_err() || controls.borrow().disconnect {
                    if let Some(task) = ws_resume_task.take() {
                        task.abort();
                    }
                    log_ws_close("call", &diag_host, diag_port, connect_started.elapsed().as_millis() as u64, 1000, 0);
                    if let Some(sock) = ws.as_mut() {
                        let _ = timeout(Duration::from_secs(1), sock.close(None)).await;
                    }
                    return Ok(DisconnectReason::Requested);
                }
                let request = controls.borrow().video_request;
                if request != video_request_seen {
                    video_request_seen = request;
                    camera_wish = shared.camera.lock().unwrap_or_else(|e| e.into_inner()).clone();
                }
                let (seq, system) = {
                    let control = controls.borrow();
                    (control.system_seq, control.system)
                };
                if seq != system_seen && let Some(system) = system {
                    system_seen = seq;
                    let now = Instant::now();
                    match system.expedite() {
                        Some((ms, _)) if ws.is_some() && heartbeat_ms.is_some() => {
                            debug!(?system, "performing an expedited voice heartbeat");
                            heartbeat_nonce = heartbeat_nonce.wrapping_add(1);
                            heartbeat_sent = now;
                            text_or_resume(&mut ws, &mut reconnect, transport.is_some(), signal::heartbeat(heartbeat_nonce, seq_ack, server_version)).await?;
                            awaiting_ack = Some(heartbeat_nonce);
                            expedited = Some(now + Duration::from_millis(u64::from(ms)));
                        }
                        // `resetBackoff`: with the socket gone after a failure, retry at once.
                        Some((_, true)) | None => {
                            if ws.is_none() && resume_attempts > 0 && ws_resume_task.is_none() {
                                debug!(?system, "voice connection backoff reset");
                                resume_attempts = 0;
                                ws_resume_at = Some(now);
                            }
                        }
                        Some((_, false)) => {}
                    }
                }
            },
            resumed = async {
                match ws_resume_task.as_mut() {
                    Some(task) => task.await,
                    None => std::future::pending().await,
                }
            } => {
                ws_resume_task = None;
                match resumed {
                    Ok(Ok(mut reopened)) => {
                        // `doResumeOrClose`: a session silent for longer than the window is not resumed (4801).
                        if last_healthy_at.elapsed() > RESUME_WINDOW {
                            let mut gone = Some(reopened.ws);
                            return Ok(refuse(&mut gone, close::UNRESUMABLE, "resume window").await);
                        }
                        // `_handleResuming` runs `fastUdpReconnect` on every resume.
                        if let Some(peer) = udp_peer
                            && let Some(fresh_udp) = rebind_udp(peer).await
                        {
                            debug!("voice fast UDP reconnect refreshed local socket");
                            qos_flow = None;
                            qos_wanted = None;
                            udp = Some(fresh_udp);
                            udp_failures.reset();
                        }
                        if text(&mut reopened.ws, signal::resume(&credentials.ids(), seq_ack)).await.is_ok() {
                            // `doResume` renews `lastHeartbeatAckTime`: each attempt gets the whole window.
                            last_healthy_at = Instant::now();
                            diag_host = reopened.host;
                            diag_port = reopened.port;
                            tls_version = reopened.tls_version;
                            connect_started = reopened.opened_at;
                            ws = Some(reopened.ws);
                            heartbeat_at = Instant::now();
                            hello_deadline = Instant::now() + HELLO_TIMEOUT;
                            deadline = Some(Instant::now() + RENEGOTIATION);
                        } else {
                            reconnect = Some("Voice resume failed; rejoin the call");
                        }
                    }
                    _ => {
                        reconnect = Some("Voice resume failed; rejoin the call");
                    }
                }
            },
            wake = pacer_wakes.recv() => {
                if wake.is_none() {
                    return Err("Pacer clock stopped");
                }
                if let (Some(sender), Some(crypto), Some(socket)) = (video_sender.as_mut(), transport.as_mut(), udp.as_ref()) {
                    let _ = sender.flush(Instant::now(), crypto, |data| {
                        udp_failures.send_best_effort(socket, data);
                        Ok::<(), &'static str>(())
                    });
                }
            },
            frame = camera_frames.recv(), if video_sender.as_ref().is_some_and(|sender| sender.accepts(Instant::now())) => {
                let Some(frame) = frame else { continue };
                if !secure || (!camera_sent_keyframe && !frame.keyframe) {
                    // Nothing can leave yet: ask for a keyframe to start from once it can.
                    camera_sent_keyframe = false;
                    if let Some(camera) = &camera {
                        camera.force_keyframe();
                    }
                    continue;
                }
                if let Some(sender) = video_sender.as_mut() {
                    match sender.offer(&frame, &mut dave, Instant::now()) {
                        Ok(true) => camera_sent_keyframe |= frame.keyframe,
                        Ok(false) => {
                            // The pacer's memory bound refused it: the chain is broken until a keyframe.
                            debug!("camera picture refused by the pacer; asking for a keyframe");
                            camera_sent_keyframe = false;
                            if let Some(camera) = &camera {
                                camera.force_keyframe();
                            }
                        }
                        Err(error) => {
                            // A bad picture costs that picture, never the call.
                            debug!(error, "camera picture dropped");
                            camera_sent_keyframe = false;
                            if let Some(camera) = &camera {
                                camera.force_keyframe();
                            }
                        }
                    }
                }
            },
            tick = ticks.recv() => {
                if tick.is_none() {
                    return Err("Media clock stopped");
                }
                let now = Instant::now();
                if ws.is_none()
                    && ws_resume_task.is_none()
                    && ws_resume_at.is_some_and(|at| now >= at)
                {
                    ws_resume_at = None;
                    let url_clone = url.clone();
                    ws_resume_task = Some(tokio::spawn(async move {
                        open_with_stages("call", &url_clone).await
                    }));
                }
                if deadline.is_some_and(|d| now >= d) {
                    if resuming && transport.is_some() && resume_attempts < MAX_RESUMES {
                        reconnect = Some("Discord voice resume acknowledgement timed out; rejoin the call");
                        continue;
                    }
                    return Err(negotiation_timeout(heartbeat_ms.is_some(), udp.is_some(), transport.is_some(), &dave, resuming));
                }
                if ws.is_some() && heartbeat_ms.is_none() && now >= hello_deadline {
                    if transport.is_none() {
                        return Err("Discord voice Hello timed out; rejoin the call");
                    }
                    reconnect = Some("Discord voice Hello timed out; rejoin the call");
                    continue;
                }
                if discovering {
                    if now >= discovery_deadline {
                        log_ws_stage_err(
                            "call",
                            "udp_discovery",
                            &diag_host,
                            diag_port,
                            discovery_started.elapsed().as_millis() as u64,
                            "timed_out",
                        );
                        return Err(no_route(&mut ws, &events, credentials.report_no_route, "discovery").await);
                    }
                    if now >= discovery_retry_at && let Some(socket) = &udp {
                        udp_failures.send_best_effort(socket, &rtp::discovery_request(ssrc));
                        discovery_retry_at = now + DISCOVERY_RETRY;
                    }
                }
                if !discovering && now >= next_udp_ping && let Some(socket) = &udp {
                    udp_ping_sequence = udp_ping_sequence.wrapping_add(1);
                    udp_failures.send_best_effort(socket, &rtp::keepalive(udp_ping_sequence));
                    next_udp_ping = now + UDP_KEEPALIVE;
                }
                if transport.is_some() && !discovering && udp.is_some() && now.duration_since(last_inbound_at) >= UDP_RECONNECT_INTERVAL {
                    last_inbound_at = now;
                    if let Some(peer) = udp_peer
                        && let Some(fresh_udp) = rebind_udp(peer).await
                    {
                        debug!("no voice datagram for the reconnect interval; UDP socket rebound");
                        qos_flow = None;
                        qos_wanted = None;
                        udp = Some(fresh_udp);
                        udp_failures.reset();
                    }
                }
                if ws.is_some() && let Some(interval) = heartbeat_ms {
                    match beat(now, heartbeat_at, awaiting_ack.is_some(), expedited) {
                        Beat::Wait => {}
                        Beat::Skip => heartbeat_at = now + Duration::from_millis(interval),
                        Beat::TimedOut => {
                            expedited = None;
                            reconnect = Some("Discord voice heartbeat was not acknowledged; rejoin the call");
                            continue;
                        }
                        Beat::Send => {
                            heartbeat_nonce = heartbeat_nonce.wrapping_add(1);
                            heartbeat_sent = Instant::now();
                            text_or_resume(&mut ws, &mut reconnect, transport.is_some(), signal::heartbeat(heartbeat_nonce, seq_ack, server_version)).await?;
                            awaiting_ack = Some(heartbeat_nonce);
                            heartbeat_at = now + Duration::from_millis(interval);
                        }
                    }
                }
                if secured_at.is_some_and(|at| now >= at + PEER_GRACE) && !discovering && !resuming && dave.should_wait_for_peer() {
                    dave.enter_sole_member_waiting()?;
                }
                // Media (UDP + DAVE) stays live while the control WebSocket resumes in the background (`921747.js`).
                let enabled = dave.media_ready() && transport.is_some() && !discovering;
                let waiting = dave.waiting && transport.is_some() && !discovering;
                if (!enabled && ready_announced) || (!waiting && waiting_announced) {
                    ready_announced = false;
                    waiting_announced = false;
                }
                if waiting && !waiting_announced {
                    deadline = None;
                    waiting_announced = true;
                }
                if enabled && !ready_announced {
                    deadline = None;
                    ready_announced = true;
                    if let Some(code) = dave.session.voice_privacy_code() {
                        emit(&events, Event::PrivacyCode(code.to_owned()));
                    }
                }
                if (enabled || waiting) && !connected {
                    connected = true;
                    emit(&events, Event::State(RtcState::RtcConnected));
                    emit(&events, Event::Connected);
                    presence = Some(Presence::begin());
                }
                if (enabled || waiting) && audio.is_none() && let Some(capture_send) = capture_sender_end.take() {
                    #[cfg(feature = "sim")]
                    let started = if crate::sim::active() {
                        Audio::start_simulated(capture_send, Arc::clone(&playout), credentials.session_id.to_string())
                    } else {
                        Audio::start(capture_send, Arc::clone(&playout))
                    };
                    #[cfg(not(feature = "sim"))]
                    let started = Audio::start(capture_send, Arc::clone(&playout));
                    audio = Some(started?);
                }
                // Quality of Service follows the setting while the call runs.
                let wanted = prefs().qos();
                if qos_wanted != Some(wanted) && let Some(socket) = &udp {
                    qos_wanted = Some(wanted);
                    match dc_winaudio::qos::plan(wanted, qos_flow.is_some()) {
                        dc_winaudio::Plan::Add => {
                            #[cfg(windows)]
                            {
                                use std::os::windows::io::AsRawSocket;
                                match dc_winaudio::QosFlow::voice(socket.as_raw_socket()) {
                                    Ok(flow) => {
                                        debug!("voice packets marked as high priority");
                                        qos_flow = Some(flow);
                                    }
                                    Err(error) => warn!("voice packet priority not set: {error}"),
                                }
                            }
                            #[cfg(not(windows))]
                            let _ = socket;
                        }
                        dc_winaudio::Plan::Remove => {
                            qos_flow = None;
                            debug!("voice packets back to normal priority");
                        }
                        dc_winaudio::Plan::Keep => {}
                    }
                }
                // Whether the microphone delivers any input, for the "No Audio Detected" warning.
                let detected = prefs().input_detected();
                if detected != input_reported {
                    input_reported = detected;
                    emit(&events, Event::InputDetected { detected });
                }
                let control = *controls.borrow();
                if let Some(audio) = &audio {
                    audio.set_ready(enabled || waiting);
                    audio.set_controls(control.self_mute, control.self_deaf);
                    audio.set_loopback(control.loopback);
                }
                // Preserve ordinary callback batches. Only a real stall (four packet
                // intervals) discards queued speech; mute and security gates always flush.
                let resumed_capture = enabled && (!capture_enabled || capture_reset);
                let stalled = !resumed_capture && now.duration_since(capture_at) >= Duration::from_millis(80);
                capture_at = now;
                capture_enabled = enabled;
                capture_reset = false;
                let muted = control.self_mute || control.self_deaf;
                let latest = if waiting {
                    capture_pacer.preview(&capture)
                } else {
                    capture_pacer.next(&capture, enabled && !muted, stalled)
                };
                local_activity = if (enabled || waiting) && !muted && !stalled {
                    activity::hold_at(latest.as_ref().map_or(0.0, activity::energy), local_activity, LOCAL_THRESHOLD_DB)
                } else {
                    0
                };
                let active = enabled && !muted && latest.is_some();
                if active && !speaking {
                    if !resuming {
                        text_or_resume(&mut ws, &mut reconnect, transport.is_some(), signal::speaking(true, ssrc)).await?;
                    }
                    speaking = true;
                }
                // Five frames of Opus silence close a burst, as Serein sends them.
                if !active && speaking && silence == 0 {
                    silence = 5;
                }
                let local_audio_level = if active {
                    latest.as_ref().map_or(0.0, |frame| activity::energy(frame).min(1.0))
                } else {
                    0.0
                };
                if enabled && (active || silence > 0) {
                    let data = if let Some(frame) = latest.filter(|_| active) {
                        outbound_audio_frames += 1;
                        for (sample, pair) in frame.iter().zip(stereo.as_chunks_mut::<2>().0) {
                            let sample = if sample.is_finite() { sample.clamp(-1.0, 1.0) } else { 0.0 };
                            pair[0] = sample;
                            pair[1] = sample;
                        }
                        soft_clip.apply(&mut stereo);
                        let length = encoder.encode_float(&stereo, &mut encoded).map_err(|_| "Opus encoding failed")?;
                        let encrypted = dave
                            .encrypt(davey::MediaType::AUDIO, davey::Codec::OPUS, &encoded[..length])
                            .map_err(|_| "DAVE audio encryption failed")?
                            .into_owned();
                        local_audio_encrypt_ok += 1;
                        encrypted
                    } else {
                        silence -= 1;
                        // Encode silence so Opus LBRR clears the final syllable of the finished spurt.
                        let silence_pcm = [0.0f32; rtp::FRAME_SAMPLES as usize * 2];
                        let _ = encoder.encode_float(&silence_pcm, &mut encoded);
                        davey::OPUS_SILENCE_PACKET.to_vec()
                    };
                    let header = rtp::header(sequence, timestamp, ssrc);
                    let wire = transport.as_mut().ok_or("Missing voice transport key")?.seal(&header, &data, now)?;
                    if let Some(socket) = &udp {
                        if udp_failures.send(socket, &wire, "Voice UDP send failed").is_err() {
                            if let Some(peer) = udp_peer
                                && let Some(fresh_udp) = rebind_udp(peer).await
                            {
                                debug!("voice UDP send outage triggered fast UDP reconnect");
                                qos_flow = None;
                                qos_wanted = None;
                                udp = Some(fresh_udp);
                                udp_failures.reset();
                            } else {
                                return Err("Voice UDP send failed");
                            }
                        }
                        outbound_audio_packets += 1;
                        outbound_audio_bytes += wire.len() as u64;
                        tally[4] += 1;
                    }
                    sequence = sequence.wrapping_add(1);
                    if !active && silence == 0 {
                        if !resuming {
                            text_or_resume(&mut ws, &mut reconnect, transport.is_some(), signal::speaking(false, ssrc)).await?;
                        }
                        speaking = false;
                    }
                    if active {
                        silence = 0;
                    }
                }
                timestamp = timestamp.wrapping_add(rtp::FRAME_SAMPLES);
                let playout_active = enabled && !control.self_deaf;
                if was_playout_active && !playout_active {
                    playout.clear();
                    playout.clear_all_streams();
                    hub.clear_stream_audio();
                }
                was_playout_active = playout_active;
                // Combined voice track for the clip buffer (off when no live stream tap is attached): local mic plus consented remote speakers.
                let local_clips = latest.filter(|_| enabled && active);
                let remote_clips = if playout_active {
                    playout.pop_clips_20ms()
                } else {
                    playout.clear_clips_tap();
                    None
                };
                match (local_clips, remote_clips) {
                    (Some(mut mixed), Some(remote)) => {
                        for (sample, r) in mixed.iter_mut().zip(remote.iter()) {
                            *sample = (*sample + *r).clamp(-1.0, 1.0);
                        }
                        dc_clips::recorder().push_voice(&mixed);
                    }
                    (Some(local), None) => dc_clips::recorder().push_voice(&local),
                    (None, Some(remote)) => dc_clips::recorder().push_voice(&remote),
                    (None, None) => {}
                }
                // Video never gates audio: every step here fails alone.
                secure = enabled && dave.media_secure();
                if !secure {
                    if let Some(sender) = video_sender.as_mut() {
                        sender.clear();
                    }
                    camera_sent_keyframe = false;
                }
                let wants = hub.has_sink() && server_version >= 5 && transport.is_some() && !discovering && !resuming;
                if ws.is_some() && !resuming && (wants != wants_video || (wants && now >= wants_video_at)) {
                    wants_video = wants;
                    wants_video_at = now + SINK_WANTS_INTERVAL;
                    text_or_resume(&mut ws, &mut reconnect, transport.is_some(), signal::media_sink_wants(if wants { 100 } else { 0 })).await?;
                }
                if secure {
                    // Repairs that expired since the last packet; arrivals are handed over as they come.
                    video_tally[2] += receive::hand_over(&mut receivers, &mut decoders, &mut dave, now);
                    if let (Some(crypto), Some(socket)) = (transport.as_mut(), udp.as_ref()) {
                        for (header, body) in receivers.feedback(ssrc, now) {
                            // Feedback is best effort: a failed send is only a lost datagram.
                            if let Ok(wire) = crypto.seal_rtcp(&header, &body)
                                && socket.try_send(&wire).is_ok()
                            {
                                udp_failures.sent_packets += 1;
                                udp_failures.sent_bytes += wire.len() as u64;
                            }
                        }
                    }
                }
                // Our camera: start, announce, stop.
                if let Some(device) = camera_wish.clone() {
                    if camera.is_none() && camera_task.is_none() && transport.is_some() && !discovering && !resuming {
                        let (frames, hub, own, overflow, dims, frames_count) = (
                            camera_send.clone(),
                            hub.clone(),
                            own,
                            camera_overflow.clone(),
                            Arc::clone(&camera_dims),
                            Arc::clone(&camera_frames_captured),
                        );
                        camera_task = Some(tokio::task::spawn_blocking(move || {
                            let device = (!device.is_empty()).then_some(device);
                            Camera::start(device.as_deref(), move |frame: CameraFrame| {
                                dims.0.store(frame.preview.width, Ordering::Relaxed);
                                dims.1.store(frame.preview.height, Ordering::Relaxed);
                                frames_count.fetch_add(1, Ordering::Relaxed);
                                hub.deliver(RemoteFrame { user: own, source: VideoSource::Camera, picture: frame.preview });
                                if let Some(encoded) = frame.encoded
                                    && frames.try_send(encoded).is_err()
                                {
                                    overflow.store(true, Ordering::Release);
                                }
                            })
                            .map_err(|error| error.to_string())
                        }));
                    }
                    if camera_task.as_ref().is_some_and(|task| task.is_finished()) {
                        let task = camera_task.take().ok_or("Camera task missing")?;
                        match task.await {
                            Ok(Ok(started)) => {
                                camera = Some(started);
                                emit(&events, Event::VideoState { camera: true });
                            }
                            other => {
                                let reason = match other {
                                    Ok(Err(reason)) => reason,
                                    _ => "the camera task failed".to_owned(),
                                };
                                emit(&events, Event::CameraFailed { reason });
                                emit(&events, Event::VideoState { camera: false });
                                *shared.camera.lock().unwrap_or_else(|e| e.into_inner()) = None;
                                camera_wish = None;
                            }
                        }
                    }
                    if let Some(started) = &camera && started.finished() {
                        let reason = started.error().unwrap_or_else(|| "the camera stopped".to_owned());
                        emit(&events, Event::CameraFailed { reason });
                        *shared.camera.lock().unwrap_or_else(|e| e.into_inner()) = None;
                        camera_wish = None;
                    }
                }
                if camera_wish.is_none() && (camera.is_some() || video_announced) {
                    camera = None;
                    video_sender = None;
                    camera_sent_keyframe = false;
                    camera_dims.0.store(0, Ordering::Relaxed);
                    camera_dims.1.store(0, Ordering::Relaxed);
                    while camera_frames.try_recv().is_ok() {}
                    if video_announced {
                        if !resuming {
                            text_or_resume(&mut ws, &mut reconnect, transport.is_some(), signal::video(ssrc, None)).await?;
                        }
                        video_announced = false;
                    }
                    emit(&events, Event::VideoState { camera: false });
                }
                if camera.is_some() && !video_announced && ws.is_some() && transport.is_some() && !discovering && !resuming {
                    let mut sender = VideoSender::new(video_ssrc, rtx_ssrc, CAMERA_MIN_BITRATE, CALL_START_BITRATE, CAMERA_MAX_BITRATE);
                    sender.set_framerate(camera::FPS);
                    sender.set_keyframe_interval(keyframe_interval);
                    video_sender = Some(sender);
                    text_or_resume(&mut ws, &mut reconnect, transport.is_some(), signal::video(ssrc, Some((video_ssrc, rtx_ssrc, Context::Call, None)))).await?;
                    video_announced = true;
                    debug!("camera announced");
                }
                if let Some(sender) = video_sender.as_mut() {
                    if sender.stale(now) {
                        sender.clear();
                        camera_sent_keyframe = false;
                    }
                    if camera_overflow.swap(false, Ordering::AcqRel) {
                        // An encoded picture found the channel full and is gone: restart from a keyframe.
                        debug!("camera picture queue full; asking for a keyframe");
                        camera_sent_keyframe = false;
                        if let Some(camera) = &camera {
                            camera.force_keyframe();
                        }
                    }
                    // The encoder follows the target, within what its picture size is given.
                    if let Some(bitrate) = sender.tick_rate(now) {
                        debug!(bitrate, "camera send rate steered");
                        if let Some(started) = &camera {
                            started.set_bitrate(bitrate);
                        }
                    }
                    if secure && sender.periodic_keyframe_due(now) && let Some(started) = &camera {
                        started.force_keyframe();
                    }
                }
                if secure && !was_secure && let Some(started) = &camera {
                    started.force_keyframe();
                }
                was_secure = secure;
                if now >= tally_at {
                    let (numbered, unnumbered) = receivers.transport_counts();
                    debug!(
                        video_packets = video_tally[0],
                        video_pictures = video_tally[2],
                        camera = camera.is_some(),
                        transport_cc_packets = numbered,
                        without_transport_cc = unnumbered,
                        "voice video tally"
                    );
                    if let Some(sender) = video_sender.as_ref() {
                        let estimation = sender.estimation(transport.as_ref());
                        debug!(
                            bitrate = sender.bitrate(),
                            delay_estimate = estimation.delay_estimate,
                            acknowledged = estimation.acknowledged,
                            overusing = estimation.overusing,
                            overuses = estimation.overuses,
                            feedback_reports = estimation.feedback_reports,
                            packets_acked = estimation.packets_acked,
                            packets_lost = estimation.packets_lost,
                            packets_unknown = estimation.packets_unknown,
                            probe_clusters_sent = estimation.probe_clusters_sent,
                            probe_clusters_dropped = estimation.probe_clusters_dropped,
                            probe_results = estimation.probe_results,
                            probe_refused = estimation.probe_refused,
                            last_probe = estimation.last_probe,
                            application_limited = estimation.application_limited,
                            padding_packets = estimation.padding_packets,
                            transport_cc = estimation.transport_cc,
                            receiver_reports = estimation.receiver_reports,
                            rembs = estimation.rembs,
                            fallback = estimation.fallback,
                            target = estimation.target,
                            encoder_rate = estimation.encoder_rate,
                            dropped_pictures = estimation.dropped_pictures,
                            "camera send-side estimation"
                        );
                    }
                    video_tally = [0; 3];
                    debug!(
                        received = tally[0],
                        opened = tally[1],
                        known = tally[2],
                        decrypted = tally[3],
                        sent = tally[4],
                        ready = dave.ready,
                        "voice packet tally"
                    );
                    tally = [0; 5];
                    let playout_snap = playout.stats_snapshot();
                    let receive = &playout_snap.voice_report;
                    debug!(
                        speakers = receive.speakers,
                        target_delay_ms = receive.target_delay_ms,
                        buffer_level_ms = receive.buffer_level_ms,
                        packets = receive.since.packets_received,
                        lost = receive.since.packets_lost,
                        late = receive.since.packets_late,
                        fec_recovered = receive.since.fec_recovered,
                        flushes = receive.since.flushes,
                        concealed_ms = receive.since.concealed_samples / 48,
                        concealment_events = receive.since.concealment_events,
                        accelerated_ms = receive.since.accelerated_samples / 48,
                        expanded_ms = receive.since.expanded_samples / 48,
                        underrun_ms = receive.since.underrun_samples / 48,
                        underruns = playout_snap.underruns,
                        dropped_ingress = playout_snap.dropped_ingress,
                        "voice receive"
                    );
                    tally_at = now + Duration::from_secs(5);
                }
                if now >= speakers_at {
                    playout.speaking_users(&mut speaking_buf);
                    let mut current: BTreeSet<u64> = speaking_buf.iter().copied().collect();
                    if local_activity > 0 {
                        current.insert(own);
                    }
                    for user in last_speakers.difference(&current) {
                        emit(&events, Event::SpeakingChanged { user_id: *user, speaking: false });
                    }
                    for user in current.difference(&last_speakers) {
                        emit(&events, Event::SpeakingChanged { user_id: *user, speaking: true });
                    }
                    // Global attenuation follows who speaks: this client, and anyone else.
                    let now_attenuating = (current.contains(&own), current.iter().any(|user| *user != own));
                    if presence.is_some() && now_attenuating != attenuating {
                        attenuating = now_attenuating;
                        dc_winaudio::attenuation().set_speaking(attenuating.0, attenuating.1);
                    }
                    last_speakers = current;
                    speakers_at = now + Duration::from_millis(100);
                }
                if now >= stats_at {
                    let playout_snap = playout.stats_snapshot();
                    let cam_captured = camera_frames_captured.load(Ordering::Relaxed);
                    let cam_w = camera_dims.0.load(Ordering::Relaxed);
                    let cam_h = camera_dims.1.load(Ordering::Relaxed);
                    let video_snap = video_sender.as_ref().map(VideoSender::stats_snapshot);
                    let video_sources = receivers.video_sources_snapshot();
                    let sf_version = if dave.ready || dave.unencrypted() {
                        u32::from(dave.protocol())
                    } else {
                        0
                    };
                    if now >= fps_at {
                        let secs = now.saturating_duration_since(last_fps_at).as_secs_f32().max(0.25);
                        camera_fps = ((cam_captured.saturating_sub(prev_camera_frames)) as f32 / secs).round() as u32;
                        prev_camera_frames = cam_captured;
                        let total_enc = video_snap.as_ref().map_or(0, |v| v.frames_encoded);
                        encoded_fps = ((total_enc.saturating_sub(prev_encoded_frames)) as f32 / secs).round() as u32;
                        prev_encoded_frames = total_enc;
                        for src in &video_sources {
                            if let Some(dec) = decoders.video_stats(src.user) {
                                let prev = prev_decoded_frames.insert(src.user, dec.frames_decoded).unwrap_or(dec.frames_decoded);
                                let fps = ((dec.frames_decoded.saturating_sub(prev)) as f32 / secs).round() as u32;
                                decoded_fps.insert(src.user, fps);
                            }
                        }
                        last_fps_at = now;
                        fps_at = now + Duration::from_secs(1);
                    }

                    let est = video_sender.as_ref().map(|s| s.estimation(transport.as_ref()));
                    let avail_bitrate = est.as_ref().map_or(64_000, |e| e.target);
                    let delay_ms = est.as_ref().map_or(0, |e| e.delay_estimate);

                    let mut inbound: HashMap<u64, InboundUserStats> = HashMap::new();
                    for spk in &playout_snap.voice_speakers {
                        inbound.entry(spk.user).or_default().audio = Some(InboundAudioStats {
                            ssrc: spk.ssrc,
                            codec: "opus",
                            payload_type: rtp::OPUS_PAYLOAD_TYPE,
                            bytes_received: inbound_audio_bytes.get(&spk.ssrc).copied().unwrap_or(0),
                            packets_received: inbound_audio_packets
                                .get(&spk.ssrc)
                                .copied()
                                .unwrap_or(spk.stats.packets_received),
                            packets_lost: spk.stats.packets_lost,
                            packets_late: spk.stats.packets_late,
                            packets_discarded: spk.packets_discarded,
                            fec_recovered: spk.stats.fec_recovered,
                            jitter_ms: receivers.audio_jitter_ms(spk.ssrc),
                            jitter_buffer_ms: spk.jitter_buffer_ms as u32,
                            jitter_buffer_preferred_ms: spk.target_delay_ms,
                            audio_level: spk.audio_level,
                            decoding_muted_output: spk.decoding_muted_output,
                            concealed_samples: spk.stats.concealed_samples,
                            concealment_events: spk.stats.concealment_events,
                            accelerated_samples: spk.stats.accelerated_samples,
                            expanded_samples: spk.stats.expanded_samples,
                            underrun_samples: spk.stats.underrun_samples,
                            flushes: spk.stats.flushes,
                            op_normal: spk.stats.op_normal,
                            op_expand: spk.stats.op_expand,
                            op_accelerate: spk.stats.op_accelerate,
                            op_preemptive_expand: spk.stats.op_preemptive_expand,
                            op_cng: spk.stats.op_cng,
                            secure_frames_protocol_version: sf_version,
                        });
                    }
                    for src in &video_sources {
                        let dec = decoders.video_stats(src.user);
                        inbound.entry(src.user).or_default().video = Some(InboundVideoStats {
                            ssrc: src.ssrc,
                            rtx_ssrc: src.rtx_ssrc,
                            codec: "H264",
                            payload_type: rtp::H264_PAYLOAD_TYPE,
                            width: dec.as_ref().map_or(0, |d| d.width),
                            height: dec.as_ref().map_or(0, |d| d.height),
                            bytes_received: src.bytes_received,
                            packets_received: src.packets_received,
                            packets_lost: src.packets_lost,
                            frames_received: dec.as_ref().map_or(0, |d| d.frames_received),
                            frames_decoded: dec.as_ref().map_or(0, |d| d.frames_decoded),
                            key_frames_decoded: dec.as_ref().map_or(0, |d| d.keyframes_decoded),
                            fps_decoded: decoded_fps.get(&src.user).copied().unwrap_or(0),
                            jitter_ms: src.jitter_ms,
                            nacks: src.nacks,
                            plis: src.plis,
                            firs: 0,
                            freeze_count: 0,
                            total_freezes_duration_ms: 0,
                            decoder_implementation_name: dec.and_then(|d| d.decoder_name),
                            secure_frames_protocol_version: sf_version,
                        });
                    }

                    let out_audio = (ssrc != 0).then_some(OutboundAudioStats {
                        ssrc,
                        codec: "opus",
                        payload_type: rtp::OPUS_PAYLOAD_TYPE,
                        bitrate: 64_000,
                        bytes_sent: outbound_audio_bytes,
                        packets_sent: outbound_audio_packets,
                        packets_lost: 0,
                        fraction_lost: (outbound_loss_percent / 100.0).clamp(0.0, 1.0),
                        frames_captured: outbound_audio_frames,
                        audio_level: local_audio_level,
                        audio_detected: input_reported,
                        encrypt_success_count: local_audio_encrypt_ok,
                        encrypt_failure_count: 0,
                        secure_frames_protocol_version: sf_version,
                    });
                    let out_video = video_snap.as_ref().map(|vs| OutboundVideoStats {
                        ssrc: vs.video_ssrc,
                        rtx_ssrc: vs.rtx_ssrc,
                        codec: "H264",
                        payload_type: rtp::H264_PAYLOAD_TYPE,
                        bitrate_target: avail_bitrate,
                        bitrate_actual: est.as_ref().map_or(0, |e| e.encoder_rate),
                        bytes_sent: vs.bytes_sent,
                        packets_sent: vs.packets_sent,
                        packets_lost: 0,
                        width: cam_w,
                        height: cam_h,
                        fps_encoded: encoded_fps,
                        frames_encoded: vs.frames_encoded,
                        frames_dropped: est.as_ref().map_or(0, |e| e.dropped_pictures),
                        key_frames_encoded: vs.keyframes_encoded,
                        qp_sum: 0,
                        nacks: vs.nacks,
                        plis: vs.plis,
                        firs: 0,
                        encoder_implementation_name: Some("OpenH264".into()),
                        average_encode_time_ms: 0.0,
                        encrypt_success_count: vs.frames_encoded,
                        encrypt_failure_count: 0,
                        secure_frames_protocol_version: sf_version,
                    });
                    let camera_stat = camera.as_ref().map(|_| CameraStats {
                        device_name: camera_wish
                            .as_deref()
                            .filter(|s| !s.is_empty())
                            .map(str::to_owned),
                        width: cam_w,
                        height: cam_h,
                        fps: camera_fps,
                        frames_captured: cam_captured,
                        frames_dropped: 0,
                        captured: true,
                        encoded: video_snap.is_some(),
                    });
                    let local_address = udp
                        .as_ref()
                        .and_then(|s| s.local_addr().ok())
                        .map(|a| a.to_string());
                    let mode_str = transport.as_ref().map(|t| t.mode_name().to_string());

                    shared.rtc_stats.update(|snap| {
                        snap.connected = enabled || waiting;
                        snap.transport = TransportStats {
                            hostname: Some(diag_host.clone()),
                            local_address,
                            media_session_id: media_session_id.clone(),
                            ping_ms,
                            available_outgoing_bitrate: avail_bitrate,
                            outbound_bitrate_estimate: avail_bitrate,
                            inbound_bitrate_estimate: 0,
                            outbound_bytes: udp_failures.sent_bytes,
                            inbound_bytes,
                            outbound_packets: udp_failures.sent_packets,
                            inbound_packets,
                            delay_estimate_ms: delay_ms,
                            pacer_delay_ms: 0,
                            decryption_failures,
                            routing_time_ms,
                            tls_version: tls_version.map(str::to_string),
                            mode: mode_str,
                            secure_frames_protocol_version: sf_version,
                            output_device_underruns: playout_snap.underruns,
                            receiver_reports: Vec::new(),
                        };
                        snap.outbound = OutboundStats {
                            audio: out_audio,
                            video: out_video,
                        };
                        snap.camera = camera_stat;
                        snap.inbound = inbound;
                    });
                    stats_at = now + Duration::from_millis(250);
                }
            },
            result = async { match &udp { Some(socket) => socket.recv(&mut packet).await, None => std::future::pending().await } } => {
                let length = match result {
                    Ok(length) => length,
                    Err(error) if transient_receive(&error) => continue,
                    Err(_) => return Err("Voice UDP receive failed"),
                };
                if length > MAX_PACKET {
                    continue;
                }
                last_inbound_at = Instant::now();
                if discovering {
                    let Ok((address, port)) = rtp::discovery_response(&packet[..length], ssrc) else {
                        continue;
                    };
                    let elapsed_ms = discovery_started.elapsed().as_millis() as u64;
                    routing_time_ms = Some(elapsed_ms);
                    log_ws_stage_ok("call", "udp_discovery", &diag_host, diag_port, elapsed_ms, None);
                    let mode = mode.ok_or("Missing voice encryption mode")?;
                    text_or_resume(&mut ws, &mut reconnect, transport.is_some(), signal::select_protocol(address, port, mode, &credentials.rtc_connection_id)).await?;
                    emit(&events, Event::ProtocolSelected);
                    discovering = false;
                    continue;
                }
                inbound_packets += 1;
                inbound_bytes += length as u64;
                tally[0] += 1;
                if tally[0] <= 2 {
                    debug!(length, first = packet[0], second = packet[1], "voice packet received");
                }
                let Some(crypto) = &transport else { continue };
                if rtp::is_rtcp(&packet[..length]) {
                    if let Some(compound) = crypto.open_rtcp(&packet[..length]) {
                        receivers.sender_reports(&compound, Instant::now());
                        if let Some(percent) = rtp::outbound_loss(&compound, ssrc) {
                            outbound_loss_percent = percent;
                            emit(&events, Event::OutboundLoss { percent });
                        }
                        // A viewer of our camera: keyframe requests, repairs and rate hints; and the transport-wide
                        // feedback of everything this connection sent.
                        if let Some(sender) = video_sender.as_mut()
                            && let Some(crypto) = transport.as_mut()
                            && let Some(feedback) = rtp::feedback(&compound, sender.video_ssrc())
                            && video_announced
                            && dave.media_secure()
                        {
                            if !feedback.transport.is_empty() && crypto.sent.reports == 0 {
                                debug!("transport-wide feedback received; send-side estimation runs");
                            }
                            if sender.feedback(&feedback, crypto, Instant::now()) && let Some(started) = &camera {
                                started.force_keyframe();
                            }
                        }
                    } else if length >= 12 {
                        decryption_failures += 1;
                    }
                    continue;
                }
                let Some(rtp) = crypto.open(&packet[..length]) else {
                    if length >= 12 {
                        decryption_failures += 1;
                    }
                    continue;
                };
                let arrived = Instant::now();
                receivers.observe_transport(rtp.ssrc, rtp.transport_sequence, arrived);
                if rtp.payload_type != rtp::OPUS_PAYLOAD_TYPE {
                    // Video and its retransmissions; ignored while nobody wants pictures.
                    if dave.media_ready() && hub.has_sink() {
                        video_tally[0] += 1;
                        let now = arrived;
                        receivers.push(rtp, now);
                        if secure {
                            video_tally[2] += receive::hand_over(&mut receivers, &mut decoders, &mut dave, now);
                            // NACK for a hole and PLI for a lost chain go as soon as they are due, not on the tick.
                            if let (Some(crypto), Some(socket)) = (transport.as_mut(), udp.as_ref()) {
                                for (header, body) in receivers.feedback(ssrc, now) {
                                    if let Ok(wire) = crypto.seal_rtcp(&header, &body)
                                        && socket.try_send(&wire).is_ok()
                                    {
                                        udp_failures.sent_packets += 1;
                                        udp_failures.sent_bytes += wire.len() as u64;
                                    }
                                }
                            }
                        }
                    }
                    continue;
                }
                receivers.observe_audio(rtp.ssrc, rtp.sequence, rtp.timestamp, arrived);
                *inbound_audio_packets.entry(rtp.ssrc).or_default() += 1;
                *inbound_audio_bytes.entry(rtp.ssrc).or_default() += rtp.payload.len() as u64;
                tally[1] += 1;
                let Some(user) = playout.user_for_ssrc(rtp.ssrc) else { continue };
                tally[2] += 1;
                if !dave.media_ready() || !dave.contains(user) || controls.borrow().self_deaf {
                    continue;
                }
                let Ok(opus) = dave.decrypt(user, davey::MediaType::AUDIO, &rtp.payload) else {
                    decryption_failures += 1;
                    continue;
                };
                tally[3] += 1;
                playout.push_packet(rtp.ssrc, rtp.sequence, rtp.timestamp, &opus);
            },
            event = async {
                match ws.as_mut() {
                    Some(sock) => sock.next().await,
                    None => std::future::pending().await,
                }
            } => {
                let event = match event {
                    Some(Ok(Message::Close(frame))) => {
                        let (code, reason_len) = frame.as_ref().map_or((u16::from(CloseCode::Status), 0usize), |f| {
                            (u16::from(f.code), f.reason.len())
                        });
                        log_ws_close("call", &diag_host, diag_port, connect_started.elapsed().as_millis() as u64, code, reason_len);
                        if transport.is_some() && close_code_is_resumable(code) {
                            ws = None;
                            reconnect = Some("Voice socket closed; resuming");
                            continue;
                        }
                        return Ok(if code == CLOSE_DISCONNECTED {
                            DisconnectReason::Kicked
                        } else {
                            DisconnectReason::Closed(code)
                        });
                    }
                    Some(Ok(message)) => message,
                    Some(Err(err)) => {
                        log_ws_stage_err(
                            "call",
                            "ws_close",
                            &diag_host,
                            diag_port,
                            connect_started.elapsed().as_millis() as u64,
                            &sanitized_ws_error(&err),
                        );
                        ws = None;
                        reconnect = Some("Voice socket failed; rejoin the call");
                        continue;
                    }
                    _ => {
                        log_ws_close("call", &diag_host, diag_port, connect_started.elapsed().as_millis() as u64, u16::from(CloseCode::Abnormal), 0);
                        ws = None;
                        reconnect = Some("Voice socket failed; rejoin the call");
                        continue;
                    }
                };
                if signal_window.elapsed() > Duration::from_secs(1) {
                    signal_window = Instant::now();
                    signal_count = 0;
                }
                signal_count += 1;
                if signal_count > 256 {
                    return Err("Voice signaling exceeded the bounded processing rate");
                }
                match event {
                    Message::Text(body) => {
                        let mut message = signal::parse_text(&body)?;
                        if let Some(seq) = message.seq {
                            seq_ack = seq;
                        }
                        let data = &mut message.d;
                        debug!(op = message.raw_op, "voice signaling op received");
                        match message.op {
                            Some(VoiceOp::Hello) => {
                                if heartbeat_ms.is_some() {
                                    return Err("Invalid voice heartbeat negotiation");
                                }
                                let hello = signal::hello(data)?;
                                heartbeat_ms = Some(hello.interval_ms);
                                server_version = hello.version;
                                heartbeat_at = Instant::now();
                                log_ws_stage_ok(
                                    "call",
                                    "op8_hello",
                                    &diag_host,
                                    diag_port,
                                    connect_started.elapsed().as_millis() as u64,
                                    tls_version,
                                );
                            }
                            Some(VoiceOp::HeartbeatAck) => {
                                if signal::acknowledges(awaiting_ack, 0, data["t"].as_u64()) {
                                    let ms = heartbeat_sent.elapsed().as_secs_f64() * 1000.;
                                    ping_ms = Some(ms.round() as u64);
                                    emit(&events, Event::Ping { ms });
                                    if expedited.take().is_some() {
                                        debug!("expedited voice heartbeat succeeded");
                                    }
                                    awaiting_ack = None;
                                    last_healthy_at = Instant::now();
                                    // A healthy resumed socket renews the consecutive resume budget.
                                    if !resuming {
                                        resume_attempts = 0;
                                    }
                                }
                            }
                            Some(VoiceOp::Ready) => {
                                if udp.is_some() {
                                    return Err("Unexpected voice transport replacement; rejoin the call");
                                }
                                let ready = signal::ready(data)?;
                                if !rtp::public_ip(ready.ip) {
                                    return Err("Voice server advertised a nonpublic address");
                                }
                                log_ws_stage_ok(
                                    "call",
                                    "op2_ready",
                                    &diag_host,
                                    diag_port,
                                    connect_started.elapsed().as_millis() as u64,
                                    None,
                                );
                                ssrc = ready.ssrc;
                                video_ssrc = ready.video_ssrc;
                                rtx_ssrc = ready.rtx_ssrc;
                                mode = Some(ready.mode);
                                if server_version >= 6 {
                                    text_or_resume(&mut ws, &mut reconnect, transport.is_some(), signal::backend_version()).await?;
                                }
                                emit(&events, Event::State(RtcState::RtcConnecting));
                                let peer = SocketAddr::new(ready.ip, ready.port);
                                let socket = UdpSocket::bind(if ready.ip.is_ipv4() { "0.0.0.0:0" } else { "[::]:0" })
                                    .await
                                    .map_err(|_| "Could not bind voice UDP socket")?;
                                socket
                                    .connect(peer)
                                    .await
                                    .map_err(|_| "Could not connect voice UDP socket")?;
                                let _ = socket.try_send(&rtp::discovery_request(ssrc));
                                udp_peer = Some(peer);
                                udp = Some(socket);
                                discovering = true;
                                discovery_started = Instant::now();
                                discovery_retry_at = discovery_started + DISCOVERY_RETRY;
                                discovery_deadline = discovery_started + DISCOVERY_TIMEOUT;
                            }
                            Some(VoiceOp::SelectProtocolAck) => {
                                if transport.is_some() || udp.is_none() || discovering {
                                    return Err("Unexpected voice session description");
                                }
                                let mode = mode.ok_or("Unexpected voice session description")?;
                                if let Some(interval) = signal::keyframe_interval(data) {
                                    keyframe_interval = interval;
                                    debug!(interval, "voice keyframe interval");
                                }
                                media_session_id = data["media_session_id"].as_str().map(str::to_owned);
                                // `ea(dave_protocol_version)`, then `_ensureDaveProtocolVersionAllowed("init")`.
                                let dave_version = signal::dave_version(&data["dave_protocol_version"]);
                                if dave_version.is_none() {
                                    return Ok(refuse(&mut ws, close::DAVE_INVALID_MESSAGE, "op 4 dave_protocol_version").await);
                                }
                                if !signal::dave_version_allowed(DaveAt::Init, dave_version, credentials.stage) {
                                    return Ok(refuse(&mut ws, close::DAVE_DOWNGRADE_REFUSED, DaveAt::Init.as_str()).await);
                                }
                                let key = signal::session_description(data, mode)?;
                                transport = Some(Transport::new(mode, &key));
                                secured_at = Some(Instant::now());
                                last_healthy_at = Instant::now();
                                log_ws_stage_ok(
                                    "call",
                                    "op4_session_description",
                                    &diag_host,
                                    diag_port,
                                    connect_started.elapsed().as_millis() as u64,
                                    tls_version,
                                );
                                // Register the audio SSRC without sending media.
                                text_or_resume(&mut ws, &mut reconnect, transport.is_some(), signal::speaking(false, ssrc)).await?;
                                if dave_version == Some(0) {
                                    // A Stage channel without end-to-end encryption: no key package, media passes through.
                                    debug!("DAVE protocol version 0: media is not end-to-end encrypted");
                                    dave.start_unencrypted();
                                } else {
                                    send_or_resume(&mut ws, &mut reconnect, transport.is_some(), Message::Binary(dave.key_package()?.into())).await?;
                                }
                                // Ask for pictures from the start when someone is listening for them.
                                if hub.has_sink() && server_version >= 5 {
                                    text_or_resume(&mut ws, &mut reconnect, transport.is_some(), signal::media_sink_wants(100)).await?;
                                    wants_video = true;
                                    wants_video_at = Instant::now() + SINK_WANTS_INTERVAL;
                                }
                            }
                            Some(VoiceOp::SessionUpdate) => {
                                if let Some(interval) = signal::keyframe_interval(data) {
                                    keyframe_interval = interval;
                                    if let Some(sender) = video_sender.as_mut() {
                                        sender.set_keyframe_interval(interval);
                                    }
                                    debug!(interval, "voice keyframe interval");
                                }
                            }
                            Some(VoiceOp::Speaking) => {
                                debug!(ssrc = data["ssrc"].as_u64(), "voice speaking op");
                                let user = signal::id(data, "user_id")?;
                                if user != own && dave.contains(user) {
                                    let value = u32::try_from(signal::number(data, "ssrc")?).map_err(|_| "Invalid voice SSRC")?;
                                    playout.announce(user, value)?;
                                }
                            }
                            Some(VoiceOp::Flags) => {
                                if let (Ok(user_id), Some(flags)) =
                                    (signal::id(data, "user_id"), data["flags"].as_u64())
                                {
                                    playout.set_user_clips(user_id, (flags & 2) != 0);
                                    emit(&events, Event::Flags { user_id, flags });
                                }
                            }
                            Some(VoiceOp::ClientConnect) => {
                                let ids = signal::client_connect(data, dave.unencrypted())?;
                                for user in &ids {
                                    if *user != own && clients.insert(*user) {
                                        emit(&events, Event::ClientConnected { user_id: *user });
                                    }
                                }
                                let was_ready = dave.ready;
                                if dave.connect(&ids)? {
                                    if was_ready {
                                        dave.ready = true;
                                    } else {
                                        deadline = Some(Instant::now() + NEGOTIATION);
                                        capture_reset = true;
                                        playout.clear();
                                    }
                                }
                            }
                            Some(VoiceOp::ClientDisconnect) => {
                                let user = signal::id(data, "user_id")?;
                                playout.remove(user);
                                playout.set_user_clips(user, false);
                                receivers.remove(user);
                                decoders.remove(user);
                                if clients.remove(&user) {
                                    emit(&events, Event::ClientDisconnected { user_id: user });
                                }
                                let was_group_member = dave.is_group_member(user);
                                let was_ready = dave.ready;
                                if dave.disconnect(user)? {
                                    if was_ready {
                                        // The rest still share the current epoch until the SFU commits removal (including 2 -> 1).
                                        dave.ready = true;
                                        deadline = None;
                                    } else if was_group_member {
                                        deadline = Some(Instant::now() + RENEGOTIATION);
                                        capture_reset = true;
                                        playout.clear();
                                    }
                                }
                            }
                            Some(VoiceOp::DavePrepareTransition) => {
                                let version = signal::dave_version(&data["protocol_version"]);
                                if version.is_none() || signal::dave_version(&data["transition_id"]).is_none() {
                                    return Ok(refuse(&mut ws, close::DAVE_INVALID_MESSAGE, "op 21").await);
                                }
                                if !signal::dave_version_allowed(DaveAt::Transition, version, credentials.stage) {
                                    return Ok(refuse(&mut ws, close::DAVE_DOWNGRADE_REFUSED, DaveAt::Transition.as_str()).await);
                                }
                                capture_reset = true;
                                let id = signal::transition(data)?;
                                dave.prepare_protocol(id);
                                if id == 0 {
                                    if dave.session.is_ready() {
                                        dave.execute(0)?;
                                    } else if dave.alone() {
                                        dave.enter_sole_member_waiting()?;
                                        deadline = None;
                                        waiting_announced = true;
                                        ready_announced = false;
                                    } else {
                                        dave.pending_protocol = None;
                                        dave.ready = false;
                                    }
                                } else {
                                    text_or_resume(&mut ws, &mut reconnect, transport.is_some(), signal::transition_ready(id)).await?;
                                }
                            }
                            Some(VoiceOp::DaveExecuteTransition) => {
                                let id = signal::transition(data)?;
                                if dave.execute(id).is_err() {
                                    ready_announced = false;
                                    waiting_announced = false;
                                    capture_reset = true;
                                    playout.clear();
                                    if id != 0 {
                                        text_or_resume(&mut ws, &mut reconnect, transport.is_some(), signal::invalid_commit_welcome(id)).await?;
                                    }
                                    dave.reset()?;
                                    deadline = Some(Instant::now() + RENEGOTIATION);
                                    send_or_resume(&mut ws, &mut reconnect, transport.is_some(), Message::Binary(dave.key_package()?.into())).await?;
                                }
                            }
                            Some(VoiceOp::DavePrepareEpoch) => {
                                let version = signal::dave_version(&data["protocol_version"]);
                                if version.is_none() || data["epoch"].as_u64().is_none() {
                                    return Ok(refuse(&mut ws, close::DAVE_INVALID_MESSAGE, "op 24").await);
                                }
                                if !signal::dave_version_allowed(DaveAt::Epoch, version, credentials.stage) {
                                    return Ok(refuse(&mut ws, close::DAVE_DOWNGRADE_REFUSED, DaveAt::Epoch.as_str()).await);
                                }
                                if signal::number(data, "epoch")? == 1 {
                                    capture_reset = true;
                                    dave.reinitialize()?;
                                    deadline = Some(Instant::now() + RENEGOTIATION);
                                    send_or_resume(&mut ws, &mut reconnect, transport.is_some(), Message::Binary(dave.key_package()?.into())).await?;
                                }
                            }
                            Some(VoiceOp::Resumed) => {
                                if !resuming {
                                    return Err("Unexpected voice resumption");
                                }
                                // `handleResumed` (`921747.js`): reset resume backoff and restore signaling cleanly.
                                resuming = false;
                                resume_attempts = 0;
                                last_healthy_at = Instant::now();
                                if dave.ready || dave.waiting {
                                    deadline = None;
                                }
                                text_or_resume(&mut ws, &mut reconnect, transport.is_some(), signal::speaking(speaking, ssrc)).await?;
                            }
                            Some(VoiceOp::Video) => {
                                // Another participant's sources: their audio SSRC, and their camera if they send one.
                                let remote = signal::remote_video(data)?;
                                if remote.user != own {
                                    if remote.audio_ssrc != 0 && dave.contains(remote.user) {
                                        playout.announce(remote.user, remote.audio_ssrc)?;
                                    }
                                    match remote.video {
                                        Some((video, rtx)) => {
                                            // A bad assignment from one sender costs only that picture.
                                            if let Err(error) = receivers.announce(remote.user, video, rtx) {
                                                debug!(error, "video source refused");
                                            }
                                        }
                                        None => {
                                            receivers.remove(remote.user);
                                            decoders.remove(remote.user);
                                        }
                                    }
                                }
                            }
                            // `sfu-update` (`_handleSfuUpdate`): the media server to send to.
                            Some(VoiceOp::SfuUpdate) => match signal::sfu_update(data) {
                                Ok(None) => debug!("SFU endpoint cleared; media keeps its current server"),
                                Ok(Some((ip, port))) if rtp::public_ip(ip) && udp.is_some() => {
                                    let peer = SocketAddr::new(ip, port);
                                    if udp_peer != Some(peer) {
                                        // `setUdpEndpoint`; a failed retarget is NO_ROUTE.
                                        let Some(fresh_udp) = rebind_udp(peer).await else {
                                            return Err(no_route(&mut ws, &events, credentials.report_no_route, "sfu retarget").await);
                                        };
                                        debug!("voice media retargeted to another SFU endpoint");
                                        qos_flow = None;
                                        qos_wanted = None;
                                        udp = Some(fresh_udp);
                                        udp_peer = Some(peer);
                                        udp_failures.reset();
                                    }
                                }
                                _ => debug!("SFU update ignored"),
                            },
                            // Session updates, sink wants, versions, flags, platform: logged (`Unhandled op`).
                            _ => debug!(op = message.raw_op, "voice signaling op ignored"),
                        }
                    }
                    Message::Binary(bytes) => {
                        let Ok(frame) = signal::parse_binary(&bytes) else {
                            return Ok(refuse(&mut ws, close::DAVE_INVALID_MESSAGE, "binary frame shorter than its header").await);
                        };
                        debug!(op = frame.op, "voice binary op received");
                        seq_ack = i64::from(frame.seq);
                        match frame.op {
                            op if op == VoiceOp::MlsExternalSender as u8 => dave
                                .session
                                .set_external_sender(frame.payload)
                                .map_err(|_| "DAVE external sender validation failed")?,
                            op if op == VoiceOp::MlsProposals as u8 => {
                                match dave.proposals(frame.payload) {
                                    Ok(Some(response)) => {
                                        send_or_resume(&mut ws, &mut reconnect, transport.is_some(), Message::Binary(response.into())).await?;
                                    }
                                    Ok(None) => {}
                                    Err(_) => {
                                        ready_announced = false;
                                        waiting_announced = false;
                                        capture_reset = true;
                                        playout.clear();
                                        dave.reset()?;
                                        deadline = Some(Instant::now() + RENEGOTIATION);
                                        send_or_resume(&mut ws, &mut reconnect, transport.is_some(), Message::Binary(dave.key_package()?.into())).await?;
                                    }
                                }
                            }
                            op if op == VoiceOp::MlsPrepareCommitTransition as u8 || op == VoiceOp::MlsWelcome as u8 => {
                                ready_announced = false;
                                waiting_announced = false;
                                match dave.group_changed(op, frame.payload) {
                                    Ok(id) => {
                                        if id != 0 {
                                            text_or_resume(&mut ws, &mut reconnect, transport.is_some(), signal::transition_ready(id)).await?;
                                        }
                                    }
                                    Err(_) => {
                                        let [first, second, ..] = *frame.payload else {
                                            return Err("Truncated DAVE transition");
                                        };
                                        let id = u16::from_be_bytes([first, second]);
                                        text_or_resume(&mut ws, &mut reconnect, transport.is_some(), signal::invalid_commit_welcome(id)).await?;
                                        dave.reset()?;
                                        send_or_resume(&mut ws, &mut reconnect, transport.is_some(), Message::Binary(dave.key_package()?.into())).await?;
                                    }
                                }
                                if dave.ready {
                                    deadline = None;
                                } else {
                                    capture_reset = true;
                                    playout.clear();
                                    deadline = Some(Instant::now() + RENEGOTIATION);
                                }
                            }
                            // `Unhandled op`: logged, never fatal.
                            op => debug!(op, "voice binary op ignored"),
                        }
                    }
                    Message::Ping(data) => send_or_resume(&mut ws, &mut reconnect, transport.is_some(), Message::Pong(data)).await?,
                    Message::Pong(_) => {}
                    Message::Close(_) => unreachable!("close frames handled before dispatch"),
                    Message::Frame(_) => return Err("Unsupported voice websocket frame"),
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transient_udp_errors_keep_the_call() {
        use std::io::{Error, ErrorKind};
        assert!(transient_receive(&Error::from(ErrorKind::ConnectionReset)));
        assert!(transient_receive(&Error::from(ErrorKind::WouldBlock)));
        assert!(transient_receive(&Error::from(
            ErrorKind::ConnectionAborted
        )));
        #[cfg(windows)]
        {
            for code in [10040, 10050, 10051, 10053, 10065] {
                assert!(
                    transient_receive(&Error::from_raw_os_error(code)),
                    "Windows error {code} must be treated as transient"
                );
            }
        }
        assert!(!transient_receive(&Error::from(
            ErrorKind::PermissionDenied
        )));
    }

    #[test]
    fn heartbeat_times_out_after_one_missed_ack_or_at_an_expedited_deadline() {
        let start = Instant::now();
        let tick = start + Duration::from_secs(13);
        assert_eq!(beat(start, tick, false, None), Beat::Wait);
        assert_eq!(beat(tick, tick, false, None), Beat::Send);
        assert_eq!(
            beat(tick, tick, true, None),
            Beat::TimedOut,
            "one unacknowledged heartbeat by the next tick"
        );
        let deadline = start + Duration::from_secs(5);
        assert_eq!(beat(start, tick, true, Some(deadline)), Beat::Wait);
        assert_eq!(beat(deadline, tick, true, Some(deadline)), Beat::TimedOut);
        assert_eq!(
            beat(tick, tick, true, Some(tick + Duration::from_secs(10))),
            Beat::Skip,
            "a pending expedited heartbeat has its own deadline"
        );
    }

    #[test]
    fn close_code_resumability_matches_official_rtc_control_socket() {
        assert!(!close_code_is_resumable(close::AUTHENTICATION_FAILED));
        assert!(!close_code_is_resumable(close::INVALID_SESSION));
        assert!(!close_code_is_resumable(close::SERVER_NOT_FOUND));
        assert!(!close_code_is_resumable(CLOSE_DISCONNECTED));
        assert!(!close_code_is_resumable(close::SERVER_CRASH));
        assert!(!close_code_is_resumable(4800));
        assert!(!close_code_is_resumable(4805));
        assert!(close_code_is_resumable(1000));
        assert!(close_code_is_resumable(1001));
        assert!(close_code_is_resumable(1006));
        assert!(close_code_is_resumable(4009));
    }

    #[test]
    fn ws_resume_backoff_stays_bounded_within_official_window() {
        for attempt in 0..8 {
            let d = ws_resume_backoff(attempt);
            assert!(
                d >= Duration::from_millis(500) && d <= Duration::from_secs(5),
                "attempt {attempt} delay {d:?} out of bounds"
            );
        }
    }

    /// The other members only hear packets near the channel's bitrate: 20 ms at 64 kb/s VBR stays far under the
    /// 1275 bytes Serein's 510 kb/s CBR sent, which the voice server forwarded to no one.
    #[test]
    fn opus_profile_stays_near_the_channels_bitrate() {
        let mut encoder = encoder().unwrap();
        let mut out = [0u8; 1275];
        let mut total = 0;
        for frame in 0..50 {
            let stereo: Vec<f32> = (0..1920)
                .map(|i| (((frame * 960 + i / 2) as f32) * 0.05).sin() * 0.3)
                .collect();
            total += encoder.encode_float(&stereo, &mut out).unwrap();
        }
        // One second of audio: 64 kb/s is 8000 bytes; VBR may run somewhat over on a pure tone.
        assert!(total < 12_000, "{total} bytes in a second"); // In-band FEC planned for the client's 30 % (`fec: !0, packetLossRate: .3`), no DTX.
        assert!(encoder.get_inband_fec().unwrap());
        assert_eq!(encoder.get_packet_loss_perc().unwrap(), 30);
        assert!(!encoder.get_dtx().unwrap());
    }

    #[test]
    fn diagnostic_logs_include_stage_host_and_elapsed_without_secrets_ids_or_nat_ip() {
        struct Capture(Arc<Mutex<Vec<String>>>);
        impl tracing::Subscriber for Capture {
            fn enabled(&self, _: &tracing::Metadata<'_>) -> bool {
                true
            }
            fn new_span(&self, _: &tracing::span::Attributes<'_>) -> tracing::span::Id {
                tracing::span::Id::from_u64(1)
            }
            fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}
            fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}
            fn event(&self, event: &tracing::Event<'_>) {
                struct Visitor(String);
                impl tracing::field::Visit for Visitor {
                    fn record_debug(
                        &mut self,
                        field: &tracing::field::Field,
                        value: &dyn std::fmt::Debug,
                    ) {
                        use std::fmt::Write;
                        let _ = write!(self.0, "{}={:?} ", field.name(), value);
                    }
                }
                let mut visitor = Visitor(String::new());
                event.record(&mut visitor);
                self.0.lock().unwrap().push(visitor.0);
            }
            fn enter(&self, _: &tracing::span::Id) {}
            fn exit(&self, _: &tracing::span::Id) {}
        }

        let captured = Arc::new(Mutex::new(Vec::new()));
        let _guard = tracing::subscriber::set_default(Capture(Arc::clone(&captured)));

        let join = crate::Join {
            endpoint: "c-ams01-abcd.discord.media:2096".into(),
            token: "SECRET_VOICE_TOKEN_ABC".into(),
            session_id: "SECRET_SESSION_XYZ".into(),
            server_id: 987654321012345,
            channel_id: 876543210987654,
            user_id: 765432109876543,
            stage: false,
            rtc_connection_id: crate::rtc_connection_id(),
            report_no_route: true,
        };
        tracing::debug!(?join, "join struct debug");

        let (host, port) = ws_target("wss://c-ams01-abcd.discord.media:2096/?v=9").unwrap();
        assert_eq!((host.as_str(), port), ("c-ams01-abcd.discord.media", 2096));

        for stage in [
            "dns",
            "tcp",
            "tls_ws",
            "op8_hello",
            "op2_ready",
            "udp_discovery",
            "op4_session_description",
        ] {
            log_ws_stage_ok("call", stage, &host, port, 12, Some("TLSv1.3"));
        }
        log_ws_close("call", &host, port, 44, 4014, 19);

        let err = tokio_tungstenite::tungstenite::Error::Http(Box::new(
            tokio_tungstenite::tungstenite::http::Response::builder()
                .status(401)
                .body(Some(b"SECRET_VOICE_TOKEN_ABC 203.0.113.77".to_vec()))
                .unwrap(),
        ));
        let category = sanitized_ws_error(&err);
        assert_eq!(category, "http:401");
        log_ws_stage_err("call", "tls_ws", &host, port, 5, &category);

        let rejection = ws_target("wss://evil.example/?token=SECRET_VOICE_TOKEN_ABC");
        assert!(rejection.is_err());

        let joined = captured.lock().unwrap().join("\n");
        for required in [
            "c-ams01-abcd.discord.media",
            "2096",
            "stage=\"dns\"",
            "stage=\"tcp\"",
            "stage=\"tls_ws\"",
            "stage=\"op8_hello\"",
            "stage=\"op2_ready\"",
            "stage=\"udp_discovery\"",
            "stage=\"op4_session_description\"",
            "stage=\"ws_close\"",
            "close_code=4014",
            "reason_len=19",
            "elapsed_ms=",
            "http:401",
        ] {
            assert!(joined.contains(required), "missing {required} in {joined}");
        }
        for forbidden in [
            "SECRET_VOICE_TOKEN_ABC",
            "SECRET_SESSION_XYZ",
            "987654321012345",
            "876543210987654",
            "765432109876543",
            "203.0.113.77",
            "?v=9",
            "token=",
        ] {
            assert!(
                !joined.contains(forbidden),
                "leaked sensitive string {forbidden} in {joined}"
            );
        }
    }
}
