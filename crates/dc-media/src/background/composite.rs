//! The picture work around the segmentation model, ported from the official engine's MediaPipe graphs (embedded as
//! text in `discord_voice.node`, shaders in `mediapipe.dll`, both under `design/snapshot/desktop/modules/
//! discord_voice-1/discord_voice/`):
//!
//! * the mask is smoothed twice with a bilateral filter (`sigma_space` 10 then 3, `sigma_color` 0.3), then against
//!   the previous mask with `SegmentationSmoothingCalculator` (`combine_with_previous_ratio` 0.95, the default the
//!   graph builder writes when the caller passes none);
//! * Blur runs `BlurCalculator`: `iterations` (5 by default) pairs of horizontal and vertical passes of a 7-tap
//!   linear-sampling Gaussian whose step is `iterations - pass + 1` texels, where a tap that lands on the person is
//!   replaced by the centre texel so the person does not bleed into the blurred room;
//! * both modes end in `LightWrapCalculator`: `mix(background, camera, mask >= threshold ? mask : 0)`.
//!
//! Masks are `f32` in 0..1, one per pixel, 1 on the person.

use std::num::NonZeroUsize;

/// The graph's `$segmentation_smoothing_ratio` default.
pub const SMOOTHING_RATIO: f32 = 0.95;
/// The graph's `$blur_iterations` default.
pub const BLUR_ITERATIONS: usize = 5;
/// The replacement graph's `$confidence_threshold` default.
pub const CONFIDENCE_THRESHOLD: f32 = 0.0;
/// The two `BilateralFilterCalculator` nodes: (`sigma_space`, `sigma_color`).
pub const BILATERAL: [(f32, f32); 2] = [(10.0, 0.3), (3.0, 0.3)];

// The blur shader's constants (rastergrid's linear-sampling Gaussian).
const OFFSETS: [f32; 3] = [1.411_764_7, 3.294_117_6, 5.176_470_6];
const WEIGHT_0: f32 = 0.196_482_55;
const WEIGHTS: [f32; 3] = [0.296_906_96, 0.094_470_4, 0.010_381_362];

/// A mask at its own size.
#[derive(Clone, Debug, PartialEq)]
pub struct Mask {
    pub width: usize,
    pub height: usize,
    pub values: Vec<f32>,
}

impl Mask {
    /// Everything background: what is used before the first mask arrives, so the room never shows unprocessed.
    pub fn background(width: usize, height: usize) -> Self {
        Self {
            width,
            height,
            values: vec![0.0; width * height],
        }
    }
}

/// How many threads a frame's work is split across.
fn threads() -> usize {
    std::thread::available_parallelism()
        .map(NonZeroUsize::get)
        .unwrap_or(1)
        .clamp(1, 8)
}

/// Runs `work(first_row, rows)` over `out` split into bands of whole rows, one band per thread.
fn bands<T: Send>(out: &mut [T], row_len: usize, work: impl Fn(usize, &mut [T]) + Sync) {
    let rows = out.len() / row_len.max(1);
    let per = rows.div_ceil(threads()).max(1);
    if per >= rows {
        work(0, out);
        return;
    }
    std::thread::scope(|scope| {
        for (index, band) in out.chunks_mut(per * row_len).enumerate() {
            let work = &work;
            scope.spawn(move || work(index * per, band));
        }
    });
}

/// The model's input: the RGBA frame stretched to `width` x `height` (MediaPipe's `ImageToTensorCalculator` without
/// keeping the aspect ratio), RGB in 0..1, sampled bilinearly at pixel centres.
pub fn model_input(
    rgba: &[u8],
    frame_w: usize,
    frame_h: usize,
    width: usize,
    height: usize,
    out: &mut Vec<f32>,
) {
    out.clear();
    out.resize(width * height * 3, 0.0);
    if frame_w == 0 || frame_h == 0 {
        return;
    }
    let sx = frame_w as f32 / width as f32;
    let sy = frame_h as f32 / height as f32;
    for y in 0..height {
        let fy = ((y as f32 + 0.5) * sy - 0.5).clamp(0.0, (frame_h - 1) as f32);
        let (y0, ty) = (fy.floor() as usize, fy.fract());
        let y1 = (y0 + 1).min(frame_h - 1);
        for x in 0..width {
            let fx = ((x as f32 + 0.5) * sx - 0.5).clamp(0.0, (frame_w - 1) as f32);
            let (x0, tx) = (fx.floor() as usize, fx.fract());
            let x1 = (x0 + 1).min(frame_w - 1);
            for c in 0..3 {
                let at = |yy: usize, xx: usize| f32::from(rgba[(yy * frame_w + xx) * 4 + c]);
                let top = at(y0, x0) * (1.0 - tx) + at(y0, x1) * tx;
                let bottom = at(y1, x0) * (1.0 - tx) + at(y1, x1) * tx;
                out[(y * width + x) * 3 + c] = (top * (1.0 - ty) + bottom * ty) / 255.0;
            }
        }
    }
}

