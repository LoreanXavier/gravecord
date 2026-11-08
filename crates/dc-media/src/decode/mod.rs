//! H.264 decoding of Annex B access units: the Windows Media Foundation decoder (`CLSID_MSH264DecoderMFT`, DXVA when a
//! GPU offers it) first, openh264 when that is missing or fails to start.
//!
//! No threads live in here and nothing is shared: one [`Decoder`] per remote source, driven by whichever thread the
//! caller picks. The hardware path keeps COM state on the thread that made it, so a `Decoder` is not `Send`; make it on
//! the thread that will call [`Decoder::decode`].
//!
//! Adapted from Serein's `crates/discord-voice/src/video_receive.rs` (`Backend`, `decode_timed`) and
//! `crates/platform/src/video/live_windows.rs` (MIT OR Apache-2.0).

use crate::{Error, Layout, Picture, Result, h264, picture::pack_plane};
use openh264::formats::YUVSource;
use std::marker::PhantomData;

#[cfg(windows)]
mod mf;

/// Pictures larger than this many pixels are refused: a corrupt stream must not make the decoder allocate gigabytes.
pub const MAX_PICTURE_PIXELS: usize = 7680 * 4320;

pub use crate::encode::Backend;

/// Why an access unit produced nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum DecodeError {
    /// The decoder has no reference picture. Nothing was fed; ask the sender for a keyframe and offer one.
    #[error("waiting for a keyframe")]
    NeedKeyframe,
    /// Empty or oversized input. The decoder's state is unchanged.
    #[error("invalid access unit")]
    Invalid,
    /// The decoder rejected the data. It now needs a keyframe.
    #[error("the decoder rejected the access unit")]
    Failed,
}

enum Inner {
    #[cfg(windows)]
    Hardware(mf::Decoder),
    Software(openh264::decoder::Decoder),
}

pub struct Decoder {
    inner: Option<Inner>,
    backend: Backend,
    needs_keyframe: bool,
    /// The backend raised an error and is rebuilt at the next keyframe.
    dirty: bool,
    /// Windows COM state of the hardware path belongs to one thread.
    _thread: PhantomData<*const ()>,
}

impl Decoder {
    /// Hardware when the machine has the Media Foundation decoder, openh264 otherwise.
    pub fn new() -> Result<Self> {
        Self::hardware().or_else(|error| {
            tracing::debug!("hardware H.264 decoder unavailable: {error}");
            Self::software()
        })
    }

    /// openh264 only, whatever the machine offers.
    pub fn software() -> Result<Self> {
        Ok(Self::with(Inner::software()?, Backend::Software))
    }

    /// The Media Foundation decoder only; an error where it cannot start.
    pub fn hardware() -> Result<Self> {
        #[cfg(windows)]
        {
            Ok(Self::with(
                Inner::Hardware(mf::Decoder::new()?),
                Backend::Hardware,
            ))
        }
        #[cfg(not(windows))]
        Err(Error::Unsupported("the hardware H.264 decoder"))
    }

    fn with(inner: Inner, backend: Backend) -> Self {
        Self {
            inner: Some(inner),
            backend,
            needs_keyframe: true,
            dirty: false,
            _thread: PhantomData,
        }
    }

    pub fn backend(&self) -> Backend {
        self.backend
    }

    /// True before the first keyframe and after any error: [`Decoder::decode`] refuses delta frames until one arrives.
    pub fn needs_keyframe(&self) -> bool {
        self.needs_keyframe
    }

    /// Feeds one access unit. `Ok(None)` when the decoder is still buffering. NV12 from the hardware path and I420 from
    /// openh264, tightly packed; colour is BT.601 limited range either way.
    pub fn decode(
        &mut self,
        access_unit: &[u8],
    ) -> std::result::Result<Option<Picture>, DecodeError> {
        if access_unit.is_empty() || access_unit.len() > h264::MAX_ACCESS_UNIT {
            return Err(DecodeError::Invalid);
        }
        let keyframe = h264::is_keyframe(access_unit);
        if self.needs_keyframe && !keyframe {
            return Err(DecodeError::NeedKeyframe);
        }
        if self.dirty {
            // A fresh decoder starts clean; the keyframe carries what it needs.
            match self.rebuild() {
                Ok(()) => self.dirty = false,
                Err(_) => return Err(DecodeError::Failed),
            }
        }
        let picture = match self
            .inner
            .as_mut()
            .ok_or(DecodeError::Failed)?
            .decode(access_unit)
        {
            Ok(picture) => picture,
            Err(error) => {
                tracing::debug!("H.264 decode failed: {error}");
                self.needs_keyframe = true;
                self.dirty = true;
                return Err(DecodeError::Failed);
            }
        };
        if keyframe {
            self.needs_keyframe = false;
        }
        Ok(picture)
    }

