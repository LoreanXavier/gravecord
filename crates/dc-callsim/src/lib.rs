//! The call simulator: calls with camera and Go Live through the real client code (`dc-voice`, the app's call state)
//! without Discord's servers and without a second person. Capture builds and tests only.
//!
//! * [`server::VoiceServer`]: a voice server on 127.0.0.1 (control socket, SFU, DAVE delivery service).
//! * [`world::Sim`]: the gateway side of a call (what Discord dispatches back to the app's voice requests), the voice
//!   servers it points at, and headless participants that are `dc-voice` clients themselves.
//! * [`link::Shape`]: the network of each participant.
//!
//! Starting a [`world::Sim`] calls `dc_voice::sim::activate`: from then on this process reaches no voice endpoint but
//! the simulator's, opens no microphone, speaker or camera, and captures no screen.
pub mod link;
mod mls;
pub mod server;
pub mod stats;
mod wire;
pub mod world;

pub use link::Shape;
pub use server::Feedback;
pub use stats::{Board, Summary};
pub use world::{STREAM_TONES, Setup, Sim, Sink, VOICE_TONES};
