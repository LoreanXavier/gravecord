//! The connecting screen's spinner: the pinned build's `AppSpinner` (module 545342) plays `7ba7fcf2c4710bb7.webm` on a
//! dark theme, a 400 x 400 loop of 64 frames at 60 fps, a white mark with an alpha channel (VP9 with alpha).
//!
//! No VP9 decoder is in the tree, so this reads the same clip's H.264 sibling, `7c5b005481eed9fc.mp4` (the build lists
//! it as the second source: the same 64 frames, the mark on a flat navy matte), and recovers the alpha from the luma:
//! the matte is the picture's first pixel, a pixel's alpha is how far its luma climbs from the matte towards white. The
//! mark is white, so the frame is white with that alpha. The substitute is listed in `BLOCKED.md`.
//!
//! The file is not bundled (DECISIONS.md): it is fetched once from the build's CDN, kept under the run's data folder and
//! decoded when the connecting screen needs it.

#[cfg(feature = "capture")]
use crate::storage;
#[cfg(feature = "capture")]
use dc_proto::pin;
#[cfg(any(feature = "capture", test))]
use openh264::formats::YUVSource;
#[cfg(feature = "capture")]
use std::path::PathBuf;

/// The dark theme's H.264 source of `AppSpinner`.
#[cfg(feature = "capture")]
const CLIP: &str = "7c5b005481eed9fc.mp4";
/// White in limited range video, the top of the luma scale.
#[cfg(any(feature = "capture", test))]
const LUMA_WHITE: f32 = 235.0;
/// Alpha below this is compression noise on the matte.
#[cfg(any(feature = "capture", test))]
const ALPHA_FLOOR: f32 = 0.03;
/// The clip's frame rate; the official page plays it at its own 60 fps.
pub const FPS: f32 = 60.0;

/// Frames of the loop, each `width * height` bytes of alpha. The mark is white, so alpha is all a frame holds; a
/// quarter of the size of the BGRA picture that [`Clip::bgra`] makes of one when it is drawn.
pub struct Clip {
    pub width: u32,
    pub height: u32,
    pub frames: Vec<Vec<u8>>,
}

impl Clip {
    /// Frame `index` as straight BGRA, as gpui's atlas takes it.
    pub fn bgra(&self, index: usize) -> Vec<u8> {
        self.frames[index]
            .iter()
            .flat_map(|alpha| [255, 255, 255, *alpha])
            .collect()
    }
}

// ---- the file ----

#[cfg(feature = "capture")]
fn cache_path() -> Option<PathBuf> {
    let dir = storage::data_dir()?.join("spinner");
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir.join(CLIP))
}

/// Writes through a temporary file in the same folder and renames it over `path`, so a crash or a full disk leaves
/// the old file or none, never half of one.
#[cfg(any(feature = "capture", test))]
fn write_atomic(path: &std::path::Path, bytes: &[u8]) -> std::io::Result<()> {
    let temp = path.with_extension("part");
    let written = std::fs::write(&temp, bytes).and_then(|()| std::fs::rename(&temp, path));
    if written.is_err() {
        let _ = std::fs::remove_file(&temp);
    }
    written
}

/// What the decode says about the cache: a download that decoded is kept, a cached file that did not is deleted.
#[cfg(any(feature = "capture", test))]
fn settle_cache(path: &std::path::Path, decoded: bool, cached: bool, bytes: &[u8]) {
    match (decoded, cached) {
        (true, false) => {
            if let Err(error) = write_atomic(path, bytes) {
                tracing::debug!("keeping spinner: {error}");
            }
        }
        (false, true) => {
            let _ = std::fs::remove_file(path);
        }
        _ => {}
    }
}

/// The clip's bytes, and whether they came from the cache. The cache is only trusted once it decoded: see `load`.
#[cfg(feature = "capture")]
async fn file(client: &dc_http::Client) -> Option<(Vec<u8>, bool)> {
    let cached = cache_path()?;
    if let Ok(bytes) = std::fs::read(&cached)
        && !bytes.is_empty()
    {
        return Some((bytes, true));
    }
    let url = format!("{}/assets/{CLIP}", pin::WEBAPP_ORIGIN);
    let bytes = client.fetch_public(&url).await.ok()?;
    Some((bytes, false))
}

