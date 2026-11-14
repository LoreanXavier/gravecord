//! One-pass letterboxed scaling with BT.601 limited-range 4:2:0 conversion, plus the plain planar shuffles and the box
//! downscale the thumbnails and previews use.
//!
//! Screen sources rarely match the stream size (a 1440p monitor streamed at 1080p), so scaling, colour conversion and
//! chroma packing happen together in integer arithmetic, split into row bands across a few threads, and written
//! straight into the encoder's input buffer. Adapted from Serein's `crates/discord-voice/src/screen/convert.rs` (MIT OR
//! Apache-2.0); the source may now be BGRA or RGBA, so the camera shares the path. Coefficients match openh264's
//! converter, so both encoders look the same.

use crate::band_pool::{self, Input, Scratch, Task};
use crate::{Error, Picture, Result};
use std::sync::{Arc, Mutex, OnceLock};

/// Byte order of a packed 4-byte source pixel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PixelOrder {
    Bgra,
    Rgba,
}

impl PixelOrder {
    /// Byte offsets of red, green and blue inside a pixel.
    fn offsets(self) -> [usize; 3] {
        match self {
            Self::Bgra => [2, 1, 0],
            Self::Rgba => [0, 1, 2],
        }
    }
}

/// Destination chroma layout after the full-size luma plane.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Chroma {
    /// NV12: one interleaved UV plane (Media Foundation input).
    Interleaved,
    /// I420: separate U then V planes (openh264 input).
    Planar,
}

/// A strided packed 4-byte-per-pixel picture borrowed from a capture API.
#[derive(Clone, Copy)]
pub(crate) struct Raw<'a> {
    pub width: usize,
    pub height: usize,
    pub stride: usize,
    pub order: PixelOrder,
    pub data: &'a [u8],
}

impl Raw<'_> {
    fn validate(&self) -> Result<()> {
        validate_raw(
            SourceMeta {
                width: self.width,
                height: self.height,
                stride: self.stride,
                order: self.order,
            },
            self.data.len(),
        )
    }
}

/// The largest size with `width:height` aspect that fits inside `max_width` x `max_height`, never above the source and
/// never below one pixel. Used for thumbnails and previews.
pub fn fit_size(width: u32, height: u32, max_width: u32, max_height: u32) -> (u32, u32) {
    if width == 0 || height == 0 || max_width == 0 || max_height == 0 {
        return (1, 1);
    }
    if width <= max_width && height <= max_height {
        return (width, height);
    }
    let scale =
        (f64::from(max_width) / f64::from(width)).min(f64::from(max_height) / f64::from(height));
    (
        ((f64::from(width) * scale).round() as u32).clamp(1, max_width),
        ((f64::from(height) * scale).round() as u32).clamp(1, max_height),
    )
}

/// Bilinear source tap: byte offsets of the two neighbours and the 8-bit blend weight.
#[derive(Clone, Copy)]
struct Tap {
    near: usize,
    far: usize,
    weight: u32,
}

/// Destination axis mapping into a source axis, with letterbox bounds.
struct Axis {
    taps: Vec<Tap>,
    start: usize,
    end: usize,
}

impl Axis {
    fn new(source: usize, scaled: usize, destination: usize, step: usize) -> Self {
        let start = (destination - scaled) / 2;
        let taps = (0..scaled)
            .map(|index| {
                let position = ((index as f64 + 0.5) * source as f64 / scaled as f64 - 0.5)
                    .clamp(0.0, (source - 1) as f64);
                let near = position as usize;
                Tap {
                    near: near * step,
                    far: (near + 1).min(source - 1) * step,
                    weight: ((position - near as f64) * 256.0) as u32,
                }
            })
            .collect();
        Self {
            taps,
            start,
            end: start + scaled,
        }
    }

    fn tap(&self, index: usize) -> Option<Tap> {
        (self.start..self.end)
            .contains(&index)
            .then(|| self.taps[index - self.start])
    }
}

