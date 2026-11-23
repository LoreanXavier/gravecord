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
    playout::CallPlayout,
    rtc_stats::{
        InboundAudioStats, InboundUserStats, InboundVideoStats, OutboundAudioStats, OutboundStats,
        OutboundVideoStats, StreamStats, TransportStats,
    },
    rtp::{self, Mode, Transport},
    session::{
        DISCOVERY_RETRY, DISCOVERY_TIMEOUT, HELLO_TIMEOUT, MAX_RESUMES, NEGOTIATION, OpenedSocket,
        PEER_GRACE, RENEGOTIATION, RESUME_WINDOW, Socket, UDP_KEEPALIVE, UdpFailures,
        close_code_is_resumable, log_ws_close, log_ws_stage_err, log_ws_stage_ok, open_with_stages,
        rebind_udp, refuse, sanitized_ws_error, send_or_resume, text, text_or_resume,
        transient_receive, ws_resume_backoff,
    },
    signal::{self, CLOSE_DISCONNECTED, Context, DaveAt, Limits},
    streams::{Role, StreamJoin},
    video::{
        RemoteFrame, VideoHub, VideoSource,
        receive::{self, Decoders, Receivers},
        sender::{CALL_START_BITRATE, VideoSender},
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
use std::{
    collections::{HashMap, VecDeque},
    net::SocketAddr,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use tokio::{
    net::UdpSocket,
    sync::{mpsc, watch},
    time::{Instant, timeout},
};
use tokio_tungstenite::tungstenite::{Message, protocol::frame::coding::CloseCode};
use tracing::debug;
use zeroize::Zeroizing;

const SCREEN_CAPTURE_ENDED: &str = "Screen capture ended";
const SCREEN_CAPTURE_FAILED: &str = "Screen capture could not start";
const NO_SOURCE: &str = "A stream sender needs a source";
const PACER_STOPPED: &str = "Pacer clock stopped";
const MEDIA_CLOCK_STOPPED: &str = "Media clock stopped";

/// Whether a stream connection ended for a reason of this machine (its capture, its clocks, a stop asked for) rather
/// than of its socket. Module 921747 `_handleDisconnect` reconnects after a socket disconnect; a sender whose capture
/// is gone has nothing to reconnect with, so its stream ends instead.
pub fn ended_locally(reason: &DisconnectReason) -> bool {
    match reason {
        DisconnectReason::Requested => true,
        DisconnectReason::Failed(text) => [
            SCREEN_CAPTURE_ENDED,
            SCREEN_CAPTURE_FAILED,
            NO_SOURCE,
            PACER_STOPPED,
            MEDIA_CLOCK_STOPPED,
        ]
        .contains(text),
        DisconnectReason::Kicked | DisconnectReason::Closed(_) => false,
    }
}

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
    /// The call simulator's share (`sim` feature): it stands in for the screen, rate control included.
    #[cfg(feature = "sim")]
    Sim(SimShare),
}

/// A simulated share and the tone its stream audio carries; the tone stops with it.
#[cfg(feature = "sim")]
struct SimShare {
    source: dc_media::sim::Source,
    tone: Arc<AtomicBool>,
}

#[cfg(feature = "sim")]
impl Drop for SimShare {
    fn drop(&mut self) {
        self.tone.store(false, Ordering::Release);
    }
}

/// The call simulator's capture (`sim` feature): whatever was asked for, a screen or the test pattern, becomes a
/// simulated share of the same size, rate and limits, drawing what the session's plan says (moving content by default),
/// with its tone as the stream's audio.
#[cfg(feature = "sim")]
fn start_simulated_capture(
    plan: crate::sim::Plan,
    source: StreamSource,
    frames: mpsc::Sender<EncodedFrame>,
    audio: mpsc::Sender<AudioChunk>,
    dropped: Arc<AtomicBool>,
) -> Result<Capture, &'static str> {
    let limits = source.limits();
    let content = plan.share.unwrap_or(dc_media::sim::Content::Motion);
    let video = move |frame: EncodedFrame| {
        if frames.try_send(frame).is_err() {
            dropped.store(true, Ordering::Release);
        }
    };
    let started = dc_media::sim::Source::start(
        content,
        limits.width,
        limits.height,
        limits.framerate,
        CALL_START_BITRATE.min(limits.bitrate),
        video,
    )
    .map_err(|_| "Simulated share could not start")?;
    let tone = Arc::new(AtomicBool::new(true));
    let with_sound = match source {
        StreamSource::Screen(s) => s.audio,
        StreamSource::Synthetic { .. } => true,
    };
    if with_sound && let Some(hz) = plan.share_tone_hz {
        let running = tone.clone();
        let _ = std::thread::Builder::new()
            .name("dc-voice-sim-share-tone".into())
            .spawn(move || {
                let mut phase = 0.0f32;
                let mut next = std::time::Instant::now();
                while running.load(Ordering::Acquire) {
                    let mono = crate::sim::tone(hz, 0.25, &mut phase);
                    let samples = mono.iter().flat_map(|sample| [*sample, *sample]).collect();
                    let _ = audio.try_send(AudioChunk { samples });
                    next += Duration::from_millis(20);
                    let now = std::time::Instant::now();
                    if next > now {
                        std::thread::sleep(next - now);
                    } else {
                        next = now;
                    }
                }
            });
    }
    Ok(Capture::Sim(SimShare {
        source: started,
        tone,
    }))
}

