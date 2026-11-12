//! The second voice connection of a Go Live stream, for the sender and for a watcher: its own control socket and UDP
//! path, its own DAVE group, and either the screen going out or the stream arriving.
//!
//! Adapted from Serein `crates/discord-voice/src/transport.rs` (`run_stream`, `run_inner`), MIT OR Apache-2.0, and
//! from this crate's `session::run`, whose socket, heartbeat, discovery and DAVE handling it repeats in a smaller
//! shape (the call loop interleaves audio devices and the mixer too tightly to share one function). Differences from the
//! official client are in DECISIONS.md.
use crate::{
    DisconnectReason, clock,
    dave::Dave,
    mixer::Mixer,
    rtp::{self, Mode, Transport},
    session::{
        DISCOVERY_TIMEOUT, HELLO_TIMEOUT, MAX_RESUMES, NEGOTIATION, PEER_GRACE, RENEGOTIATION,
        Socket, UDP_KEEPALIVE, UdpFailures, open, send, text, transient_receive,
    },
    signal::{self, CLOSE_DISCONNECTED, Context, Limits},
    streams::{Role, StreamJoin},
    video::{
        RemoteFrame, VideoHub, VideoSource,
        receive::{Decoders, Offer, Receivers},
        sender::VideoSender,
    },
};
use dc_media::{
    EncodedFrame,
    screen::{self, AudioChunk},
    synthetic::{self, Synthetic},
};
use dc_proto::voice_op::{VoiceOp, close};
use futures_util::StreamExt;
use opus2::{Application, Bitrate, Channels, Encoder};
use std::{collections::VecDeque, net::SocketAddr, time::Duration};
use tokio::{
    net::UdpSocket,
    sync::{mpsc, watch},
    time::{Instant, timeout},
};
use tokio_tungstenite::tungstenite::{Message, protocol::frame::coding::CloseCode};
use tracing::debug;
use zeroize::Zeroizing;

/// What a stream sender shows.
#[derive(Clone, Copy, Debug)]
pub enum StreamSource {
    /// A display or window with the picker's settings (`dc_media::screen`).
    Screen(screen::Settings),
    /// A moving test pattern, for capture-only runs that must not touch the owner's screen.
    Synthetic {
        width: u32,
        height: u32,
        fps: u32,
        bitrate: u32,
    },
}

impl StreamSource {
    fn limits(&self) -> Limits {
        match *self {
            Self::Screen(s) => Limits {
                bitrate: s.bitrate,
                framerate: s.fps,
                width: s.width,
                height: s.height,
            },
            Self::Synthetic {
                width,
                height,
                fps,
                bitrate,
            } => Limits {
                bitrate,
                framerate: fps,
                width,
                height,
            },
        }
    }
}

