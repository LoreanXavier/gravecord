//! Discord voice: the voice control socket, UDP media with `rtpsize` AEAD transport encryption, DAVE end-to-end
//! encryption, Opus, the default microphone and speaker, and video: cameras in a call and Go Live streams.
//!
//! Adapted from Serein's `discord-voice` crate (`vendor/serein/crates/discord-voice`, MIT OR Apache-2.0); each
//! module names the Serein file it comes from.
//!
//! # A call
//!
//! ```no_run
//! # async fn call() {
//! let controls = dc_voice::Controls::new();
//! let join = dc_voice::Join {
//!     endpoint: "c-ams01-abcd.discord.media:443".into(),
//!     token: "voice token from VOICE_SERVER_UPDATE".into(),
//!     session_id: "session id from VOICE_STATE_UPDATE".into(),
//!     server_id: 1,
//!     channel_id: 2,
//!     user_id: 3,
//! };
//! let (task, mut events) = dc_voice::connect(join, &controls);
//! controls.set_self_mute(true);
//! while let Some(event) = events.recv().await {
//!     if let dc_voice::Event::Disconnected { .. } = event {
//!         break;
//!     }
//! }
//! let _ = task.await;
//! # }
//! ```
//!
//! # Video
//!
//! * **Seeing pictures.** [`Controls::set_video_sink`] registers a [`VideoSink`]. It is called on decoder threads
//!   with a [`RemoteFrame`] for every decoded picture of a participant's camera ([`VideoSource::Camera`]) and of a
//!   watched stream ([`VideoSource::Stream`]), and with our own camera as the local preview (`user` is our id). It must
//!   return quickly. The sink is shared with the stream connections that take [`Controls::hub`].
//! * **Camera.** [`camera_devices`] lists them. [`Controls::set_camera`] turns ours on (`Some(device id)`, empty for
//!   the default camera) or off (`None`). [`Event::VideoState`] says when the camera is really on or off, and is the
//!   cue to put `self_video` into the main gateway's voice state (op 4). [`Event::CameraFailed`] says why it did not
//!   start.
//! * **Go Live, main gateway.** [`streams`] has the stream keys, the payloads of ops 18 to 22 and the parser of the
//!   four dispatches. [`streams::Tracker`] turns the dispatches and the connection's events into
//!   [`streams::StreamUpdate`] values (starting, connecting, live, paused, viewers, ended and why) and says when to
//!   connect.
//! * **Go Live, voice side.** [`connect_stream`] starts the second voice connection of a stream from a
//!   [`streams::StreamJoin`]: as the sender with a [`StreamSource`] (a display or window, or a test pattern), or as a
//!   watcher, whose pictures reach the sink and whose audio joins the call's mix at [`VideoHub::set_stream_volume`].
//!   [`StreamControls`] stops it, pauses it and switches previews; [`StreamEvent::Preview`] carries a JPEG for
//!   `POST /streams/<key>/preview`.
//!
//! Nothing video blocks the audio loop: pictures are dropped, never queued without bound, and a failing decoder or
//! capture ends only its own source.
mod activity;
mod audio;
mod bwe;
mod capture;
mod clock;
mod dave;
mod dereverb;
pub mod devices;
mod gate;
pub mod input_watch;
#[cfg(test)]
mod jitter;
mod mixer;
mod neteq;
pub mod ogg_opus;
mod playout;
mod process;
pub mod recorder;
pub mod rtc_stats;
mod rtp;
mod session;
mod signal;
#[cfg(feature = "sim")]
pub mod sim;
pub mod sound;
mod stream_conn;
pub mod streams;
mod transport_cc;
mod video;
pub mod voice_player;
pub mod voice_recorder;
pub mod voice_wave;
pub use stream_conn::{StreamEvent, StreamSource};
pub use video::{RemoteFrame, VideoHub, VideoSink, VideoSource};

pub use audio::{
    Prefs, Preview, default_input_device, default_output_device, input_devices, output_devices,
    prefs, preview,
};
pub use gate::FLOOR_DB;
pub use process::{ProcessingPrefs, Suppression};
pub use rtc_stats::RtcStatsSnapshot;
pub use rtp::Mode;

use session::{ControlState, Credentials, Shared};
use std::{fmt, sync::Arc};
use tokio::{
    sync::{mpsc, watch},
    task::JoinHandle,
};
use zeroize::Zeroizing;

/// One 20 ms mono frame at 48 kHz.
pub(crate) type Frame = [f32; 960];