    fn rebuild(&mut self) -> Result<()> {
        self.inner = None;
        self.inner = Some(match self.backend {
            Backend::Software => Inner::software()?,
            #[cfg(windows)]
            Backend::Hardware => Inner::Hardware(mf::Decoder::new()?),
            #[cfg(not(windows))]
            Backend::Hardware => return Err(Error::Unsupported("the hardware H.264 decoder")),
        });
        Ok(())
    }
}

impl Inner {
    fn software() -> Result<Self> {
        openh264::decoder::Decoder::new()
            .map(Self::Software)
            .map_err(|e| Error::unavailable(format!("openh264 decoder could not start: {e}")))
    }

    fn decode(&mut self, access_unit: &[u8]) -> Result<Option<Picture>> {
        match self {
            #[cfg(windows)]
            Self::Hardware(decoder) => decoder.decode(access_unit),
            Self::Software(decoder) => {
                let decoded = match decoder.decode(access_unit) {
                    Ok(Some(yuv)) => yuv,
                    Ok(None) => return Ok(None),
                    Err(e) => return Err(Error::failed(format!("openh264: {e}"))),
                };
                let (width, height) = decoded.dimensions();
                if width == 0 || height == 0 || width.saturating_mul(height) > MAX_PICTURE_PIXELS {
                    return Err(Error::failed("decoded picture has an unsupported size"));
                }
                let (chroma_width, chroma_height) = (width.div_ceil(2), height.div_ceil(2));
                let (luma_stride, cb_stride, cr_stride) = decoded.strides();
                let (width32, height32) = (width as u32, height as u32);
                let mut data = Vec::with_capacity(
                    Layout::I420
                        .len(width32, height32)
                        .ok_or_else(|| Error::failed("picture size overflows"))?,
                );
                pack_plane(&mut data, decoded.y(), luma_stride, width, height)
                    .and_then(|()| {
                        pack_plane(
                            &mut data,
                            decoded.u(),
                            cb_stride,
                            chroma_width,
                            chroma_height,
                        )
                    })
                    .and_then(|()| {
                        pack_plane(
                            &mut data,
                            decoded.v(),
                            cr_stride,
                            chroma_width,
                            chroma_height,
                        )
                    })
                    .ok_or_else(|| {
                        Error::failed("decoded planes are shorter than their strides say")
                    })?;
                Ok(Some(Picture {
                    width: width32,
                    height: height32,
                    layout: Layout::I420,
                    data,
                }))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::convert::{Chroma, PixelOrder, Raw, to_yuv420};
    use crate::encode::{Config, Content, Encoder, Profile};

    /// A test card: black with a pure red square at (16..48, 16..48) of a 128x96 picture, as BGRA.
    pub(crate) fn test_card(width: usize, height: usize) -> Vec<u8> {
        let mut bgra = vec![0u8; width * height * 4];
        for pixel in bgra.chunks_exact_mut(4) {
            pixel.copy_from_slice(&[0, 0, 0, 255]);
        }
        for y in 16..48 {
            for x in 16..48 {
                bgra[(y * width + x) * 4..][..4].copy_from_slice(&[0, 0, 255, 255]);
            }
        }
        bgra
    }

    pub(crate) fn encoder(width: u32, height: u32) -> Encoder {
        Encoder::software(Config {
            width,
            height,
            fps: 30,
            bitrate: 2_000_000,
            max_bytes: h264::MAX_ACCESS_UNIT,
            profile: Profile::Main,
            content: Content::Screen,
        })
        .unwrap()
    }

    fn yuv(bgra: &[u8], width: usize, height: usize) -> Vec<u8> {
        let mut out = vec![0; width * height * 3 / 2];
        let frame = Raw {
            width,
            height,
            stride: width * 4,
            order: PixelOrder::Bgra,
            data: bgra,
        };
        to_yuv420(frame, width, height, Chroma::Planar, &mut out).unwrap();
        out
    }

    fn rgb_at(rgba: &[u8], width: usize, x: usize, y: usize) -> [u8; 3] {
        let at = (y * width + x) * 4;
        [rgba[at], rgba[at + 1], rgba[at + 2]]
    }

    /// Software encode, software decode: the size survives, the red square comes back red and the black stays black
    /// within a stated tolerance (lossy coding at 2 Mb/s plus 4:2:0 chroma: 24 levels), and keyframes are honoured.
    #[test]
    fn software_round_trip_keeps_size_and_colours() {
        let (width, height) = (128usize, 96usize);
        let planes = yuv(&test_card(width, height), width, height);
        let mut encoder = encoder(width as u32, height as u32);
        let mut decoder = Decoder::software().unwrap();
        assert!(decoder.needs_keyframe());

        let first = encoder
            .encode(&planes, Chroma::Planar, true)
            .unwrap()
            .unwrap();
        assert!(first.keyframe && h264::is_keyframe(&first.data));
        assert!(h264::has_parameter_sets(&first.data));
        let picture = decoder.decode(&first.data).unwrap().expect("a picture");
        assert!(!decoder.needs_keyframe());
        assert_eq!(
            (picture.width, picture.height),
            (width as u32, height as u32)
        );
        assert_eq!(picture.layout, Layout::I420);
        let rgba = picture.to_rgba().unwrap();
        let inside = rgb_at(&rgba, width, 32, 32);
        assert!(
            inside[0] >= 255 - 24 && inside[1] <= 24 && inside[2] <= 24,
            "red square decoded as {inside:?}"
        );
        for outside in [(4, 4), (100, 80), (70, 30)] {
            let pixel = rgb_at(&rgba, width, outside.0, outside.1);
            assert!(pixel.iter().all(|&c| c <= 24), "black decoded as {pixel:?}");
        }

        // Delta frames follow without a keyframe request; a requested keyframe really is one.
        let mut deltas = 0;
        for _ in 0..4 {
            if let Some(out) = encoder.encode(&planes, Chroma::Planar, false).unwrap() {
                assert!(!out.keyframe);
                decoder.decode(&out.data).unwrap();
                deltas += 1;
            }
        }
        assert!(deltas > 0);
        let forced = encoder
            .encode(&planes, Chroma::Planar, true)
            .unwrap()
            .unwrap();
        assert!(forced.keyframe && h264::has_parameter_sets(&forced.data));
    }

    #[test]
    fn errors_demand_a_keyframe_and_the_next_one_recovers() {
        let (width, height) = (64usize, 64usize);
        let planes = yuv(&test_card(width, height), width, height);
        let mut encoder = encoder(64, 64);
        let mut decoder = Decoder::software().unwrap();

        // A delta frame before any keyframe is refused without being fed.
        let key = encoder
            .encode(&planes, Chroma::Planar, true)
            .unwrap()
            .unwrap();
        let delta = loop {
            if let Some(out) = encoder.encode(&planes, Chroma::Planar, false).unwrap() {
                break out;
            }
        };
        assert_eq!(
            decoder.decode(&delta.data).unwrap_err(),
            DecodeError::NeedKeyframe
        );
        assert_eq!(decoder.decode(&[]).unwrap_err(), DecodeError::Invalid);
        assert!(decoder.needs_keyframe());

        decoder.decode(&key.data).unwrap();
        assert!(!decoder.needs_keyframe());
        assert!(decoder.decode(&delta.data).is_ok());

        // Garbage that claims to be a keyframe fails and puts the decoder back to waiting.
        let garbage = [0, 0, 0, 1, 0x65, 0xde, 0xad, 0xbe, 0xef, 0x13, 0x37];
        if decoder.decode(&garbage).is_err() {
            assert!(decoder.needs_keyframe());
            assert_eq!(
                decoder.decode(&delta.data).unwrap_err(),
                DecodeError::NeedKeyframe
            );
        }
        // A real keyframe always brings it back.
        let key = encoder
            .encode(&planes, Chroma::Planar, true)
            .unwrap()
            .unwrap();
        decoder.decode(&key.data).unwrap();
        assert!(!decoder.needs_keyframe());
    }

    #[test]
    #[ignore = "starts the hardware decoder"]
    fn the_default_decoder_works_whichever_backend_it_picked() {
        let mut decoder = Decoder::new().unwrap();
        assert!(decoder.needs_keyframe());
        let _ = decoder.backend();
        assert_eq!(
            decoder
                .decode(&vec![0; h264::MAX_ACCESS_UNIT + 1])
                .unwrap_err(),
            DecodeError::Invalid
        );
    }
}