/// Synchronously decodes the cached spinner clip from disk, if present and valid.
#[cfg(feature = "capture")]
pub fn load_cached() -> Option<Clip> {
    let cached = storage::data_dir()?.join("spinner").join(CLIP);
    let bytes = std::fs::read(&cached).ok()?;
    if bytes.is_empty() {
        return None;
    }
    let decoded = decode(&bytes);
    settle_cache(&cached, decoded.is_ok(), true, &bytes);
    decoded
        .map_err(|error| tracing::debug!("loading cached spinner: {error}"))
        .ok()
}

/// The spinner, fetched or read from disk and decoded. None when the file or the decode is not to be had: the screen
/// shows without the mark then, as the official page does while its video loads. A download is kept on disk only
/// once it decoded; a cached file that no longer decodes is deleted so the next start downloads it again.
#[cfg(feature = "capture")]
pub async fn load(client: &dc_http::Client) -> Option<Clip> {
    let (bytes, cached) = file(client).await?;
    tokio::task::spawn_blocking(move || {
        let decoded = decode(&bytes);
        if let Some(path) = cache_path() {
            settle_cache(&path, decoded.is_ok(), cached, &bytes);
        }
        decoded
            .map_err(|error| tracing::debug!("loading spinner: {error}"))
            .ok()
    })
    .await
    .ok()
    .flatten()
}

// ---- MP4 ----

/// One box: its four-character type and its payload.
pub(crate) struct BoxRef<'a> {
    pub(crate) kind: [u8; 4],
    pub(crate) payload: &'a [u8],
}

pub(crate) fn be32(bytes: &[u8]) -> Option<u32> {
    Some(u32::from_be_bytes(bytes.get(..4)?.try_into().ok()?))
}

pub(crate) fn be64(bytes: &[u8]) -> Option<u64> {
    Some(u64::from_be_bytes(bytes.get(..8)?.try_into().ok()?))
}

/// The boxes laid one after another in `data`. A box that claims more than is there ends the walk.
pub(crate) fn boxes(mut data: &[u8]) -> Vec<BoxRef<'_>> {
    let mut out = Vec::new();
    while data.len() >= 8 {
        let size = be32(data).unwrap_or(0) as usize;
        let kind: [u8; 4] = data[4..8].try_into().unwrap_or_default();
        let (head, total) = match size {
            0 => (8, data.len()),
            1 => match data.get(8..).and_then(be64) {
                Some(large) => (16, large as usize),
                None => break,
            },
            _ => (8, size),
        };
        if total < head || total > data.len() {
            break;
        }
        out.push(BoxRef {
            kind,
            payload: &data[head..total],
        });
        data = &data[total..];
    }
    out
}

pub(crate) fn child<'a>(data: &'a [u8], kind: &[u8; 4]) -> Option<&'a [u8]> {
    boxes(data)
        .into_iter()
        .find(|entry| &entry.kind == kind)
        .map(|entry| entry.payload)
}

pub(crate) fn path<'a>(mut data: &'a [u8], kinds: &[&[u8; 4]]) -> Option<&'a [u8]> {
    for kind in kinds {
        data = child(data, kind)?;
    }
    Some(data)
}

/// What the decoder needs besides the samples: the parameter sets and the length prefix size.
struct Avc {
    sps: Vec<Vec<u8>>,
    pps: Vec<Vec<u8>>,
    length_size: usize,
}

fn avcc(payload: &[u8]) -> Option<Avc> {
    let length_size = usize::from(*payload.get(4)? & 3) + 1;
    let mut at = 6;
    let sets = |count: usize, at: &mut usize| -> Option<Vec<Vec<u8>>> {
        let mut out = Vec::new();
        for _ in 0..count {
            let len = usize::from(u16::from_be_bytes(
                payload.get(*at..*at + 2)?.try_into().ok()?,
            ));
            out.push(payload.get(*at + 2..*at + 2 + len)?.to_vec());
            *at += 2 + len;
        }
        Some(out)
    };
    let sps = sets(usize::from(*payload.get(5)? & 31), &mut at)?;
    let pps_count = usize::from(*payload.get(at)?);
    at += 1;
    let pps = sets(pps_count, &mut at)?;
    Some(Avc {
        sps,
        pps,
        length_size,
    })
}

