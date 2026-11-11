//! What the clip editor shows of a clip file: its video as access units a decoder takes, and a decoded picture cut
//! down to the size it is drawn at. Playback decodes a clip's whole length, so the colour conversion works on the
//! few pixels that are kept (nearest samples, BT.601 limited range as `dc_media::Picture::to_rgba`) instead of the
//! full picture.
use crate::mp4::{ClipFile, VIDEO_TIMESCALE};
use dc_media::{Layout, Picture};

/// One video sample as the decoder is fed it.
pub struct Unit {
    /// Annex B; the keyframes carry the sequence and picture parameter sets in front.
    pub data: Vec<u8>,
    pub start_ms: u64,
    pub sync: bool,
}

/// The clip's video as access units with their start times, and its length in milliseconds.
pub fn units(file: &ClipFile) -> (Vec<Unit>, u64) {
    let mut out = Vec::with_capacity(file.video.samples.len());
    let mut ticks = 0u64;
    for sample in &file.video.samples {
        let mut data = Vec::with_capacity(sample.data.len() + 64);
        if sample.sync {
            for nal in [&file.video.sps, &file.video.pps] {
                data.extend_from_slice(&[0, 0, 0, 1]);
                data.extend_from_slice(nal);
            }
        }
        let mut rest = sample.data.as_slice();
        while rest.len() >= 4 {
            let length = u32::from_be_bytes([rest[0], rest[1], rest[2], rest[3]]) as usize;
            let Some(nal) = rest.get(4..4 + length) else {
                break;
            };
            data.extend_from_slice(&[0, 0, 0, 1]);
            data.extend_from_slice(nal);
            rest = &rest[4 + length..];
        }
        out.push(Unit {
            data,
            start_ms: ticks * 1000 / u64::from(VIDEO_TIMESCALE),
            sync: sample.sync,
        });
        ticks += u64::from(sample.duration);
    }
    (out, ticks * 1000 / u64::from(VIDEO_TIMESCALE))
}

/// The index of the keyframe a picture at `at_ms` is decoded from: the last one starting at or before it.
pub fn keyframe_for(units: &[Unit], at_ms: u64) -> usize {
    units
        .iter()
        .rposition(|unit| unit.sync && unit.start_ms <= at_ms)
        .or_else(|| units.iter().position(|unit| unit.sync))
        .unwrap_or(0)
}

/// The index of the last unit starting at or before `at_ms`.
pub fn unit_at(units: &[Unit], at_ms: u64) -> usize {
    units
        .iter()
        .rposition(|unit| unit.start_ms <= at_ms)
        .unwrap_or(0)
}

fn clamp8(value: i32) -> u8 {
    value.clamp(0, 255) as u8
}