/// A bilateral filter guided by the mask itself, run as a horizontal then a vertical pass. `sigma_space` is in mask
/// pixels.
pub fn bilateral(mask: &mut Mask, sigma_space: f32, sigma_color: f32) {
    if sigma_space <= 0.0 || mask.width == 0 || mask.height == 0 {
        return;
    }
    let radius = (2.0 * sigma_space).ceil() as isize;
    let space: Vec<f32> = (-radius..=radius)
        .map(|d| (-((d * d) as f32) / (2.0 * sigma_space * sigma_space)).exp())
        .collect();
    let color = 1.0 / (2.0 * sigma_color * sigma_color);
    let (w, h) = (mask.width as isize, mask.height as isize);
    let pass = |src: &[f32], horizontal: bool| -> Vec<f32> {
        let mut out = vec![0.0; src.len()];
        for y in 0..h {
            for x in 0..w {
                let center = src[(y * w + x) as usize];
                let (mut sum, mut total) = (0.0, 0.0);
                for (k, d) in (-radius..=radius).enumerate() {
                    let (sx, sy) = if horizontal {
                        ((x + d).clamp(0, w - 1), y)
                    } else {
                        (x, (y + d).clamp(0, h - 1))
                    };
                    let value = src[(sy * w + sx) as usize];
                    let diff = value - center;
                    let weight = space[k] * (-diff * diff * color).exp();
                    sum += value * weight;
                    total += weight;
                }
                out[(y * w + x) as usize] = sum / total;
            }
        }
        out
    };
    let across = pass(&mask.values, true);
    mask.values = pass(&across, false);
}

/// `SegmentationSmoothingCalculator`'s shader: pulls an uncertain new value towards the previous one.
pub fn smooth_with_previous(current: &mut [f32], previous: &[f32], ratio: f32) {
    const C1: f32 = 5.68842;
    const C2: f32 = -0.748699;
    const C3: f32 = -57.8051;
    const C4: f32 = 291.309;
    const C5: f32 = -624.717;
    for (new, &prev) in current.iter_mut().zip(previous) {
        let t = *new - 0.5;
        let x = t * t;
        let uncertainty = 1.0 - (x * (C1 + x * (C2 + x * (C3 + x * (C4 + x * C5))))).min(1.0);
        *new += (prev - *new) * (uncertainty * ratio);
    }
}

/// The mask stretched to `width` x `height`, bilinear at pixel centres.
pub fn upsample(mask: &Mask, width: usize, height: usize, out: &mut Vec<f32>) {
    out.clear();
    out.resize(width * height, 0.0);
    if mask.width == 0 || mask.height == 0 {
        return;
    }
    let sx = mask.width as f32 / width as f32;
    let sy = mask.height as f32 / height as f32;
    let (mw, mh) = (mask.width, mask.height);
    bands(out, width, |first, band| {
        for (row, line) in band.chunks_exact_mut(width).enumerate() {
            let y = first + row;
            let fy = ((y as f32 + 0.5) * sy - 0.5).clamp(0.0, (mh - 1) as f32);
            let (y0, ty) = (fy.floor() as usize, fy.fract());
            let y1 = (y0 + 1).min(mh - 1);
            for (x, value) in line.iter_mut().enumerate() {
                let fx = ((x as f32 + 0.5) * sx - 0.5).clamp(0.0, (mw - 1) as f32);
                let (x0, tx) = (fx.floor() as usize, fx.fract());
                let x1 = (x0 + 1).min(mw - 1);
                let at = |yy: usize, xx: usize| mask.values[yy * mw + xx];
                let top = at(y0, x0) * (1.0 - tx) + at(y0, x1) * tx;
                let bottom = at(y1, x0) * (1.0 - tx) + at(y1, x1) * tx;
                *value = top * (1.0 - ty) + bottom * ty;
            }
        }
    });
}

