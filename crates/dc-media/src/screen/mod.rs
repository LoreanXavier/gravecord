//! Screen sharing from the machine's side: which displays and windows can be shared, a picture of each for the picker,
//! and a worker that captures one and produces H.264 plus optional system audio. No network, no Discord.
//!
//! Capture is Windows Graphics Capture over the `windows` crate, the encoder is Media Foundation hardware H.264 with
//! openh264 as the fallback, and the audio is WASAPI process loopback that excludes this process tree. Other platforms
//! compile to stubs that return [`crate::Error::Unsupported`].

pub mod audio;
mod settings;
mod sources;

#[cfg(windows)]
mod capture;
#[cfg(windows)]
mod enumerate;
#[cfg(windows)]
mod worker;

use crate::convert::{PixelOrder, Raw, downscale_rgba};
use crate::{Error, Picture, Result};

pub use audio::{AudioCapture, AudioChunk};
pub use settings::{
    BITRATE_HIGH, BITRATE_MIN, BITRATE_STANDARD, FRAME_RATES, MAX_HEIGHT, MAX_WIDTH, Resolution,
    Settings, SourceId, StreamQuality, max_bitrate, offered_frame_rates, offered_resolutions,
    output_size, required_quality, software_rate_change,
};
#[cfg(windows)]
pub use worker::{PREVIEW_WIDTH, Worker};

/// A display or window that can be shared.
#[derive(Clone, Debug)]
pub struct Source {
    pub id: SourceId,
    pub name: String,
    pub width: u32,
    pub height: u32,
    /// 0 when the system does not say.
    pub refresh_hz: u32,
    /// A window's icon (RGBA, at most 64x64); displays have none.
    pub icon: Option<Picture>,
}

/// The displays and then the titled windows that can be shared, without this process's own windows.
#[cfg(windows)]
pub fn sources() -> Result<Vec<Source>> {
    enumerate::sources()
}

#[cfg(not(windows))]
pub fn sources() -> Result<Vec<Source>> {
    Err(Error::Unsupported("screen sources"))
}

/// One picture of `source`, scaled down (never up) to fit `max_width` x `max_height`, RGBA.
#[cfg(windows)]
pub fn thumbnail(source: &SourceId, max_width: u32, max_height: u32) -> Result<Picture> {
    if max_width == 0 || max_height == 0 {
        return Err(Error::invalid("thumbnail size must be nonzero"));
    }
    worker::thumbnail(*source, max_width, max_height)
}

#[cfg(not(windows))]
pub fn thumbnail(_source: &SourceId, _max_width: u32, _max_height: u32) -> Result<Picture> {
    Err(Error::Unsupported("screen thumbnails"))
}

/// Cuts a tight BGRA capture down to a thumbnail.
pub(crate) fn thumbnail_from(
    bgra: &[u8],
    width: u32,
    height: u32,
    max_width: u32,
    max_height: u32,
) -> Result<Picture> {
    downscale_rgba(
        Raw {
            width: width as usize,
            height: height as usize,
            stride: width as usize * 4,
            order: PixelOrder::Bgra,
            data: bgra,
        },
        max_width,
        max_height,
    )
}

/// The share worker is Windows-only for now.
#[cfg(not(windows))]
pub struct Worker(());

#[cfg(not(windows))]
impl Worker {
    pub fn start(
        _settings: Settings,
        _video: impl FnMut(crate::EncodedFrame) + Send + 'static,
        _audio: impl FnMut(AudioChunk) + Send + 'static,
    ) -> Result<Self> {
        Err(Error::Unsupported("screen sharing"))
    }

    pub fn set_bitrate(&self, _bitrate: u32) {}
    pub fn force_keyframe(&self) {}
    pub fn take_preview(&self) -> Option<Picture> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thumbnails_keep_the_aspect_ratio_and_never_grow() {
        // 4x2 BGRA: left half blue (B=255), right half red.
        let mut bgra = Vec::new();
        for _ in 0..2 {
            for x in 0..4 {
                bgra.extend_from_slice(if x < 2 {
                    &[255, 0, 0, 255]
                } else {
                    &[0, 0, 255, 255]
                });
            }
        }
        let small = thumbnail_from(&bgra, 4, 2, 2, 2).unwrap();
        assert_eq!((small.width, small.height), (2, 1));
        assert_eq!(&small.data[..4], &[0, 0, 255, 255]);
        assert_eq!(&small.data[4..8], &[255, 0, 0, 255]);
        let same = thumbnail_from(&bgra, 4, 2, 100, 100).unwrap();
        assert_eq!((same.width, same.height), (4, 2));
    }
}
