//! H.264 decoding of Annex B access units: the Windows Media Foundation decoder (`CLSID_MSH264DecoderMFT`, on its
//! multi-threaded CPU path: see `mf.rs` for why not DXVA) first, openh264 when that is missing or fails to start.
//!
//! No threads live in here and nothing is shared: one [`Decoder`] per remote source, driven by whichever thread the
//! caller picks. The hardware path keeps COM state on the thread that made it, so a `Decoder` is not `Send`; make it on
//! the thread that will call [`Decoder::decode`].
//!
//! Adapted from Serein's `crates/discord-voice/src/video_receive.rs` (`Backend`, `decode_timed`) and
//! `crates/platform/src/video/live_windows.rs` (MIT OR Apache-2.0).

use crate::{Error, Layout, Picture, Result, h264, picture::pack_plane};
use openh264::{
    OpenH264API,
    decoder::{DecodeOptions, DecoderConfig, Flush},
    formats::YUVSource,
};
use std::cmp::{Ordering, Reverse};
use std::collections::BinaryHeap;
use std::marker::PhantomData;

#[cfg(windows)]
mod mf;

/// Pictures larger than this many pixels are refused: a corrupt stream must not make the decoder allocate gigabytes.
pub const MAX_PICTURE_PIXELS: usize = 7680 * 4320;
/// Maximum DPB / B-frame reorder depth handled in VOD mode.
pub const VOD_MAX_REORDER: usize = 16;

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

#[derive(Debug)]
struct OrderedPicture {
    pts_100ns: i64,
    seq: u64,
    picture: Picture,
}

impl Eq for OrderedPicture {}

impl PartialEq for OrderedPicture {
    fn eq(&self, other: &Self) -> bool {
        self.pts_100ns == other.pts_100ns && self.seq == other.seq
    }
}

impl Ord for OrderedPicture {
    fn cmp(&self, other: &Self) -> Ordering {
        self.pts_100ns
            .cmp(&other.pts_100ns)
            .then_with(|| self.seq.cmp(&other.seq))
    }
}

impl PartialOrd for OrderedPicture {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

#[derive(Default)]
struct VodReorder {
    max_reorder: usize,
    pending_pts: BinaryHeap<Reverse<i64>>,
    ready: BinaryHeap<Reverse<OrderedPicture>>,
    seq: u64,
}

impl VodReorder {
    fn new(max_reorder: usize) -> Self {
        Self {
            max_reorder: max_reorder.min(VOD_MAX_REORDER),
            ..Self::default()
        }
    }

    fn clear(&mut self) {
        self.pending_pts.clear();
        self.ready.clear();
    }

    fn push_ready(&mut self, pts_100ns: i64, picture: Picture) {
        let seq = self.seq;
        self.seq = self.seq.wrapping_add(1);
        self.ready.push(Reverse(OrderedPicture {
            pts_100ns,
            seq,
            picture,
        }));
    }

    fn drain_excess(&mut self, out: &mut Vec<(i64, Picture)>) {
        while self.ready.len() > self.max_reorder {
            if let Some(Reverse(entry)) = self.ready.pop() {
                out.push((entry.pts_100ns, entry.picture));
            }
        }
    }

    fn drain_all(&mut self, out: &mut Vec<(i64, Picture)>) {
        while let Some(Reverse(entry)) = self.ready.pop() {
            out.push((entry.pts_100ns, entry.picture));
        }
        self.pending_pts.clear();
    }
}

pub struct Decoder {
    inner: Option<Inner>,
    backend: Backend,
    vod: bool,
    reorder: VodReorder,
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

    /// VOD decoder configured without low-latency flushing so Main/High MP4 B-frames reorder cleanly.
    pub fn new_vod() -> Result<Self> {
        Self::hardware_vod().or_else(|error| {
            tracing::debug!("hardware H.264 VOD decoder unavailable: {error}");
            Self::software_vod()
        })
    }