/// Cached source-to-destination bilinear axes shared across bands and frames of constant geometry.
pub(crate) struct Axes {
    columns: Axis,
    rows: Axis,
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct AxisKey {
    src_width: usize,
    src_height: usize,
    stride: usize,
    dst_width: usize,
    dst_height: usize,
}

type AxisCacheEntry = (AxisKey, Arc<Axes>);

fn cached_axes(key: AxisKey) -> Arc<Axes> {
    static CACHE: OnceLock<Mutex<Option<AxisCacheEntry>>> = OnceLock::new();
    let mutex = CACHE.get_or_init(|| Mutex::new(None));
    {
        let guard = mutex.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((cached_key, axes)) = guard.as_ref()
            && *cached_key == key
        {
            return Arc::clone(axes);
        }
    }
    let scale = (key.dst_width as f64 / key.src_width as f64)
        .min(key.dst_height as f64 / key.src_height as f64);
    let scaled_width = ((key.src_width as f64 * scale).round() as usize).clamp(1, key.dst_width);
    let scaled_height =
        ((key.src_height as f64 * scale).round() as usize).clamp(1, key.dst_height);
    let axes = Arc::new(Axes {
        columns: Axis::new(key.src_width, scaled_width, key.dst_width, 4),
        rows: Axis::new(key.src_height, scaled_height, key.dst_height, key.stride),
    });
    let mut guard = mutex.lock().unwrap_or_else(|e| e.into_inner());
    *guard = Some((key, Arc::clone(&axes)));
    axes
}

/// Geometry descriptor for one worker row band of 4:2:0 conversion.
pub(crate) struct YuvBand {
    identity: bool,
    offsets: [usize; 3],
    axes: Arc<Axes>,
    width: usize,
    first: usize,
    row_count: usize,
    chroma: Chroma,
}

impl YuvBand {
    pub(crate) fn out_len(&self) -> usize {
        let luma = self.row_count * self.width;
        let chroma = (self.row_count / 2) * self.width;
        luma + chroma
    }
}

pub(crate) fn run_band(source: &[u8], out: &mut [u8], scratch: &mut Scratch, job: &YuvBand) {
    let luma_len = job.row_count * job.width;
    let (luma, chroma_out) = out.split_at_mut(luma_len);
    let chroma = match job.chroma {
        Chroma::Interleaved => Out::Interleaved(chroma_out),
        Chroma::Planar => {
            let plane = chroma_out.len() / 2;
            let (u, v) = chroma_out.split_at_mut(plane);
            Out::Planar(u, v)
        }
    };
    Band {
        source,
        identity: job.identity,
        offsets: job.offsets,
        columns: &job.axes.columns,
        rows: &job.axes.rows,
        width: job.width,
        first: job.first,
        luma,
        chroma,
    }
    .run_into(&mut scratch.yuv_top, &mut scratch.yuv_bottom);
}

#[derive(Clone, Copy)]
pub(crate) struct SourceMeta {
    pub(crate) width: usize,
    pub(crate) height: usize,
    pub(crate) stride: usize,
    pub(crate) order: PixelOrder,
}

enum BandTarget<'a> {
    Interleaved {
        luma: &'a mut [u8],
        uv: &'a mut [u8],
    },
    Planar {
        luma: &'a mut [u8],
        u: &'a mut [u8],
        v: &'a mut [u8],
    },
}

impl<'a> BandTarget<'a> {
    fn copy_from_worker(&mut self, bytes: &[u8]) {
        match self {
            Self::Interleaved { luma, uv } => {
                luma.copy_from_slice(&bytes[..luma.len()]);
                uv.copy_from_slice(&bytes[luma.len()..][..uv.len()]);
            }
            Self::Planar { luma, u, v } => {
                luma.copy_from_slice(&bytes[..luma.len()]);
                let rest = &bytes[luma.len()..];
                u.copy_from_slice(&rest[..u.len()]);
                v.copy_from_slice(&rest[u.len()..][..v.len()]);
            }
        }
    }
}

fn validate_raw(meta: SourceMeta, data_len: usize) -> Result<()> {
    let row = meta
        .width
        .checked_mul(4)
        .ok_or_else(|| Error::invalid("frame width overflows"))?;
    let needed = meta
        .stride
        .checked_mul(meta.height.saturating_sub(1))
        .and_then(|bytes| bytes.checked_add(row))
        .ok_or_else(|| Error::invalid("frame size overflows"))?;
    if meta.width == 0 || meta.height == 0 || meta.stride < row || data_len < needed {
        return Err(Error::invalid("frame planes do not match its size"));
    }
    Ok(())
}