/// One 20 ms stereo frame at 48 kHz, left and right interleaved: what the call plays.
pub(crate) type Stereo = [f32; 1920];

/// A mono frame on both channels.
pub(crate) fn to_stereo(frame: &Frame) -> Stereo {
    let mut stereo = [0.0; 1920];
    for (pair, sample) in stereo.as_chunks_mut::<2>().0.iter_mut().zip(frame) {
        *pair = [*sample, *sample];
    }
    stereo
}

/// The mean of both channels.
pub(crate) fn to_mono(stereo: &Stereo) -> Frame {
    let mut frame = [0.0; 960];
    for (sample, pair) in frame.iter_mut().zip(stereo.as_chunks::<2>().0) {
        *sample = (pair[0] + pair[1]) * 0.5;
    }
    frame
}

/// What the main gateway hands over for a voice join: `endpoint` and `token` from
/// VOICE_SERVER_UPDATE, `session_id` from our own VOICE_STATE_UPDATE. For a DM or group call
/// `server_id` is the channel id.
pub struct Join {
    pub endpoint: String,
    pub token: String,
    pub session_id: String,
    pub server_id: u64,
    pub channel_id: u64,
    pub user_id: u64,
}
impl fmt::Debug for Join {
    /// Secrets and ids never reach logs.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Join")
            .field("endpoint", &self.endpoint)
            .finish_non_exhaustive()
    }
}

/// App-side handle for a running connection. Cheap to clone; every clone controls the same
/// state. Dropping every clone ends the call, like `disconnect`.
#[derive(Clone)]
pub struct Controls {
    state: Arc<watch::Sender<ControlState>>,
    shared: Arc<Shared>,
}
impl Default for Controls {
    fn default() -> Self {
        Self::new()
    }
}
impl Controls {
    pub fn new() -> Self {
        Self {
            state: Arc::new(watch::channel(ControlState::default()).0),
            shared: Arc::new(Shared::default()),
        }
    }
    /// Stop sending microphone audio. The main gateway's voice state still has to report it.
    pub fn set_self_mute(&self, muted: bool) {
        self.state
            .send_if_modified(|state| std::mem::replace(&mut state.self_mute, muted) != muted);
    }
    /// Stop playback and sending. The main gateway's voice state still has to report it.
    pub fn set_self_deaf(&self, deafened: bool) {
        self.state.send_if_modified(|state| {
            std::mem::replace(&mut state.self_deaf, deafened) != deafened
        });
    }
    /// Close the voice connection. Leaving the channel itself is a main gateway voice state.
    pub fn disconnect(&self) {
        self.state
            .send_if_modified(|state| !std::mem::replace(&mut state.disconnect, true));
    }
    /// Starts a fresh control watch for a moved/rejoined session while keeping the shared media state (camera,
    /// `user_mix`, and `VideoHub`).
    pub fn rejoin(&self) -> Self {
        let current = *self.state.borrow();
        Self {
            state: Arc::new(
                watch::channel(ControlState {
                    disconnect: false,
                    ..current
                })
                .0,
            ),
            shared: self.shared.clone(),
        }
    }
    pub fn self_mute(&self) -> bool {
        self.state.borrow().self_mute
    }
    pub fn self_deaf(&self) -> bool {
        self.state.borrow().self_deaf
    }
    /// Turns our camera on with `Some(device id from [`camera_devices`])` (an empty id picks the default camera) or
    /// off with `None`. The call opens the device only after this; [`Event::VideoState`] reports the result.
    pub fn set_camera(&self, device: Option<String>) {
        *self.shared.camera.lock().unwrap_or_else(|e| e.into_inner()) = device;
        self.state.send_modify(|state| {
            state.video_request = state.video_request.wrapping_add(1);
        });
    }
    /// Whether the camera is wanted (requested and not yet turned off), not whether it has opened.
    pub fn camera_wanted(&self) -> bool {
        self.shared
            .camera
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .is_some()
    }
    /// Registers (or with `None` removes) the receiver of decoded pictures; see [`VideoSink`]. The call tells the
    /// server whether it wants video (op 15) from this.
    pub fn set_video_sink(&self, sink: Option<VideoSink>) {
        self.shared.hub.set_sink(sink);
        self.state.send_modify(|state| {
            state.video_request = state.video_request.wrapping_add(1);
        });
    }
    /// Sets how one person sounds to us: a volume factor (1.0 is 100%, up to 2.0) and a local mute. Applies from
    /// the next 10 ms block, and holds for the rest of the call.
    pub fn set_user_mix(&self, user: u64, volume: f32, muted: bool) {
        let clamped = volume.clamp(0.0, 2.0);
        let mut table = self
            .shared
            .user_mix
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        if clamped == 1.0 && !muted {
            table.remove(&user);
        } else {
            table.insert(user, (clamped, muted));
        }
        drop(table);
        self.shared.playout.set_user_mix(user, clamped, muted);
    }
    /// The hub that carries pictures and watched stream audio; pass it to [`connect_stream`].
    pub fn hub(&self) -> VideoHub {
        self.shared.hub.clone()
    }
    /// Live snapshot of RTC transport, outbound/inbound media, and Go Live stream statistics (`PopoutWindowRTCDebug`).
    pub fn rtc_stats(&self) -> rtc_stats::RtcStatsSnapshot {
        self.shared
            .rtc_stats
            .snapshot(self.shared.hub.stream_stats().all())
    }
}