impl Capture {
    fn force_keyframe(&self) {
        match self {
            Self::Screen(worker) => worker.force_keyframe(),
            Self::Synthetic(source, ..) => source.force_keyframe(),
            #[cfg(feature = "sim")]
            Self::Sim(share) => share.source.force_keyframe(),
        }
    }
    fn set_bitrate(&self, bitrate: u32) {
        if let Self::Screen(worker) = self {
            worker.set_bitrate(bitrate);
        }
        #[cfg(feature = "sim")]
        if let Self::Sim(share) = self {
            share.source.set_bitrate(bitrate);
        }
    }
    fn hold(&self, held: bool) {
        if let Self::Screen(worker) = self {
            worker.hold(held);
        }
        #[cfg(feature = "sim")]
        if let Self::Sim(share) = self {
            share.source.hold(held);
        }
    }
    fn encoder_name(&self) -> &'static str {
        match self {
            Self::Screen(worker) => {
                #[cfg(windows)]
                if worker.backend() == Some(dc_media::Backend::Hardware) {
                    return "MediaFoundation";
                }
                let _ = worker;
                "OpenH264"
            }
            Self::Synthetic(..) => "OpenH264",
            #[cfg(feature = "sim")]
            Self::Sim(_) => "Simulated",
        }
    }
    fn soundshare_failed(&self) -> bool {
        match self {
            Self::Screen(worker) => worker.soundshare_failed(),
            Self::Synthetic(..) => false,
            #[cfg(feature = "sim")]
            Self::Sim(_) => false,
        }
    }
    /// The owner's own tile shows what the encoder takes, every picture, as the native engine's local video sink does
    /// (the self stream tile of module 555148 renders the stream's own video). Only the test pattern keeps the timed
    /// preview of `LOCAL_PREVIEW_INTERVAL`.
    fn show_locally(&self, hub: VideoHub, owner: u64) -> bool {
        match self {
            Self::Screen(worker) => {
                worker.set_local_sink(Some(Arc::new(move |picture| {
                    if hub.has_sink() {
                        hub.deliver(RemoteFrame {
                            user: owner,
                            source: VideoSource::Stream(owner),
                            picture,
                        });
                    }
                })));
                true
            }
            Self::Synthetic(..) => false,
            #[cfg(feature = "sim")]
            Self::Sim(share) => {
                share.source.set_local_sink(Some(Arc::new(move |picture| {
                    if hub.has_sink() {
                        hub.deliver(RemoteFrame {
                            user: owner,
                            source: VideoSource::Stream(owner),
                            picture,
                        });
                    }
                })));
                true
            }
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
            #[cfg(feature = "sim")]
            Self::Sim(share) => share.source.take_preview(),
        }
    }
    fn finished(&self) -> Option<String> {
        match self {
            Self::Screen(worker) => worker
                .finished()
                .then(|| worker.error().unwrap_or_else(|| "capture ended".into())),
            Self::Synthetic(source, ..) => source.finished().then(|| "capture ended".into()),
            #[cfg(feature = "sim")]
            Self::Sim(share) => share.source.finished().then(|| "capture ended".into()),
        }
    }
}

/// Starts the capture on a blocking thread. Frames and system audio go to the loop through bounded channels and are
/// dropped when it is behind; a dropped picture sets `dropped`, so the loop asks for a keyframe (the pictures after it
/// predict from one the viewers never got).
/// Starting bitrate for a Go Live stream: high-tier shares (> 720p or > 30 fps, 9 Mb/s cap) open at 2 Mb/s so the
/// hardware encoder's initial frames stay crisp at 60 fps without choking the pacer and the first exponential probe pair
/// reaches 9 Mb/s in one round; standard shares start at `CALL_START_BITRATE`.
fn stream_start_bitrate(max_bitrate: u32) -> u32 {
    if max_bitrate > screen::BITRATE_STANDARD {
        2_000_000.min(max_bitrate)
    } else {
        CALL_START_BITRATE.min(max_bitrate)
    }
}