/// Where each sample of a track lies in the file: `(offset, size)` in decoding order.
fn samples(stbl: &[u8], file_len: usize) -> Option<Vec<(usize, usize)>> {
    let stsz = child(stbl, b"stsz")?;
    let fixed = be32(stsz.get(4..)?)? as usize;
    let count = be32(stsz.get(8..)?)? as usize;
    if count > file_len || count > 100_000 {
        return None;
    }
    let sizes: Vec<usize> = (0..count)
        .map(|index| {
            if fixed != 0 {
                Some(fixed)
            } else {
                be32(stsz.get(12 + index * 4..)?).map(|size| size as usize)
            }
        })
        .collect::<Option<_>>()?;
    let offsets: Vec<usize> = if let Some(stco) = child(stbl, b"stco") {
        let n = be32(stco.get(4..)?)? as usize;
        (0..n)
            .map(|i| be32(stco.get(8 + i * 4..)?).map(|v| v as usize))
            .collect::<Option<_>>()?
    } else {
        let co64 = child(stbl, b"co64")?;
        let n = be32(co64.get(4..)?)? as usize;
        (0..n)
            .map(|i| be64(co64.get(8 + i * 8..)?).map(|v| v as usize))
            .collect::<Option<_>>()?
    };
    let stsc = child(stbl, b"stsc")?;
    let runs = be32(stsc.get(4..)?)? as usize;
    let table: Vec<(usize, usize)> = (0..runs)
        .map(|i| {
            let first = be32(stsc.get(8 + i * 12..)?)? as usize;
            let per = be32(stsc.get(12 + i * 12..)?)? as usize;
            Some((first, per))
        })
        .collect::<Option<_>>()?;
    let mut out = Vec::with_capacity(sizes.len());
    let mut sample = 0;
    for (chunk, offset) in offsets.iter().enumerate() {
        let per = table
            .iter()
            .rev()
            .find(|(first, _)| *first <= chunk + 1)
            .map(|(_, per)| *per)?;
        let mut at = *offset;
        for _ in 0..per {
            let Some(size) = sizes.get(sample) else {
                return Some(out);
            };
            out.push((at, *size));
            at = at.checked_add(*size)?;
            sample += 1;
        }
    }
    Some(out)
}

/// Length prefixed NAL units to Annex B start codes.
fn annex_b(sample: &[u8], length_size: usize, out: &mut Vec<u8>) -> Option<()> {
    let mut at = 0;
    while at < sample.len() {
        let len = sample
            .get(at..at + length_size)?
            .iter()
            .fold(0usize, |acc, byte| (acc << 8) | usize::from(*byte));
        at += length_size;
        out.extend_from_slice(&[0, 0, 0, 1]);
        out.extend_from_slice(sample.get(at..at + len)?);
        at += len;
    }
    Some(())
}

/// Per-sample timing and sync point info for VOD MP4 playback (timestamps in `100 ns` units, display-zeroed).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct VodSample {
    pub index: usize,
    pub dts_100ns: i64,
    pub pts_100ns: i64,
    pub duration_100ns: i64,
    pub is_sync: bool,
}

/// The existing MP4 reader, shared by the spinner and silent inline H.264 clips.
pub(crate) struct H264Track<'a> {
    file: &'a [u8],
    mdhd: &'a [u8],
    stbl: &'a [u8],
    config: Avc,
    located: Vec<(usize, usize)>,
    sync: Vec<bool>,
}

