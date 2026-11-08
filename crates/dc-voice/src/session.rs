//! One voice connection: control socket, UDP media, DAVE, Opus, mixer and audio devices.
//!
//! Adapted from Serein `crates/discord-voice/src/transport.rs` (`run_inner`, `send`,
//! `json_send`, `UdpFailures`, `transient_receive`, `negotiation_timeout`), MIT OR
//! Apache-2.0. Changes: audio only (no camera, Go Live, video receive, RTCP feedback or
//! diagnostics), protocol URL `v=9` with `channel_id` in IDENTIFY and RESUME, op 16 after
//! READY on server version 6+, a 20 s HELLO timeout, AES-256-GCM preferred over XChaCha20,
//! Origin/User-Agent of the pinned build on the socket, audio devices owned by the session,
//! and app-facing events in place of Serein's `Status`.
use crate::{
    DisconnectReason, Event, Frame,
    activity::{self, LOCAL_THRESHOLD_DB},
    audio::Audio,
    capture::CapturePacer,
    clock,
    dave::Dave,
    mixer::Mixer,
    rtp::{self, MAX_PACKET, Mode, Transport},
    signal::{self, CLOSE_DISCONNECTED, Ids, MAX_SIGNAL},
};
use dc_proto::voice_op::{VoiceOp, close};
use futures_util::{SinkExt, StreamExt};
use opus2::{Application, Bitrate, Channels, Encoder, SoftClip};
use std::{collections::BTreeSet, net::SocketAddr, sync::mpsc as std_mpsc, time::Duration};
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
use tracing::debug;
use zeroize::Zeroizing;

type Socket = WebSocketStream<MaybeTlsStream<TcpStream>>;

/// Time a sole member gives the roster announcement before waiting for a peer (Serein).
const PEER_GRACE: Duration = Duration::from_millis(500);
/// HELLO timeout of the pinned build (`tools/voice-notes.md`).
const HELLO_TIMEOUT: Duration = Duration::from_secs(20);
/// Serein's bound for reaching a secured call, and for recovering one after a change.
const NEGOTIATION: Duration = Duration::from_secs(90);
const RENEGOTIATION: Duration = Duration::from_secs(30);
/// Serein: only a path that keeps failing this long ends the call.
const UDP_OUTAGE: Duration = Duration::from_secs(10);
const UDP_KEEPALIVE: Duration = Duration::from_secs(5);
const DISCOVERY_TIMEOUT: Duration = Duration::from_secs(8);
const MAX_RESUMES: u8 = 2;

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
    pub disconnect: bool,
}

async fn send(ws: &mut Socket, message: Message) -> Result<(), &'static str> {
    timeout(Duration::from_secs(5), ws.send(message))
        .await
        .map_err(|_| "Voice signaling write timed out")?
        .map_err(|_| "Voice signaling disconnected")
}
async fn text(ws: &mut Socket, value: String) -> Result<(), &'static str> {
    send(ws, Message::Text(value.into())).await
}

async fn open(url: &str) -> Result<Socket, &'static str> {
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
    let (ws, _) = timeout(
        Duration::from_secs(15),
        tokio_tungstenite::connect_async_with_config(request, Some(config), false),
    )
    .await
    .map_err(|_| "Voice connection timed out")?
    .map_err(|_| "Voice TLS connection failed")?;
    Ok(ws)
}

/// Media datagrams are lossy by design: a full buffer or a stray ICMP unreachable drops
/// one packet. Never wait for socket capacity, that would stall audio and signaling.
#[derive(Default)]
struct UdpFailures(Option<Instant>);
impl UdpFailures {
    fn send(
        &mut self,
        socket: &UdpSocket,
        data: &[u8],
        outage: &'static str,
    ) -> Result<(), &'static str> {
        if socket.try_send(data).is_ok() {
            self.0 = None;
            return Ok(());
        }
        let now = Instant::now();
        if now.duration_since(*self.0.get_or_insert(now)) >= UDP_OUTAGE {
            return Err(outage);
        }
        Ok(())
    }
}

/// Receive errors that describe one lost or rejected datagram rather than a dead socket.
fn transient_receive(error: &std::io::Error) -> bool {
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
fn encoder() -> Result<Encoder, &'static str> {
    let fail = |_| "Opus encoder configuration failed";
    let mut encoder = Encoder::new(48_000, Channels::Stereo, Application::Voip).map_err(fail)?;
    encoder.set_bitrate(Bitrate::Bits(64_000)).map_err(fail)?;
    encoder.set_vbr(true).map_err(fail)?;
    encoder.set_complexity(10).map_err(fail)?;
    encoder.set_signal(opus2::Signal::Voice).map_err(fail)?;
    encoder.set_inband_fec(true).map_err(fail)?;
    encoder.set_packet_loss_perc(10).map_err(fail)?;
    encoder.set_dtx(false).map_err(fail)?;
    Ok(encoder)
}

fn emit(events: &mpsc::UnboundedSender<Event>, event: Event) {
    // A dropped receiver is the app not listening; the call itself is ended through Controls.
    let _ = events.send(event);
}