/// Work buffers for [`blur`], kept between frames.
#[derive(Default)]
pub struct BlurBuffers {
    color: Vec<[f32; 3]>,
    spare: Vec<[f32; 3]>,
    mask: Vec<f32>,
}

/// Linear interpolation along one axis with the texture clamped to its edge.
#[derive(Clone, Copy)]
struct Tap {
    /// Whole texels from the centre to the lower neighbour (may be negative).
    base: isize,
    frac: f32,
    weight: f32,
}

fn taps(step: f32) -> [Tap; 6] {
    let make = |offset: f32, weight: f32| {
        let base = offset.floor();
        Tap {
            base: base as isize,
            frac: offset - base,
            weight,
        }
    };
    [
        make(OFFSETS[0] * step, WEIGHTS[0]),
        make(-OFFSETS[0] * step, WEIGHTS[0]),
        make(OFFSETS[1] * step, WEIGHTS[1]),
        make(-OFFSETS[1] * step, WEIGHTS[1]),
        make(OFFSETS[2] * step, WEIGHTS[2]),
        make(-OFFSETS[2] * step, WEIGHTS[2]),
    ]
}

/// One `BlurCalculator` pass from `src` into `dst` along one axis.
fn blur_pass(
    src: &[[f32; 3]],
    mask: &[f32],
    dst: &mut [[f32; 3]],
    width: usize,
    height: usize,
    step: f32,
    horizontal: bool,
) {
    let taps = taps(step);
    let (w, h) = (width as isize, height as isize);
    bands(dst, width, |first, band| {
        for (row, line) in band.chunks_exact_mut(width).enumerate() {
            let y = (first + row) as isize;
            for (x, out) in line.iter_mut().enumerate() {
                let x = x as isize;
                let center = src[(y * w + x) as usize];
                let mut sum = [
                    center[0] * WEIGHT_0,
                    center[1] * WEIGHT_0,
                    center[2] * WEIGHT_0,
                ];
                for tap in &taps {
                    let index = |d: isize| -> usize {
                        if horizontal {
                            (y * w + (x + d).clamp(0, w - 1)) as usize
                        } else {
                            ((y + d).clamp(0, h - 1) * w + x) as usize
                        }
                    };
                    let (a, b) = (index(tap.base), index(tap.base + 1));
                    let person = mask[a] * (1.0 - tap.frac) + mask[b] * tap.frac;
                    for c in 0..3 {
                        let sample = src[a][c] * (1.0 - tap.frac) + src[b][c] * tap.frac;
                        sum[c] += (sample + (center[c] - sample) * person) * tap.weight;
                    }
                }
                *out = sum;
            }
        }
    });
}

/// How much smaller than the frame the room is blurred. The official engine blurs on the GPU at the frame's own size;
/// the CPU here blurs a half-size copy with half the step, which is the same blur in frame pixels.
pub const BLUR_SCALE: usize = 2;