    /// openh264 only, whatever the machine offers.
    pub fn software() -> Result<Self> {
        Ok(Self::with(Inner::software()?, Backend::Software, false))
    }

    /// openh264 only in VOD mode (`Flush::NoFlush` + PTS reorder buffer).
    pub fn software_vod() -> Result<Self> {
        Ok(Self::with(Inner::software_vod()?, Backend::Software, true))
    }

    /// The Media Foundation decoder only; an error where it cannot start.
    pub fn hardware() -> Result<Self> {
        #[cfg(windows)]
        {
            Ok(Self::with(
                Inner::Hardware(mf::Decoder::new()?),
                Backend::Hardware,
                false,
            ))
        }
        #[cfg(not(windows))]
        Err(Error::Unsupported("the hardware H.264 decoder"))
    }

    /// The Media Foundation decoder in VOD mode (full DPB B-frame reordering).
    pub fn hardware_vod() -> Result<Self> {
        #[cfg(windows)]
        {
            Ok(Self::with(
                Inner::Hardware(mf::Decoder::new_vod()?),
                Backend::Hardware,
                true,
            ))
        }
        #[cfg(not(windows))]
        Err(Error::Unsupported("the hardware H.264 VOD decoder"))
    }

    fn with(inner: Inner, backend: Backend, vod: bool) -> Self {
        Self {
            inner: Some(inner),
            backend,
            vod,
            reorder: VodReorder::new(if vod { VOD_MAX_REORDER } else { 0 }),
            needs_keyframe: true,
            dirty: false,
            _thread: PhantomData,
        }
    }