/// Fit `frame` into `width` x `height` 4:2:0 `output` (exactly `width * height * 3 / 2` bytes), keeping the aspect
/// ratio and filling the rest with black.
pub(crate) fn to_yuv420(
    frame: Raw<'_>,
    width: usize,
    height: usize,
    chroma: Chroma,
    output: &mut [u8],
) -> Result<()> {
    to_yuv420_impl(
        Input::Slice(frame.data),
        SourceMeta {
            width: frame.width,
            height: frame.height,
            stride: frame.stride,
            order: frame.order,
        },
        width,
        height,
        chroma,
        output,
    )
}

/// Same as [`to_yuv420`], sharing `source` across persistent conversion workers without copying pixel bytes.
pub(crate) fn to_yuv420_vec(
    source: &mut Vec<u8>,
    meta: SourceMeta,
    width: usize,
    height: usize,
    chroma: Chroma,
    output: &mut [u8],
) -> Result<()> {
    to_yuv420_impl(Input::Vec(source), meta, width, height, chroma, output)
}

fn to_yuv420_impl(
    input: Input<'_>,
    meta: SourceMeta,
    width: usize,
    height: usize,
    chroma: Chroma,
    output: &mut [u8],
) -> Result<()> {
    let input_len = match &input {
        Input::Slice(slice) => slice.len(),
        Input::Vec(vec) => vec.len(),
    };
    validate_raw(meta, input_len)?;
    if width == 0
        || height == 0
        || !width.is_multiple_of(2)
        || !height.is_multiple_of(2)
        || output.len() != width * height * 3 / 2
    {
        return Err(Error::invalid("encoder picture size must be even"));
    }
    let axes = cached_axes(AxisKey {
        src_width: meta.width,
        src_height: meta.height,
        stride: meta.stride,
        dst_width: width,
        dst_height: height,
    });
    let identity = meta.width == width && meta.height == height;
    let offsets = meta.order.offsets();

    let (mut luma, chroma_planes) = output.split_at_mut(width * height);
    let bands = band_pool::band_count(height);
    // Even row counts keep every chroma row inside one band.
    let band_rows = height.div_ceil(bands).next_multiple_of(2);
    let (mut u, mut v): (&mut [u8], &mut [u8]) = match chroma {
        Chroma::Interleaved => (chroma_planes, &mut []),
        Chroma::Planar => chroma_planes.split_at_mut(width * height / 4),
    };

    let mut worker_tasks = Vec::with_capacity(bands.saturating_sub(1));
    let mut worker_targets = Vec::with_capacity(bands.saturating_sub(1));
    let mut caller: Option<(usize, &mut [u8], Out<'_>)> = None;

    let mut first = 0;
    while first < height {
        let count = band_rows.min(height - first);
        let (band_luma, rest_luma) = std::mem::take(&mut luma).split_at_mut(count * width);
        luma = rest_luma;
        let chroma_bytes = count / 2 * width / 2;
        if first + count >= height {
            let caller_chroma = match chroma {
                Chroma::Interleaved => Out::Interleaved(std::mem::take(&mut u)),
                Chroma::Planar => Out::Planar(std::mem::take(&mut u), std::mem::take(&mut v)),
            };
            caller = Some((first, band_luma, caller_chroma));
        } else {
            let target = match chroma {
                Chroma::Interleaved => {
                    let (band, rest) = std::mem::take(&mut u).split_at_mut(chroma_bytes * 2);
                    u = rest;
                    BandTarget::Interleaved {
                        luma: band_luma,
                        uv: band,
                    }
                }
                Chroma::Planar => {
                    let (band_u, rest_u) = std::mem::take(&mut u).split_at_mut(chroma_bytes);
                    let (band_v, rest_v) = std::mem::take(&mut v).split_at_mut(chroma_bytes);
                    (u, v) = (rest_u, rest_v);
                    BandTarget::Planar {
                        luma: band_luma,
                        u: band_u,
                        v: band_v,
                    }
                }
            };
            worker_targets.push(target);
            worker_tasks.push(Task::Yuv420(YuvBand {
                identity,
                offsets,
                axes: Arc::clone(&axes),
                width,
                first,
                row_count: count,
                chroma,
            }));
        }
        first += count;
    }

    let (caller_first, caller_luma, caller_chroma) =
        caller.ok_or_else(|| Error::invalid("encoder picture size must be even"))?;

    band_pool::dispatch(
        input,
        worker_tasks,
        |source, scratch| {
            Band {
                source,
                identity,
                offsets,
                columns: &axes.columns,
                rows: &axes.rows,
                width,
                first: caller_first,
                luma: caller_luma,
                chroma: caller_chroma,
            }
            .run_into(&mut scratch.yuv_top, &mut scratch.yuv_bottom);
        },
        |index, bytes| worker_targets[index].copy_from_worker(bytes),
    );
    Ok(())
}

enum Out<'a> {
    Interleaved(&'a mut [u8]),
    Planar(&'a mut [u8], &'a mut [u8]),
}

struct Band<'a> {
    source: &'a [u8],
    /// The source is already the destination size (a share at the source's resolution): no scaling taps.
    identity: bool,
    offsets: [usize; 3],
    columns: &'a Axis,
    rows: &'a Axis,
    width: usize,
    first: usize,
    luma: &'a mut [u8],
    chroma: Out<'a>,
}