/// App-facing events of one stream connection.
#[derive(Clone, Debug, PartialEq)]
pub enum StreamEvent {
    /// The voice connection and its DAVE group are up. A sender still waits for a viewer before media flows.
    Connected,
    /// The sender's first keyframe left, or the watcher's first picture is on its way: the stream is live.
    Live { privacy_code: Option<String> },
    /// A JPEG thumbnail of the stream (at most 512 x 288) for `POST /streams/<key>/preview`.
    Preview { jpeg: Vec<u8> },
    /// What the watched stream says it sends at most (`max_resolution`, `max_framerate` of its op 12): the height,
    /// `None` for the source's own, and the frames per second.
    Quality { height: Option<u32>, fps: u32 },
    /// Another participant of the stream connection came or went (viewers of a sent stream).
    Participants { count: usize },
    /// The sender's capture or the connection ended. Always the last event, unless the task was aborted.
    Ended { reason: DisconnectReason },
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct StreamControlState {
    pub stop: bool,
    pub paused: bool,
    /// Whether the sender uploads previews (`audio.stream_previews`).
    pub previews: bool,
}

enum Capture {
    Screen(screen::Worker),
    Synthetic(Synthetic, u32, u32),
}

impl Capture {
    fn force_keyframe(&self) {
        match self {
            Self::Screen(worker) => worker.force_keyframe(),
            Self::Synthetic(source, ..) => source.force_keyframe(),
        }
    }
    fn set_bitrate(&self, bitrate: u32) {
        if let Self::Screen(worker) = self {
            worker.set_bitrate(bitrate);
        }
    }
    fn preview(&self) -> Option<dc_media::Picture> {
        match self {
            Self::Screen(worker) => worker.take_preview(),
            Self::Synthetic(_, width, height) => {
                let yuv = synthetic::pattern_i420(*width, *height, 0);
                let picture = dc_media::Picture {
                    width: *width,
                    height: *height,
                    layout: dc_media::Layout::I420,
                    data: yuv,
                };
                picture
                    .to_rgba()
                    .and_then(|rgba| dc_media::Picture::rgba(*width, *height, rgba))
            }
        }
    }
    fn finished(&self) -> Option<String> {
        match self {
            Self::Screen(worker) => worker
                .finished()
                .then(|| worker.error().unwrap_or_else(|| "capture ended".into())),
            Self::Synthetic(source, ..) => source.finished().then(|| "capture ended".into()),
        }
    }
}

/// Starts the capture on a blocking thread. Frames and system audio go to the loop through bounded channels and are
/// dropped when it is behind.
fn start_capture(
    source: StreamSource,
    frames: mpsc::Sender<EncodedFrame>,
    audio: mpsc::Sender<AudioChunk>,
    clips: dc_clips::Tap,
) -> Result<Capture, &'static str> {
    // The clip buffer sees every picture and every chunk of audio the capture makes, before the loop does: it is a
    // second consumer of the stream's own encode (plan section 1.2). It does nothing while clipping is off.
    let video_tap = clips.clone();
    let video = move |frame: EncodedFrame| {
        video_tap.push_video(&frame);
        let _ = frames.try_send(frame);
    };
    let audio_tap = clips.clone();
    let sound = move |chunk: AudioChunk| {
        audio_tap.push_audio(dc_clips::Track::Application, &chunk.samples);
        let _ = audio.try_send(chunk);
    };
    match source {
        StreamSource::Screen(settings) => screen::Worker::start(settings, video, sound)
            .map(Capture::Screen)
            .map_err(|_| "Screen capture could not start"),
        StreamSource::Synthetic {
            width,
            height,
            fps,
            bitrate,
        } => {
            test_tone(clips);
            Synthetic::start(width, height, fps, bitrate, video)
                .map(|source| Capture::Synthetic(source, width, height))
                .map_err(|_| "Test source could not start")
        }
    }
}

/// The test pattern has no audio, so a clip cut from it would have no audio track to check. This gives the clip
/// buffer (only the buffer: nothing goes to the network) a 440 Hz tone in 20 ms steps for as long as the stream is live.
/// A real source never runs it.
fn test_tone(clips: dc_clips::Tap) {
    let _ = std::thread::Builder::new()
        .name("dc-clips-test-tone".into())
        .spawn(move || {
            let mut phase = 0.0f32;
            while clips.is_live() {
                let mut samples = Vec::with_capacity(1920);
                for _ in 0..960 {
                    let value = (phase * std::f32::consts::TAU).sin() * 0.2;
                    phase = (phase + 440.0 / 48_000.0).fract();
                    samples.extend_from_slice(&[value, value]);
                }
                clips.push_audio(dc_clips::Track::Application, &samples);
                std::thread::sleep(Duration::from_millis(20));
            }
        });
}

/// A JPEG that fits 512 x 288 the way the official canvas does (`GK`, scaled by the smaller factor), quality 0.92.
pub(crate) fn jpeg_thumbnail(picture: &dc_media::Picture) -> Option<Vec<u8>> {
    use image::{DynamicImage, RgbaImage, codecs::jpeg::JpegEncoder, imageops};
    let rgba = picture.to_rgba()?;
    let image = RgbaImage::from_raw(picture.width, picture.height, rgba)?;
    let scale = (512.0 / f64::from(picture.width)).min(288.0 / f64::from(picture.height));
    let (width, height) = if scale < 1.0 {
        (
            ((f64::from(picture.width) * scale) as u32).max(1),
            ((f64::from(picture.height) * scale) as u32).max(1),
        )
    } else {
        (picture.width, picture.height)
    };
    let image = if (width, height) == (picture.width, picture.height) {
        image
    } else {
        imageops::resize(&image, width, height, imageops::FilterType::Triangle)
    };
    let rgb = DynamicImage::ImageRgba8(image).to_rgb8();
    let mut jpeg = Vec::new();
    JpegEncoder::new_with_quality(&mut jpeg, 92)
        .encode_image(&rgb)
        .ok()?;
    Some(jpeg)
}

/// First preview after the debounce the official client uses; then every five minutes, every minute while paused.
const PREVIEW_FIRST: Duration = Duration::from_millis(500);
const PREVIEW_INTERVAL: Duration = Duration::from_secs(300);
const PREVIEW_PAUSED_INTERVAL: Duration = Duration::from_secs(60);
/// How often the sender's own tile gets a new picture.
const LOCAL_PREVIEW_INTERVAL: Duration = Duration::from_millis(200);
/// A watcher refreshes its sink wants while video stalls or before the server forgets them (Serein: five seconds).
const SINK_WANTS_INTERVAL: Duration = Duration::from_secs(5);
/// Longest stream audio backlog before the oldest frame goes.
const AUDIO_BACKLOG: usize = 1920 * 6;

