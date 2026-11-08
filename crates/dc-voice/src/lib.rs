//! Audio-only Discord voice connection: the voice control socket, UDP media with `rtpsize`
//! AEAD transport encryption, DAVE end-to-end encryption, Opus, and the default microphone
//! and speaker.
//!
//! Adapted from Serein's `discord-voice` crate (`vendor/serein/crates/discord-voice`,
//! MIT OR Apache-2.0); each module names the Serein file it comes from. Video, screen share,
//! camera, Go Live, echo cancellation and noise suppression are not ported.
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
mod activity;
mod audio;
mod capture;
mod clock;
mod dave;
mod jitter;
mod mixer;
mod rtp;
mod session;
mod signal;

pub use audio::{
    Prefs, default_input_device, default_output_device, input_devices, output_devices, prefs,
};
pub use rtp::Mode;

use session::{ControlState, Credentials};
use std::{fmt, sync::Arc};
use tokio::{
    sync::{mpsc, watch},
    task::JoinHandle,
};
use zeroize::Zeroizing;

/// One 20 ms mono frame at 48 kHz.
pub(crate) type Frame = [f32; 960];

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
pub struct Controls(Arc<watch::Sender<ControlState>>);
impl Default for Controls {
    fn default() -> Self {
        Self::new()
    }
}
impl Controls {
    pub fn new() -> Self {
        Self(Arc::new(watch::channel(ControlState::default()).0))
    }
    /// Stop sending microphone audio. The main gateway's voice state still has to report it.
    pub fn set_self_mute(&self, muted: bool) {
        self.0
            .send_if_modified(|state| std::mem::replace(&mut state.self_mute, muted) != muted);
    }
    /// Stop playback and sending. The main gateway's voice state still has to report it.
    pub fn set_self_deaf(&self, deafened: bool) {
        self.0.send_if_modified(|state| {
            std::mem::replace(&mut state.self_deaf, deafened) != deafened
        });
    }
    /// Close the voice connection. Leaving the channel itself is a main gateway voice state.
    pub fn disconnect(&self) {
        self.0
            .send_if_modified(|state| !std::mem::replace(&mut state.disconnect, true));
    }
    pub fn self_mute(&self) -> bool {
        self.0.borrow().self_mute
    }
    pub fn self_deaf(&self) -> bool {
        self.0.borrow().self_deaf
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
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
    ClientConnected {
        user_id: u64,
    },
    ClientDisconnected {
        user_id: u64,
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
    let state = controls.0.subscribe();
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
            session::run(credentials, state, session_events)
        })
        .await;
        let reason = result.unwrap_or_else(DisconnectReason::Failed);
        tracing::info!(?reason, "voice connection ended");
        let _ = events.send(Event::Disconnected { reason });
    });
    (task, receiver)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn controls_coalesce_and_debug_hides_secrets() {
        let controls = Controls::new();
        let mut state = controls.0.subscribe();
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