    /// Sets the VOD PTS reorder window (`0..=VOD_MAX_REORDER`); callers with container `ctts` info can set `0` for
    /// monotonic I/P tracks to eliminate startup/seek lag while preserving up to 16 frames for B-frame tracks.
    pub fn set_vod_reorder(&mut self, max_reorder: usize) {
        self.reorder.max_reorder = max_reorder.min(VOD_MAX_REORDER);
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

    /// Feeds one VOD access unit with its container presentation timestamp (`100 ns` units) and returns reordered
    /// display frames ready at this point.
    pub fn decode_vod(
        &mut self,
        access_unit: &[u8],
        pts_100ns: i64,
    ) -> std::result::Result<Vec<(i64, Picture)>, DecodeError> {
        if !self.vod || access_unit.is_empty() || access_unit.len() > h264::MAX_VOD_ACCESS_UNIT {
            return Err(DecodeError::Invalid);
        }
        let keyframe = h264::is_keyframe(access_unit);
        if self.needs_keyframe && !keyframe {
            return Err(DecodeError::NeedKeyframe);
        }
        if self.dirty {
            match self.rebuild() {
                Ok(()) => self.dirty = false,
                Err(_) => return Err(DecodeError::Failed),
            }
        }
        let inner = self.inner.as_mut().ok_or(DecodeError::Failed)?;
        match inner {
            #[cfg(windows)]
            Inner::Hardware(decoder) => match decoder.decode_vod(access_unit, pts_100ns) {
                Ok(frames) => {
                    for (pts, pic) in frames {
                        self.reorder.push_ready(pts, pic);
                    }
                }
                Err(error) => {
                    tracing::debug!("MF VOD decode failed: {error}");
                    self.needs_keyframe = true;
                    self.dirty = true;
                    self.reorder.clear();
                    return Err(DecodeError::Failed);
                }
            },
            Inner::Software(decoder) => {
                let has_slice = h264::nals(access_unit).any(|nal| matches!(nal[0] & 31, 1..=5));
                let options = DecodeOptions::new().flush_after_decode(Flush::NoFlush);
                match decoder.decode_with_options(access_unit, options) {
                    Ok(opt) => {
                        if has_slice {
                            self.reorder.pending_pts.push(Reverse(pts_100ns));
                            // Cap runaway pending timestamps on malformed streams without matching output.
                            while self.reorder.pending_pts.len() > 64 {
                                self.reorder.pending_pts.pop();
                            }
                        }
                        if let Some(yuv) = opt {
                            let pic = yuv_to_picture(yuv).map_err(|_| DecodeError::Failed)?;
                            let assigned = self
                                .reorder
                                .pending_pts
                                .pop()
                                .map_or(pts_100ns, |Reverse(p)| p);
                            self.reorder.push_ready(assigned, pic);
                        }
                    }
                    Err(error) => {
                        tracing::debug!("openh264 VOD decode failed: {error}");
                        self.needs_keyframe = true;
                        self.dirty = true;
                        self.reorder.clear();
                        return Err(DecodeError::Failed);
                    }
                }
            }
        }
        if keyframe {
            self.needs_keyframe = false;
        }
        let mut out = Vec::new();
        self.reorder.drain_excess(&mut out);
        Ok(out)
    }

    /// Drains all remaining delayed/reordered VOD frames at EOS in ascending presentation timestamp order.
    pub fn finish_vod(&mut self) -> std::result::Result<Vec<(i64, Picture)>, DecodeError> {
        if !self.vod {
            return Err(DecodeError::Invalid);
        }
        if let Some(inner) = self.inner.as_mut() {
            match inner {
                #[cfg(windows)]
                Inner::Hardware(decoder) => match decoder.drain_eos() {
                    Ok(frames) => {
                        for (pts, pic) in frames {
                            self.reorder.push_ready(pts, pic);
                        }
                    }
                    Err(error) => {
                        tracing::debug!("MF VOD EOS drain failed: {error}");
                        self.dirty = true;
                        self.needs_keyframe = true;
                        self.reorder.clear();
                        return Err(DecodeError::Failed);
                    }
                },
                Inner::Software(decoder) => match decoder.flush_remaining() {
                    Ok(frames) => {
                        for yuv in frames {
                            let pic = yuv_to_picture(yuv).map_err(|_| DecodeError::Failed)?;
                            let assigned = self.reorder.pending_pts.pop().map_or(0, |Reverse(p)| p);
                            self.reorder.push_ready(assigned, pic);
                        }
                    }
                    Err(error) => {
                        tracing::debug!("openh264 VOD flush failed: {error}");
                        self.dirty = true;
                        self.needs_keyframe = true;
                        self.reorder.clear();
                        return Err(DecodeError::Failed);
                    }
                },
            }
        }
        let mut out = Vec::new();
        self.reorder.drain_all(&mut out);
        Ok(out)
    }

    /// Flushes DPB state on seek so decoding can resume cleanly from the next keyframe.
    pub fn flush_vod(&mut self) -> Result<()> {
        if !self.vod {
            return Err(Error::invalid("flush_vod called on live decoder"));
        }
        self.reorder.clear();
        self.needs_keyframe = true;
        if self.dirty {
            return self.rebuild();
        }
        if let Some(inner) = self.inner.as_mut() {
            match inner {
                #[cfg(windows)]
                Inner::Hardware(decoder) => {
                    if let Err(error) = decoder.flush() {
                        tracing::debug!("MF VOD flush failed, rebuilding decoder: {error}");
                        return self.rebuild();
                    }
                }
                Inner::Software(decoder) => {
                    let _ = decoder.flush_remaining();
                }
            }
        }
        Ok(())
    }

    fn rebuild(&mut self) -> Result<()> {
        self.inner = None;
        self.reorder.clear();
        self.inner = Some(match self.backend {
            Backend::Software if self.vod => Inner::software_vod()?,
            Backend::Software => Inner::software()?,
            #[cfg(windows)]
            Backend::Hardware if self.vod => Inner::Hardware(mf::Decoder::new_vod()?),
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

    fn software_vod() -> Result<Self> {
        let config = DecoderConfig::new().flush_after_decode(Flush::NoFlush);
        openh264::decoder::Decoder::with_api_config(OpenH264API::from_source(), config)
            .map(Self::Software)
            .map_err(|e| Error::unavailable(format!("openh264 VOD decoder could not start: {e}")))
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
                Ok(Some(yuv_to_picture(decoded)?))
            }
        }
    }
}

fn yuv_to_picture(decoded: openh264::decoder::DecodedYUV<'_>) -> Result<Picture> {
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
        .ok_or_else(|| Error::failed("decoded planes are shorter than their strides say"))?;
    Ok(Picture {
        width: width32,
        height: height32,
        layout: Layout::I420,
        data,
    })
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
    fn vod_reorder_buffer_emits_monotonic_pts_across_b_frame_patterns() {
        let mut reorder = VodReorder::new(VOD_MAX_REORDER);
        let blank = Picture {
            width: 2,
            height: 2,
            layout: Layout::Rgba,
            data: vec![0; 16],
        };
        let dts_to_pts = [
            0, 1_000_000, 333_333, 666_666, 2_000_000, 1_333_333, 1_666_666,
        ];
        let mut out = Vec::new();
        for &pts in &dts_to_pts {
            reorder.push_ready(pts, blank.clone());
            reorder.drain_excess(&mut out);
        }
        assert!(out.is_empty());
        reorder.drain_all(&mut out);
        let got: Vec<i64> = out.into_iter().map(|(pts, _)| pts).collect();
        assert_eq!(
            got,
            vec![
                0, 333_333, 666_666, 1_000_000, 1_333_333, 1_666_666, 2_000_000
            ]
        );
    }

    #[test]
    fn vod_software_decodes_flushes_and_seeks_cleanly() {
        let (width, height) = (128usize, 96usize);
        let planes = yuv(&test_card(width, height), width, height);
        let mut encoder = encoder(width as u32, height as u32);
        let mut encoded = Vec::new();
        for i in 0..8 {
            if let Some(frame) = encoder.encode(&planes, Chroma::Planar, i % 4 == 0).unwrap() {
                encoded.push((i as i64 * 333_333, frame));
            }
        }
        assert!(encoded.len() >= 4);

        let mut decoder = Decoder::software_vod().unwrap();
        let mut frames = Vec::new();
        for (pts, au) in &encoded {
            frames.extend(decoder.decode_vod(&au.data, *pts).unwrap());
        }
        frames.extend(decoder.finish_vod().unwrap());
        assert_eq!(frames.len(), encoded.len());
        for pair in frames.windows(2) {
            assert!(pair[0].0 < pair[1].0);
        }

        // Flush simulates a seek: delta frames wait for the seek keyframe, then decode resumes.
        decoder.flush_vod().unwrap();
        assert!(decoder.needs_keyframe());
        assert_eq!(
            decoder.decode_vod(&encoded[1].1.data, 0).unwrap_err(),
            DecodeError::NeedKeyframe
        );
        let mut after_seek = decoder.decode_vod(&encoded[0].1.data, 10_000_000).unwrap();
        after_seek.extend(decoder.finish_vod().unwrap());
        assert_eq!(after_seek.len(), 1);
        assert_eq!(after_seek[0].0, 10_000_000);
    }

    #[test]
    fn vod_allows_access_units_larger_than_live_two_mib_cap() {
        let mut live = Decoder::software().unwrap();
        let mut vod = Decoder::software_vod().unwrap();
        let (width, height) = (64usize, 64usize);
        let planes = yuv(&test_card(width, height), width, height);
        let mut encoder = encoder(width as u32, height as u32);
        let key = encoder
            .encode(&planes, Chroma::Planar, true)
            .unwrap()
            .unwrap();
        let mut padded = key.data.clone();
        // SEI filler NAL so total AU size exceeds 2 MiB MAX_ACCESS_UNIT.
        padded.extend_from_slice(&[0, 0, 0, 1, 0x0C]);
        padded.resize(h264::MAX_ACCESS_UNIT + 256, 0xFF);
        assert_eq!(live.decode(&padded).unwrap_err(), DecodeError::Invalid);
        let mut frames = vod.decode_vod(&padded, 0).unwrap();
        frames.extend(vod.finish_vod().unwrap());
        assert_eq!(frames.len(), 1);
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