impl Band<'_> {
    #[cfg(test)]
    fn run(self) {
        let (mut top, mut bottom) = (Vec::new(), Vec::new());
        self.run_into(&mut top, &mut bottom);
    }

    fn run_into(mut self, top: &mut Vec<[u32; 3]>, bottom: &mut Vec<[u32; 3]>) {
        let width = self.width;
        top.resize(width, [0; 3]);
        bottom.resize(width, [0; 3]);
        for pair in 0..self.luma.len() / width / 2 {
            let row = self.first + pair * 2;
            self.sample(row, top);
            self.sample(row + 1, bottom);
            let offset = pair * 2 * width;
            for (x, rgb) in top.iter().enumerate() {
                self.luma[offset + x] = luma(*rgb);
            }
            for (x, rgb) in bottom.iter().enumerate() {
                self.luma[offset + width + x] = luma(*rgb);
            }
            let chroma_row = pair * width / 2;
            for x in 0..width / 2 {
                let mut sum = [0u32; 3];
                for rgb in [top[2 * x], top[2 * x + 1], bottom[2 * x], bottom[2 * x + 1]] {
                    for (total, channel) in sum.iter_mut().zip(rgb) {
                        *total += channel;
                    }
                }
                let (u, v) = chroma(sum.map(|total| (total + 2) / 4));
                match &mut self.chroma {
                    Out::Interleaved(uv) => {
                        uv[2 * (chroma_row + x)] = u;
                        uv[2 * (chroma_row + x) + 1] = v;
                    }
                    Out::Planar(plane_u, plane_v) => {
                        plane_u[chroma_row + x] = u;
                        plane_v[chroma_row + x] = v;
                    }
                }
            }
        }
    }

    /// One destination row as RGB, black outside the letterboxed picture.
    fn sample(&self, row: usize, out: &mut [[u32; 3]]) {
        let Some(y) = self.rows.tap(row) else {
            out.fill([0; 3]);
            return;
        };
        if self.identity {
            // Every tap has weight 0 here; reading the bytes straight gives the same values a few times faster.
            let line = &self.source[y.near..][..out.len() * 4];
            let [r, g, b] = self.offsets;
            for (rgb, pixel) in out.iter_mut().zip(line.as_chunks::<4>().0) {
                *rgb = [
                    u32::from(pixel[r]),
                    u32::from(pixel[g]),
                    u32::from(pixel[b]),
                ];
            }
            return;
        }
        let (near, far) = (&self.source[y.near..], &self.source[y.far..]);
        for (column, rgb) in out.iter_mut().enumerate() {
            let Some(x) = self.columns.tap(column) else {
                *rgb = [0; 3];
                continue;
            };
            for (channel, value) in rgb.iter_mut().enumerate() {
                let byte = self.offsets[channel];
                let lerp = |line: &[u8]| {
                    let a = u32::from(line[x.near + byte]);
                    if x.weight == 0 {
                        return a << 8;
                    }
                    a * (256 - x.weight) + u32::from(line[x.far + byte]) * x.weight
                };
                let upper = lerp(near);
                *value = if y.weight == 0 {
                    (upper + 128) >> 8
                } else {
                    (upper * (256 - y.weight) + lerp(far) * y.weight + 32_768) >> 16
                };
            }
        }
    }
}

fn luma([r, g, b]: [u32; 3]) -> u8 {
    (((66 * r + 129 * g + 25 * b + 128) >> 8) + 16) as u8
}