impl<'a> H264Track<'a> {
    pub fn read(file: &'a [u8]) -> Result<Self, String> {
        let moov = child(file, b"moov").ok_or("no moov box")?;
        let (mdhd, stbl, config) = boxes(moov)
            .into_iter()
            .filter(|entry| &entry.kind == b"trak")
            .find_map(|trak| {
                let mdia = child(trak.payload, b"mdia")?;
                let mdhd = child(mdia, b"mdhd")?;
                let stbl = path(mdia, &[b"minf", b"stbl"])?;
                let stsd = child(stbl, b"stsd")?;
                let entry = boxes(stsd.get(8..)?)
                    .into_iter()
                    .find(|entry| &entry.kind == b"avc1" || &entry.kind == b"avc3")?;
                let config = avcc(child(entry.payload.get(78..)?, b"avcC")?)?;
                Some((mdhd, stbl, config))
            })
            .ok_or("no H.264 track")?;
        let located = samples(stbl, file.len()).ok_or("unreadable sample table")?;
        let sync = parse_stss(child(stbl, b"stss"), located.len());
        Ok(Self {
            file,
            mdhd,
            stbl,
            config,
            located,
            sync,
        })
    }

    pub fn len(&self) -> usize {
        self.located.len()
    }

    pub fn is_sync(&self, index: usize) -> bool {
        self.sync.get(index).copied().unwrap_or(false)
    }

    /// Picture dimensions from the first SPS in `avcC`, when present and valid.
    pub fn dimensions(&self) -> Option<(u32, u32)> {
        self.config
            .sps
            .first()
            .and_then(|sps| dc_clips::sps::parse(sps))
            .map(|info| (info.width, info.height))
    }

    pub fn sample(&self, index: usize) -> Result<Vec<u8>, String> {
        self.sample_with_headers(index, index == 0)
    }

    /// VOD sample access unit: prepends `avcC` SPS/PPS Annex B parameter sets on every sync sample so seeking
    /// directly to any IDR frame is self-contained.
    pub fn vod_sample(&self, index: usize) -> Result<Vec<u8>, String> {
        self.sample_with_headers(index, index == 0 || self.is_sync(index))
    }

    fn sample_with_headers(&self, index: usize, prepend_headers: bool) -> Result<Vec<u8>, String> {
        let (offset, length) = self.located.get(index).ok_or("sample not found")?;
        let end = offset.checked_add(*length).ok_or("sample overflow")?;
        let data = self.file.get(*offset..end).ok_or("sample outside file")?;
        let mut unit = Vec::new();
        if prepend_headers {
            for set in self.config.sps.iter().chain(&self.config.pps) {
                unit.extend_from_slice(&[0, 0, 0, 1]);
                unit.extend_from_slice(set);
            }
        }
        annex_b(data, self.config.length_size, &mut unit).ok_or("broken sample")?;
        Ok(unit)
    }

    /// Durations in presentation order, including composition offsets for reordered H.264 pictures.
    pub fn durations(&self) -> Option<Vec<std::time::Duration>> {
        let scale = self.timescale()?;
        timing(
            child(self.stbl, b"stts")?,
            child(self.stbl, b"ctts"),
            scale,
            self.len(),
        )
    }

    fn timescale(&self) -> Option<u32> {
        let offset = match self.mdhd.first()? {
            0 => 12,
            1 => 20,
            _ => return None,
        };
        be32(self.mdhd.get(offset..)?)
    }

    /// Per-sample decode/presentation timestamps (`100 ns` units, display-zeroed), total track duration, and computed
    /// B-frame reorder depth (`0` for monotonic I/P streams, `..=16` for reordered streams).
    pub fn vod_timing(&self) -> Result<(Vec<VodSample>, i64, usize), String> {
        let count = self.len();
        if count == 0 {
            return Err("empty video track".to_owned());
        }
        let scale = self
            .timescale()
            .filter(|&s| s > 0)
            .ok_or("invalid timescale")?;
        let stts = child(self.stbl, b"stts").ok_or("missing stts")?;
        let ctts = child(self.stbl, b"ctts");
        let points = sample_points(stts, ctts, count, true)
            .ok_or_else(|| "unreadable stts/ctts".to_owned())?;
        let min_pts = points.iter().map(|p| p.pts).min().unwrap_or(0);
        let to_100ns = |ticks: i128| -> i64 {
            ((ticks * 10_000_000) / i128::from(scale))
                .clamp(i128::from(i64::MIN), i128::from(i64::MAX)) as i64
        };
        let mut samples_out = Vec::with_capacity(count);
        let mut max_end_100ns = 0i64;
        for (index, point) in points.iter().enumerate() {
            let pts_100ns = to_100ns(i128::from(point.pts) - i128::from(min_pts));
            let dts_100ns = to_100ns(i128::from(point.dts));
            let duration_100ns = to_100ns(i128::from(point.delta)).max(1);
            max_end_100ns = max_end_100ns.max(pts_100ns.saturating_add(duration_100ns));
            samples_out.push(VodSample {
                index,
                dts_100ns,
                pts_100ns,
                duration_100ns,
                is_sync: self.is_sync(index),
            });
        }
        let mut display_order: Vec<usize> = (0..count).collect();
        display_order.sort_by_key(|&idx| (samples_out[idx].pts_100ns, idx));
        let mut reorder_depth = 0usize;
        for (display_idx, &decode_idx) in display_order.iter().enumerate() {
            if display_idx != decode_idx {
                reorder_depth = reorder_depth.max(display_idx.abs_diff(decode_idx));
            }
        }
        Ok((samples_out, max_end_100ns.max(1), reorder_depth.min(16)))
    }

