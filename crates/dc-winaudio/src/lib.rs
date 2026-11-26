//! Windows-only audio and network pieces of the voice stack, kept out of `dc-voice` so that crate stays free of
//! `unsafe`: other applications' audio session volumes (global attenuation), qWAVE packet priority and the notifications
//! of audio endpoint changes.
//!
//! The attenuation's decisions and bookkeeping are plain Rust over a [`attenuation::Backend`]; only
//! `wasapi.rs` and `qos.rs` call into Windows.

pub mod attenuation;
#[cfg(windows)]
mod endpoints;
pub mod qos;
#[cfg(windows)]
mod wasapi;

pub use attenuation::{Controller, Settings};
pub use qos::{Plan, QosError, QosFlow};

use std::path::Path;
use std::sync::OnceLock;

/// The process-wide attenuation controller on the system's audio sessions.
pub fn attenuation() -> &'static Controller {
    static CONTROLLER: OnceLock<Controller> = OnceLock::new();
    CONTROLLER.get_or_init(|| Controller::new(system_backend))
}

#[cfg(windows)]
fn system_backend() -> Box<dyn attenuation::Backend> {
    match wasapi::SystemBackend::new() {
        Ok(backend) => Box::new(backend),
        Err(error) => {
            tracing::warn!("global attenuation unavailable: {error}");
            Box::new(Unavailable)
        }
    }
}

#[cfg(not(windows))]
fn system_backend() -> Box<dyn attenuation::Backend> {
    Box::new(Unavailable)
}

/// No sessions to hold: attenuation exists on Windows only.
struct Unavailable;

impl attenuation::Backend for Unavailable {
    fn sessions(&mut self) -> Result<Vec<attenuation::Session>, String> {
        Ok(Vec::new())
    }
    fn set_volume(&mut self, _: &str, _: f32) -> Result<(), String> {
        Err("global attenuation needs Windows".to_owned())
    }
}

/// Whether global attenuation exists here (`supports(ATTENUATION)`: Windows only).
pub const fn attenuation_supported() -> bool {
    cfg!(windows)
}

/// At start: puts back what a run that was killed left held down, from its journal. Returns how many sessions.
pub fn recover_attenuation(journal: &Path) -> usize {
    #[cfg(windows)]
    {
        // COM lives on this thread only for the call.
        match wasapi::SystemBackend::new() {
            Ok(mut backend) => attenuation::recover(&mut backend, journal),
            Err(_) => 0,
        }
    }
    #[cfg(not(windows))]
    {
        let _ = journal;
        0
    }
}

/// At quit: restores everything the attenuation holds and stops its thread.
pub fn shutdown() {
    attenuation().shutdown();
}

/// Calls `changed` whenever Windows reports an audio endpoint added, removed, enabled or disabled, renamed, or made
/// the default, for as long as the returned registration lives (build and drop it on one thread). An error where
/// that is not available: the caller lists the devices on a timer instead.
#[cfg(windows)]
pub fn watch_endpoints(
    changed: impl Fn() + Send + Sync + 'static,
) -> Result<endpoints::EndpointWatch, String> {
    endpoints::EndpointWatch::new(changed)
}

#[cfg(not(windows))]
pub fn watch_endpoints(changed: impl Fn() + Send + Sync + 'static) -> Result<(), String> {
    let _ = changed;
    Err("endpoint notifications need Windows".to_owned())
}
