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

use crate::storage;
use dc_proto::pin;
use openh264::formats::YUVSource;
use std::path::PathBuf;

/// The dark theme's H.264 source of `AppSpinner`.
const CLIP: &str = "7c5b005481eed9fc.mp4";
/// White in limited range video, the top of the luma scale.
const LUMA_WHITE: f32 = 235.0;
/// Alpha below this is compression noise on the matte.
const ALPHA_FLOOR: f32 = 0.03;
/// The clip's frame rate; the official page plays it at its own 60 fps.
pub const FPS: f32 = 60.0;

/// Frames of the loop, each `width * height` pixels of straight BGRA, as gpui's atlas takes them.
pub struct Clip {
    pub width: u32,
    pub height: u32,
    pub frames: Vec<Vec<u8>>,
}

// ---- the file ----

fn cache_path() -> Option<PathBuf> {
    let dir = storage::data_dir()?.join("spinner");
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir.join(CLIP))
}

async fn file(client: &dc_http::Client) -> Option<Vec<u8>> {
    let cached = cache_path()?;
    if let Ok(bytes) = std::fs::read(&cached)
        && !bytes.is_empty()
    {
        return Some(bytes);
    }
    let url = format!("{}/assets/{CLIP}", pin::WEBAPP_ORIGIN);
    let bytes = client.fetch_public(&url).await.ok()?;
    let _ = std::fs::write(&cached, &bytes);
    Some(bytes)
}

/// The spinner, fetched or read from disk and decoded. None when the file or the decode is not to be had: the screen
/// shows without the mark then, as the official page does while its video loads.
pub async fn load(client: &dc_http::Client) -> Option<Clip> {
    let bytes = file(client).await?;
    tokio::task::spawn_blocking(move || {
        decode(&bytes)
            .map_err(|error| tracing::debug!("loading spinner: {error}"))
            .ok()
    })
    .await
    .ok()
    .flatten()
}

// ---- MP4 ----

/// One box: its four-character type and its payload.
struct BoxRef<'a> {
    kind: [u8; 4],
    payload: &'a [u8],
}

fn be32(bytes: &[u8]) -> Option<u32> {
    Some(u32::from_be_bytes(bytes.get(..4)?.try_into().ok()?))
}

fn be64(bytes: &[u8]) -> Option<u64> {
    Some(u64::from_be_bytes(bytes.get(..8)?.try_into().ok()?))
}

/// The boxes laid one after another in `data`. A box that claims more than is there ends the walk.
fn boxes(mut data: &[u8]) -> Vec<BoxRef<'_>> {
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

fn child<'a>(data: &'a [u8], kind: &[u8; 4]) -> Option<&'a [u8]> {
    boxes(data)
        .into_iter()
        .find(|entry| &entry.kind == kind)
        .map(|entry| entry.payload)
}

fn path<'a>(mut data: &'a [u8], kinds: &[&[u8; 4]]) -> Option<&'a [u8]> {
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

/// The existing MP4 reader, shared by the spinner and silent inline H.264 clips.
pub(crate) struct H264Track<'a> {
    file: &'a [u8],
    mdhd: &'a [u8],
    stbl: &'a [u8],
    config: Avc,
    located: Vec<(usize, usize)>,
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
        Ok(Self {
            file,
            mdhd,
            stbl,
            config,
            located,
        })
    }

    pub fn len(&self) -> usize {
        self.located.len()
    }

    pub fn sample(&self, index: usize) -> Result<Vec<u8>, String> {
        let (offset, length) = self.located.get(index).ok_or("sample not found")?;
        let end = offset.checked_add(*length).ok_or("sample overflow")?;
        let data = self.file.get(*offset..end).ok_or("sample outside file")?;
        let mut unit = Vec::new();
        if index == 0 {
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
        let offset = match self.mdhd.first()? {
            0 => 12,
            1 => 20,
            _ => return None,
        };
        let scale = be32(self.mdhd.get(offset..)?)?;
        timing(
            child(self.stbl, b"stts")?,
            child(self.stbl, b"ctts"),
            scale,
            self.len(),
        )
    }
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
    let mut points = Vec::with_capacity(count);
    let mut timestamp = 0i64;
    for index in 0..be32(stts.get(4..)?)? as usize {
        let row = stts.get(8 + index * 8..)?;
        let samples = be32(row)? as usize;
        let delta = be32(row.get(4..)?)?;
        if delta == 0 || samples > count - points.len() {
            return None;
        }
        for _ in 0..samples {
            points.push((timestamp, delta));
            timestamp = timestamp.checked_add(i64::from(delta))?;
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
            let end = at.checked_add(samples)?;
            for point in points.get_mut(at..end)? {
                point.0 = point.0.checked_add(offset)?;
            }
            at = end;
        }
        if at != count {
            return None;
        }
    }
    points.sort_by_key(|(timestamp, _)| *timestamp);
    points
        .iter()
        .enumerate()
        .map(|(index, (timestamp, tail))| {
            let delta = points
                .get(index + 1)
                .map_or(i64::from(*tail), |next| next.0 - timestamp);
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

/// Straight BGRA of a white mark from one luma plane.
fn frame(plane: &[u8], stride: usize, width: usize, height: usize) -> Vec<u8> {
    let matte = plane.first().copied().unwrap_or(0);
    let mut out = Vec::with_capacity(width * height * 4);
    for row in 0..height {
        for luma in &plane[row * stride..row * stride + width] {
            out.extend_from_slice(&[255, 255, 255, alpha(*luma, matte)]);
        }
    }
    out
}

/// The loop of an H.264 MP4 with one video track, as frames of a white mark.
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
            clip.frames.iter().all(|frame| frame
                .as_chunks::<4>()
                .0
                .iter()
                .any(|pixel| pixel[3] > 128))
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
        assert_eq!(out.len(), 16);
        assert_eq!(&out[..8], &[255, 255, 255, 0, 255, 255, 255, 255]);
        assert_eq!(out[7], 255);
        assert_eq!(out[11], alpha(40, 40));
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
}