pub(crate) async fn run(
    join: StreamJoin,
    source: Option<StreamSource>,
    mut controls: watch::Receiver<StreamControlState>,
    hub: VideoHub,
    events: mpsc::UnboundedSender<StreamEvent>,
) -> Result<DisconnectReason, &'static str> {
    let url = signal::endpoint_url(&join.endpoint)?;
    let own = join.user_id;
    let owner = join.key.owner();
    let sending = join.role == Role::Send;
    if sending && source.is_none() {
        return Err("A stream sender needs a source");
    }
    if controls.borrow_and_update().stop {
        return Ok(DisconnectReason::Requested);
    }
    let token = Zeroizing::new(join.token.clone());
    let session_id = Zeroizing::new(join.session_id.clone());
    let ids = || signal::Ids {
        server_id: join.server_id,
        channel_id: join.channel_id,
        user_id: own,
        session_id: &session_id,
        token: &token,
    };
    let mut ws: Socket = open(&url).await?;
    text(&mut ws, signal::identify(&ids(), Context::Stream)).await?;

    let mut dave = Dave::new(own, join.channel_id)?;
    let mut mode: Option<Mode> = None;
    let mut transport: Option<Transport> = None;
    let mut secured_at: Option<Instant> = None;
    let mut udp: Option<UdpSocket> = None;
    let mut udp_failures = UdpFailures::default();
    let mut next_udp_ping = Instant::now();
    let mut udp_ping_sequence = 0u32;
    let mut discovering = false;
    let mut discovery_deadline = Instant::now();
    let mut ssrc = 0u32;
    let mut video_ssrc = 0u32;
    let mut rtx_ssrc = 0u32;
    let mut seq_ack: i64 = -1;
    let mut server_version = 0u64;
    let mut heartbeat_ms: Option<u64> = None;
    let mut heartbeat_at = Instant::now();
    let mut hello_deadline = Instant::now() + HELLO_TIMEOUT;
    let mut awaiting_ack = None;
    let mut heartbeat_nonce = 0u64;
    let mut missed_acks = 0u8;
    let mut reconnect: Option<&'static str> = None;
    let mut deadline = Some(Instant::now() + NEGOTIATION);
    let mut resuming = false;
    let mut resume_attempts = 0u8;
    let mut connected = false;
    let mut waiting_announced = false;
    let mut signal_window = Instant::now();
    let mut signal_count = 0u16;
    let mut packet = [0u8; rtp::MAX_PACKET + 1];
    let mut ticks = clock::ticks()?;
    let mut clients = std::collections::BTreeSet::new();

    // Sender state.
    let (frame_send, mut frames) = mpsc::channel::<EncodedFrame>(4);
    let (audio_send, mut sound) = mpsc::channel::<AudioChunk>(32);
    let mut capture: Option<Capture> = None;
    let mut capture_task: Option<tokio::task::JoinHandle<Result<Capture, &'static str>>> = None;
    // The clip buffer's tap while this sender runs; dropping it, on any exit, frees the buffer.
    let mut _clip_guard: Option<dc_clips::TapGuard> = None;
    let mut sender: Option<VideoSender> = None;
    let mut announced = false;
    let mut awaiting_keyframe = true;
    let mut live_announced = false;
    let mut sent_keyframe = false;
    let mut pending_audio: VecDeque<f32> = VecDeque::new();
    let mut audio_encoder = match sending {
        true => {
            let fail = |_| "Stream audio encoder configuration failed";
            let mut encoder =
                Encoder::new(48_000, Channels::Stereo, Application::Audio).map_err(fail)?;
            encoder.set_bitrate(Bitrate::Bits(128_000)).map_err(fail)?;
            Some(encoder)
        }
        false => None,
    };
    let mut audio_sequence: u16 = rand::random();
    let mut audio_timestamp: u32 = rand::random();
    let mut audio_speaking = false;
    let mut encoded = [0u8; 1275];
    let mut preview_at = Instant::now() + PREVIEW_FIRST;
    let mut previewed = false;
    // The sender's own tile: the capture's newest picture, handed to the sink a few times a second. The preview upload
    // reads the same picture, because taking a preview from the capture consumes it.
    let mut local_preview_at = Instant::now();
    let mut last_preview: Option<dc_media::Picture> = None;

    // Watcher state.
    let mut receivers = Receivers::default();
    let mut decoders = Decoders::new(hub.clone(), VideoSource::Stream);
    let mut mixer = Mixer::default();
    let mut sink_wants_at = Instant::now();
    let mut tally = [0u32; 4];
    let mut tally_at = Instant::now() + Duration::from_secs(5);

    loop {
        if let Some(reason) = reconnect.take() {
            if transport.is_none() || resume_attempts >= MAX_RESUMES {
                return Err(reason);
            }
            debug!("stream socket lost; resuming");
            resume_attempts += 1;
            resuming = true;
            waiting_announced = false;
            deadline = Some(Instant::now() + RENEGOTIATION);
            hello_deadline = Instant::now() + HELLO_TIMEOUT;
            heartbeat_ms = None;
            awaiting_ack = None;
            missed_acks = 0;
            ws = open(&url).await.map_err(|_| "Stream resume failed")?;
            text(&mut ws, signal::resume(&ids(), seq_ack)).await?;
        }
        let pacing = sender.as_ref().and_then(VideoSender::deadline);
        tokio::select! {
            changed = controls.changed() => {
                if changed.is_err() || controls.borrow().stop {
                    if announced && sending {
                        let _ = text(&mut ws, signal::video(ssrc, None)).await;
                    }
                    let _ = timeout(Duration::from_secs(1), ws.close(None)).await;
                    return Ok(DisconnectReason::Requested);
                }
            },
            _ = async { match pacing { Some(at) => tokio::time::sleep_until(at).await, None => std::future::pending().await } } => {
                if let (Some(sender), Some(crypto), Some(socket)) = (sender.as_mut(), transport.as_mut(), udp.as_ref()) {
                    let was_empty = sender.ready();
                    sender.flush(Instant::now(), crypto, |data| udp_failures.send(socket, data, "Stream UDP send failed"))?;
                    if !was_empty && sender.ready() && sent_keyframe && !live_announced {
                        live_announced = true;
                        awaiting_keyframe = false;
                        let _ = events.send(StreamEvent::Live { privacy_code: dave.session.voice_privacy_code().map(str::to_owned) });
                    }
                }
            },
            frame = frames.recv(), if sender.as_ref().is_some_and(VideoSender::ready) => {
                let Some(frame) = frame else { return Ok(DisconnectReason::Closed(0)) };
                let secure = announced && dave.ready && dave.session.is_ready() && !dave.transitioning() && transport.is_some() && !discovering && !controls.borrow().paused;
                if !secure {
                    awaiting_keyframe = true;
                    sent_keyframe = false;
                    if let Some(capture) = &capture {
                        capture.force_keyframe();
                    }
                    continue;
                }
                if awaiting_keyframe && !frame.keyframe && !sent_keyframe {
                    if let Some(capture) = &capture {
                        capture.force_keyframe();
                    }
                    continue;
                }
                if let Some(sender) = sender.as_mut() {
                    match sender.offer(&frame, &mut dave, Instant::now()) {
                        Ok(true) => sent_keyframe |= frame.keyframe,
                        Ok(false) => {}
                        Err(error) => {
                            // A bad picture costs that picture, not the stream.
                            debug!(error, "stream picture dropped");
                            sent_keyframe = false;
                            if let Some(capture) = &capture {
                                capture.force_keyframe();
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
                    return Err(if heartbeat_ms.is_none() {
                        "Stream voice Hello timed out"
                    } else if transport.is_none() {
                        "Stream voice negotiation timed out"
                    } else {
                        "Stream DAVE negotiation timed out"
                    });
                }
                if heartbeat_ms.is_none() && now >= hello_deadline {
                    if transport.is_none() {
                        return Err("Stream voice Hello timed out");
                    }
                    reconnect = Some("Stream voice Hello timed out");
                    continue;
                }
                if discovering && now >= discovery_deadline {
                    return Err("Stream voice UDP discovery timed out");
                }
                if !discovering && now >= next_udp_ping && let Some(socket) = &udp {
                    udp_ping_sequence = udp_ping_sequence.wrapping_add(1);
                    udp_failures.send(socket, &rtp::keepalive(udp_ping_sequence), "Stream UDP keepalive failed")?;
                    next_udp_ping = now + UDP_KEEPALIVE;
                }
                if let Some(interval) = heartbeat_ms && now >= heartbeat_at {
                    if awaiting_ack.is_some() {
                        missed_acks += 1;
                        if missed_acks >= 2 {
                            reconnect = Some("Stream voice heartbeat was not acknowledged");
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
                if waiting && !waiting_announced {
                    deadline = None;
                    waiting_announced = true;
                }
                if enabled {
                    deadline = None;
                }
                if !waiting && !enabled {
                    waiting_announced = false;
                }
                if (enabled || waiting) && !connected {
                    connected = true;
                    let _ = events.send(StreamEvent::Connected);
                }
                let secure = enabled && dave.session.is_ready() && !dave.transitioning();

                if sending {
                    // Capture starts once the connection is secured or waiting; the blocking start runs off the loop.
                    if (enabled || waiting) && capture.is_none() && capture_task.is_none() && let Some(source) = source {
                        let (frames, audio) = (frame_send.clone(), audio_send.clone());
                        let tap = dc_clips::recorder().attach();
                        _clip_guard = Some(dc_clips::TapGuard(tap.clone()));
                        capture_task = Some(tokio::task::spawn_blocking(move || start_capture(source, frames, audio, tap)));
                    }
                    if capture_task.as_ref().is_some_and(|task| task.is_finished()) {
                        let task = capture_task.take().ok_or("Capture task missing")?;
                        capture = Some(task.await.map_err(|_| "Capture task failed")??);
                    }
                    if let Some(capture) = &capture && let Some(reason) = capture.finished() {
                        debug!(reason, "stream capture ended");
                        let _ = text(&mut ws, signal::video(ssrc, None)).await;
                        return Err("Screen capture ended");
                    }
                    // Announce the source once, as soon as both ends are ready for it.
                    if !announced && capture.is_some() && transport.is_some() && !discovering && !resuming && let Some(source) = source {
                        let bitrate = source.limits().bitrate;
                        sender = Some(VideoSender::new(video_ssrc, rtx_ssrc, bitrate));
                        text(&mut ws, signal::video(ssrc, Some((video_ssrc, rtx_ssrc, Context::Stream, Some(source.limits()))))).await?;
                        announced = true;
                        debug!("stream source announced");
                    }
                    if !secure && let Some(sender) = sender.as_mut() {
                        sender.clear();
                        awaiting_keyframe = true;
                        sent_keyframe = false;
                        live_announced = false;
                    }
                    if sender.as_ref().is_some_and(|sender| sender.stale(now)) {
                        if let Some(sender) = sender.as_mut() {
                            sender.clear();
                        }
                        awaiting_keyframe = true;
                        sent_keyframe = false;
                        if let Some(capture) = &capture {
                            capture.force_keyframe();
                        }
                    }
                    if let (Some(sender), Some(capture)) = (sender.as_mut(), &capture)
                        && let Some(bitrate) = sender.tick_rate(now)
                    {
                        capture.set_bitrate(bitrate);
                        debug!(bitrate, "stream bit rate steered");
                    }
                    // System audio: stereo 20 ms frames into Opus, DAVE, soundshare.
                    while let Ok(chunk) = sound.try_recv() {
                        pending_audio.extend(chunk.samples);
                    }
                    while pending_audio.len() > AUDIO_BACKLOG {
                        pending_audio.drain(..1920);
                    }
                    if secure && !controls.borrow().paused && sent_keyframe && pending_audio.len() >= 1920 {
                        let stereo: Vec<f32> = pending_audio.drain(..1920).collect();
                        if !audio_speaking {
                            text(&mut ws, signal::speaking_flags(2, ssrc)).await?;
                            audio_speaking = true;
                        }
                        let encoder = audio_encoder.as_mut().ok_or("Stream audio encoder missing")?;
                        let length = encoder.encode_float(&stereo, &mut encoded).map_err(|_| "Stream audio encoding failed")?;
                        let data = dave.session.encrypt_opus(&encoded[..length]).map_err(|_| "DAVE stream audio encryption failed")?.into_owned();
                        let header = rtp::header(audio_sequence, audio_timestamp, ssrc);
                        if let (Some(crypto), Some(socket)) = (transport.as_mut(), udp.as_ref()) {
                            udp_failures.send(socket, &crypto.seal_soundshare(&header, &data)?, "Stream audio UDP send failed")?;
                        }
                        audio_sequence = audio_sequence.wrapping_add(1);
                        audio_timestamp = audio_timestamp.wrapping_add(rtp::FRAME_SAMPLES);
                    } else if !secure {
                        pending_audio.clear();
                    }
                    if now >= local_preview_at && let Some(capture) = &capture {
                        local_preview_at = now + LOCAL_PREVIEW_INTERVAL;
                        if let Some(picture) = capture.preview() {
                            if hub.has_sink() {
                                hub.deliver(RemoteFrame {
                                    user: owner,
                                    source: VideoSource::Stream(owner),
                                    picture: picture.clone(),
                                });
                            }
                            last_preview = Some(picture);
                        }
                    }
                    // Preview: the official client waits for a real picture, then uploads every five minutes.
                    if secure && sent_keyframe && controls.borrow().previews && now >= preview_at && capture.is_some() {
                        if let Some(picture) = last_preview.as_ref()
                            && let Some(jpeg) = jpeg_thumbnail(picture)
                        {
                            let _ = events.send(StreamEvent::Preview { jpeg });
                            previewed = true;
                        }
                        let paused = controls.borrow().paused;
                        preview_at = now + if previewed && !paused { PREVIEW_INTERVAL } else if paused { PREVIEW_PAUSED_INTERVAL } else { PREVIEW_FIRST * 20 };
                    }
                } else {
                    // Watching: audio from the mixer into the call's bus; keep the sink wants alive; feedback upstream.
                    if secure {
                        if let Some(frame) = mixer.pop() {
                            hub.push_stream_audio(frame);
                        }
                        let mut ready = Vec::new();
                        receivers.drain(now, &mut ready);
                        for unit in ready {
                            if !dave.contains(unit.user) {
                                continue;
                            }
                            let Ok(plain) = dave.session.decrypt(unit.user, davey::MediaType::VIDEO, &unit.data) else {
                                receivers.require_keyframe(unit.user);
                                continue;
                            };
                            tally[3] += 1;
                            let keyframe = dc_media::h264::is_keyframe(&plain);
                            if !receivers.accept(unit.user, keyframe) {
                                continue;
                            }
                            if decoders.offer(unit.user, plain, now) == Offer::Dropped {
                                receivers.require_keyframe(unit.user);
                            }
                        }
                        for user in decoders.take_lost() {
                            receivers.require_keyframe(user);
                        }
                        if let (Some(crypto), Some(socket)) = (transport.as_mut(), udp.as_ref()) {
                            for (header, body) in receivers.feedback(ssrc, now) {
                                udp_failures.send(socket, &crypto.seal_rtcp(&header, &body)?, "Stream feedback send failed")?;
                            }
                        }
                        if now >= sink_wants_at && hub.has_sink() {
                            text(&mut ws, signal::media_sink_wants(100)).await?;
                            sink_wants_at = now + SINK_WANTS_INTERVAL;
                        }
                    } else {
                        mixer.clear();
                        hub.clear_stream_audio();
                    }
                }
                if now >= tally_at {
                    debug!(
                        received = tally[0],
                        opened = tally[1],
                        video_packets = tally[2],
                        pictures = tally[3],
                        sending,
                        ready = dave.ready,
                        "stream packet tally"
                    );
                    tally = [0; 4];
                    tally_at = now + Duration::from_secs(5);
                }
            },
            result = async { match &udp { Some(socket) => socket.recv(&mut packet).await, None => std::future::pending().await } } => {
                let length = match result {
                    Ok(length) => length,
                    Err(error) if transient_receive(&error) => continue,
                    Err(_) => return Err("Stream UDP receive failed"),
                };
                if length > rtp::MAX_PACKET {
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
                let Some(crypto) = &transport else { continue };
                if rtp::is_rtcp(&packet[..length]) {
                    if let Some(sender) = sender.as_mut()
                        && let Some(feedback) = crypto.feedback(&packet[..length], video_ssrc)
                        && announced
                        && dave.ready
                        && !dave.transitioning()
                        && sender.feedback(&feedback, Instant::now())
                        && let Some(capture) = &capture
                    {
                        capture.force_keyframe();
                    }
                    continue;
                }
                if sending {
                    continue;
                }
                let Some(rtp) = crypto.open(&packet[..length]) else { continue };
                tally[1] += 1;
                if !dave.ready {
                    continue;
                }
                match rtp.payload_type {
                    rtp::OPUS_PAYLOAD_TYPE => {
                        let Some(user) = mixer.user(rtp.ssrc) else { continue };
                        if !dave.contains(user) {
                            continue;
                        }
                        let Ok(opus) = dave.session.decrypt(user, davey::MediaType::AUDIO, &rtp.payload) else { continue };
                        mixer.push(rtp.ssrc, rtp.sequence, opus);
                    }
                    rtp::H264_PAYLOAD_TYPE | rtp::H264_RTX_PAYLOAD_TYPE => {
                        tally[2] += 1;
                        receivers.push(rtp, Instant::now());
                    }
                    _ => {}
                }
            },
            event = ws.next() => {
                let event = match event {
                    Some(Ok(message)) if !matches!(&message, Message::Close(Some(frame)) if u16::from(frame.code) == close::SERVER_CRASH) => message,
                    _ => {
                        reconnect = Some("Stream voice socket failed");
                        continue;
                    }
                };
                if signal_window.elapsed() > Duration::from_secs(1) {
                    signal_window = Instant::now();
                    signal_count = 0;
                }
                signal_count += 1;
                if signal_count > 256 {
                    return Err("Stream signaling exceeded the bounded processing rate");
                }
                match event {
                    Message::Text(body) => {
                        let mut message = signal::parse_text(&body)?;
                        if let Some(seq) = message.seq {
                            seq_ack = seq;
                        }
                        let data = &mut message.d;
                        debug!(op = message.raw_op, "stream signaling op received");
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
                                    if !resuming {
                                        resume_attempts = 0;
                                    }
                                }
                            }
                            Some(VoiceOp::Ready) => {
                                if udp.is_some() {
                                    return Err("Unexpected stream transport replacement");
                                }
                                let ready = signal::ready(data)?;
                                if !rtp::public_ip(ready.ip) {
                                    return Err("Voice server advertised a nonpublic address");
                                }
                                ssrc = ready.ssrc;
                                video_ssrc = ready.video_ssrc;
                                rtx_ssrc = ready.rtx_ssrc;
                                mode = Some(ready.mode);
                                if server_version >= 6 {
                                    text(&mut ws, signal::backend_version()).await?;
                                }
                                let socket = UdpSocket::bind(if ready.ip.is_ipv4() { "0.0.0.0:0" } else { "[::]:0" })
                                    .await
                                    .map_err(|_| "Could not bind stream UDP socket")?;
                                socket
                                    .connect(SocketAddr::new(ready.ip, ready.port))
                                    .await
                                    .map_err(|_| "Could not connect stream UDP socket")?;
                                socket
                                    .send(&rtp::discovery_request(ssrc))
                                    .await
                                    .map_err(|_| "Stream UDP discovery failed")?;
                                udp = Some(socket);
                                discovering = true;
                                discovery_deadline = Instant::now() + DISCOVERY_TIMEOUT;
                            }
                            Some(VoiceOp::SelectProtocolAck) => {
                                if transport.is_some() || udp.is_none() || discovering {
                                    return Err("Unexpected stream session description");
                                }
                                let mode = mode.ok_or("Unexpected stream session description")?;
                                let key = signal::session_description(data, mode)?;
                                transport = Some(Transport::new(mode, &key));
                                secured_at = Some(Instant::now());
                                text(&mut ws, signal::speaking_flags(0, ssrc)).await?;
                                send(&mut ws, Message::Binary(dave.key_package()?.into())).await?;
                                if !sending && hub.has_sink() {
                                    text(&mut ws, signal::media_sink_wants(100)).await?;
                                    sink_wants_at = Instant::now() + SINK_WANTS_INTERVAL;
                                }
                            }
                            Some(VoiceOp::Speaking) => {
                                // The owner's audio source (stream audio).
                                let user = signal::id(data, "user_id")?;
                                if !sending && user == owner && dave.contains(user) {
                                    let value = u32::try_from(signal::number(data, "ssrc")?).map_err(|_| "Invalid voice SSRC")?;
                                    mixer.announce(user, value)?;
                                }
                            }
                            Some(VoiceOp::Video) => {
                                let remote = signal::remote_video(data)?;
                                if !sending && remote.user == owner {
                                    if let Some((height, fps)) = remote.quality {
                                        let _ = events.send(StreamEvent::Quality { height, fps });
                                    }
                                    if remote.audio_ssrc != 0 && dave.contains(remote.user) {
                                        mixer.announce(remote.user, remote.audio_ssrc)?;
                                    }
                                    match remote.video {
                                        Some((video, rtx)) => {
                                            receivers.announce(remote.user, video, rtx)?;
                                            debug!("stream video source announced");
                                        }
                                        None => {
                                            receivers.remove(remote.user);
                                            decoders.remove(remote.user);
                                        }
                                    }
                                }
                            }
                            Some(VoiceOp::ClientConnect) => {
                                let ids = signal::client_connect(data)?;
                                for user in &ids {
                                    if *user != own {
                                        clients.insert(*user);
                                    }
                                }
                                let _ = events.send(StreamEvent::Participants { count: clients.len() });
                                let was_ready = dave.ready;
                                if dave.connect(&ids)? {
                                    if was_ready {
                                        dave.ready = true;
                                    } else {
                                        deadline = Some(Instant::now() + NEGOTIATION);
                                    }
                                }
                                // A new viewer needs a keyframe to start from.
                                if let Some(capture) = &capture {
                                    capture.force_keyframe();
                                }
                            }
                            Some(VoiceOp::ClientDisconnect) => {
                                let user = signal::id(data, "user_id")?;
                                mixer.remove(user);
                                if clients.remove(&user) {
                                    let _ = events.send(StreamEvent::Participants { count: clients.len() });
                                }
                                if !sending && user == owner {
                                    receivers.remove(user);
                                    decoders.remove(user);
                                }
                                let was_group_member = dave.is_group_member(user);
                                let was_ready = dave.ready;
                                if dave.disconnect(user)? {
                                    if dave.alone() {
                                        dave.enter_sole_member_waiting()?;
                                        deadline = None;
                                        waiting_announced = true;
                                    } else if was_ready {
                                        dave.ready = true;
                                        deadline = None;
                                    } else if was_group_member {
                                        deadline = Some(Instant::now() + RENEGOTIATION);
                                    }
                                }
                            }
                            Some(VoiceOp::DavePrepareTransition) => {
                                if signal::number(data, "protocol_version")? != signal::DAVE_PROTOCOL_VERSION {
                                    return Err("Discord requested a stream encryption downgrade");
                                }
                                let id = signal::transition(data)?;
                                dave.prepare_protocol(id);
                                if id == 0 {
                                    if dave.session.is_ready() {
                                        dave.execute(0)?;
                                    } else if dave.alone() {
                                        dave.enter_sole_member_waiting()?;
                                        deadline = None;
                                        waiting_announced = true;
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
                                    return Err("Unsupported DAVE protocol version");
                                }
                                if signal::number(data, "epoch")? == 1 {
                                    dave.reinitialize()?;
                                    deadline = Some(Instant::now() + RENEGOTIATION);
                                    send(&mut ws, Message::Binary(dave.key_package()?.into())).await?;
                                }
                            }
                            Some(VoiceOp::Resumed) => {
                                if !resuming {
                                    return Err("Unexpected stream resumption");
                                }
                                text(&mut ws, signal::speaking_flags(0, ssrc)).await?;
                                audio_speaking = false;
                                resuming = false;
                            }
                            _ => debug!(op = message.raw_op, "stream signaling op ignored"),
                        }
                    }
                    Message::Binary(bytes) => {
                        let frame = signal::parse_binary(&bytes)?;
                        debug!(op = frame.op, "stream binary op received");
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
                                    deadline = Some(Instant::now() + RENEGOTIATION);
                                }
                                // The epoch changed: pictures and audio of the old one are gone.
                                if let Some(sender) = sender.as_mut() {
                                    sender.clear();
                                }
                                if let Some(capture) = &capture {
                                    capture.force_keyframe();
                                }
                                mixer.clear();
                            }
                            _ => return Err("Unsupported DAVE opcode; stream stopped"),
                        }
                    }
                    Message::Ping(data) => send(&mut ws, Message::Pong(data)).await?,
                    Message::Pong(_) => {}
                    Message::Close(frame) => {
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
    fn thumbnails_fit_512_by_288_and_are_jpeg() {
        let wide = dc_media::Picture::rgba(1920, 1080, vec![200; 1920 * 1080 * 4]).unwrap();
        let jpeg = jpeg_thumbnail(&wide).unwrap();
        assert_eq!(&jpeg[..3], &[0xff, 0xd8, 0xff], "JPEG start of image");
        let decoded = image::load_from_memory_with_format(&jpeg, image::ImageFormat::Jpeg).unwrap();
        assert_eq!((decoded.width(), decoded.height()), (512, 288));
        // A tall picture is limited by the height; a small one is not enlarged.
        let tall = dc_media::Picture::rgba(100, 400, vec![9; 100 * 400 * 4]).unwrap();
        let decoded = image::load_from_memory(&jpeg_thumbnail(&tall).unwrap()).unwrap();
        assert_eq!((decoded.width(), decoded.height()), (72, 288));
        let small = dc_media::Picture::rgba(64, 48, vec![9; 64 * 48 * 4]).unwrap();
        let decoded = image::load_from_memory(&jpeg_thumbnail(&small).unwrap()).unwrap();
        assert_eq!((decoded.width(), decoded.height()), (64, 48));
    }

    #[test]
    fn limits_follow_the_source() {
        let source = StreamSource::Synthetic {
            width: 320,
            height: 180,
            fps: 15,
            bitrate: 500_000,
        };
        assert_eq!(
            source.limits(),
            Limits {
                bitrate: 500_000,
                framerate: 15,
                width: 320,
                height: 180
            }
        );
    }
}