pub(crate) async fn run(
    credentials: Credentials,
    mut controls: watch::Receiver<ControlState>,
    events: mpsc::UnboundedSender<Event>,
) -> Result<DisconnectReason, &'static str> {
    let url = signal::endpoint_url(&credentials.endpoint)?;
    if controls.borrow_and_update().disconnect {
        return Ok(DisconnectReason::Requested);
    }
    let own = credentials.user_id;
    let mut ws = open(&url).await?;
    // Only voice-scoped credentials go to this validated endpoint.
    text(&mut ws, signal::identify(&credentials.ids())).await?;

    let mut dave = Dave::new(own, credentials.channel_id)?;
    let mut mode: Option<Mode> = None;
    let mut transport: Option<Transport> = None;
    let mut secured_at: Option<Instant> = None;
    // Where packets go, for the debug log every five seconds: received, opened by the transport, from a known user,
    // decrypted by DAVE; and sent.
    let mut tally = [0u32; 5];
    let mut tally_at = Instant::now() + Duration::from_secs(5);
    let mut udp: Option<UdpSocket> = None;
    let mut next_udp_ping = Instant::now();
    let mut udp_ping_sequence = 0u32;
    let mut udp_failures = UdpFailures::default();
    let mut discovering = false;
    let mut discovery_deadline = Instant::now();
    let mut ssrc = 0u32;
    let mut mixer = Mixer::default();
    let mut seq_ack: i64 = -1;
    let mut server_version = 0u64;
    let mut heartbeat_ms: Option<u64> = None;
    let mut heartbeat_at = Instant::now();
    let mut hello_deadline = Instant::now() + HELLO_TIMEOUT;
    let mut awaiting_ack = None;
    let mut heartbeat_nonce = 0u64;
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
    let mut signal_window = Instant::now();
    let mut signal_count = 0u16;

    // Devices open only once the call is secured (or waiting alone), never before.
    let (capture_send, capture) = std_mpsc::sync_channel::<Frame>(8);
    let (playback, playback_receive) = std_mpsc::sync_channel::<Frame>(8);
    let mut device_ends = Some((capture_send, playback_receive));
    let mut audio: Option<Audio> = None;
    let mut capture_pacer = CapturePacer::default();
    let mut capture_at = Instant::now();
    let mut capture_enabled = false;
    let mut capture_reset = true;
    let mut local_activity = 0u8;
    let mut speakers_at = Instant::now();
    let mut last_speakers = BTreeSet::new();
    let mut clients = BTreeSet::new();

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
            hello_deadline = Instant::now() + HELLO_TIMEOUT;
            heartbeat_ms = None;
            awaiting_ack = None;
            missed_acks = 0;
            ws = open(&url)
                .await
                .map_err(|_| "Voice resume failed; rejoin the call")?;
            text(&mut ws, signal::resume(&credentials.ids(), seq_ack)).await?;
        }
        tokio::select! {
            changed = controls.changed() => {
                if changed.is_err() || controls.borrow().disconnect {
                    let _ = timeout(Duration::from_secs(1), ws.close(None)).await;
                    return Ok(DisconnectReason::Requested);
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
                }
                if (enabled || waiting) && audio.is_none() && let Some((capture_send, playback_receive)) = device_ends.take() {
                    audio = Some(Audio::start(capture_send, playback_receive)?);
                }
                let control = *controls.borrow();
                if let Some(audio) = &audio {
                    audio.set_ready(enabled || waiting);
                    audio.set_controls(control.self_mute, control.self_deaf);
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
                if enabled && (active || silence > 0) {
                    let data = if let Some(frame) = latest.filter(|_| active) {
                        for (sample, pair) in frame.iter().zip(stereo.as_chunks_mut::<2>().0) {
                            let sample = if sample.is_finite() { sample.clamp(-1.0, 1.0) } else { 0.0 };
                            pair[0] = sample;
                            pair[1] = sample;
                        }
                        soft_clip.apply(&mut stereo);
                        let length = encoder.encode_float(&stereo, &mut encoded).map_err(|_| "Opus encoding failed")?;
                        dave.session.encrypt_opus(&encoded[..length]).map_err(|_| "DAVE audio encryption failed")?.into_owned()
                    } else {
                        silence -= 1;
                        davey::OPUS_SILENCE_PACKET.to_vec()
                    };
                    let header = rtp::header(sequence, timestamp, ssrc);
                    let wire = transport.as_mut().ok_or("Missing voice transport key")?.seal(&header, &data)?;
                    if let Some(socket) = &udp {
                        udp_failures.send(socket, &wire, "Voice UDP send failed")?;
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
                if enabled && !control.self_deaf {
                    if stalled {
                        mixer.clear();
                    }
                    if let Some(frame) = mixer.pop() {
                        // A full queue means the device is behind; drop rather than block.
                        let _ = playback.try_send(frame);
                    }
                } else {
                    mixer.clear();
                }
                if now >= tally_at {
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
                    tally_at = now + Duration::from_secs(5);
                }
                if now >= speakers_at {
                    let mut current: BTreeSet<u64> = mixer.speaking().collect();
                    if local_activity > 0 {
                        current.insert(own);
                    }
                    for user in last_speakers.difference(&current) {
                        emit(&events, Event::SpeakingChanged { user_id: *user, speaking: false });
                    }
                    for user in current.difference(&last_speakers) {
                        emit(&events, Event::SpeakingChanged { user_id: *user, speaking: true });
                    }
                    last_speakers = current;
                    speakers_at = now + Duration::from_millis(100);
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
                    let mode = mode.ok_or("Missing voice encryption mode")?;
                    text(&mut ws, signal::select_protocol(address, port, mode)).await?;
                    discovering = false;
                    continue;
                }
                tally[0] += 1;
                if tally[0] <= 2 {
                    debug!(length, first = packet[0], second = packet[1], "voice packet received");
                }
                let Some(crypto) = &transport else { continue };
                let Some(rtp) = crypto.open(&packet[..length]) else { continue };
                tally[1] += 1;
                let Some(user) = mixer.user(rtp.ssrc) else { continue };
                tally[2] += 1;
                if !dave.ready || !dave.contains(user) || controls.borrow().self_deaf {
                    continue;
                }
                let Ok(opus) = dave.session.decrypt(user, davey::MediaType::AUDIO, &rtp.payload) else { continue };
                tally[3] += 1;
                mixer.push(rtp.ssrc, rtp.sequence, opus);
            },
            event = ws.next() => {
                let event = match event {
                    // A server crash preserves the voice session; other close codes are terminal.
                    Some(Ok(message)) if !matches!(&message, Message::Close(Some(frame)) if u16::from(frame.code) == close::SERVER_CRASH) => message,
                    _ => {
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
                            }
                            Some(VoiceOp::HeartbeatAck) => {
                                if signal::acknowledges(awaiting_ack, missed_acks, data["t"].as_u64()) {
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
                                ssrc = ready.ssrc;
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
                                discovery_deadline = Instant::now() + DISCOVERY_TIMEOUT;
                            }
                            Some(VoiceOp::SelectProtocolAck) => {
                                if transport.is_some() || udp.is_none() || discovering {
                                    return Err("Unexpected voice session description");
                                }
                                let mode = mode.ok_or("Unexpected voice session description")?;
                                let key = signal::session_description(data, mode)?;
                                transport = Some(Transport::new(mode, &key));
                                secured_at = Some(Instant::now());
                                // Register the audio SSRC without sending media.
                                text(&mut ws, signal::speaking(false, ssrc)).await?;
                                send(&mut ws, Message::Binary(dave.key_package()?.into())).await?;
                            }
                            Some(VoiceOp::Speaking) => {
                                debug!(ssrc = data["ssrc"].as_u64(), "voice speaking op");
                                let user = signal::id(data, "user_id")?;
                                if user != own && dave.contains(user) {
                                    let value = u32::try_from(signal::number(data, "ssrc")?).map_err(|_| "Invalid voice SSRC")?;
                                    mixer.announce(user, value)?;
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
                                        mixer.clear();
                                    }
                                }
                            }
                            Some(VoiceOp::ClientDisconnect) => {
                                let user = signal::id(data, "user_id")?;
                                mixer.remove(user);
                                if clients.remove(&user) {
                                    emit(&events, Event::ClientDisconnected { user_id: user });
                                }
                                let was_group_member = dave.is_group_member(user);
                                let was_ready = dave.ready;
                                if dave.disconnect(user)? {
                                    if dave.alone() {
                                        capture_reset = true;
                                        mixer.clear();
                                        dave.enter_sole_member_waiting()?;
                                        deadline = None;
                                        waiting_announced = true;
                                        ready_announced = false;
                                    } else if was_ready {
                                        // The rest still share the current epoch until the removal commit.
                                        dave.ready = true;
                                        deadline = None;
                                    } else if was_group_member {
                                        deadline = Some(Instant::now() + RENEGOTIATION);
                                        capture_reset = true;
                                        mixer.clear();
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
                            Some(VoiceOp::DaveExecuteTransition) => dave.execute(signal::transition(data)?)?,
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
                                // Audio only: just the sender's audio SSRC.
                                let user = signal::id(data, "user_id")?;
                                if user != own
                                    && dave.contains(user)
                                    && let Some(value) = data["audio_ssrc"].as_u64().and_then(|v| u32::try_from(v).ok()).filter(|v| *v != 0)
                                {
                                    mixer.announce(user, value)?;
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
                                if let Some(response) = dave.proposals(frame.payload)? {
                                    send(&mut ws, Message::Binary(response.into())).await?;
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
                                    mixer.clear();
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
                        let code = frame.map_or(u16::from(CloseCode::Status), |frame| u16::from(frame.code));
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
        assert!(total < 12_000, "{total} bytes in a second");
    }
}