fn start_capture(
    source: StreamSource,
    frames: mpsc::Sender<EncodedFrame>,
    audio: mpsc::Sender<AudioChunk>,
    clips: dc_clips::Tap,
    dropped: Arc<AtomicBool>,
) -> Result<Capture, &'static str> {
    // The clip buffer sees every picture and every chunk of audio the capture makes, before the loop does: it is a
    // second consumer of the stream's own encode (plan section 1.2). It does nothing while clipping is off.
    let video_tap = clips.clone();
    let video = move |frame: EncodedFrame| {
        video_tap.push_video(&frame);
        if frames.try_send(frame).is_err() {
            dropped.store(true, Ordering::Release);
        }
    };
    let audio_tap = clips.clone();
    let sound = move |chunk: AudioChunk| {
        audio_tap.push_audio(dc_clips::Track::Application, &chunk.samples);
        let _ = audio.try_send(chunk);
    };
    match source {
        StreamSource::Screen(settings) => {
            let start = stream_start_bitrate(settings.bitrate);
            #[cfg(windows)]
            let worker = screen::Worker::start_at(settings, start, video, sound);
            #[cfg(not(windows))]
            let worker = screen::Worker::start(settings, video, sound);
            worker
                .map(|worker| {
                    worker.set_bitrate(start);
                    Capture::Screen(worker)
                })
                .map_err(|_| SCREEN_CAPTURE_FAILED)
        }
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
/// How often the sender's own tile gets a new picture of the test pattern (a real share shows every picture).
const LOCAL_PREVIEW_INTERVAL: Duration = Duration::from_millis(200);
/// A watcher refreshes its sink wants while video stalls or before the server forgets them (Serein: five seconds).
const SINK_WANTS_INTERVAL: Duration = Duration::from_secs(5);
/// The lowest rate a share is steered to (`desktopBitrate.min` of module 731854).
const STREAM_MIN_BITRATE: u32 = screen::BITRATE_MIN;
/// Encoded pictures the sender keeps while its pacer is busy (a keyframe at a reduced rate takes several frame times).
const FRAME_QUEUE: usize = 8;
/// Waiting pictures from which the capture is held.
const HOLD_AT: usize = 2;
/// Longest stream audio backlog before the oldest frame goes.
const AUDIO_BACKLOG: usize = 1920 * 6;

/// Drops a watched stream's audio queue, playout slot, and stream telemetry when its connection ends.
struct ForgetAudio {
    hub: VideoHub,
    playout: Option<Arc<CallPlayout>>,
    owner: u64,
    audio_queue: u64,
    stream_key: String,
}

impl Drop for ForgetAudio {
    fn drop(&mut self) {
        // A replacement connection that already began owns the queue, the playout slot and the telemetry.
        if !self.hub.forget_stream_audio(self.owner, self.audio_queue) {
            return;
        }
        if let Some(playout) = &self.playout {
            playout.remove_stream(self.owner);
        }
        self.hub.stream_stats().remove(&self.stream_key);
    }
}

pub(crate) async fn run(
    join: StreamJoin,
    source: Option<StreamSource>,
    mut controls: watch::Receiver<StreamControlState>,
    hub: VideoHub,
    events: mpsc::UnboundedSender<StreamEvent>,
) -> Result<DisconnectReason, &'static str> {
    #[cfg(feature = "sim")]
    let _live = crate::sim::Live::stream();
    let url = signal::endpoint_url(&join.endpoint)?;
    let own = join.user_id;
    let stage = join.stage;
    let owner = join.key.owner();
    let stream_key_str = join.key.to_string();
    let playout = hub.playout();
    // However this connection ends, the audio that waited for the call and telemetry go with it.
    let audio_queue = hub.begin_stream_audio(owner);
    let _audio = ForgetAudio {
        hub: hub.clone(),
        playout: playout.clone(),
        owner,
        audio_queue,
        stream_key: stream_key_str.clone(),
    };
    let sending = join.role == Role::Send;
    if sending && source.is_none() {
        return Err(NO_SOURCE);
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
    let opened = open_with_stages("stream", &url).await?;
    let mut ws: Option<Socket> = Some(opened.ws);
    let mut ws_resume_task: Option<tokio::task::JoinHandle<Result<OpenedSocket, &'static str>>> =
        None;
    let mut ws_resume_at: Option<Instant> = None;
    let mut last_healthy_at = Instant::now();
    let mut diag_host = opened.host;
    let mut diag_port = opened.port;
    let mut tls_version = opened.tls_version;
    let mut connect_started = opened.opened_at;
    text(
        ws.as_mut().expect("opened socket"),
        signal::identify(&ids(), Context::Stream),
    )
    .await?;

    let mut dave = Dave::new(own, join.channel_id)?;
    let mut mode: Option<Mode> = None;
    let mut transport: Option<Transport> = None;
    let mut secured_at: Option<Instant> = None;
    let mut udp: Option<UdpSocket> = None;
    let mut udp_peer: Option<SocketAddr> = None;
    let mut udp_failures = UdpFailures::default();
    let mut next_udp_ping = Instant::now();
    let mut udp_ping_sequence = 0u32;
    let mut discovering = false;
    let mut discovery_started = Instant::now();
    let mut discovery_retry_at = Instant::now();
    let mut discovery_deadline = Instant::now();
    let mut routing_time_ms: Option<u64> = None;
    let mut ssrc = 0u32;
    let mut video_ssrc = 0u32;
    let mut rtx_ssrc = 0u32;
    let mut media_session_id: Option<String> = None;
    let mut seq_ack: i64 = -1;
    let mut server_version = 0u64;
    let mut heartbeat_ms: Option<u64> = None;
    let mut heartbeat_at = Instant::now();
    let mut heartbeat_sent = Instant::now();
    let mut ping_ms: Option<u64> = None;
    let mut hello_deadline = Instant::now() + HELLO_TIMEOUT;
    let mut awaiting_ack = None;
    let mut heartbeat_nonce = 0u64;
    // One `RTCConnection` per stream connection: its `rtc_connection_id` for op 1.
    let rtc_connection_id = crate::rtc_connection_id();
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
    // The pacer wakes on a high-resolution clock: tokio's timer would round each wait up to the system tick.
    let (pacer_clock, mut pacer_wakes) = clock::PacerClock::start()?;
    let mut clients = std::collections::BTreeSet::new();

    // Sender state.
    // Encoded pictures waiting for the pacer. From `HOLD_AT` waiting the capture skips pictures before encoding them;
    // only a full queue drops an encoded one, and that costs a keyframe.
    let (frame_send, mut frames) = mpsc::channel::<EncodedFrame>(FRAME_QUEUE);
    let dropped = Arc::new(AtomicBool::new(false));
    let (audio_send, mut sound) = mpsc::channel::<AudioChunk>(32);
    let mut capture: Option<Capture> = None;
    let mut capture_task: Option<tokio::task::JoinHandle<Result<Capture, &'static str>>> = None;
    // The clip buffer's tap while this sender runs; dropping it, on any exit, frees the buffer.
    let mut _clip_guard: Option<dc_clips::TapGuard> = None;
    let mut sender: Option<VideoSender> = None;
    // The server's keyframe interval (op 4 or 14), applied to the sender when it starts.
    let mut keyframe_interval = 0u64;
    let mut announced = false;
    let mut awaiting_keyframe = true;
    let mut live_announced = false;
    let mut sent_keyframe = false;
    let mut pending_audio: VecDeque<f32> = VecDeque::new();
    let mut audio_encoder = match sending {
        true => {
            // Soundshare: at least 128 kb/s (`handleSoundshare`, module 31595) with in-band FEC (`fec: true` in the
            // same module's transport options), full complexity, stereo music.
            let fail = |_| "Stream audio encoder configuration failed";
            let mut encoder =
                Encoder::new(48_000, Channels::Stereo, Application::Audio).map_err(fail)?;
            encoder.set_bitrate(Bitrate::Bits(128_000)).map_err(fail)?;
            encoder.set_complexity(10).map_err(fail)?;
            encoder.set_inband_fec(true).map_err(fail)?;
            encoder.set_packet_loss_perc(10).map_err(fail)?;
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
    // Whether the capture hands its pictures to the owner's tile itself.
    let mut shown_locally = false;

    // Watcher state.
    let mut receivers = Receivers::default();
    let mut decoders = Decoders::new(hub.clone(), VideoSource::Stream);
    // The owner's soundshare is stereo; fallback headless mixer used when no CallPlayout is attached.
    let mut mixer = Mixer::stereo();
    let mut audio_clock = clock::FrameClock::default();
    let mut stream_audio_ssrc: Option<u32> = None;
    let mut was_watch_active = false;
    let mut sink_wants_at = Instant::now();
    let mut tally = [0u32; 4];
    let mut tally_at = Instant::now() + Duration::from_secs(5);
    let mut stats_at = Instant::now() + Duration::from_millis(250);
    let mut fps_at = Instant::now() + Duration::from_secs(1);
    let mut last_fps_at = Instant::now();
    let mut prev_encoded_frames = 0u64;
    let mut encoded_fps = 0u32;
    let mut prev_decoded_frames = 0u64;
    let mut decoded_fps = 0u32;
    let mut outbound_audio_packets = 0u64;
    let mut outbound_audio_bytes = 0u64;
    let mut outbound_audio_frames = 0u64;
    let mut inbound_packets = 0u64;
    let mut inbound_bytes = 0u64;
    let mut stream_audio_packets = 0u64;
    let mut stream_audio_bytes = 0u64;
    let mut decryption_failures = 0u64;
    let mut video_packets_lost = 0u64;
    let mut outbound_loss_rate: Option<f32> = None;
    let sent_dims = source.map(|s| {
        let l = s.limits();
        (l.width, l.height)
    });

    loop {
        if let Some(reason) = reconnect.take() {
            ws = None;
            if let Some(task) = ws_resume_task.take() {
                task.abort();
            }
            if transport.is_none() || resume_attempts >= MAX_RESUMES {
                return Err(reason);
            }
            let delay = ws_resume_backoff(resume_attempts);
            resume_attempts += 1;
            debug!(
                attempt = resume_attempts,
                delay_ms = delay.as_millis() as u64,
                "stream socket lost; scheduling in-place resume while keeping media alive"
            );
            resuming = true;
            ws_resume_at = Some(Instant::now() + delay);
            deadline = Some(Instant::now() + RENEGOTIATION + delay);
            heartbeat_ms = None;
            awaiting_ack = None;
        }
        pacer_clock.set(
            sender
                .as_ref()
                .and_then(VideoSender::deadline)
                .map(Instant::into_std),
        );
        tokio::select! {
            changed = controls.changed() => {
                if changed.is_err() || controls.borrow().stop {
                    if let Some(task) = ws_resume_task.take() {
                        task.abort();
                    }
                    if announced && sending && let Some(sock) = ws.as_mut() {
                        let _ = text(sock, signal::video(ssrc, None)).await;
                    }
                    log_ws_close("stream", &diag_host, diag_port, connect_started.elapsed().as_millis() as u64, 1000, 0);
                    if let Some(sock) = ws.as_mut() {
                        let _ = timeout(Duration::from_secs(1), sock.close(None)).await;
                    }
                    return Ok(DisconnectReason::Requested);
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
                        if last_healthy_at.elapsed() > RESUME_WINDOW {
                            let mut gone = Some(reopened.ws);
                            return Ok(refuse(&mut gone, close::UNRESUMABLE, "resume window").await);
                        }
                        // `_handleResuming` runs `fastUdpReconnect` on every resume.
                        if let Some(peer) = udp_peer
                            && let Some(fresh_udp) = rebind_udp(peer).await
                        {
                            debug!("stream fast UDP reconnect refreshed local socket");
                            udp = Some(fresh_udp);
                            udp_failures.reset();
                        }
                        if text(&mut reopened.ws, signal::resume(&ids(), seq_ack)).await.is_ok() {
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
                            reconnect = Some("Stream resume failed");
                        }
                    }
                    _ => {
                        reconnect = Some("Stream resume failed");
                    }
                }
            },
            wake = pacer_wakes.recv() => {
                if wake.is_none() {
                    return Err(PACER_STOPPED);
                }
                if let (Some(sender), Some(crypto), Some(socket)) = (sender.as_mut(), transport.as_mut(), udp.as_ref()) {
                    let was_empty = sender.idle();
                    let _ = sender.flush(Instant::now(), crypto, |data| {
                        udp_failures.send_best_effort(socket, data);
                        Ok::<(), &'static str>(())
                    });
                    if !was_empty && sender.idle() && sent_keyframe && !live_announced {
                        live_announced = true;
                        awaiting_keyframe = false;
                        let _ = events.send(StreamEvent::Live { privacy_code: dave.session.voice_privacy_code().map(str::to_owned) });
                    }
                }
            },
            frame = frames.recv(), if sender.as_ref().is_some_and(|sender| sender.accepts(Instant::now())) => {
                let Some(frame) = frame else { return Err(SCREEN_CAPTURE_ENDED) };
                if let Some(capture) = &capture {
                    capture.hold(frames.len() >= HOLD_AT);
                }
                let secure = announced && dave.media_secure() && transport.is_some() && !discovering && !controls.borrow().paused;
                if !secure {
                    let was_awaiting = awaiting_keyframe;
                    awaiting_keyframe = true;
                    sent_keyframe = false;
                    if !was_awaiting
                        && let Some(capture) = &capture
                    {
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
                        Ok(false) => {
                            // The pacer's memory bound refused it: the chain is broken until a keyframe.
                            debug!("stream picture refused by the pacer; asking for a keyframe");
                            awaiting_keyframe = true;
                            sent_keyframe = false;
                            if let Some(capture) = &capture {
                                capture.force_keyframe();
                            }
                        }
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
                    return Err(MEDIA_CLOCK_STOPPED);
                }
                let now = Instant::now();
                if ws.is_none()
                    && ws_resume_task.is_none()
                    && ws_resume_at.is_some_and(|at| now >= at)
                {
                    ws_resume_at = None;
                    let url_clone = url.clone();
                    ws_resume_task = Some(tokio::spawn(async move {
                        open_with_stages("stream", &url_clone).await
                    }));
                }
                if deadline.is_some_and(|d| now >= d) {
                    if resuming && transport.is_some() && resume_attempts < MAX_RESUMES {
                        reconnect = Some("Stream resume acknowledgement timed out");
                        continue;
                    }
                    return Err(if heartbeat_ms.is_none() {
                        "Stream voice Hello timed out"
                    } else if transport.is_none() {
                        "Stream voice negotiation timed out"
                    } else {
                        "Stream DAVE negotiation timed out"
                    });
                }
                if ws.is_some() && heartbeat_ms.is_none() && now >= hello_deadline {
                    if transport.is_none() {
                        return Err("Stream voice Hello timed out");
                    }
                    reconnect = Some("Stream voice Hello timed out");
                    continue;
                }
                if discovering {
                    if now >= discovery_deadline {
                        log_ws_stage_err(
                            "stream",
                            "udp_discovery",
                            &diag_host,
                            diag_port,
                            discovery_started.elapsed().as_millis() as u64,
                            "timed_out",
                        );
                        return Err("Stream voice UDP discovery timed out");
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
                if ws.is_some() && let Some(interval) = heartbeat_ms && now >= heartbeat_at {
                    // One unacknowledged heartbeat by the next tick is a dead socket (`handleHeartbeatTimeout`).
                    if awaiting_ack.is_some() {
                        reconnect = Some("Stream voice heartbeat was not acknowledged");
                        continue;
                    }
                    heartbeat_nonce = heartbeat_nonce.wrapping_add(1);
                    heartbeat_sent = Instant::now();
                    text_or_resume(&mut ws, &mut reconnect, transport.is_some(), signal::heartbeat(heartbeat_nonce, seq_ack, server_version)).await?;
                    awaiting_ack = Some(heartbeat_nonce);
                    heartbeat_at = now + Duration::from_millis(interval);
                }
                if secured_at.is_some_and(|at| now >= at + PEER_GRACE) && !discovering && !resuming && dave.should_wait_for_peer() {
                    dave.enter_sole_member_waiting()?;
                }
                // Media (UDP + DAVE) stays live while the control WebSocket resumes in the background (`921747.js`).
                let enabled = dave.media_ready() && transport.is_some() && !discovering;
                let waiting = dave.waiting && transport.is_some() && !discovering;
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
                        let (frames, audio, dropped) = (frame_send.clone(), audio_send.clone(), dropped.clone());
                        let tap = dc_clips::recorder().attach();
                        _clip_guard = Some(dc_clips::TapGuard(tap.clone()));
                        #[cfg(feature = "sim")]
                        let simulated = crate::sim::active().then(|| crate::sim::plan(&join.session_id));
                        capture_task = Some(tokio::task::spawn_blocking(move || {
                            #[cfg(feature = "sim")]
                            if let Some(plan) = simulated {
                                return start_simulated_capture(plan, source, frames, audio, dropped);
                            }
                            start_capture(source, frames, audio, tap, dropped)
                        }));
                    }
                    if capture_task.as_ref().is_some_and(|task| task.is_finished()) {
                        let task = capture_task.take().ok_or("Capture task missing")?;
                        let started = task.await.map_err(|_| "Capture task failed")??;
                        shown_locally = started.show_locally(hub.clone(), owner);
                        capture = Some(started);
                    }
                    if let Some(capture) = &capture && let Some(reason) = capture.finished() {
                        debug!(reason, "stream capture ended");
                        if let Some(sock) = ws.as_mut() {
                            let _ = text(sock, signal::video(ssrc, None)).await;
                        }
                        return Err(SCREEN_CAPTURE_ENDED);
                    }
                    // Announce the source once, as soon as both ends are ready for it.
                    if !announced && capture.is_some() && ws.is_some() && transport.is_some() && !discovering && !resuming && let Some(source) = source {
                        let bitrate = source.limits().bitrate;
                        let start = stream_start_bitrate(bitrate);
                        let mut video = VideoSender::new(video_ssrc, rtx_ssrc, STREAM_MIN_BITRATE, start, bitrate);
                        video.set_keyframe_interval(keyframe_interval);
                        video.set_screen_share(true);
                        video.set_framerate(source.limits().framerate);
                        sender = Some(video);
                        text_or_resume(&mut ws, &mut reconnect, transport.is_some(), signal::video(ssrc, Some((video_ssrc, rtx_ssrc, Context::Stream, Some(source.limits()))))).await?;
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
                    if let Some(capture) = &capture {
                        // Backpressure before the encoder; a picture lost after it restarts the chain.
                        capture.hold(frames.len() >= HOLD_AT);
                        if dropped.swap(false, Ordering::AcqRel) {
                            debug!("stream picture queue full; asking for a keyframe");
                            capture.force_keyframe();
                        }
                    }
                    if let (Some(sender), Some(capture)) = (sender.as_mut(), &capture)
                        && let Some(bitrate) = sender.tick_rate(now)
                    {
                        capture.set_bitrate(bitrate);
                        debug!(bitrate, "stream bit rate steered");
                    }
                    if let (Some(sender), Some(capture)) = (sender.as_ref(), &capture)
                        && secure
                        && sender.periodic_keyframe_due(now)
                    {
                        capture.force_keyframe();
                    }
                    // System audio: stereo 20 ms frames into Opus, DAVE, soundshare.
                    while let Ok(chunk) = sound.try_recv() {
                        pending_audio.extend(chunk.samples);
                    }
                    while pending_audio.len() > AUDIO_BACKLOG {
                        pending_audio.drain(..1920);
                    }
                    // Every whole frame the capture has delivered goes now. One frame per tick let the backlog grow with
                    // each late tick and with the device clock running ahead of ours, until the oldest 20 ms was cut.
                    let audio_open = secure && !controls.borrow().paused && sent_keyframe;
                    while audio_open && pending_audio.len() >= 1920 {
                        let stereo: Vec<f32> = pending_audio.drain(..1920).collect();
                        if !audio_speaking {
                            if !resuming {
                                text_or_resume(&mut ws, &mut reconnect, transport.is_some(), signal::speaking_flags(2, ssrc)).await?;
                            }
                            audio_speaking = true;
                        }
                        let encoder = audio_encoder.as_mut().ok_or("Stream audio encoder missing")?;
                        let length = encoder.encode_float(&stereo, &mut encoded).map_err(|_| "Stream audio encoding failed")?;
                        let data = dave.encrypt(davey::MediaType::AUDIO, davey::Codec::OPUS, &encoded[..length]).map_err(|_| "DAVE stream audio encryption failed")?.into_owned();
                        let header = rtp::header(audio_sequence, audio_timestamp, ssrc);
                        if let Some(crypto) = transport.as_mut() {
                            let wire = crypto.seal_soundshare(&header, &data, now)?;
                            if let Some(socket) = &udp {
                                if udp_failures.send(socket, &wire, "Stream audio UDP send failed").is_err() {
                                    if let Some(peer) = udp_peer
                                        && let Some(fresh_udp) = rebind_udp(peer).await
                                    {
                                        debug!("stream audio UDP send outage triggered fast UDP reconnect");
                                        udp = Some(fresh_udp);
                                        udp_failures.reset();
                                    } else {
                                        return Err("Stream audio UDP send failed");
                                    }
                                }
                                outbound_audio_packets += 1;
                                outbound_audio_bytes += wire.len() as u64;
                                outbound_audio_frames += 1;
                            }
                        }
                        audio_sequence = audio_sequence.wrapping_add(1);
                        audio_timestamp = audio_timestamp.wrapping_add(rtp::FRAME_SAMPLES);
                    }
                    if !secure {
                        pending_audio.clear();
                    }
                    if !shown_locally && now >= local_preview_at && let Some(capture) = &capture {
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
                    if secure && sent_keyframe && controls.borrow().previews && now >= preview_at && let Some(capture) = &capture {
                        // A real share cuts the thumbnail only while one is asked for: the first ask starts it, a
                        // later tick finds it.
                        let picture = if shown_locally { capture.preview() } else { last_preview.clone() };
                        if let Some(picture) = picture {
                            if let Some(jpeg) = jpeg_thumbnail(&picture) {
                                let _ = events.send(StreamEvent::Preview { jpeg });
                                previewed = true;
                            }
                            let paused = controls.borrow().paused;
                            preview_at = now + if previewed && !paused { PREVIEW_INTERVAL } else if paused { PREVIEW_PAUSED_INTERVAL } else { PREVIEW_FIRST * 20 };
                        }
                    }
                } else {
                    // Watching: audio flows directly into CallPlayout (or legacy VideoHub fallback if headless).
                    if secure {
                        if let Some(p) = &playout {
                            was_watch_active = true;
                            let _ = p;
                        } else {
                            // One frame per 20 ms of real time: fallback path when no CallPlayout is attached.
                            for _ in 0..audio_clock.due(now.into_std()) {
                                if let Some(frame) = mixer.pop_stereo() {
                                    hub.push_stream_audio(owner, audio_queue, frame);
                                }
                            }
                        }
                        // Repairs that expired since the last packet; arrivals are handed over as they come.
                        tally[3] += receive::hand_over(&mut receivers, &mut decoders, &mut dave, now);
                        if let (Some(crypto), Some(socket)) = (transport.as_mut(), udp.as_ref()) {
                            for (header, body) in receivers.feedback(ssrc, now) {
                                // Feedback is best effort: a transient UDP error is only a lost RTCP packet.
                                if let Ok(wire) = crypto.seal_rtcp(&header, &body) {
                                    udp_failures.send_best_effort(socket, &wire);
                                }
                            }
                        }
                        if ws.is_some() && !resuming && now >= sink_wants_at && hub.has_sink() {
                            text_or_resume(&mut ws, &mut reconnect, transport.is_some(), signal::media_sink_wants(100)).await?;
                            sink_wants_at = now + SINK_WANTS_INTERVAL;
                        }
                    } else if let Some(p) = &playout {
                        if was_watch_active {
                            p.clear_stream(owner);
                            was_watch_active = false;
                        }
                    } else {
                        mixer.clear();
                        hub.clear_stream_audio_of(owner);
                        audio_clock.reset();
                    }
                }
                if now >= tally_at {
                    let (numbered, unnumbered) = receivers.transport_counts();
                    let (stream_audio_underruns, stream_audio_trimmed) = playout.as_ref().map_or_else(
                        || hub.stream_audio_counts(owner),
                        |p| (p.stats_snapshot().stream_underruns, 0),
                    );
                    debug!(
                        received = tally[0],
                        opened = tally[1],
                        video_packets = tally[2],
                        pictures = tally[3],
                        sending,
                        ready = dave.ready,
                        transport_cc_packets = numbered,
                        without_transport_cc = unnumbered,
                        stream_audio_underruns,
                        stream_audio_trimmed,
                        repairs_sent = sender.as_ref().map_or(0, |sender| sender.repairs),
                        bitrate = sender.as_ref().map_or(0, VideoSender::bitrate),
                        "stream packet tally"
                    );
                    if let Some(sender) = sender.as_ref() {
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
                            "stream send-side estimation"
                        );
                    }
                    tally = [0; 4];
                    tally_at = now + Duration::from_secs(5);
                }
                if now >= stats_at {
                    let sf_version = if dave.ready || dave.unencrypted() {
                        u32::from(dave.protocol())
                    } else {
                        0
                    };
                    let video_snap = sender.as_ref().map(VideoSender::stats_snapshot);
                    let est = sender.as_ref().map(|s| s.estimation(transport.as_ref()));
                    let video_sources = receivers.video_sources_snapshot();
                    let dec = (!sending).then(|| decoders.video_stats(owner)).flatten();

                    if now >= fps_at {
                        let secs = now.saturating_duration_since(last_fps_at).as_secs_f32().max(0.25);
                        let enc_total = video_snap.as_ref().map_or(0, |v| v.frames_encoded);
                        encoded_fps = ((enc_total.saturating_sub(prev_encoded_frames)) as f32 / secs).round() as u32;
                        prev_encoded_frames = enc_total;
                        let dec_total = dec.as_ref().map_or(0, |d| d.frames_decoded);
                        decoded_fps = ((dec_total.saturating_sub(prev_decoded_frames)) as f32 / secs).round() as u32;
                        prev_decoded_frames = dec_total;
                        last_fps_at = now;
                        fps_at = now + Duration::from_secs(1);
                    }

                    let mut inbound: HashMap<u64, InboundUserStats> = HashMap::new();
                    if !sending {
                        let mut owner_stats = InboundUserStats::default();
                        if let Some(p) = &playout {
                            let snap = p.stats_snapshot();
                            if let Some(spk) = snap.stream_speakers.iter().find(|s| s.user == owner) {
                                owner_stats.audio = Some(InboundAudioStats {
                                    ssrc: spk.ssrc,
                                    codec: "opus",
                                    payload_type: rtp::OPUS_PAYLOAD_TYPE,
                                    bytes_received: stream_audio_bytes,
                                    packets_received: stream_audio_packets.max(spk.stats.packets_received),
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
                        }
                        if let Some(src) = video_sources.iter().find(|s| s.user == owner) {
                            owner_stats.video = Some(InboundVideoStats {
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
                                fps_decoded: decoded_fps,
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
                        if owner_stats.audio.is_some() || owner_stats.video.is_some() {
                            inbound.insert(owner, owner_stats);
                        }
                    }

                    let avail_bitrate = est.as_ref().map_or(0, |e| e.target);
                    let delay_ms = est.as_ref().map_or(0, |e| e.delay_estimate);
                    let (out_w, out_h) = sent_dims.unwrap_or((0, 0));
                    let outbound = if sending {
                        OutboundStats {
                            audio: (ssrc != 0).then_some(OutboundAudioStats {
                                ssrc,
                                codec: "opus",
                                payload_type: rtp::OPUS_PAYLOAD_TYPE,
                                bitrate: 128_000,
                                bytes_sent: outbound_audio_bytes,
                                packets_sent: outbound_audio_packets,
                                packets_lost: 0,
                                fraction_lost: 0.0,
                                frames_captured: outbound_audio_frames,
                                audio_level: 0.0,
                                audio_detected: None,
                                encrypt_success_count: outbound_audio_frames,
                                encrypt_failure_count: 0,
                                secure_frames_protocol_version: sf_version,
                            }),
                            video: video_snap.as_ref().map(|vs| OutboundVideoStats {
                                ssrc: vs.video_ssrc,
                                rtx_ssrc: vs.rtx_ssrc,
                                codec: "H264",
                                payload_type: rtp::H264_PAYLOAD_TYPE,
                                bitrate_target: avail_bitrate,
                                bitrate_actual: est.as_ref().map_or(0, |e| e.encoder_rate),
                                bytes_sent: vs.bytes_sent,
                                packets_sent: vs.packets_sent,
                                packets_lost: video_packets_lost,
                                width: out_w,
                                height: out_h,
                                fps_encoded: encoded_fps,
                                frames_encoded: vs.frames_encoded,
                                frames_dropped: est.as_ref().map_or(0, |e| e.dropped_pictures),
                                key_frames_encoded: vs.keyframes_encoded,
                                qp_sum: 0,
                                nacks: vs.nacks,
                                plis: vs.plis,
                                firs: 0,
                                encoder_implementation_name: Some(
                                    capture
                                        .as_ref()
                                        .map_or("OpenH264", Capture::encoder_name)
                                        .into(),
                                ),
                                average_encode_time_ms: 0.0,
                                encrypt_success_count: vs.frames_encoded,
                                encrypt_failure_count: 0,
                                secure_frames_protocol_version: sf_version,
                            }),
                        }
                    } else {
                        OutboundStats::default()
                    };

                    hub.stream_stats().upsert(StreamStats {
                        stream_key: stream_key_str.clone(),
                        owner_id: owner,
                        sending,
                        connected: enabled || waiting,
                        paused: controls.borrow().paused,
                        outbound_loss_rate,
                        soundshare_failed: capture.as_ref().is_some_and(Capture::soundshare_failed),
                        transport: TransportStats {
                            hostname: Some(diag_host.clone()),
                            local_address: udp
                                .as_ref()
                                .and_then(|s| s.local_addr().ok())
                                .map(|a| a.to_string()),
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
                            mode: transport.as_ref().map(|t| t.mode_name().to_string()),
                            secure_frames_protocol_version: sf_version,
                            output_device_underruns: playout
                                .as_ref()
                                .map_or(0, |p| p.stats_snapshot().stream_underruns),
                            receiver_reports: Vec::new(),
                        },
                        outbound,
                        inbound,
                    });
                    stats_at = now + Duration::from_millis(250);
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
                    let Ok((address, port)) = rtp::discovery_response(&packet[..length], ssrc) else {
                        continue;
                    };
                    let elapsed_ms = discovery_started.elapsed().as_millis() as u64;
                    routing_time_ms = Some(elapsed_ms);
                    log_ws_stage_ok("stream", "udp_discovery", &diag_host, diag_port, elapsed_ms, None);
                    let mode = mode.ok_or("Missing voice encryption mode")?;
                    text_or_resume(&mut ws, &mut reconnect, transport.is_some(), signal::select_protocol(address, port, mode, &rtc_connection_id)).await?;
                    discovering = false;
                    continue;
                }
                inbound_packets += 1;
                inbound_bytes += length as u64;
                tally[0] += 1;
                let Some(crypto) = &transport else { continue };
                if rtp::is_rtcp(&packet[..length]) {
                    let Some(compound) = crypto.open_rtcp(&packet[..length]) else {
                        if length >= 12 {
                            decryption_failures += 1;
                        }
                        continue;
                    };
                    if !sending {
                        receivers.sender_reports(&compound, Instant::now());
                    }
                    if sending
                        && video_ssrc != 0
                        && let Some(rate) = rtp::outbound_loss(&compound, video_ssrc)
                    {
                        outbound_loss_rate = Some(rate);
                    }
                    if let Some(sender) = sender.as_mut()
                        && let Some(crypto) = transport.as_mut()
                        && let Some(feedback) = rtp::feedback(&compound, video_ssrc)
                        && announced
                        && dave.media_secure()
                    {
                        if let Some(cum) = feedback.cumulative_lost {
                            video_packets_lost = video_packets_lost.max(cum);
                        }
                        if !feedback.transport.is_empty() && crypto.sent.reports == 0 {
                            debug!("transport-wide feedback received; send-side estimation runs");
                        }
                        if sender.feedback(&feedback, crypto, Instant::now()) && let Some(capture) = &capture {
                            capture.force_keyframe();
                        }
                    }
                    continue;
                }
                if sending {
                    continue;
                }
                let Some(rtp) = crypto.open(&packet[..length]) else {
                    if length >= 12 {
                        decryption_failures += 1;
                    }
                    continue;
                };
                tally[1] += 1;
                let arrived = Instant::now();
                receivers.observe_transport(rtp.ssrc, rtp.transport_sequence, arrived);
                if !dave.media_ready() {
                    continue;
                }
                match rtp.payload_type {
                    rtp::OPUS_PAYLOAD_TYPE => {
                        receivers.observe_audio(rtp.ssrc, rtp.sequence, rtp.timestamp, arrived);
                        if let Some(p) = &playout {
                            if stream_audio_ssrc == Some(rtp.ssrc) && dave.contains(owner) {
                                stream_audio_packets += 1;
                                stream_audio_bytes += rtp.payload.len() as u64;
                                match dave.decrypt(owner, davey::MediaType::AUDIO, &rtp.payload) {
                                    Ok(opus) => p.push_stream_packet(owner, rtp.ssrc, rtp.sequence, rtp.timestamp, &opus),
                                    Err(_) => decryption_failures += 1,
                                }
                            }
                        } else {
                            let Some(user) = mixer.user(rtp.ssrc) else { continue };
                            if !dave.contains(user) {
                                continue;
                            }
                            let Ok(opus) = dave.decrypt(user, davey::MediaType::AUDIO, &rtp.payload) else {
                                decryption_failures += 1;
                                continue;
                            };
                            mixer.push(rtp.ssrc, rtp.sequence, rtp.timestamp, &opus);
                        }
                    }
                    rtp::H264_PAYLOAD_TYPE | rtp::H264_RTX_PAYLOAD_TYPE => {
                        tally[2] += 1;
                        let now = arrived;
                        receivers.push(rtp, now);
                        if dave.session.is_ready() && !dave.transitioning() {
                            tally[3] += receive::hand_over(&mut receivers, &mut decoders, &mut dave, now);
                            // NACK for a hole and PLI for a lost chain go as soon as they are due, not on the tick.
                            if let (Some(crypto), Some(socket)) = (transport.as_mut(), udp.as_ref()) {
                                for (header, body) in receivers.feedback(ssrc, now) {
                                    // Feedback is best effort: a transient UDP error never kills the stream.
                                    if let Ok(wire) = crypto.seal_rtcp(&header, &body) {
                                        udp_failures.send_best_effort(socket, &wire);
                                    }
                                }
                            }
                        }
                    }
                    _ => {}
                }
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
                        log_ws_close("stream", &diag_host, diag_port, connect_started.elapsed().as_millis() as u64, code, reason_len);
                        if transport.is_some() && close_code_is_resumable(code) {
                            ws = None;
                            reconnect = Some("Stream socket closed; resuming");
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
                            "stream",
                            "ws_close",
                            &diag_host,
                            diag_port,
                            connect_started.elapsed().as_millis() as u64,
                            &sanitized_ws_error(&err),
                        );
                        ws = None;
                        reconnect = Some("Stream voice socket failed");
                        continue;
                    }
                    _ => {
                        log_ws_close("stream", &diag_host, diag_port, connect_started.elapsed().as_millis() as u64, u16::from(CloseCode::Abnormal), 0);
                        ws = None;
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
                                log_ws_stage_ok(
                                    "stream",
                                    "op8_hello",
                                    &diag_host,
                                    diag_port,
                                    connect_started.elapsed().as_millis() as u64,
                                    tls_version,
                                );
                            }
                            Some(VoiceOp::HeartbeatAck) => {
                                if signal::acknowledges(awaiting_ack, 0, data["t"].as_u64()) {
                                    ping_ms = Some(heartbeat_sent.elapsed().as_millis() as u64);
                                    awaiting_ack = None;
                                    last_healthy_at = Instant::now();
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
                                log_ws_stage_ok(
                                    "stream",
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
                                let peer = SocketAddr::new(ready.ip, ready.port);
                                let socket = UdpSocket::bind(if ready.ip.is_ipv4() { "0.0.0.0:0" } else { "[::]:0" })
                                    .await
                                    .map_err(|_| "Could not bind stream UDP socket")?;
                                socket
                                    .connect(peer)
                                    .await
                                    .map_err(|_| "Could not connect stream UDP socket")?;
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
                                    return Err("Unexpected stream session description");
                                }
                                let mode = mode.ok_or("Unexpected stream session description")?;
                                if let Some(interval) = signal::keyframe_interval(data) {
                                    keyframe_interval = interval;
                                    debug!(interval, "stream keyframe interval");
                                }
                                media_session_id = data["media_session_id"].as_str().map(str::to_owned);
                                let dave_version = signal::dave_version(&data["dave_protocol_version"]);
                                if dave_version.is_none() {
                                    return Ok(refuse(&mut ws, close::DAVE_INVALID_MESSAGE, "op 4 dave_protocol_version").await);
                                }
                                if !signal::dave_version_allowed(DaveAt::Init, dave_version, stage) {
                                    return Ok(refuse(&mut ws, close::DAVE_DOWNGRADE_REFUSED, DaveAt::Init.as_str()).await);
                                }
                                let key = signal::session_description(data, mode)?;
                                transport = Some(Transport::new(mode, &key));
                                secured_at = Some(Instant::now());
                                last_healthy_at = Instant::now();
                                log_ws_stage_ok(
                                    "stream",
                                    "op4_session_description",
                                    &diag_host,
                                    diag_port,
                                    connect_started.elapsed().as_millis() as u64,
                                    tls_version,
                                );
                                text_or_resume(&mut ws, &mut reconnect, transport.is_some(), signal::speaking_flags(0, ssrc)).await?;
                                if dave_version == Some(0) {
                                    debug!("stream at DAVE protocol version 0: media is not end-to-end encrypted");
                                    dave.start_unencrypted();
                                } else {
                                    send_or_resume(&mut ws, &mut reconnect, transport.is_some(), Message::Binary(dave.key_package()?.into())).await?;
                                }
                                if !sending && hub.has_sink() {
                                    text_or_resume(&mut ws, &mut reconnect, transport.is_some(), signal::media_sink_wants(100)).await?;
                                    sink_wants_at = Instant::now() + SINK_WANTS_INTERVAL;
                                }
                            }
                            Some(VoiceOp::SessionUpdate) => {
                                if let Some(interval) = signal::keyframe_interval(data) {
                                    keyframe_interval = interval;
                                    if let Some(sender) = sender.as_mut() {
                                        sender.set_keyframe_interval(interval);
                                    }
                                    debug!(interval, "stream keyframe interval");
                                }
                            }
                            Some(VoiceOp::Speaking) => {
                                // The owner's audio source (stream audio).
                                let user = signal::id(data, "user_id")?;
                                if !sending && user == owner && dave.contains(user) {
                                    let value = u32::try_from(signal::number(data, "ssrc")?).map_err(|_| "Invalid voice SSRC")?;
                                    stream_audio_ssrc = Some(value);
                                    if let Some(p) = &playout {
                                        p.announce_stream(owner, value, hub.stream_volume(owner));
                                    } else {
                                        mixer.announce(user, value)?;
                                    }
                                }
                            }
                            Some(VoiceOp::Video) => {
                                let remote = signal::remote_video(data)?;
                                if !sending && remote.user == owner {
                                    if let Some((height, fps)) = remote.quality {
                                        let _ = events.send(StreamEvent::Quality { height, fps });
                                    }
                                    if remote.audio_ssrc != 0 && dave.contains(remote.user) {
                                        stream_audio_ssrc = Some(remote.audio_ssrc);
                                        if let Some(p) = &playout {
                                            p.announce_stream(owner, remote.audio_ssrc, hub.stream_volume(owner));
                                        } else {
                                            mixer.announce(remote.user, remote.audio_ssrc)?;
                                        }
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
                                let ids = signal::client_connect(data, dave.unencrypted())?;
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
                                if let Some(p) = &playout {
                                    if user == owner {
                                        p.remove_stream(owner);
                                        stream_audio_ssrc = None;
                                    }
                                } else {
                                    mixer.remove(user);
                                }
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
                                    if was_ready {
                                        // Keep the epoch-N MLS group intact until the SFU commits removal (including 2 -> 1).
                                        dave.ready = true;
                                        deadline = None;
                                    } else if was_group_member {
                                        deadline = Some(Instant::now() + RENEGOTIATION);
                                    }
                                }
                            }
                            Some(VoiceOp::DavePrepareTransition) => {
                                let version = signal::dave_version(&data["protocol_version"]);
                                if version.is_none() || signal::dave_version(&data["transition_id"]).is_none() {
                                    return Ok(refuse(&mut ws, close::DAVE_INVALID_MESSAGE, "op 21").await);
                                }
                                if !signal::dave_version_allowed(DaveAt::Transition, version, stage) {
                                    return Ok(refuse(&mut ws, close::DAVE_DOWNGRADE_REFUSED, DaveAt::Transition.as_str()).await);
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
                                    text_or_resume(&mut ws, &mut reconnect, transport.is_some(), signal::transition_ready(id)).await?;
                                }
                            }
                            Some(VoiceOp::DaveExecuteTransition) => {
                                let id = signal::transition(data)?;
                                if dave.execute(id).is_err() {
                                    waiting_announced = false;
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
                                if !signal::dave_version_allowed(DaveAt::Epoch, version, stage) {
                                    return Ok(refuse(&mut ws, close::DAVE_DOWNGRADE_REFUSED, DaveAt::Epoch.as_str()).await);
                                }
                                if signal::number(data, "epoch")? == 1 {
                                    dave.reinitialize()?;
                                    deadline = Some(Instant::now() + RENEGOTIATION);
                                    send_or_resume(&mut ws, &mut reconnect, transport.is_some(), Message::Binary(dave.key_package()?.into())).await?;
                                }
                            }
                            Some(VoiceOp::Resumed) => {
                                if !resuming {
                                    return Err("Unexpected stream resumption");
                                }
                                // `handleResumed` (`921747.js`): reset resume backoff and restore signaling cleanly.
                                resuming = false;
                                resume_attempts = 0;
                                last_healthy_at = Instant::now();
                                if dave.ready || dave.waiting {
                                    deadline = None;
                                }
                                text_or_resume(&mut ws, &mut reconnect, transport.is_some(), signal::speaking_flags(if audio_speaking { 2 } else { 0 }, ssrc)).await?;
                            }
                            _ => debug!(op = message.raw_op, "stream signaling op ignored"),
                        }
                    }
                    Message::Binary(bytes) => {
                        let Ok(frame) = signal::parse_binary(&bytes) else {
                            return Ok(refuse(&mut ws, close::DAVE_INVALID_MESSAGE, "binary frame shorter than its header").await);
                        };
                        debug!(op = frame.op, "stream binary op received");
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
                                        waiting_announced = false;
                                        dave.reset()?;
                                        deadline = Some(Instant::now() + RENEGOTIATION);
                                        send_or_resume(&mut ws, &mut reconnect, transport.is_some(), Message::Binary(dave.key_package()?.into())).await?;
                                    }
                                }
                            }
                            op if op == VoiceOp::MlsPrepareCommitTransition as u8 || op == VoiceOp::MlsWelcome as u8 => {
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
                                    deadline = Some(Instant::now() + RENEGOTIATION);
                                }
                                // The epoch changed: pictures and audio of the old one are gone.
                                if let Some(sender) = sender.as_mut() {
                                    sender.clear();
                                }
                                if let Some(capture) = &capture {
                                    capture.force_keyframe();
                                }
                                if let Some(p) = &playout {
                                    p.clear_stream(owner);
                                } else {
                                    mixer.clear();
                                }
                            }
                            op => debug!(op, "stream binary op ignored"),
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