/// The blurred room behind the person, as `BlurCalculator` makes it, written into `background` (RGBA, frame size).
/// `mask` is at frame size.
pub fn blur(
    rgba: &[u8],
    width: usize,
    height: usize,
    mask: &[f32],
    iterations: usize,
    buffers: &mut BlurBuffers,
    background: &mut Vec<u8>,
) {
    let (sw, sh) = (
        width.div_ceil(BLUR_SCALE).max(1),
        height.div_ceil(BLUR_SCALE).max(1),
    );
    // Average BLUR_SCALE x BLUR_SCALE blocks of colour and mask.
    buffers.color.clear();
    buffers.color.resize(sw * sh, [0.0; 3]);
    buffers.mask.clear();
    buffers.mask.resize(sw * sh, 0.0);
    bands(&mut buffers.color, sw, |first, band| {
        for (row, line) in band.chunks_exact_mut(sw).enumerate() {
            let y = first + row;
            for (x, out) in line.iter_mut().enumerate() {
                let (mut sum, mut n) = ([0.0f32; 3], 0.0);
                for yy in y * BLUR_SCALE..((y + 1) * BLUR_SCALE).min(height) {
                    for xx in x * BLUR_SCALE..((x + 1) * BLUR_SCALE).min(width) {
                        let p = (yy * width + xx) * 4;
                        for c in 0..3 {
                            sum[c] += f32::from(rgba[p + c]);
                        }
                        n += 1.0;
                    }
                }
                *out = [sum[0] / n, sum[1] / n, sum[2] / n];
            }
        }
    });
    for y in 0..sh {
        for x in 0..sw {
            let (mut person, mut n) = (0.0, 0.0);
            for yy in y * BLUR_SCALE..((y + 1) * BLUR_SCALE).min(height) {
                for xx in x * BLUR_SCALE..((x + 1) * BLUR_SCALE).min(width) {
                    person += mask[yy * width + xx];
                    n += 1.0;
                }
            }
            buffers.mask[y * sw + x] = person / n;
        }
    }
    buffers.spare.clear();
    buffers.spare.resize(sw * sh, [0.0; 3]);
    for pass in 0..2 * iterations {
        let step = (iterations - pass / 2 + 1) as f32 / BLUR_SCALE as f32;
        let horizontal = pass % 2 == 0;
        blur_pass(
            &buffers.color,
            &buffers.mask,
            &mut buffers.spare,
            sw,
            sh,
            step,
            horizontal,
        );
        std::mem::swap(&mut buffers.color, &mut buffers.spare);
    }
    // Back to frame size, bilinear.
    background.clear();
    background.resize(width * height * 4, 255);
    let src = &buffers.color;
    let sx = sw as f32 / width as f32;
    let sy = sh as f32 / height as f32;
    bands(background, width * 4, |first, band| {
        for (row, line) in band.chunks_exact_mut(width * 4).enumerate() {
            let y = first + row;
            let fy = ((y as f32 + 0.5) * sy - 0.5).clamp(0.0, (sh - 1) as f32);
            let (y0, ty) = (fy.floor() as usize, fy.fract());
            let y1 = (y0 + 1).min(sh - 1);
            for (x, pixel) in line.chunks_exact_mut(4).enumerate() {
                let fx = ((x as f32 + 0.5) * sx - 0.5).clamp(0.0, (sw - 1) as f32);
                let (x0, tx) = (fx.floor() as usize, fx.fract());
                let x1 = (x0 + 1).min(sw - 1);
                for c in 0..3 {
                    let at = |yy: usize, xx: usize| src[yy * sw + xx][c];
                    let top = at(y0, x0) * (1.0 - tx) + at(y0, x1) * tx;
                    let bottom = at(y1, x0) * (1.0 - tx) + at(y1, x1) * tx;
                    pixel[c] = (top * (1.0 - ty) + bottom * ty).round().clamp(0.0, 255.0) as u8;
                }
            }
        }
    });
}

/// An RGBA picture scaled to cover `width` x `height` and centred, bilinear.
pub fn cover(
    source: &[u8],
    source_w: usize,
    source_h: usize,
    width: usize,
    height: usize,
) -> Vec<u8> {
    let mut out = vec![255; width * height * 4];
    if source_w == 0 || source_h == 0 || source.len() < source_w * source_h * 4 {
        return out;
    }
    let scale = (width as f32 / source_w as f32).max(height as f32 / source_h as f32);
    let offset_x = (source_w as f32 * scale - width as f32) / 2.0;
    let offset_y = (source_h as f32 * scale - height as f32) / 2.0;
    for y in 0..height {
        let fy = (((y as f32 + 0.5 + offset_y) / scale) - 0.5).clamp(0.0, (source_h - 1) as f32);
        let (y0, ty) = (fy.floor() as usize, fy.fract());
        let y1 = (y0 + 1).min(source_h - 1);
        for x in 0..width {
            let fx =
                (((x as f32 + 0.5 + offset_x) / scale) - 0.5).clamp(0.0, (source_w - 1) as f32);
            let (x0, tx) = (fx.floor() as usize, fx.fract());
            let x1 = (x0 + 1).min(source_w - 1);
            for c in 0..3 {
                let at = |yy: usize, xx: usize| f32::from(source[(yy * source_w + xx) * 4 + c]);
                let top = at(y0, x0) * (1.0 - tx) + at(y0, x1) * tx;
                let bottom = at(y1, x0) * (1.0 - tx) + at(y1, x1) * tx;
                out[(y * width + x) * 4 + c] = (top * (1.0 - ty) + bottom * ty).round() as u8;
            }
        }
    }
    out
}