/// `(width, height, RGBA)` of `picture` scaled down (never up) to fit `max_width` x `max_height`. `None` for a
/// malformed picture.
pub fn scaled_rgba(
    picture: &Picture,
    max_width: u32,
    max_height: u32,
) -> Option<(u32, u32, Vec<u8>)> {
    if picture.width == 0
        || picture.height == 0
        || picture.data.len() != picture.layout.len(picture.width, picture.height)?
    {
        return None;
    }
    let scale = (f64::from(max_width) / f64::from(picture.width))
        .min(f64::from(max_height) / f64::from(picture.height))
        .min(1.0);
    let out_width = ((f64::from(picture.width) * scale) as u32).max(1);
    let out_height = ((f64::from(picture.height) * scale) as u32).max(1);
    let (width, height) = (picture.width as usize, picture.height as usize);
    let (chroma_width, chroma_height) = (width.div_ceil(2), height.div_ceil(2));
    let mut out = vec![255u8; out_width as usize * out_height as usize * 4];
    let columns: Vec<usize> = (0..out_width as usize)
        .map(|x| ((x * width) / out_width as usize).min(width - 1))
        .collect();
    for y in 0..out_height as usize {
        let source_y = ((y * height) / out_height as usize).min(height - 1);
        for (x, source_x) in columns.iter().enumerate() {
            let at = (y * out_width as usize + x) * 4;
            if picture.layout == Layout::Rgba {
                let from = (source_y * width + source_x) * 4;
                out[at..at + 4].copy_from_slice(&picture.data[from..from + 4]);
                continue;
            }
            let luma = i32::from(picture.data[source_y * width + source_x]);
            let (cx, cy) = (source_x / 2, source_y / 2);
            let plane = width * height;
            let (cb, cr) = match picture.layout {
                Layout::Nv12 => {
                    let pair = plane + (cy * chroma_width + cx) * 2;
                    (picture.data[pair], picture.data[pair + 1])
                }
                _ => {
                    let cb = plane + cy * chroma_width + cx;
                    (
                        picture.data[cb],
                        picture.data[cb + chroma_width * chroma_height],
                    )
                }
            };
            // BT.601 limited range in 8.8 fixed point.
            let c = (luma - 16) * 298;
            let d = i32::from(cb) - 128;
            let e = i32::from(cr) - 128;
            out[at] = clamp8((c + 409 * e + 128) >> 8);
            out[at + 1] = clamp8((c - 100 * d - 208 * e + 128) >> 8);
            out[at + 2] = clamp8((c + 516 * d + 128) >> 8);
        }
    }
    Some((out_width, out_height, out))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flat(layout: Layout, width: u32, height: u32, y: u8, cb: u8, cr: u8) -> Picture {
        let luma = (width * height) as usize;
        let chroma = (width.div_ceil(2) * height.div_ceil(2)) as usize;
        let mut data = vec![y; luma];
        match layout {
            Layout::Nv12 => data.extend((0..chroma).flat_map(|_| [cb, cr])),
            _ => {
                data.extend(vec![cb; chroma]);
                data.extend(vec![cr; chroma]);
            }
        }
        Picture {
            width,
            height,
            layout,
            data,
        }
    }

    #[test]
    fn grey_stays_grey_and_black_and_white_are_the_limits() {
        // Limited range: luma 16 is black, 235 is white, chroma 128 is none.
        let black = scaled_rgba(&flat(Layout::I420, 8, 4, 16, 128, 128), 8, 4).unwrap();
        assert_eq!(&black.2[..4], &[0, 0, 0, 255]);
        let white = scaled_rgba(&flat(Layout::Nv12, 8, 4, 235, 128, 128), 8, 4).unwrap();
        assert_eq!(&white.2[..4], &[255, 255, 255, 255]);
    }

    #[test]
    fn it_agrees_with_the_full_conversion_within_rounding() {
        // A saturated red-ish picture: the fast path is within a few levels of `Picture::to_rgba`.
        let picture = flat(Layout::I420, 16, 8, 81, 90, 240);
        let full = picture.to_rgba().unwrap();
        let fast = scaled_rgba(&picture, 16, 8).unwrap().2;
        for (a, b) in full.chunks(4).zip(fast.chunks(4)) {
            for channel in 0..3 {
                assert!(a[channel].abs_diff(b[channel]) <= 3, "{a:?} {b:?}");
            }
        }
    }

    #[test]
    fn a_big_picture_is_scaled_down_to_fit_and_a_small_one_is_kept() {
        let big = scaled_rgba(&flat(Layout::I420, 1920, 1080, 100, 128, 128), 960, 960).unwrap();
        assert_eq!((big.0, big.1), (960, 540));
        assert_eq!(big.2.len(), 960 * 540 * 4);
        let small = scaled_rgba(&flat(Layout::I420, 64, 36, 100, 128, 128), 960, 540).unwrap();
        assert_eq!((small.0, small.1), (64, 36));
    }

    #[test]
    fn a_malformed_picture_gives_nothing() {
        let mut picture = flat(Layout::I420, 8, 4, 16, 128, 128);
        picture.data.pop();
        assert!(scaled_rgba(&picture, 8, 4).is_none());
    }

    #[test]
    fn keyframes_and_positions_are_found_by_time() {
        let unit = |start_ms, sync| Unit {
            data: Vec::new(),
            start_ms,
            sync,
        };
        let all = vec![
            unit(0, true),
            unit(100, false),
            unit(200, true),
            unit(300, false),
        ];
        assert_eq!(keyframe_for(&all, 0), 0);
        assert_eq!(keyframe_for(&all, 199), 0);
        assert_eq!(keyframe_for(&all, 250), 2);
        assert_eq!(unit_at(&all, 250), 2);
        assert_eq!(unit_at(&all, 99), 0);
        assert_eq!(unit_at(&all, 10_000), 3);
    }
}