fn chroma([r, g, b]: [u32; 3]) -> (u8, u8) {
    let (r, g, b) = (r as i32, g as i32, b as i32);
    let u = ((-38 * r - 74 * g + 112 * b + 128) >> 8) + 128;
    let v = ((112 * r - 94 * g - 18 * b + 128) >> 8) + 128;
    (u.clamp(0, 255) as u8, v.clamp(0, 255) as u8)
}

/// Interleaves tight I420 planes into NV12, or back. Adapted from Serein's `screen.rs` (`i420_to_nv12`).
pub(crate) fn i420_to_nv12(i420: &[u8], width: usize, height: usize) -> Result<Vec<u8>> {
    let (luma, chroma) = planes(i420, width, height)?;
    let (u, v) = chroma.split_at(chroma.len() / 2);
    let mut out = Vec::with_capacity(i420.len());
    out.extend_from_slice(luma);
    out.extend(u.iter().zip(v).flat_map(|(&u, &v)| [u, v]));
    Ok(out)
}

pub(crate) fn nv12_to_i420(nv12: &[u8], width: usize, height: usize) -> Result<Vec<u8>> {
    let (luma, chroma) = planes(nv12, width, height)?;
    let mut out = Vec::with_capacity(nv12.len());
    out.extend_from_slice(luma);
    out.extend(chroma.chunks_exact(2).map(|pair| pair[0]));
    out.extend(chroma.chunks_exact(2).map(|pair| pair[1]));
    Ok(out)
}

fn planes(data: &[u8], width: usize, height: usize) -> Result<(&[u8], &[u8])> {
    if width == 0
        || height == 0
        || !width.is_multiple_of(2)
        || !height.is_multiple_of(2)
        || data.len() != width * height * 3 / 2
    {
        return Err(Error::invalid("4:2:0 planes do not match the picture size"));
    }
    Ok(data.split_at(width * height))
}