    /// Returns the decode index of the nearest `stss` sync sample whose PTS is `<= target_100ns` (or the earliest sync
    /// sample / `0` before the first keyframe).
    pub fn keyframe_before(&self, samples: &[VodSample], target_100ns: i64) -> usize {
        let mut candidate = None;
        for sample in samples {
            if !sample.is_sync {
                continue;
            }
            if sample.pts_100ns <= target_100ns || candidate.is_none() {
                candidate = Some(sample.index);
            }
        }
        candidate.unwrap_or(0)
    }
}

fn parse_stss(stss: Option<&[u8]>, count: usize) -> Vec<bool> {
    let Some(stss) = stss else {
        return vec![true; count];
    };
    let Some(entries) = stss.get(4..8).and_then(be32).map(|n| n as usize) else {
        return vec![true; count];
    };
    let mut sync = vec![false; count];
    let available = stss.len().saturating_sub(8) / 4;
    for index in 0..entries.min(available) {
        if let Some(num) = be32(stss.get(8 + index * 4..).unwrap_or(&[]))
            && let Some(slot) = (num as usize).checked_sub(1).and_then(|i| sync.get_mut(i))
        {
            *slot = true;
        }
    }
    if !sync.iter().any(|&flag| flag) && !sync.is_empty() {
        sync[0] = true;
    }
    sync
}

#[derive(Clone, Copy)]
struct SamplePoint {
    dts: i64,
    pts: i64,
    delta: u32,
}

fn sample_points(
    stts: &[u8],
    ctts: Option<&[u8]>,
    count: usize,
    tolerant: bool,
) -> Option<Vec<SamplePoint>> {
    if count == 0 {
        return None;
    }
    let mut points = Vec::with_capacity(count);
    let mut timestamp = 0i64;
    for index in 0..be32(stts.get(4..)?)? as usize {
        let row = stts.get(8 + index * 8..)?;
        let samples = be32(row)? as usize;
        let delta = be32(row.get(4..)?)?;
        if (!tolerant && delta == 0) || samples > count - points.len() {
            return None;
        }
        let step = if tolerant { delta.max(1) } else { delta };
        for _ in 0..samples {
            points.push(SamplePoint {
                dts: timestamp,
                pts: timestamp,
                delta: step,
            });
            timestamp = timestamp.checked_add(i64::from(step))?;
        }
    }
    if points.len() != count {
        return None;
    }
    if let Some(ctts) = ctts {
        let version = *ctts.first()?;
        if version > 1 {
            return None;
        }
        let mut at = 0usize;
        for index in 0..be32(ctts.get(4..)?)? as usize {
            let row = ctts.get(8 + index * 8..)?;
            let samples = be32(row)? as usize;
            let value = be32(row.get(4..)?)?;
            let offset = if version == 1 {
                i64::from(value as i32)
            } else {
                i64::from(value)
            };
            let end = if tolerant {
                at.checked_add(samples)?.min(count)
            } else {
                at.checked_add(samples)?
            };
            for point in points.get_mut(at..end)? {
                point.pts = point.pts.checked_add(offset)?;
            }
            at = end;
            if tolerant && at >= count {
                break;
            }
        }
        if !tolerant && at != count {
            return None;
        }
    }
    Some(points)
}

