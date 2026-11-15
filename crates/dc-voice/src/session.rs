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
    DisconnectReason, Event, Frame,
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
    signal::{self, CLOSE_DISCONNECTED, Context, Ids, MAX_SIGNAL},
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
/// Serein: only a path that keeps failing this long ends the call.
const UDP_OUTAGE: Duration = Duration::from_secs(10);
pub(crate) const UDP_KEEPALIVE: Duration = Duration::from_secs(5);
pub(crate) const DISCOVERY_TIMEOUT: Duration = Duration::from_secs(8);
pub(crate) const MAX_RESUMES: u8 = 2;

pub(crate) struct Credentials {
    pub endpoint: String,
    pub token: Zeroizing<String>,
    pub session_id: Zeroizing<String>,
    pub server_id: u64,
    pub channel_id: u64,
    pub user_id: u64,
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
        Duration::from_secs(5),
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
    let tcp = match timeout(Duration::from_secs(5), TcpStream::connect(&addrs[..])).await {
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
        Duration::from_secs(10),
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
    pub sent_packets: u64,
    pub sent_bytes: u64,
}
impl UdpFailures {
    pub(crate) fn send(
        &mut self,
        socket: &UdpSocket,
        data: &[u8],
        outage: &'static str,
    ) -> Result<(), &'static str> {
        if socket.try_send(data).is_ok() {
            self.since = None;
            self.sent_packets += 1;
            self.sent_bytes += data.len() as u64;
            return Ok(());
        }
        let now = Instant::now();
        if now.duration_since(*self.since.get_or_insert(now)) >= UDP_OUTAGE {
            return Err(outage);
        }
        Ok(())
    }
}