/// `LightWrapCalculator`'s mix, in place: the camera where the mask says person, `background` elsewhere.
pub fn light_wrap(camera: &mut [u8], background: &[u8], mask: &[f32], threshold: f32) {
    const SPAN: usize = 4 * 4096;
    bands(camera, SPAN, |first, band| {
        let start = first * SPAN;
        let pixels = band.chunks_exact_mut(4);
        let backs = background[start..].chunks_exact(4);
        for ((pixel, back), &m) in pixels.zip(backs).zip(&mask[start / 4..]) {
            let weight = if m >= threshold {
                m.clamp(0.0, 1.0)
            } else {
                0.0
            };
            for c in 0..3 {
                let mixed =
                    f32::from(back[c]) + (f32::from(pixel[c]) - f32::from(back[c])) * weight;
                pixel[c] = mixed.round().clamp(0.0, 255.0) as u8;
            }
            pixel[3] = 255;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flat(width: usize, height: usize, rgb: [u8; 3]) -> Vec<u8> {
        let mut out = Vec::with_capacity(width * height * 4);
        for _ in 0..width * height {
            out.extend_from_slice(&[rgb[0], rgb[1], rgb[2], 255]);
        }
        out
    }

    #[test]
    fn certain_values_ignore_the_previous_mask_and_uncertain_ones_follow_it() {
        let mut current = vec![0.0, 1.0, 0.5];
        smooth_with_previous(&mut current, &[1.0, 0.0, 1.0], SMOOTHING_RATIO);
        assert!(current[0] < 0.01 && current[1] > 0.99, "{current:?}");
        // p = 0.5 is fully uncertain: 0.5 + (1 - 0.5) * 0.95.
        assert!((current[2] - 0.975).abs() < 1e-5, "{current:?}");
    }

    #[test]
    fn light_wrap_mixes_by_the_mask_and_honours_the_threshold() {
        let mut camera = flat(4, 1, [200, 100, 0]);
        let room = flat(4, 1, [0, 100, 200]);
        light_wrap(&mut camera, &room, &[1.0, 0.0, 0.5, 0.2], 0.0);
        assert_eq!(&camera[0..4], &[200, 100, 0, 255]);
        assert_eq!(&camera[4..8], &[0, 100, 200, 255]);
        assert_eq!(&camera[8..12], &[100, 100, 100, 255]);
        let mut camera = flat(1, 1, [200, 100, 0]);
        light_wrap(&mut camera, &flat(1, 1, [0, 100, 200]), &[0.2], 0.3);
        assert_eq!(&camera[0..3], &[0, 100, 200]);
    }

    #[test]
    fn a_hard_mask_edge_is_feathered_on_upsampling() {
        let mask = Mask {
            width: 4,
            height: 1,
            values: vec![1.0, 1.0, 0.0, 0.0],
        };
        let mut full = Vec::new();
        upsample(&mask, 16, 4, &mut full);
        let row = &full[..16];
        assert_eq!(row[0], 1.0);
        assert_eq!(row[15], 0.0);
        let between = row.iter().filter(|v| **v > 0.01 && **v < 0.99).count();
        assert!(between >= 3, "a soft edge several pixels wide: {row:?}");
        assert!(row.windows(2).all(|w| w[0] >= w[1]), "monotonic: {row:?}");
        // The bilateral filters keep the edge but smooth noise inside a region.
        let mut noisy = Mask {
            width: 8,
            height: 1,
            values: vec![0.9, 1.0, 0.95, 1.0, 0.0, 0.05, 0.0, 0.1],
        };
        bilateral(&mut noisy, 2.0, 0.3);
        assert!(
            noisy.values[..4].iter().all(|v| *v > 0.85)
                && noisy.values[4..].iter().all(|v| *v < 0.15),
            "{noisy:?}"
        );
    }

    #[test]
    fn blur_smooths_the_room_without_the_person_bleeding_into_it() {
        let (w, h) = (64, 32);
        // A white person on the left half, a black and grey striped room on the right.
        let mut frame = flat(w, h, [0, 0, 0]);
        let mut mask = vec![0.0; w * h];
        for y in 0..h {
            for x in 0..w {
                let p = (y * w + x) * 4;
                if x < w / 2 {
                    frame[p..p + 3].copy_from_slice(&[255, 255, 255]);
                    mask[y * w + x] = 1.0;
                } else if (x / 2) % 2 == 0 {
                    frame[p..p + 3].copy_from_slice(&[128, 128, 128]);
                }
            }
        }
        let mut buffers = BlurBuffers::default();
        let mut room = Vec::new();
        blur(
            &frame,
            w,
            h,
            &mask,
            BLUR_ITERATIONS,
            &mut buffers,
            &mut room,
        );
        let right: Vec<u8> = (w / 2 + 6..w - 6).map(|x| room[(16 * w + x) * 4]).collect();
        // Two-pixel stripes of 0 and 128 become a smooth ramp (the clamped frame edge darkens the far end).
        let ripple = right.windows(2).map(|p| p[0].abs_diff(p[1])).max().unwrap();
        assert!(ripple < 4, "stripes are blurred away: {right:?}");
        let mean = right.iter().map(|v| f32::from(*v)).sum::<f32>() / right.len() as f32;
        assert!(
            (mean - 64.0).abs() < 16.0,
            "the room keeps its own brightness, no white from the person: {mean}"
        );
        // Without the mask the white side bleeds in.
        let mut leaky = Vec::new();
        blur(
            &frame,
            w,
            h,
            &vec![0.0; w * h],
            BLUR_ITERATIONS,
            &mut buffers,
            &mut leaky,
        );
        let edge = |pic: &[u8]| pic[(16 * w + w / 2 + 2) * 4];
        assert!(
            edge(&leaky) > edge(&room) + 20,
            "{} vs {}",
            edge(&leaky),
            edge(&room)
        );
    }

    #[test]
    fn a_uniform_room_stays_uniform() {
        let frame = flat(40, 30, [10, 120, 230]);
        let mut room = Vec::new();
        blur(
            &frame,
            40,
            30,
            &vec![0.0; 1200],
            BLUR_ITERATIONS,
            &mut BlurBuffers::default(),
            &mut room,
        );
        assert!(room.chunks_exact(4).all(|p| p == [10, 120, 230, 255]));
    }

    #[test]
    fn cover_fills_and_centres() {
        // Red, green, blue side by side covering a square: only the middle (green) shows, at its own scale.
        let mut source = flat(3, 1, [255, 0, 0]);
        source[4..7].copy_from_slice(&[0, 255, 0]);
        source[8..11].copy_from_slice(&[0, 0, 255]);
        assert_eq!(cover(&source, 3, 1, 1, 1), vec![0, 255, 0, 255]);
        // Stacked the other way, a wide frame shows the middle row.
        assert_eq!(cover(&source, 1, 3, 1, 1), vec![0, 255, 0, 255]);
        // A frame of another shape is filled entirely.
        let out = cover(&flat(4, 1, [7, 8, 9]), 4, 1, 3, 5);
        assert_eq!(out.len(), 60);
        assert!(out.chunks_exact(4).all(|p| p == [7, 8, 9, 255]));
    }

    #[test]
    fn the_model_input_is_the_frame_stretched_to_0_1() {
        let frame = flat(8, 6, [255, 0, 51]);
        let mut input = Vec::new();
        model_input(&frame, 8, 6, 4, 4, &mut input);
        assert_eq!(input.len(), 48);
        assert!(input.chunks_exact(3).all(|p| p == [1.0, 0.0, 0.2]));
    }
}
