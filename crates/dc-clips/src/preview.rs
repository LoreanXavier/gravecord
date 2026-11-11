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

/// Like [`scaled_rgba`], but every output pixel averages the source pixels it covers (area sampling), so a 1080p
/// picture shown at a third of its size keeps thin lines and text instead of dropping to nearest samples. Luma is
/// averaged over its box and each chroma plane over the matching box of the half-size plane; the colour conversion is
/// the same BT.601 limited range. RGBA sources fall back to [`scaled_rgba`].
pub fn scaled_rgba_area(
    picture: &Picture,
    max_width: u32,
    max_height: u32,
) -> Option<(u32, u32, Vec<u8>)> {
    if picture.layout == Layout::Rgba {
        return scaled_rgba(picture, max_width, max_height);
    }
    if picture.width == 0
        || picture.height == 0
        || picture.data.len() != picture.layout.len(picture.width, picture.height)?
    {
        return None;
    }
    let scale = (f64::from(max_width) / f64::from(picture.width))
        .min(f64::from(max_height) / f64::from(picture.height))
        .min(1.0);
    let out_width = ((f64::from(picture.width) * scale) as u32).max(1) as usize;
    let out_height = ((f64::from(picture.height) * scale) as u32).max(1) as usize;
    let (width, height) = (picture.width as usize, picture.height as usize);
    let (chroma_width, chroma_height) = (width.div_ceil(2), height.div_ceil(2));
    let plane = width * height;
    let edges = |count: usize, source: usize| -> Vec<(usize, usize)> {
        (0..count)
            .map(|index| {
                let from = index * source / count;
                let to = ((index + 1) * source / count).max(from + 1).min(source);
                (from, to)
            })
            .collect()
    };
    let columns = edges(out_width, width);
    let rows = edges(out_height, height);
    let chroma_at = |x: usize, y: usize| -> (usize, usize) {
        match picture.layout {
            Layout::Nv12 => {
                let pair = plane + (y * chroma_width + x) * 2;
                (pair, pair + 1)
            }
            _ => {
                let cb = plane + y * chroma_width + x;
                (cb, cb + chroma_width * chroma_height)
            }
        }
    };
    let mut out = vec![255u8; out_width * out_height * 4];
    for (y, (y0, y1)) in rows.iter().enumerate() {
        for (x, (x0, x1)) in columns.iter().enumerate() {
            let mut luma_sum = 0u32;
            for row in *y0..*y1 {
                let line = &picture.data[row * width + x0..row * width + x1];
                luma_sum += line.iter().map(|value| u32::from(*value)).sum::<u32>();
            }
            let luma = (luma_sum / ((y1 - y0) * (x1 - x0)) as u32) as i32;
            let (cx0, cx1) = (x0 / 2, x1.div_ceil(2).min(chroma_width).max(x0 / 2 + 1));
            let (cy0, cy1) = (y0 / 2, y1.div_ceil(2).min(chroma_height).max(y0 / 2 + 1));
            let (mut cb_sum, mut cr_sum, mut count) = (0u32, 0u32, 0u32);
            for cy in cy0..cy1 {
                for cx in cx0..cx1 {
                    let (cb, cr) = chroma_at(cx, cy);
                    cb_sum += u32::from(picture.data[cb]);
                    cr_sum += u32::from(picture.data[cr]);
                    count += 1;
                }
            }
            let cb = (cb_sum / count) as i32;
            let cr = (cr_sum / count) as i32;
            let c = (luma - 16) * 298;
            let d = cb - 128;
            let e = cr - 128;
            let at = (y * out_width + x) * 4;
            out[at] = clamp8((c + 409 * e + 128) >> 8);
            out[at + 1] = clamp8((c - 100 * d - 208 * e + 128) >> 8);
            out[at + 2] = clamp8((c + 516 * d + 128) >> 8);
        }
    }
    Some((out_width as u32, out_height as u32, out))
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
    fn area_sampling_averages_the_pixels_it_covers() {
        // Alternating black and white rows of luma: area sampling gives the mean, nearest sampling gives an extreme.
        let (width, height) = (8u32, 8u32);
        let mut picture = flat(Layout::I420, width, height, 16, 128, 128);
        for row in 0..height as usize {
            let value = if row % 2 == 0 { 16 } else { 235 };
            for column in 0..width as usize {
                picture.data[row * width as usize + column] = value;
            }
        }
        let area = scaled_rgba_area(&picture, 4, 4).unwrap();
        assert_eq!((area.0, area.1), (4, 4));
        let mid = i32::from(area.2[0]);
        assert!((mid - 128).abs() <= 3, "{mid}");
        let nearest = scaled_rgba(&picture, 4, 4).unwrap();
        assert!(i32::from(nearest.2[0]) < 20 || i32::from(nearest.2[0]) > 230);
        // A flat picture stays flat and agrees with the nearest path; a bigger limit keeps the size.
        let flat_grey = flat(Layout::Nv12, 64, 36, 126, 128, 128);
        let a = scaled_rgba_area(&flat_grey, 64, 36).unwrap();
        let b = scaled_rgba(&flat_grey, 64, 36).unwrap();
        assert_eq!(a, b);
        assert!(
            scaled_rgba_area(&flat(Layout::I420, 1920, 1080, 100, 128, 128), 960, 540).is_some()
        );
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
