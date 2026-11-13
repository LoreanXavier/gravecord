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

/// Where each of `to` pixels along one axis samples a picture `from` pixels long, bilinear at pixel centres: the two
/// source indices and the weight of the second.
fn axis(from: usize, to: usize) -> Vec<(usize, usize, f32)> {
    if from == 0 {
        return vec![(0, 0, 0.0); to];
    }
    let scale = from as f32 / to.max(1) as f32;
    (0..to)
        .map(|i| {
            let f = ((i as f32 + 0.5) * scale - 0.5).clamp(0.0, (from - 1) as f32);
            let i0 = f.floor() as usize;
            (i0, (i0 + 1).min(from - 1), f - i0 as f32)
        })
        .collect()
}

/// One row of `mask` sampled at another size: `row` from [`axis`] over its height, `xs` over its width.
fn sample_row(
    mask: &Mask,
    (y0, y1, ty): (usize, usize, f32),
    xs: &[(usize, usize, f32)],
    out: &mut [f32],
) {
    let top = &mask.values[y0 * mask.width..][..mask.width];
    let bottom = &mask.values[y1 * mask.width..][..mask.width];
    for (value, &(x0, x1, tx)) in out.iter_mut().zip(xs) {
        let upper = top[x0] * (1.0 - tx) + top[x1] * tx;
        let lower = bottom[x0] * (1.0 - tx) + bottom[x1] * tx;
        *value = upper * (1.0 - ty) + lower * ty;
    }
}

/// The mask stretched to `width` x `height`, bilinear at pixel centres.
pub fn upsample(mask: &Mask, width: usize, height: usize, out: &mut Vec<f32>) {
    out.clear();
    out.resize(width * height, 0.0);
    if mask.width == 0 || mask.height == 0 || width == 0 {
        return;
    }
    let xs = axis(mask.width, width);
    let ys = axis(mask.height, height);
    bands(out, width, |first, band| {
        for (row, line) in band.chunks_exact_mut(width).enumerate() {
            sample_row(mask, ys[first + row], &xs, line);
        }
    });
}

type Band = std::sync::RwLock<Vec<[f32; 3]>>;

/// Work buffers for [`blur`], kept between frames: the blur-size mask and the two picture buffers the passes
/// alternate between, each split into one band of rows per thread.
#[derive(Default)]
pub struct BlurBuffers {
    mask: Vec<f32>,
    front: Vec<Band>,
    back: Vec<Band>,
    /// (width, height, rows per band) the bands were made for.
    shape: (usize, usize, usize),
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

/// A tap resolved for one position along the pass's axis: the two clamped texels and the weights.
#[derive(Clone, Copy)]
struct Resolved {
    a: usize,
    b: usize,
    frac: f32,
    weight: f32,
}

/// The taps of one pass for every position along an axis `len` long.
fn pass_taps(len: usize, step: f32) -> Vec<[Resolved; 6]> {
    let offsets = taps(step);
    let last = len as isize - 1;
    (0..len as isize)
        .map(|i| {
            offsets.map(|tap| Resolved {
                a: (i + tap.base).clamp(0, last) as usize,
                b: (i + tap.base + 1).clamp(0, last) as usize,
                frac: tap.frac,
                weight: tap.weight,
            })
        })
        .collect()
}

/// `sample_frame` of the blur shader for one tap: the sample mixed towards the centre texel where it is the person.
#[inline]
fn mixed(center: [f32; 3], a: [f32; 3], b: [f32; 3], person: f32, frac: f32) -> [f32; 3] {
    let mut out = [0.0; 3];
    for c in 0..3 {
        let sample = a[c] * (1.0 - frac) + b[c] * frac;
        out[c] = sample + (center[c] - sample) * person;
    }
    out
}

/// A horizontal `BlurCalculator` pass of one row.
fn blur_row_across(src: &[[f32; 3]], mask: &[f32], table: &[[Resolved; 6]], out: &mut [[f32; 3]]) {
    for ((value, center), taps) in out.iter_mut().zip(src).zip(table) {
        let mut sum = [
            center[0] * WEIGHT_0,
            center[1] * WEIGHT_0,
            center[2] * WEIGHT_0,
        ];
        for tap in taps {
            let person = mask[tap.a] * (1.0 - tap.frac) + mask[tap.b] * tap.frac;
            let sample = mixed(*center, src[tap.a], src[tap.b], person, tap.frac);
            for c in 0..3 {
                sum[c] += sample[c] * tap.weight;
            }
        }
        *value = sum;
    }
}

fn write_band(band: &Band) -> std::sync::RwLockWriteGuard<'_, Vec<[f32; 3]>> {
    band.write().unwrap_or_else(|e| e.into_inner())
}

