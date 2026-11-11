//! `frames grab <ms> <x> <y> <w> <h> <label>`: frames drawn back to back for `ms` and the pixels of one box read back
//! from each, so a picture that blanks, jumps or changes size between two frames shows in the numbers. Capture only.

use gpui::{AnyWindowHandle, AsyncApp};
use image::RgbaImage;
use std::time::{Duration, Instant};

/// One frame's reading of the box: when it was drawn and what it holds.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Reading {
    pub ms: f64,
    /// Pixels that differ from the box's top-left pixel (the background it sits on).
    pub ink: usize,
    /// Bounding box of those pixels (x0, y0, x1, y1), or `None` when the box is empty.
    pub extent: Option<(u32, u32, u32, u32)>,
    pub hash: u64,
}

/// The box's numbers in one frame.
type BoxRead = (usize, Option<(u32, u32, u32, u32)>, u64);

pub(super) fn read_box(image: &RgbaImage, x: u32, y: u32, w: u32, h: u32) -> BoxRead {
    let (width, height) = image.dimensions();
    let (x1, y1) = ((x + w).min(width), (y + h).min(height));
    if x >= x1 || y >= y1 {
        return (0, None, 0);
    }
    let background = *image.get_pixel(x, y);
    let mut ink = 0;
    let mut extent: Option<(u32, u32, u32, u32)> = None;
    let mut hash: u64 = 0xcbf29ce484222325;
    for py in y..y1 {
        for px in x..x1 {
            let pixel = image.get_pixel(px, py);
            for channel in 0..4 {
                hash = (hash ^ u64::from(pixel[channel])).wrapping_mul(0x100000001b3);
            }
            let differs = (0..3).any(|c| pixel[c].abs_diff(background[c]) > 8);
            if differs {
                ink += 1;
                extent = Some(match extent {
                    None => (px, py, px, py),
                    Some((a, b, c, d)) => (a.min(px), b.min(py), c.max(px), d.max(py)),
                });
            }
        }
    }
    (ink, extent, hash)
}

/// What the sequence says: frames, those with ink, blanks after the first ink, changes of the ink's extent after the
/// first ink, and distinct pictures.
pub(super) fn summarize(readings: &[Reading]) -> String {
    let first = readings.iter().position(|reading| reading.ink > 0);
    let (mut blanks, mut jumps, mut longest_blank, mut run) = (0, 0, 0, 0);
    let mut pictures = std::collections::HashSet::new();
    let mut worst_gap: f64 = 0.;
    if let Some(first) = first {
        let seen = &readings[first..];
        for reading in seen {
            pictures.insert(reading.hash);
            if reading.ink == 0 {
                blanks += 1;
                run += 1;
                longest_blank = longest_blank.max(run);
            } else {
                run = 0;
            }
        }
        for pair in seen.windows(2) {
            worst_gap = worst_gap.max(pair[1].ms - pair[0].ms);
            // A moving figure changes its ink a little between two frames; half of it going or coming is a jump.
            if pair[0].ink > 0
                && pair[1].ink > 0
                && pair[0].ink.abs_diff(pair[1].ink) * 2 > pair[0].ink.max(pair[1].ink)
            {
                jumps += 1;
            }
        }
    }
    format!(
        "frames={} first_ink_at_ms={} blank_after_first={blanks} longest_blank_run={longest_blank} ink_jumps={jumps} distinct={} worst_gap_ms={worst_gap:.1}",
        readings.len(),
        first.map_or("never".to_owned(), |at| format!("{:.0}", readings[at].ms)),
        pictures.len(),
    )
}

pub(super) async fn run(words: &[&str], window: AnyWindowHandle, cx: &mut AsyncApp) -> String {
    let [ms, x, y, w, h, label @ ..] = words else {
        return "usage: frames grab <ms> <x> <y> <w> <h> <label>".to_owned();
    };
    let (Ok(ms), Ok(x), Ok(y), Ok(w), Ok(h)) = (
        ms.parse::<u64>(),
        x.parse::<u32>(),
        y.parse::<u32>(),
        w.parse::<u32>(),
        h.parse::<u32>(),
    ) else {
        return "frames grab: numbers please".to_owned();
    };
    let started = Instant::now();
    let until = started + Duration::from_millis(ms);
    let mut readings = Vec::new();
    while Instant::now() < until {
        let at = started.elapsed().as_secs_f64() * 1000.;
        let image = cx.update(|cx| {
            window
                .update(cx, |_, window, cx| {
                    window.draw(cx).clear(cx);
                    window.render_to_image().ok()
                })
                .ok()
                .flatten()
        });
        if let Some(image) = image {
            let (ink, extent, hash) = read_box(&image, x, y, w, h);
            readings.push(Reading {
                ms: at,
                ink,
                extent,
                hash,
            });
        }
        cx.background_executor()
            .timer(Duration::from_millis(8))
            .await;
    }
    let mut line = format!("frames grab {}: {}", label.join(" "), summarize(&readings));
    line.push_str(" seq=");
    for reading in readings.iter().take(120) {
        line.push_str(&format!(
            "{:.0}:{}:{:04x} ",
            reading.ms,
            reading.ink,
            reading.hash & 0xffff
        ));
    }
    line
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;

    fn reading(ms: f64, ink: usize, hash: u64) -> Reading {
        Reading {
            ms,
            ink,
            extent: None,
            hash,
        }
    }

    #[test]
    fn a_box_that_blanks_after_it_appeared_is_counted() {
        let readings = [
            reading(0., 0, 1),
            reading(16., 0, 1),
            reading(32., 900, 2),
            reading(48., 0, 3),
            reading(64., 0, 3),
            reading(80., 910, 4),
        ];
        let line = summarize(&readings);
        assert!(line.contains("first_ink_at_ms=32"));
        assert!(line.contains("blank_after_first=2"));
        assert!(line.contains("longest_blank_run=2"));
    }

    #[test]
    fn a_steady_box_has_no_blank_and_no_jump() {
        let readings: Vec<Reading> = (0u32..10)
            .map(|at| reading(f64::from(at) * 16., 900 + at as usize, u64::from(at)))
            .collect();
        let line = summarize(&readings);
        assert!(line.contains("blank_after_first=0"));
        assert!(line.contains("ink_jumps=0"));
    }

    #[test]
    fn the_box_reads_its_ink_and_extent() {
        let mut image = RgbaImage::from_pixel(40, 40, Rgba([10, 10, 10, 255]));
        for x in 12..20 {
            for y in 14..18 {
                image.put_pixel(x, y, Rgba([250, 250, 250, 255]));
            }
        }
        let (ink, extent, _) = read_box(&image, 5, 5, 30, 30);
        assert_eq!(ink, 32);
        assert_eq!(extent, Some((12, 14, 19, 17)));
        let (blank, none, _) = read_box(&image, 25, 25, 10, 10);
        assert_eq!((blank, none), (0, None));
    }
}