fn timing(
    stts: &[u8],
    ctts: Option<&[u8]>,
    scale: u32,
    count: usize,
) -> Option<Vec<std::time::Duration>> {
    if scale == 0 || count == 0 {
        return None;
    }
    let mut points = sample_points(stts, ctts, count, false)?;
    points.sort_by_key(|point| point.pts);
    points
        .iter()
        .enumerate()
        .map(|(index, point)| {
            let delta = points
                .get(index + 1)
                .map_or(i64::from(point.delta), |next| next.pts - point.pts);
            if delta <= 0 {
                return None;
            }
            Some(std::time::Duration::from_secs_f64(
                delta as f64 / f64::from(scale),
            ))
        })
        .collect()
}

/// A pixel's alpha from its luma and the matte's.
#[cfg(any(feature = "capture", test))]
fn alpha(luma: u8, matte: u8) -> u8 {
    let matte = f32::from(matte);
    let span = (LUMA_WHITE - matte).max(1.0);
    let value = ((f32::from(luma) - matte) / span).clamp(0.0, 1.0);
    if value < ALPHA_FLOOR {
        0
    } else {
        (value * 255.0).round() as u8
    }
}

/// The alpha of a white mark from one luma plane.
#[cfg(any(feature = "capture", test))]
fn frame(plane: &[u8], stride: usize, width: usize, height: usize) -> Vec<u8> {
    let matte = plane.first().copied().unwrap_or(0);
    let mut out = Vec::with_capacity(width * height);
    for row in 0..height {
        for luma in &plane[row * stride..row * stride + width] {
            out.push(alpha(*luma, matte));
        }
    }
    out
}