/// The rows the passes read: one read guard per band of `per` rows.
struct Rows<'a> {
    guards: Vec<std::sync::RwLockReadGuard<'a, Vec<[f32; 3]>>>,
    per: usize,
    width: usize,
}

impl<'a> Rows<'a> {
    fn read(bands: &'a [Band], per: usize, width: usize) -> Self {
        Self {
            guards: bands
                .iter()
                .map(|band| band.read().unwrap_or_else(|e| e.into_inner()))
                .collect(),
            per,
            width,
        }
    }

    fn row(&self, y: usize) -> &[[f32; 3]] {
        &self.guards[y / self.per][(y % self.per) * self.width..][..self.width]
    }
}

/// A vertical `BlurCalculator` pass of row `y`.
fn blur_row_down(
    rows: &Rows<'_>,
    mask: &[f32],
    y: usize,
    taps: &[Resolved; 6],
    out: &mut [[f32; 3]],
) {
    let width = rows.width;
    let center = rows.row(y);
    for (value, c) in out.iter_mut().zip(center) {
        *value = [c[0] * WEIGHT_0, c[1] * WEIGHT_0, c[2] * WEIGHT_0];
    }
    for tap in taps {
        let (above, below) = (rows.row(tap.a), rows.row(tap.b));
        let (mask_a, mask_b) = (
            &mask[tap.a * width..][..width],
            &mask[tap.b * width..][..width],
        );
        for x in 0..width {
            let person = mask_a[x] * (1.0 - tap.frac) + mask_b[x] * tap.frac;
            let sample = mixed(center[x], above[x], below[x], person, tap.frac);
            for c in 0..3 {
                out[x][c] += sample[c] * tap.weight;
            }
        }
    }
}

/// How much smaller than the frame the room is blurred. The official engine blurs on the GPU at the frame's own size
/// (`BlurCalculator` takes `throttled_input_video`), with the mask at the model's size (`blurred_segmentation_gpu`,
/// 256x256 or 256x144). The CPU here blurs a half-size copy with half the step, which is the same blur in frame
/// pixels, and still has more texels across than the mask (640 / 2 = 320 > 256).
pub const BLUR_SCALE: usize = 2;