/// Box-filter `frame` down to fit `max_width` x `max_height`, keeping the aspect ratio and never scaling up. Each
/// destination pixel averages the source pixels it covers, so a thumbnail a tenth the size does not alias.
pub(crate) fn downscale_rgba(frame: Raw<'_>, max_width: u32, max_height: u32) -> Result<Picture> {
    frame.validate()?;
    let (width, height) = fit_size(
        frame.width as u32,
        frame.height as u32,
        max_width,
        max_height,
    );
    let (width, height) = (width as usize, height as usize);
    let [r, g, b] = frame.order.offsets();
    let mut out = vec![0u8; width * height * 4];
    for (y, row) in out.chunks_exact_mut(width * 4).enumerate() {
        let y0 = y * frame.height / height;
        let y1 = ((y + 1) * frame.height / height).max(y0 + 1);
        for (x, pixel) in row.chunks_exact_mut(4).enumerate() {
            let x0 = x * frame.width / width;
            let x1 = ((x + 1) * frame.width / width).max(x0 + 1);
            let mut sum = [0u64; 3];
            for sy in y0..y1 {
                let line = &frame.data[sy * frame.stride..];
                for sx in x0..x1 {
                    let at = sx * 4;
                    sum[0] += u64::from(line[at + r]);
                    sum[1] += u64::from(line[at + g]);
                    sum[2] += u64::from(line[at + b]);
                }
            }
            let count = ((y1 - y0) * (x1 - x0)) as u64;
            pixel.copy_from_slice(&[
                ((sum[0] + count / 2) / count) as u8,
                ((sum[1] + count / 2) / count) as u8,
                ((sum[2] + count / 2) / count) as u8,
                255,
            ]);
        }
    }
    Picture::rgba(width as u32, height as u32, out)
        .ok_or_else(|| Error::failed("downscaled picture has the wrong size"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solid(width: usize, height: usize, stride: usize, bgra: [u8; 4]) -> Vec<u8> {
        let mut data = vec![0xee; stride * height];
        for row in data.chunks_exact_mut(stride) {
            for pixel in row[..width * 4].chunks_exact_mut(4) {
                pixel.copy_from_slice(&bgra);
            }
        }
        data
    }

    fn raw(data: &[u8], width: usize, height: usize, stride: usize, order: PixelOrder) -> Raw<'_> {
        Raw {
            width,
            height,
            stride,
            order,
            data,
        }
    }

    #[test]
    fn scales_letterboxes_and_matches_bt601_limited_range() {
        // Padded 4:3 red source into 16:9: a red picture between black pillars, in both layouts.
        let data = solid(64, 48, 64 * 4 + 12, [0, 0, 255, 255]);
        let frame = raw(&data, 64, 48, 64 * 4 + 12, PixelOrder::Bgra);
        for chroma in [Chroma::Interleaved, Chroma::Planar] {
            let (width, height) = (128, 72);
            let mut out = vec![0; width * height * 3 / 2];
            to_yuv420(frame, width, height, chroma, &mut out).unwrap();
            let (y, uv) = out.split_at(width * height);
            assert_eq!(y[36 * width + 64], 82);
            assert_eq!(y[36 * width], 16);
            let (u, v) = match chroma {
                Chroma::Interleaved => (uv[18 * width + 64], uv[18 * width + 65]),
                Chroma::Planar => (uv[18 * 64 + 32], uv[width * height / 4 + 18 * 64 + 32]),
            };
            assert_eq!((u, v), (90, 240));
            let black = match chroma {
                Chroma::Interleaved => (uv[18 * width], uv[18 * width + 1]),
                Chroma::Planar => (uv[18 * 64], uv[width * height / 4 + 18 * 64]),
            };
            assert_eq!(black, (128, 128));
        }
    }

    #[test]
    fn rgba_and_bgra_sources_give_the_same_picture() {
        let bgra = solid(8, 8, 32, [10, 200, 30, 255]);
        let rgba = solid(8, 8, 32, [30, 200, 10, 255]);
        let (mut a, mut b) = (vec![0; 96], vec![0; 96]);
        to_yuv420(
            raw(&bgra, 8, 8, 32, PixelOrder::Bgra),
            8,
            8,
            Chroma::Planar,
            &mut a,
        )
        .unwrap();
        to_yuv420(
            raw(&rgba, 8, 8, 32, PixelOrder::Rgba),
            8,
            8,
            Chroma::Planar,
            &mut b,
        )
        .unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn identity_size_keeps_a_sharp_edge_and_bad_sizes_fail() {
        let mut data = solid(4, 2, 16, [0, 0, 0, 255]);
        data[4..8].copy_from_slice(&[255, 255, 255, 255]);
        let frame = raw(&data, 4, 2, 16, PixelOrder::Bgra);
        let mut out = vec![0; 12];
        to_yuv420(frame, 4, 2, Chroma::Planar, &mut out).unwrap();
        assert_eq!(&out[..4], &[16, 235, 16, 16]);
        assert!(to_yuv420(frame, 3, 2, Chroma::Planar, &mut out).is_err());
        assert!(to_yuv420(frame, 4, 2, Chroma::Planar, &mut out[..11]).is_err());
        // A buffer shorter than stride * height is refused instead of read past.
        assert!(
            to_yuv420(
                raw(&data[..20], 4, 2, 16, PixelOrder::Bgra),
                4,
                2,
                Chroma::Planar,
                &mut out
            )
            .is_err()
        );
    }

    #[test]
    fn banded_conversion_equals_a_single_band() {
        let (width, height) = (320, 180);
        let mut data = solid(333, 250, 333 * 4, [0; 4]);
        for (index, byte) in data.iter_mut().enumerate() {
            *byte = (index * 31 % 251) as u8;
        }
        let frame = raw(&data, 333, 250, 333 * 4, PixelOrder::Bgra);
        let mut banded = vec![0; width * height * 3 / 2];
        to_yuv420(frame, width, height, Chroma::Interleaved, &mut banded).unwrap();
        let columns = Axis::new(333, 240, width, 4);
        let rows = Axis::new(250, 180, height, frame.stride);
        let mut single = vec![0; width * height * 3 / 2];
        let (luma, uv) = single.split_at_mut(width * height);
        Band {
            source: &data,
            identity: false,
            offsets: PixelOrder::Bgra.offsets(),
            columns: &columns,
            rows: &rows,
            width,
            first: 0,
            luma,
            chroma: Out::Interleaved(uv),
        }
        .run();
        assert!(banded == single);
    }

    #[test]
    fn the_same_size_fast_path_equals_the_scaling_path() {
        let (width, height) = (256, 130);
        let mut data = solid(width, height, width * 4, [0; 4]);
        for (index, byte) in data.iter_mut().enumerate() {
            *byte = (index * 37 % 253) as u8;
        }
        for chroma in [Chroma::Interleaved, Chroma::Planar] {
            let frame = raw(&data, width, height, width * 4, PixelOrder::Rgba);
            let mut fast = vec![0; width * height * 3 / 2];
            to_yuv420(frame, width, height, chroma, &mut fast).unwrap();
            let columns = Axis::new(width, width, width, 4);
            let rows = Axis::new(height, height, height, frame.stride);
            let mut slow = vec![0; width * height * 3 / 2];
            let (luma, rest) = slow.split_at_mut(width * height);
            let out = match chroma {
                Chroma::Interleaved => Out::Interleaved(rest),
                Chroma::Planar => {
                    let (u, v) = rest.split_at_mut(width * height / 4);
                    Out::Planar(u, v)
                }
            };
            Band {
                source: &data,
                identity: false,
                offsets: PixelOrder::Rgba.offsets(),
                columns: &columns,
                rows: &rows,
                width,
                first: 0,
                luma,
                chroma: out,
            }
            .run();
            assert!(fast == slow, "{chroma:?}");
        }
    }

    #[test]
    fn planar_shuffles_round_trip() {
        let mut nv12 = [0; 12];
        nv12[..8].copy_from_slice(&[1, 2, 3, 4, 5, 6, 7, 8]);
        nv12[8..].copy_from_slice(&[9, 11, 10, 12]);
        let i420 = nv12_to_i420(&nv12, 4, 2).unwrap();
        assert_eq!(i420, [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12]);
        assert_eq!(i420_to_nv12(&i420, 4, 2).unwrap(), nv12);
        assert!(nv12_to_i420(&nv12[..11], 4, 2).is_err());
        assert!(i420_to_nv12(&i420, 3, 2).is_err());
    }

    #[test]
    fn fit_size_keeps_aspect_and_never_scales_up() {
        assert_eq!(fit_size(1920, 1080, 320, 320), (320, 180));
        assert_eq!(fit_size(1080, 1920, 320, 320), (180, 320));
        assert_eq!(fit_size(100, 50, 320, 320), (100, 50));
        assert_eq!(fit_size(3840, 2160, 1, 1), (1, 1));
        assert_eq!(fit_size(0, 5, 10, 10), (1, 1));
        assert_eq!(fit_size(10000, 10, 100, 100), (100, 1));
    }

    #[test]
    fn thumbnails_average_the_pixels_they_cover() {
        // A 4x2 BGRA picture, left half black and right half white, halved to 2x1 and quartered to 1x1.
        let mut data = vec![0u8; 4 * 2 * 4];
        for y in 0..2 {
            for x in 2..4 {
                data[(y * 4 + x) * 4..][..4].copy_from_slice(&[255, 255, 255, 255]);
            }
        }
        let frame = raw(&data, 4, 2, 16, PixelOrder::Bgra);
        let half = downscale_rgba(frame, 2, 1).unwrap();
        assert_eq!((half.width, half.height), (2, 1));
        assert_eq!(half.data, [0, 0, 0, 255, 255, 255, 255, 255]);
        let one = downscale_rgba(frame, 1, 1).unwrap();
        assert_eq!(one.data, [128, 128, 128, 255]);
        // Channels land in RGBA order whatever the source order was.
        let blue_bgra = vec![255, 0, 0, 255];
        let pixel = downscale_rgba(raw(&blue_bgra, 1, 1, 4, PixelOrder::Bgra), 8, 8).unwrap();
        assert_eq!(pixel.data, [0, 0, 255, 255]);
    }

    #[test]
    fn vec_conversion_matches_slice_path_and_preserves_caller_buffer() {
        let (width, height) = (512, 288);
        let mut owned = solid(width, height, width * 4, [40, 120, 200, 255]);
        let ptr = owned.as_ptr();
        let cap = owned.capacity();
        let mut expected = vec![0; width * height * 3 / 2];
        let mut actual = vec![0; width * height * 3 / 2];
        for chroma in [Chroma::Interleaved, Chroma::Planar] {
            let frame = raw(&owned, width, height, width * 4, PixelOrder::Bgra);
            to_yuv420(frame, width, height, chroma, &mut expected).unwrap();
            to_yuv420_vec(
                &mut owned,
                SourceMeta {
                    width,
                    height,
                    stride: width * 4,
                    order: PixelOrder::Bgra,
                },
                width,
                height,
                chroma,
                &mut actual,
            )
            .unwrap();
            assert_eq!(owned.as_ptr(), ptr);
            assert_eq!(owned.capacity(), cap);
            assert!(actual == expected, "{chroma:?}");
        }
    }
}