/// The loop of an H.264 MP4 with one video track, as frames of a white mark.
#[cfg(any(feature = "capture", test))]
pub fn decode(mp4: &[u8]) -> Result<Clip, String> {
    let track = H264Track::read(mp4)?;
    let mut decoder = openh264::decoder::Decoder::new().map_err(|error| error.to_string())?;
    let mut frames = Vec::new();
    let mut size = (0, 0);
    let mut keep = |yuv: &openh264::decoder::DecodedYUV<'_>| {
        let (width, height) = yuv.dimensions();
        size = (width as u32, height as u32);
        frames.push(frame(yuv.y(), yuv.strides().0, width, height));
    };
    for index in 0..track.len() {
        let unit = track.sample(index)?;
        // This file contains B-frames. Flushing between samples drains pictures that later samples reference.
        let options = openh264::decoder::DecodeOptions::new()
            .flush_after_decode(openh264::decoder::Flush::NoFlush);
        if let Some(yuv) = decoder
            .decode_with_options(&unit, options)
            .map_err(|error| format!("sample {index}: {error}"))?
        {
            keep(&yuv);
        }
    }
    for yuv in decoder
        .flush_remaining()
        .map_err(|error| error.to_string())?
    {
        keep(&yuv);
    }
    if frames.is_empty() {
        return Err("the clip decoded to no frames".to_owned());
    }
    Ok(Clip {
        width: size.0,
        height: size.1,
        frames,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sample_timing_follows_presentation_order_and_rejects_incomplete_tables() {
        let table = |version: u8, rows: &[(u32, u32)]| {
            let mut out = vec![version, 0, 0, 0];
            out.extend_from_slice(&(rows.len() as u32).to_be_bytes());
            for (count, value) in rows {
                out.extend_from_slice(&count.to_be_bytes());
                out.extend_from_slice(&value.to_be_bytes());
            }
            out
        };
        let stts = table(0, &[(3, 10)]);
        let ctts = table(1, &[(1, 0), (1, 10), (1, (-10i32) as u32)]);
        assert_eq!(
            timing(&stts, Some(&ctts), 100, 3).unwrap(),
            vec![std::time::Duration::from_millis(100); 3]
        );
        assert!(timing(&stts, None, 0, 3).is_none());
        assert!(timing(&stts, None, 100, 4).is_none());
        assert!(timing(&stts, Some(&table(0, &[(2, 0)])), 100, 3).is_none());
    }

    #[test]
    #[ignore = "requires the pinned clip path in GRAVECORD_SPINNER_TEST_FILE"]
    fn pinned_clip_has_visible_frames() {
        let file = std::env::var("GRAVECORD_SPINNER_TEST_FILE").unwrap();
        let clip = decode(&std::fs::read(file).unwrap()).unwrap();
        assert_eq!((clip.width, clip.height), (400, 400));
        assert_eq!(clip.frames.len(), 64);
        assert!(
            clip.frames
                .iter()
                .all(|frame| frame.iter().any(|alpha| *alpha > 128))
        );
    }

    fn make_box(kind: &[u8; 4], payload: &[u8]) -> Vec<u8> {
        let mut out = ((payload.len() + 8) as u32).to_be_bytes().to_vec();
        out.extend_from_slice(kind);
        out.extend_from_slice(payload);
        out
    }

    #[test]
    fn alpha_climbs_from_the_matte_to_white() {
        assert_eq!(alpha(40, 40), 0);
        assert_eq!(alpha(235, 40), 255);
        assert_eq!(alpha(42, 40), 0, "noise on the matte is not mark");
        let middle = alpha(137, 40);
        assert!((120..=135).contains(&middle), "{middle}");
        assert_eq!(alpha(250, 40), 255, "overshoot stays opaque");
    }

    #[test]
    fn frame_is_white_with_the_recovered_alpha() {
        // A 2 x 2 picture in a stride of 3: the matte, white, and two midtones.
        let plane = [40, 235, 99, 40, 137, 99];
        let out = frame(&plane, 3, 2, 2);
        assert_eq!(out.len(), 4);
        assert_eq!(&out[..2], &[0, 255]);
        assert_eq!(out[2], alpha(40, 40));
        assert_eq!(out[3], alpha(137, 40));
    }

    #[test]
    fn a_frame_is_drawn_as_white_with_its_alpha() {
        let clip = Clip {
            width: 2,
            height: 1,
            frames: vec![vec![0, 255], vec![9, 128]],
        };
        assert_eq!(clip.bgra(0), [255, 255, 255, 0, 255, 255, 255, 255]);
        assert_eq!(clip.bgra(1), [255, 255, 255, 9, 255, 255, 255, 128]);
    }

    #[test]
    fn length_prefixed_samples_become_start_codes() {
        let sample = [0, 0, 0, 2, 0x65, 0x88, 0, 0, 0, 1, 0x06];
        let mut out = Vec::new();
        annex_b(&sample, 4, &mut out).unwrap();
        assert_eq!(out, [0, 0, 0, 1, 0x65, 0x88, 0, 0, 0, 1, 0x06]);
        assert!(annex_b(&[0, 0, 0, 9, 1], 4, &mut Vec::new()).is_none());
    }

    #[test]
    fn boxes_nest_and_a_lying_size_ends_the_walk() {
        let inner = make_box(b"stbl", &[1, 2, 3]);
        let outer = make_box(b"minf", &inner);
        assert_eq!(path(&outer, &[b"stbl"]), None);
        let wrapped = make_box(b"moov", &make_box(b"mdia", &outer));
        assert_eq!(
            path(&wrapped, &[b"moov", b"mdia", b"minf", b"stbl"]),
            Some(&[1u8, 2, 3][..])
        );
        let mut lying = make_box(b"free", &[0; 4]);
        lying.extend_from_slice(&[0, 0, 1, 0, b'm', b'o', b'o', b'v', 1]);
        assert_eq!(boxes(&lying).len(), 1);
    }

    #[test]
    fn sample_table_follows_chunks_and_runs() {
        // Three samples of 4, 5 and 6 bytes: chunk 1 holds two (at 100), chunk 2 one (at 200).
        let mut stsz = vec![0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 3];
        for size in [4u32, 5, 6] {
            stsz.extend_from_slice(&size.to_be_bytes());
        }
        let mut stco = vec![0, 0, 0, 0, 0, 0, 0, 2];
        stco.extend_from_slice(&100u32.to_be_bytes());
        stco.extend_from_slice(&200u32.to_be_bytes());
        let mut stsc = vec![0, 0, 0, 0, 0, 0, 0, 2];
        for (first, per) in [(1u32, 2u32), (2, 1)] {
            stsc.extend_from_slice(&first.to_be_bytes());
            stsc.extend_from_slice(&per.to_be_bytes());
            stsc.extend_from_slice(&1u32.to_be_bytes());
        }
        let mut stbl = make_box(b"stsz", &stsz);
        stbl.extend(make_box(b"stco", &stco));
        stbl.extend(make_box(b"stsc", &stsc));
        assert_eq!(
            samples(&stbl, 206),
            Some(vec![(100, 4), (104, 5), (200, 6)])
        );
    }

    #[test]
    fn avcc_reads_parameter_sets() {
        let payload = [
            1, 100, 0, 30, 0xFF, 0xE1, 0, 2, 0x67, 0x64, 1, 0, 3, 0x68, 0xEE, 0x3C,
        ];
        let config = avcc(&payload).unwrap();
        assert_eq!(config.length_size, 4);
        assert_eq!(config.sps, vec![vec![0x67, 0x64]]);
        assert_eq!(config.pps, vec![vec![0x68, 0xEE, 0x3C]]);
    }

    #[test]
    fn vod_metadata_parses_stss_ctts_and_keyframe_lookup() {
        let stss = [
            0, 0, 0, 0, // version + flags
            0, 0, 0, 2, // 2 entries
            0, 0, 0, 1, // sample 0 (1-based)
            0, 0, 0, 5, // sample 4 (1-based)
        ];
        assert_eq!(
            parse_stss(Some(&stss), 6),
            vec![true, false, false, false, true, false]
        );
        assert_eq!(parse_stss(None, 3), vec![true, true, true]);

        // Test B-frame ctts timing computation directly via sample_points.
        let mut stts = vec![0, 0, 0, 0, 0, 0, 0, 1];
        stts.extend_from_slice(&4u32.to_be_bytes());
        stts.extend_from_slice(&3000u32.to_be_bytes()); // 33.3ms at 90kHz
        let mut ctts = vec![1, 0, 0, 0, 0, 0, 0, 4]; // version 1 signed
        for off in [0i32, 6000, -3000, -3000] {
            ctts.extend_from_slice(&1u32.to_be_bytes());
            ctts.extend_from_slice(&(off as u32).to_be_bytes());
        }
        let pts: Vec<i64> = sample_points(&stts, Some(&ctts), 4, true)
            .unwrap()
            .into_iter()
            .map(|p| p.pts)
            .collect();
        // DTS = 0, 3000, 6000, 9000 -> PTS = 0, 9000, 3000, 6000.
        assert_eq!(pts, vec![0, 9000, 3000, 6000]);
    }
}

#[cfg(test)]
mod cache_tests {
    use super::{settle_cache, write_atomic};

    #[test]
    fn a_cache_write_replaces_the_file_whole_and_leaves_no_temporary() {
        let dir = std::env::temp_dir().join(format!("gravecord-spinner-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("clip.mp4");
        write_atomic(&path, b"first").unwrap();
        write_atomic(&path, b"second").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"second");
        assert!(!path.with_extension("part").exists());
        // A target that cannot be written (a folder is in the way) fails and still leaves no temporary.
        let blocked = dir.join("blocked.mp4");
        std::fs::create_dir_all(&blocked).unwrap();
        assert!(write_atomic(&blocked, b"x").is_err());
        assert!(!blocked.with_extension("part").exists());
        // A download that decoded is kept; one that did not is not; a cached file that stopped decoding goes.
        let fresh = dir.join("fresh.mp4");
        settle_cache(&fresh, false, false, b"bad");
        assert!(!fresh.exists());
        settle_cache(&fresh, true, false, b"good");
        assert_eq!(std::fs::read(&fresh).unwrap(), b"good");
        settle_cache(&fresh, true, true, b"other");
        assert_eq!(std::fs::read(&fresh).unwrap(), b"good");
        settle_cache(&fresh, false, true, b"");
        assert!(!fresh.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