/// The blurred room behind the person, as `BlurCalculator` makes it, written into `background` (RGBA, frame size).
/// `mask` may be at any size (the model's): it is sampled bilinearly, as the shader's texture lookup does.
///
/// All passes run on one set of threads, one band of rows each, meeting at a barrier between passes: starting threads
/// for each of the ten passes cost more than the passes themselves at 640x480.
pub fn blur(
    rgba: &[u8],
    width: usize,
    height: usize,
    mask: &Mask,
    iterations: usize,
    buffers: &mut BlurBuffers,
    background: &mut Vec<u8>,
) {
    background.clear();
    background.resize(width * height * 4, 255);
    if width == 0 || height == 0 || rgba.len() < width * height * 4 {
        return;
    }
    let (sw, sh) = (
        width.div_ceil(BLUR_SCALE).max(1),
        height.div_ceil(BLUR_SCALE).max(1),
    );
    buffers.mask.clear();
    buffers.mask.resize(sw * sh, 0.0);
    if mask.width > 0 && mask.height > 0 {
        let xs = axis(mask.width, sw);
        let ys = axis(mask.height, sh);
        for (y, line) in buffers.mask.chunks_exact_mut(sw).enumerate() {
            sample_row(mask, ys[y], &xs, line);
        }
    }
    let per = sh.div_ceil(threads().min(sh).max(1));
    let workers = sh.div_ceil(per);
    if buffers.shape != (sw, sh, per) || buffers.front.len() != workers {
        let make = || -> Vec<Band> {
            (0..workers)
                .map(|i| Band::new(vec![[0.0; 3]; per.min(sh - i * per) * sw]))
                .collect()
        };
        buffers.front = make();
        buffers.back = make();
        buffers.shape = (sw, sh, per);
    }
    let passes: Vec<(bool, Vec<[Resolved; 6]>)> = (0..2 * iterations)
        .map(|pass| {
            let step = (iterations - pass / 2 + 1) as f32 / BLUR_SCALE as f32;
            let horizontal = pass % 2 == 0;
            (
                horizontal,
                pass_taps(if horizontal { sw } else { sh }, step),
            )
        })
        .collect();
    let up_x = axis(sw, width);
    let up_y = axis(sh, height);
    let barrier = std::sync::Barrier::new(workers);
    let out_rows = per * BLUR_SCALE;
    let (front, back, small_mask) = (&buffers.front, &buffers.back, &buffers.mask[..]);
    let result = if passes.len().is_multiple_of(2) {
        front
    } else {
        back
    };
    std::thread::scope(|scope| {
        let mut outs = background.chunks_mut(out_rows * width * 4);
        for index in 0..workers {
            let out = outs.next();
            let (passes, barrier, up_x, up_y) = (&passes, &barrier, &up_x, &up_y);
            scope.spawn(move || {
                let first = index * per;
                {
                    // Average BLUR_SCALE x BLUR_SCALE blocks of the frame.
                    let mut own = write_band(&front[index]);
                    for (row, line) in own.chunks_exact_mut(sw).enumerate() {
                        let y = first + row;
                        let rows = y * BLUR_SCALE..((y + 1) * BLUR_SCALE).min(height);
                        for (x, value) in line.iter_mut().enumerate() {
                            let (mut sum, mut n) = ([0.0f32; 3], 0.0);
                            for yy in rows.clone() {
                                for xx in x * BLUR_SCALE..((x + 1) * BLUR_SCALE).min(width) {
                                    let p = (yy * width + xx) * 4;
                                    for c in 0..3 {
                                        sum[c] += f32::from(rgba[p + c]);
                                    }
                                    n += 1.0;
                                }
                            }
                            *value = [sum[0] / n, sum[1] / n, sum[2] / n];
                        }
                    }
                }
                barrier.wait();
                for (pass, (horizontal, table)) in passes.iter().enumerate() {
                    let (src, dst) = if pass % 2 == 0 {
                        (front, back)
                    } else {
                        (back, front)
                    };
                    {
                        let mut own = write_band(&dst[index]);
                        let rows = Rows::read(src, per, sw);
                        for (row, line) in own.chunks_exact_mut(sw).enumerate() {
                            let y = first + row;
                            if *horizontal {
                                let mask_row = &small_mask[y * sw..][..sw];
                                blur_row_across(rows.row(y), mask_row, table, line);
                            } else {
                                blur_row_down(&rows, small_mask, y, &table[y], line);
                            }
                        }
                    }
                    barrier.wait();
                }
                // Back to frame size, bilinear.
                let Some(out) = out else {
                    return;
                };
                let rows = Rows::read(result, per, sw);
                for (row, line) in out.chunks_exact_mut(width * 4).enumerate() {
                    let (y0, y1, ty) = up_y[index * out_rows + row];
                    let (top, bottom) = (rows.row(y0), rows.row(y1));
                    for (pixel, &(x0, x1, tx)) in line.chunks_exact_mut(4).zip(up_x) {
                        for c in 0..3 {
                            let upper = top[x0][c] * (1.0 - tx) + top[x1][c] * tx;
                            let lower = bottom[x0][c] * (1.0 - tx) + bottom[x1][c] * tx;
                            pixel[c] =
                                (upper * (1.0 - ty) + lower * ty).round().clamp(0.0, 255.0) as u8;
                        }
                    }
                }
            });
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
    if source_w == 0 || source_h == 0 || width == 0 || source.len() < source_w * source_h * 4 {
        return out;
    }
    let scale = (width as f32 / source_w as f32).max(height as f32 / source_h as f32);
    let offset_x = (source_w as f32 * scale - width as f32) / 2.0;
    let offset_y = (source_h as f32 * scale - height as f32) / 2.0;
    let xs: Vec<(usize, usize, f32)> = (0..width)
        .map(|x| {
            let fx =
                (((x as f32 + 0.5 + offset_x) / scale) - 0.5).clamp(0.0, (source_w - 1) as f32);
            let x0 = fx.floor() as usize;
            (x0, (x0 + 1).min(source_w - 1), fx - x0 as f32)
        })
        .collect();
    bands(&mut out, width * 4, |first, band| {
        for (row, line) in band.chunks_exact_mut(width * 4).enumerate() {
            let y = first + row;
            let fy =
                (((y as f32 + 0.5 + offset_y) / scale) - 0.5).clamp(0.0, (source_h - 1) as f32);
            let (y0, ty) = (fy.floor() as usize, fy.fract());
            let y1 = (y0 + 1).min(source_h - 1);
            for (pixel, &(x0, x1, tx)) in line.chunks_exact_mut(4).zip(&xs) {
                for c in 0..3 {
                    let at = |yy: usize, xx: usize| f32::from(source[(yy * source_w + xx) * 4 + c]);
                    let top = at(y0, x0) * (1.0 - tx) + at(y0, x1) * tx;
                    let bottom = at(y1, x0) * (1.0 - tx) + at(y1, x1) * tx;
                    pixel[c] = (top * (1.0 - ty) + bottom * ty).round() as u8;
                }
            }
        }
    });
    out
}

/// `LightWrapCalculator`'s mix, in place: the camera where the mask says person, `background` elsewhere. `mask` may be
/// at any size; it is sampled bilinearly at each pixel, as the shader's texture lookup does.
pub fn light_wrap(
    camera: &mut [u8],
    width: usize,
    height: usize,
    background: &[u8],
    mask: &Mask,
    threshold: f32,
) {
    if width == 0
        || height == 0
        || mask.width == 0
        || mask.height == 0
        || camera.len() < width * height * 4
        || background.len() < width * height * 4
    {
        return;
    }
    let xs = axis(mask.width, width);
    let ys = axis(mask.height, height);
    bands(
        &mut camera[..width * height * 4],
        width * 4,
        |first, band| {
            let mut weights = vec![0.0; width];
            for (row, line) in band.chunks_exact_mut(width * 4).enumerate() {
                let y = first + row;
                sample_row(mask, ys[y], &xs, &mut weights);
                let backs = background[y * width * 4..][..width * 4].chunks_exact(4);
                for ((pixel, back), &m) in line.chunks_exact_mut(4).zip(backs).zip(&weights) {
                    let weight = if m >= threshold {
                        m.clamp(0.0, 1.0)
                    } else {
                        0.0
                    };
                    for c in 0..3 {
                        let mixed = f32::from(back[c])
                            + (f32::from(pixel[c]) - f32::from(back[c])) * weight;
                        pixel[c] = mixed.round().clamp(0.0, 255.0) as u8;
                    }
                    pixel[3] = 255;
                }
            }
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn full(width: usize, height: usize, values: Vec<f32>) -> Mask {
        Mask {
            width,
            height,
            values,
        }
    }

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
        light_wrap(
            &mut camera,
            4,
            1,
            &room,
            &full(4, 1, vec![1.0, 0.0, 0.5, 0.2]),
            0.0,
        );
        assert_eq!(&camera[0..4], &[200, 100, 0, 255]);
        assert_eq!(&camera[4..8], &[0, 100, 200, 255]);
        assert_eq!(&camera[8..12], &[100, 100, 100, 255]);
        let mut camera = flat(1, 1, [200, 100, 0]);
        light_wrap(
            &mut camera,
            1,
            1,
            &flat(1, 1, [0, 100, 200]),
            &full(1, 1, vec![0.2]),
            0.3,
        );
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
            &full(w, h, mask.clone()),
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
            &full(w, h, vec![0.0; w * h]),
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
            &full(40, 30, vec![0.0; 1200]),
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

    /// The blur as one pass after the other on one thread with the mask at frame size, the way it was written first:
    /// the banded version must give the same picture.
    fn reference_blur(rgba: &[u8], width: usize, height: usize, mask: &[f32]) -> Vec<u8> {
        let (sw, sh) = (width.div_ceil(BLUR_SCALE), height.div_ceil(BLUR_SCALE));
        let mut color = vec![[0.0f32; 3]; sw * sh];
        let mut small = vec![0.0f32; sw * sh];
        for y in 0..sh {
            for x in 0..sw {
                let (mut sum, mut person, mut n) = ([0.0f32; 3], 0.0, 0.0);
                for yy in y * BLUR_SCALE..((y + 1) * BLUR_SCALE).min(height) {
                    for xx in x * BLUR_SCALE..((x + 1) * BLUR_SCALE).min(width) {
                        for c in 0..3 {
                            sum[c] += f32::from(rgba[(yy * width + xx) * 4 + c]);
                        }
                        person += mask[yy * width + xx];
                        n += 1.0;
                    }
                }
                color[y * sw + x] = [sum[0] / n, sum[1] / n, sum[2] / n];
                small[y * sw + x] = person / n;
            }
        }
        for pass in 0..2 * BLUR_ITERATIONS {
            let step = (BLUR_ITERATIONS - pass / 2 + 1) as f32 / BLUR_SCALE as f32;
            let horizontal = pass % 2 == 0;
            let mut out = vec![[0.0f32; 3]; sw * sh];
            for y in 0..sh as isize {
                for x in 0..sw as isize {
                    let center = color[(y * sw as isize + x) as usize];
                    let mut sum = center.map(|v| v * WEIGHT_0);
                    for tap in taps(step) {
                        let index = |d: isize| -> usize {
                            if horizontal {
                                (y * sw as isize + (x + d).clamp(0, sw as isize - 1)) as usize
                            } else {
                                ((y + d).clamp(0, sh as isize - 1) * sw as isize + x) as usize
                            }
                        };
                        let (a, b) = (index(tap.base), index(tap.base + 1));
                        let person = small[a] * (1.0 - tap.frac) + small[b] * tap.frac;
                        let sample = mixed(center, color[a], color[b], person, tap.frac);
                        for c in 0..3 {
                            sum[c] += sample[c] * tap.weight;
                        }
                    }
                    out[(y * sw as isize + x) as usize] = sum;
                }
            }
            color = out;
        }
        let mut room = vec![255u8; width * height * 4];
        let (xs, ys) = (axis(sw, width), axis(sh, height));
        for y in 0..height {
            let (y0, y1, ty) = ys[y];
            for x in 0..width {
                let (x0, x1, tx) = xs[x];
                for c in 0..3 {
                    let at = |yy: usize, xx: usize| color[yy * sw + xx][c];
                    let top = at(y0, x0) * (1.0 - tx) + at(y0, x1) * tx;
                    let bottom = at(y1, x0) * (1.0 - tx) + at(y1, x1) * tx;
                    room[(y * width + x) * 4 + c] =
                        (top * (1.0 - ty) + bottom * ty).round().clamp(0.0, 255.0) as u8;
                }
            }
        }
        room
    }

    #[test]
    fn the_banded_blur_matches_the_pass_by_pass_one() {
        let (w, h) = (96, 62);
        let frame: Vec<u8> = (0..w * h * 4).map(|i| ((i * 37) % 251) as u8).collect();
        let mask: Vec<f32> = (0..w * h)
            .map(|i| if (i % w) * 3 < w { 1.0 } else { 0.0 })
            .collect();
        let mut room = Vec::new();
        blur(
            &frame,
            w,
            h,
            &full(w, h, mask.clone()),
            BLUR_ITERATIONS,
            &mut BlurBuffers::default(),
            &mut room,
        );
        let reference = reference_blur(&frame, w, h, &mask);
        let worst = room
            .iter()
            .zip(&reference)
            .map(|(a, b)| a.abs_diff(*b))
            .max()
            .unwrap();
        assert!(worst <= 1, "largest difference {worst}");
    }

    #[test]
    fn bench_blur_banded_against_pass_by_pass_at_640x480() {
        let (w, h) = (640, 480);
        let frame: Vec<u8> = (0..w * h * 4).map(|i| ((i * 37) % 251) as u8).collect();
        let mask: Vec<f32> = (0..w * h)
            .map(|i| if (i % w) * 2 < w { 1.0 } else { 0.0 })
            .collect();
        let model_mask = Mask {
            width: 256,
            height: 256,
            values: (0..256 * 256)
                .map(|i| if (i % 256) < 128 { 1.0 } else { 0.0 })
                .collect(),
        };
        let started = std::time::Instant::now();
        for _ in 0..3 {
            reference_blur(&frame, w, h, &mask);
        }
        let before = started.elapsed().as_secs_f64() * 1000.0 / 3.0;
        let mut buffers = BlurBuffers::default();
        let mut room = Vec::new();
        blur(
            &frame,
            w,
            h,
            &model_mask,
            BLUR_ITERATIONS,
            &mut buffers,
            &mut room,
        );
        let started = std::time::Instant::now();
        for _ in 0..10 {
            blur(
                &frame,
                w,
                h,
                &model_mask,
                BLUR_ITERATIONS,
                &mut buffers,
                &mut room,
            );
        }
        let after = started.elapsed().as_secs_f64() * 1000.0 / 10.0;
        eprintln!(
            "blur at 640x480: {before:.2} ms pass by pass on one thread, {after:.2} ms banded"
        );
    }
}