/// The cameras this machine offers, for [`Controls::set_camera`]. Enumeration opens no device.
pub fn camera_devices() -> Vec<dc_media::camera::CameraDevice> {
    dc_media::camera::devices().unwrap_or_default()
}

/// Handle of one stream connection (see [`connect_stream`]).
#[derive(Clone)]
pub struct StreamControls(Arc<watch::Sender<stream_conn::StreamControlState>>);
impl Default for StreamControls {
    fn default() -> Self {
        Self::new()
    }
}
impl StreamControls {
    /// Previews are on, the default of `audio.stream_previews`.
    pub fn new() -> Self {
        Self(Arc::new(
            watch::channel(stream_conn::StreamControlState {
                stop: false,
                paused: false,
                previews: true,
            })
            .0,
        ))
    }
    /// Ends the stream connection. The main gateway's `STREAM_DELETE` (op 19) is separate.
    pub fn stop(&self) {
        self.0
            .send_if_modified(|state| !std::mem::replace(&mut state.stop, true));
    }
    /// A sender stops sending media while paused.
    pub fn set_paused(&self, paused: bool) {
        self.0
            .send_if_modified(|state| std::mem::replace(&mut state.paused, paused) != paused);
    }
    /// Whether the sender uploads previews (`audio.stream_previews`).
    pub fn set_previews(&self, previews: bool) {
        self.0
            .send_if_modified(|state| std::mem::replace(&mut state.previews, previews) != previews);
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    /// Transport and DAVE are up: media flows, or we are alone and waiting for a peer.
    /// Sent once per connection.
    Connected,
    /// The call's voice privacy code, each time the DAVE group settles on a new epoch.
    PrivacyCode(String),
    /// Display-only activity from decoded audio (and our own microphone), at most every 100 ms.
    SpeakingChanged {
        user_id: u64,
        speaking: bool,
    },
    /// `inputDetected` changed (module 754712 `InputWatcher`): the microphone delivers input (true), has been
    /// silent for five seconds (false), or is unknown again (None, after an unmute or a new call).
    InputDetected {
        detected: Option<bool>,
    },
    /// A round trip of the voice control socket, ms (`_handleControlPing`, the official fallback without native pings).
    Ping {
        ms: f64,
    },
    /// What the server reports losing of what this client sends, in percent (`OutboundLossRate`).
    OutboundLoss {
        percent: f32,
    },
    ClientConnected {
        user_id: u64,
    },
    ClientDisconnected {
        user_id: u64,
    },
    /// Op 18: the flags a participant's client announces (`RTC_CONNECTION_FLAGS`; 1 is clips enabled).
    Flags {
        user_id: u64,
        flags: u64,
    },
    /// Our camera opened (true) or is off (false). Report it as `self_video` in the main gateway's voice state.
    VideoState {
        camera: bool,
    },
    /// The camera could not start or stopped by itself; `VideoState { camera: false }` follows.
    CameraFailed {
        reason: String,
    },
    /// Always the last event, unless the task was aborted.
    Disconnected {
        reason: DisconnectReason,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DisconnectReason {
    /// `Controls::disconnect`, or every `Controls` handle was dropped.
    Requested,
    /// Close code 4014: kicked, moved, or the channel was deleted. Do not rejoin automatically.
    Kicked,
    /// The server closed the socket with this code (see `dc_proto::voice_op::close`).
    Closed(u16),
    /// A local or protocol failure; the text never contains secrets or ids.
    Failed(&'static str),
}

/// At quit (or a panic): other applications' volumes go back and the attenuation thread stops.
pub fn shutdown() {
    dc_winaudio::shutdown();
    prefs().flush_recording();
}

/// Global Attenuation exists on Windows only (`supports(ATTENUATION)`).
pub fn attenuation_supported() -> bool {
    dc_winaudio::attenuation_supported()
}

/// At start: puts back what a killed run left lowered, from the journal the attenuation keeps.
pub fn recover_attenuation(journal: &std::path::Path) -> usize {
    dc_winaudio::recover_attenuation(journal)
}

/// Start a voice connection on the current tokio runtime. Media runs on its own threads; no
/// audio device opens before the call is secured. Aborting the returned task also stops it
/// (without a final `Disconnected` event).
pub fn connect(
    join: Join,
    controls: &Controls,
) -> (JoinHandle<()>, mpsc::UnboundedReceiver<Event>) {
    // Dependencies pull in more than one rustls backend, so one is chosen; harmless if set.
    let _ = rustls::crypto::ring::default_provider().install_default();
    // Event sources are rate bounded: signaling at 256 messages/s, speaking at 10 Hz.
    let (events, receiver) = mpsc::unbounded_channel();
    let state = controls.state.subscribe();
    let shared = controls.shared.clone();
    let credentials = Credentials {
        endpoint: join.endpoint,
        token: Zeroizing::new(join.token),
        session_id: Zeroizing::new(join.session_id),
        server_id: join.server_id,
        channel_id: join.channel_id,
        user_id: join.user_id,
    };
    let session_events = events.clone();
    let task = tokio::spawn(async move {
        let result = clock::isolated("dc-voice", move || {
            session::run(credentials, state, session_events, shared)
        })
        .await;
        let reason = result.unwrap_or_else(DisconnectReason::Failed);
        tracing::info!(?reason, "voice connection ended");
        let _ = events.send(Event::Disconnected { reason });
    });
    (task, receiver)
}

/// Starts the second voice connection of a Go Live stream on the current tokio runtime: as the sender with
/// `source`, or as a watcher with `None`. The stream's key, role, endpoint and token come from a
/// [`streams::Tracker`]. Pictures and audio of a watched stream reach `hub`.
pub fn connect_stream(
    join: streams::StreamJoin,
    source: Option<StreamSource>,
    hub: &VideoHub,
    controls: &StreamControls,
) -> (JoinHandle<()>, mpsc::UnboundedReceiver<StreamEvent>) {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let (events, receiver) = mpsc::unbounded_channel();
    let state = controls.0.subscribe();
    let hub = hub.clone();
    let session_events = events.clone();
    let task = tokio::spawn(async move {
        let result = clock::isolated("dc-voice-stream", move || {
            stream_conn::run(join, source, state, hub, session_events)
        })
        .await;
        let reason = result.unwrap_or_else(DisconnectReason::Failed);
        tracing::info!(?reason, "stream connection ended");
        let _ = events.send(StreamEvent::Ended { reason });
    });
    (task, receiver)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn controls_coalesce_and_debug_hides_secrets() {
        let controls = Controls::new();
        let mut state = controls.state.subscribe();
        controls.set_self_mute(false);
        assert!(
            !state.has_changed().unwrap(),
            "no-op changes do not wake the loop"
        );
        controls.clone().set_self_mute(true);
        assert!(state.has_changed().unwrap());
        assert!(state.borrow_and_update().self_mute);
        controls.disconnect();
        assert!(state.borrow_and_update().disconnect);

        let join = Join {
            endpoint: "x.discord.media".into(),
            token: "secret-token".into(),
            session_id: "secret-session".into(),
            server_id: 1,
            channel_id: 2,
            user_id: 987_654_321,
        };
        let shown = format!("{join:?}");
        assert!(!shown.contains("secret") && !shown.contains("987654321"));
    }

    #[tokio::test]
    async fn rejected_endpoint_ends_without_network() {
        let controls = Controls::new();
        let join = Join {
            endpoint: "voice.example.com".into(),
            token: "t".into(),
            session_id: "s".into(),
            server_id: 1,
            channel_id: 2,
            user_id: 3,
        };
        let (task, mut events) = connect(join, &controls);
        task.await.unwrap();
        assert_eq!(
            events.recv().await,
            Some(Event::Disconnected {
                reason: DisconnectReason::Failed("Voice endpoint is outside Discord media")
            })
        );
    }
}
