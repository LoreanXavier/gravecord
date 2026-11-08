//! What the camera and Go Live work needs from the machine: camera and screen capture, system-audio capture, and H.264
//! encoding and decoding. No network and no Discord code lives here; the caller owns every thread that touches a
//! network and decides when a device is opened.
//!
//! Windows only for now. Elsewhere the same names exist and return [`Error::Unsupported`], so the workspace builds on
//! every target. Adapted from Serein (`vendor/serein`, MIT OR Apache-2.0); every adapted part names its source file.

// Per-pixel loops over tight 4-byte pixels read better with `chunks_exact(4)` than with `as_chunks`.
#![allow(clippy::chunks_exact_to_as_chunks)]

pub mod camera;
pub mod decode;
pub mod h264;
pub mod screen;

mod convert;
mod encode;
#[cfg(windows)]
mod mfrt;
mod picture;

pub use convert::fit_size;
pub use encode::Backend;
pub use picture::{Layout, Picture};

/// One encoded H.264 access unit in Annex B form, as the encoders hand it over.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EncodedFrame {
    pub data: Vec<u8>,
    /// An IDR picture: a viewer can start decoding here.
    pub keyframe: bool,
    /// 90 kHz ticks since the capture started, wrapping like an RTP timestamp.
    pub timestamp: u32,
}

/// Why a device or codec could not be used.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// This platform has no implementation yet.
    #[error("{0} is not available on this platform")]
    Unsupported(&'static str),
    /// The caller passed settings or an id that cannot work.
    #[error("invalid argument: {0}")]
    Invalid(String),
    /// The device or source is gone, busy, or blocked by the system.
    #[error("{0}")]
    Unavailable(String),
    /// The operation started and failed.
    #[error("{0}")]
    Failed(String),
}

pub type Result<T> = std::result::Result<T, Error>;

impl From<&'static str> for Error {
    fn from(message: &'static str) -> Self {
        Self::Unavailable(message.into())
    }
}

impl Error {
    pub(crate) fn unavailable(message: impl Into<String>) -> Self {
        Self::Unavailable(message.into())
    }

    pub(crate) fn failed(message: impl Into<String>) -> Self {
        Self::Failed(message.into())
    }

    pub(crate) fn invalid(message: impl Into<String>) -> Self {
        Self::Invalid(message.into())
    }
}

/// 90 kHz RTP ticks for a duration, wrapping at 32 bits.
pub fn rtp_ticks(elapsed: std::time::Duration) -> u32 {
    (elapsed.as_micros() * 90 / 1000) as u32
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn rtp_ticks_are_90_khz_and_wrap() {
        assert_eq!(rtp_ticks(Duration::from_secs(1)), 90_000);
        assert_eq!(rtp_ticks(Duration::from_millis(40)), 3_600);
        // 2^32 ticks is 47_721.858... seconds: one second past it lands on a small value again.
        let wrap = Duration::from_micros(((1u128 << 32) * 1_000_000 / 90_000) as u64);
        assert!(rtp_ticks(wrap + Duration::from_secs(1)) < 100_000);
    }
}