/// Receive errors that describe one lost or rejected datagram rather than a dead socket.
pub(crate) fn transient_receive(error: &std::io::Error) -> bool {
    // WSAEMSGSIZE: Windows fails an oversized datagram instead of truncating it.
    const WSAEMSGSIZE: i32 = 10040;
    matches!(
        error.kind(),
        std::io::ErrorKind::ConnectionReset
            | std::io::ErrorKind::ConnectionRefused
            | std::io::ErrorKind::Interrupted
    ) || (cfg!(windows) && error.raw_os_error() == Some(WSAEMSGSIZE))
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
    let playout = Arc::clone(&shared.playout);
    let _guard = SessionGuard {
        playout: Arc::clone(&playout),
        rtc_stats: Arc::clone(&shared.rtc_stats),
    };
    playout.clear();
    playout.sync_user_mix_table(&shared.user_mix.lock().unwrap_or_else(|e| e.into_inner()));
    let own = credentials.user_id;
    let opened = open_with_stages("call", &url).await?;
    let mut ws = opened.ws;
    let mut diag_host = opened.host;
    let mut diag_port = opened.port;
    let mut tls_version = opened.tls_version;
    let mut connect_started = opened.opened_at;
    // Only voice-scoped credentials go to this validated endpoint.
    text(&mut ws, signal::identify(&credentials.ids(), Context::Call)).await?;

    let mut dave = Dave::new(own, credentials.channel_id)?;
    let mut mode: Option<Mode> = None;
    let mut transport: Option<Transport> = None;
    let mut secured_at: Option<Instant> = None;
    // Where packets go, for the debug log every five seconds: received, opened by the transport, from a known user,
    // decrypted by DAVE; and sent.
    let mut tally = [0u32; 5];
    let mut tally_at = Instant::now() + Duration::from_secs(5);
    let mut udp: Option<UdpSocket> = None;
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
    let mut missed_acks = 0u8;
    // Set when the socket died or went silent; the next loop turn resumes it.
    let mut reconnect: Option<&'static str> = None;
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
            if transport.is_none() || resume_attempts >= MAX_RESUMES {
                return Err(reason);
            }
            debug!("voice socket lost; resuming");
            resume_attempts += 1;
            resuming = true;
            ready_announced = false;
            waiting_announced = false;
            capture_reset = true;
            deadline = Some(Instant::now() + RENEGOTIATION);
            heartbeat_at = Instant::now();
            hello_deadline = Instant::now() + HELLO_TIMEOUT;
            heartbeat_ms = None;
            awaiting_ack = None;
            missed_acks = 0;
            let reopened = open_with_stages("call", &url)
                .await
                .map_err(|_| "Voice resume failed; rejoin the call")?;
            ws = reopened.ws;
            diag_host = reopened.host;
            diag_port = reopened.port;
            tls_version = reopened.tls_version;
            connect_started = reopened.opened_at;
            text(&mut ws, signal::resume(&credentials.ids(), seq_ack)).await?;
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
                    log_ws_close("call", &diag_host, diag_port, connect_started.elapsed().as_millis() as u64, 1000, 0);
                    let _ = timeout(Duration::from_secs(1), ws.close(None)).await;
                    return Ok(DisconnectReason::Requested);
                }
                let request = controls.borrow().video_request;
                if request != video_request_seen {
                    video_request_seen = request;
                    camera_wish = shared.camera.lock().unwrap_or_else(|e| e.into_inner()).clone();
                }
            },
            wake = pacer_wakes.recv() => {
                if wake.is_none() {
                    return Err("Pacer clock stopped");
                }
                if let (Some(sender), Some(crypto), Some(socket)) = (video_sender.as_mut(), transport.as_mut(), udp.as_ref()) {
                    sender.flush(Instant::now(), crypto, |data| udp_failures.send(socket, data, "Voice UDP send failed"))?;
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
                if deadline.is_some_and(|d| now >= d) {
                    return Err(negotiation_timeout(heartbeat_ms.is_some(), udp.is_some(), transport.is_some(), &dave, resuming));
                }
                if heartbeat_ms.is_none() && now >= hello_deadline {
                    if transport.is_none() {
                        return Err("Discord voice Hello timed out; rejoin the call");
                    }
                    reconnect = Some("Discord voice Hello timed out; rejoin the call");
                    continue;
                }
                if discovering && now >= discovery_deadline {
                    log_ws_stage_err(
                        "call",
                        "udp_discovery",
                        &diag_host,
                        diag_port,
                        discovery_started.elapsed().as_millis() as u64,
                        "timed_out",
                    );
                    return Err("Discord voice UDP discovery timed out; check the network firewall");
                }
                if !discovering && now >= next_udp_ping && let Some(socket) = &udp {
                    udp_ping_sequence = udp_ping_sequence.wrapping_add(1);
                    udp_failures.send(socket, &rtp::keepalive(udp_ping_sequence), "Voice UDP keepalive failed")?;
                    next_udp_ping = now + UDP_KEEPALIVE;
                }
                if let Some(interval) = heartbeat_ms && now >= heartbeat_at {
                    // A delayed wake can find the acknowledgement still unread; only a second
                    // silent interval marks the socket as zombied, and Discord expects a resume.
                    if awaiting_ack.is_some() {
                        missed_acks += 1;
                        if missed_acks >= 2 {
                            reconnect = Some("Discord voice heartbeat was not acknowledged; rejoin the call");
                            continue;
                        }
                    }
                    heartbeat_nonce = heartbeat_nonce.wrapping_add(1);
                    heartbeat_sent = Instant::now();
                    text(&mut ws, signal::heartbeat(heartbeat_nonce, seq_ack, server_version)).await?;
                    awaiting_ack = Some(heartbeat_nonce);
                    heartbeat_at = now + Duration::from_millis(interval);
                }
                if secured_at.is_some_and(|at| now >= at + PEER_GRACE) && !discovering && !resuming && dave.should_wait_for_peer() {
                    dave.enter_sole_member_waiting()?;
                }
                let enabled = dave.ready && transport.is_some() && !discovering && !resuming;
                let waiting = dave.waiting && transport.is_some() && !discovering && !resuming;
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
                    text(&mut ws, signal::speaking(true, ssrc)).await?;
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
                            .session
                            .encrypt_opus(&encoded[..length])
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
                        udp_failures.send(socket, &wire, "Voice UDP send failed")?;
                        outbound_audio_packets += 1;
                        outbound_audio_bytes += wire.len() as u64;
                        tally[4] += 1;
                    }
                    sequence = sequence.wrapping_add(1);
                    if !active && silence == 0 {
                        text(&mut ws, signal::speaking(false, ssrc)).await?;
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
                secure = enabled && dave.session.is_ready() && !dave.transitioning();
                if !secure {
                    if let Some(sender) = video_sender.as_mut() {
                        sender.clear();
                    }
                    camera_sent_keyframe = false;
                }
                let wants = hub.has_sink() && server_version >= 5 && transport.is_some() && !discovering && !resuming;
                if wants != wants_video || (wants && now >= wants_video_at) {
                    wants_video = wants;
                    wants_video_at = now + SINK_WANTS_INTERVAL;
                    text(&mut ws, signal::media_sink_wants(if wants { 100 } else { 0 })).await?;
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
                        text(&mut ws, signal::video(ssrc, None)).await?;
                        video_announced = false;
                    }
                    emit(&events, Event::VideoState { camera: false });
                }
                if camera.is_some() && !video_announced && transport.is_some() && !discovering && !resuming {
                    let mut sender = VideoSender::new(video_ssrc, rtx_ssrc, CAMERA_MIN_BITRATE, CALL_START_BITRATE, CAMERA_MAX_BITRATE);
                    sender.set_framerate(camera::FPS);
                    sender.set_keyframe_interval(keyframe_interval);
                    video_sender = Some(sender);
                    text(&mut ws, signal::video(ssrc, Some((video_ssrc, rtx_ssrc, Context::Call, None)))).await?;
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
                    let sf_version = if dave.ready {
                        signal::DAVE_PROTOCOL_VERSION as u32
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
                if discovering {
                    let (address, port) = rtp::discovery_response(&packet[..length], ssrc)?;
                    let elapsed_ms = discovery_started.elapsed().as_millis() as u64;
                    routing_time_ms = Some(elapsed_ms);
                    log_ws_stage_ok("call", "udp_discovery", &diag_host, diag_port, elapsed_ms, None);
                    let mode = mode.ok_or("Missing voice encryption mode")?;
                    text(&mut ws, signal::select_protocol(address, port, mode)).await?;
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
                            && dave.ready
                            && !dave.transitioning()
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
                    if dave.ready && hub.has_sink() {
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
                if !dave.ready || !dave.contains(user) || controls.borrow().self_deaf {
                    continue;
                }
                let Ok(opus) = dave.session.decrypt(user, davey::MediaType::AUDIO, &rtp.payload) else {
                    decryption_failures += 1;
                    continue;
                };
                tally[3] += 1;
                playout.push_packet(rtp.ssrc, rtp.sequence, rtp.timestamp, &opus);
            },
            event = ws.next() => {
                let event = match event {
                    // A server crash preserves the voice session; other close codes are terminal.
                    Some(Ok(message)) if !matches!(&message, Message::Close(Some(frame)) if u16::from(frame.code) == close::SERVER_CRASH) => message,
                    Some(Ok(Message::Close(frame))) => {
                        let (code, reason_len) = frame.as_ref().map_or((u16::from(CloseCode::Status), 0usize), |f| {
                            (u16::from(f.code), f.reason.len())
                        });
                        log_ws_close("call", &diag_host, diag_port, connect_started.elapsed().as_millis() as u64, code, reason_len);
                        reconnect = Some("Voice socket failed; rejoin the call");
                        continue;
                    }
                    Some(Err(err)) => {
                        log_ws_stage_err(
                            "call",
                            "ws_close",
                            &diag_host,
                            diag_port,
                            connect_started.elapsed().as_millis() as u64,
                            &sanitized_ws_error(&err),
                        );
                        reconnect = Some("Voice socket failed; rejoin the call");
                        continue;
                    }
                    _ => {
                        log_ws_close("call", &diag_host, diag_port, connect_started.elapsed().as_millis() as u64, u16::from(CloseCode::Abnormal), 0);
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
                                if signal::acknowledges(awaiting_ack, missed_acks, data["t"].as_u64()) {
                                    if missed_acks == 0 {
                                        let ms = heartbeat_sent.elapsed().as_secs_f64() * 1000.;
                                        ping_ms = Some(ms.round() as u64);
                                        emit(&events, Event::Ping { ms });
                                    }
                                    awaiting_ack = None;
                                    missed_acks = 0;
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
                                    text(&mut ws, signal::backend_version()).await?;
                                }
                                let socket = UdpSocket::bind(if ready.ip.is_ipv4() { "0.0.0.0:0" } else { "[::]:0" })
                                    .await
                                    .map_err(|_| "Could not bind voice UDP socket")?;
                                socket
                                    .connect(SocketAddr::new(ready.ip, ready.port))
                                    .await
                                    .map_err(|_| "Could not connect voice UDP socket")?;
                                socket
                                    .send(&rtp::discovery_request(ssrc))
                                    .await
                                    .map_err(|_| "Voice UDP discovery failed")?;
                                udp = Some(socket);
                                discovering = true;
                                discovery_started = Instant::now();
                                discovery_deadline = Instant::now() + DISCOVERY_TIMEOUT;
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
                                let key = signal::session_description(data, mode)?;
                                transport = Some(Transport::new(mode, &key));
                                secured_at = Some(Instant::now());
                                log_ws_stage_ok(
                                    "call",
                                    "op4_session_description",
                                    &diag_host,
                                    diag_port,
                                    connect_started.elapsed().as_millis() as u64,
                                    tls_version,
                                );
                                // Register the audio SSRC without sending media.
                                text(&mut ws, signal::speaking(false, ssrc)).await?;
                                send(&mut ws, Message::Binary(dave.key_package()?.into())).await?;
                                // Ask for pictures from the start when someone is listening for them.
                                if hub.has_sink() && server_version >= 5 {
                                    text(&mut ws, signal::media_sink_wants(100)).await?;
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
                                let ids = signal::client_connect(data)?;
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
                                if signal::number(data, "protocol_version")? != signal::DAVE_PROTOCOL_VERSION {
                                    return Err("Discord requested a voice encryption downgrade; call stopped");
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
                                    text(&mut ws, signal::transition_ready(id)).await?;
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
                                        text(&mut ws, signal::invalid_commit_welcome(id)).await?;
                                    }
                                    dave.reset()?;
                                    deadline = Some(Instant::now() + RENEGOTIATION);
                                    send(&mut ws, Message::Binary(dave.key_package()?.into())).await?;
                                }
                            }
                            Some(VoiceOp::DavePrepareEpoch) => {
                                if signal::number(data, "protocol_version")? != signal::DAVE_PROTOCOL_VERSION {
                                    return Err("Unsupported DAVE protocol version; call stopped");
                                }
                                if signal::number(data, "epoch")? == 1 {
                                    capture_reset = true;
                                    dave.reinitialize()?;
                                    deadline = Some(Instant::now() + RENEGOTIATION);
                                    send(&mut ws, Message::Binary(dave.key_package()?.into())).await?;
                                }
                            }
                            Some(VoiceOp::Resumed) => {
                                if !resuming {
                                    return Err("Unexpected voice resumption");
                                }
                                text(&mut ws, signal::speaking(false, ssrc)).await?;
                                speaking = false;
                                silence = 0;
                                resuming = false;
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
                            // Session updates, sink wants, versions, flags, platform, SFU updates.
                            _ => debug!(op = message.raw_op, "voice signaling op ignored"),
                        }
                    }
                    Message::Binary(bytes) => {
                        let frame = signal::parse_binary(&bytes)?;
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
                                        send(&mut ws, Message::Binary(response.into())).await?;
                                    }
                                    Ok(None) => {}
                                    Err(_) => {
                                        ready_announced = false;
                                        waiting_announced = false;
                                        capture_reset = true;
                                        playout.clear();
                                        dave.reset()?;
                                        deadline = Some(Instant::now() + RENEGOTIATION);
                                        send(&mut ws, Message::Binary(dave.key_package()?.into())).await?;
                                    }
                                }
                            }
                            op if op == VoiceOp::MlsPrepareCommitTransition as u8 || op == VoiceOp::MlsWelcome as u8 => {
                                ready_announced = false;
                                waiting_announced = false;
                                match dave.group_changed(op, frame.payload) {
                                    Ok(id) => {
                                        if id != 0 {
                                            text(&mut ws, signal::transition_ready(id)).await?;
                                        }
                                    }
                                    Err(_) => {
                                        let [first, second, ..] = *frame.payload else {
                                            return Err("Truncated DAVE transition");
                                        };
                                        let id = u16::from_be_bytes([first, second]);
                                        text(&mut ws, signal::invalid_commit_welcome(id)).await?;
                                        dave.reset()?;
                                        send(&mut ws, Message::Binary(dave.key_package()?.into())).await?;
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
                            _ => return Err("Unsupported DAVE opcode; call stopped"),
                        }
                    }
                    Message::Ping(data) => send(&mut ws, Message::Pong(data)).await?,
                    Message::Pong(_) => {}
                    Message::Close(frame) => {
                        // No close frame reads as 1005, "no status received".
                        let (code, reason_len) = frame.as_ref().map_or(
                            (u16::from(CloseCode::Status), 0usize),
                            |frame| (u16::from(frame.code), frame.reason.len()),
                        );
                        log_ws_close(
                            "call",
                            &diag_host,
                            diag_port,
                            connect_started.elapsed().as_millis() as u64,
                            code,
                            reason_len,
                        );
                        return Ok(if code == CLOSE_DISCONNECTED {
                            DisconnectReason::Kicked
                        } else {
                            DisconnectReason::Closed(code)
                        });
                    }
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
        assert!(!transient_receive(&Error::from(
            ErrorKind::PermissionDenied
        )));
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
