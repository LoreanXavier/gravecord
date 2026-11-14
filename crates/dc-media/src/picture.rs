//! Pictures in the layout a decoder or capture API hands over. Planar YUV stays planar so a renderer can convert colour
//! on the GPU; [`Picture::to_rgba`] is the CPU path for the preview, tests and thumbnails.
//!
//! Colour is BT.601 limited range (luma 16-235, chroma 16-240) everywhere: that is what WebRTC assumes for camera and
//! Go Live H.264 and what Serein converts with. Adapted from Serein's `crates/platform/src/video/picture.rs` (MIT OR
//! Apache-2.0), with the matrix and range fixed instead of carried per picture. The encoders here write no VUI colour
//! description, so a viewer makes the same assumption.

/// Plane layout of [`Picture::data`]; every plane is tightly packed, row after row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Layout {
    /// One plane of 4-byte RGBA pixels; alpha is opaque.
    Rgba,
    /// Full-size luma, then half-size interleaved Cb/Cr pairs.
    Nv12,
    /// Full-size luma, then half-size Cb, then half-size Cr.
    I420,
}

impl Layout {
    /// Exact byte length of a `width` x `height` picture, or `None` on overflow.
    pub fn len(self, width: u32, height: u32) -> Option<usize> {
        let (width, height) = (width as usize, height as usize);
        let luma = width.checked_mul(height)?;
        let chroma = width.div_ceil(2).checked_mul(height.div_ceil(2))?;
        match self {
            Self::Rgba => luma.checked_mul(4),
            Self::Nv12 | Self::I420 => luma.checked_add(chroma.checked_mul(2)?),
        }
    }
}

/// One picture. `data` holds the planes described by `layout`, with no row padding.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Picture {
    pub width: u32,
    pub height: u32,
    pub layout: Layout,
    pub data: Vec<u8>,
}

/// BT.601 limited range, as `rgb = row.xyz . [y, cb, cr] + row.w` over 0-1 samples.
const COEFFICIENTS: [[f32; 4]; 3] = {
    const KR: f32 = 0.299;
    const KB: f32 = 0.114;
    const KG: f32 = 1.0 - KR - KB;
    const LUMA_SCALE: f32 = 255.0 / 219.0;
    const LUMA_OFFSET: f32 = -16.0 / 219.0;
    const CHROMA_SCALE: f32 = 255.0 / 224.0;
    const CHROMA_OFFSET: f32 = -128.0 / 224.0;
    const CR_R: f32 = 2.0 * (1.0 - KR);
    const CB_B: f32 = 2.0 * (1.0 - KB);
    const CB_G: f32 = 2.0 * KB * (1.0 - KB) / KG;
    const CR_G: f32 = 2.0 * KR * (1.0 - KR) / KG;
    [
        [
            LUMA_SCALE,
            0.0,
            CR_R * CHROMA_SCALE,
            LUMA_OFFSET + CR_R * CHROMA_OFFSET,
        ],
        [
            LUMA_SCALE,
            -CB_G * CHROMA_SCALE,
            -CR_G * CHROMA_SCALE,
            LUMA_OFFSET - (CB_G + CR_G) * CHROMA_OFFSET,
        ],
        [
            LUMA_SCALE,
            CB_B * CHROMA_SCALE,
            0.0,
            LUMA_OFFSET + CB_B * CHROMA_OFFSET,
        ],
    ]
};

impl Picture {
    /// An RGBA picture, or `None` when `data` is not exactly `width * height * 4` bytes.
    pub fn rgba(width: u32, height: u32, data: Vec<u8>) -> Option<Self> {
        let picture = Self {
            width,
            height,
            layout: Layout::Rgba,
            data,
        };
        picture.is_valid().then_some(picture)
    }

    /// Dimensions and plane length agree with each other.
    pub fn is_valid(&self) -> bool {
        self.width > 0
            && self.height > 0
            && self.layout.len(self.width, self.height) == Some(self.data.len())
    }

    /// Packed RGBA pixels, or `None` for an invalid picture.
    ///
    /// NV12 and I420 chroma is interpolated bilinearly at each pixel centre with clamped edges, the same taps a GPU
    /// sampler uses, so a renderer converting on the GPU and this path agree up to rounding.
    pub fn to_rgba(&self) -> Option<Vec<u8>> {
        if !self.is_valid() {
            return None;
        }
        let (width, height) = (self.width as usize, self.height as usize);
        if self.layout == Layout::Rgba {
            return Some(self.data.clone());
        }
        let (chroma_width, chroma_height) = (width.div_ceil(2), height.div_ceil(2));
        let (luma, chroma) = self.data.split_at(width * height);
        let (first, second) = chroma.split_at(chroma_width * chroma_height);
        let fetch = |x: usize, y: usize| {
            let at = y * chroma_width + x;
            match self.layout {
                Layout::Nv12 => [chroma[at * 2], chroma[at * 2 + 1]],
                _ => [first[at], second[at]],
            }
            .map(f32::from)
        };
        let columns = chroma_taps(width, chroma_width);
        let lines = chroma_taps(height, chroma_height);
        let mut rgba = vec![0; width * height * 4];
        for (y, row) in rgba.chunks_exact_mut(width * 4).enumerate() {
            let (top, bottom, down) = lines[y];
            for (x, pixel) in row.chunks_exact_mut(4).enumerate() {
                let (left, right, across) = columns[x];
                let blend = |a: [f32; 2], b: [f32; 2], t: f32| {
                    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t]
                };
                let [cb, cr] = blend(
                    blend(fetch(left, top), fetch(right, top), across),
                    blend(fetch(left, bottom), fetch(right, bottom), across),
                    down,
                );
                let sample = [
                    f32::from(luma[y * width + x]) / 255.0,
                    cb / 255.0,
                    cr / 255.0,
                ];
                let channel = |row: [f32; 4]| {
                    let value =
                        row[0] * sample[0] + row[1] * sample[1] + row[2] * sample[2] + row[3];
                    (value * 255.0).round().clamp(0.0, 255.0) as u8
                };
                pixel.copy_from_slice(&[
                    channel(COEFFICIENTS[0]),
                    channel(COEFFICIENTS[1]),
                    channel(COEFFICIENTS[2]),
                    255,
                ]);
            }
        }
        Some(rgba)
    }
}

/// [`COEFFICIENTS`] in 14-bit fixed point for the integer path of [`Picture::write_bgra`]: luma scale, Cr into red, Cb
/// into blue, Cb and Cr out of green.
const FIXED_LUMA: i32 = 19_077;
const FIXED_CR_R: i32 = 26_149;
const FIXED_CB_B: i32 = 33_050;
const FIXED_CB_G: i32 = 6_419;
const FIXED_CR_G: i32 = 13_320;

impl Picture {
    /// Packed BGRA pixels (what gpui's atlas takes), or `None` for an invalid picture. Same colours as
    /// [`Picture::to_rgba`] within one level, at a fraction of its cost: see [`Picture::write_bgra`].
    pub fn to_bgra(&self) -> Option<Vec<u8>> {
        let mut out = vec![0; Layout::Rgba.len(self.width, self.height)?];
        self.write_bgra(&mut out)?;
        Some(out)
    }

    /// Writes the picture as packed BGRA into `out` (exactly `width * height * 4` bytes). `None` for an invalid picture
    /// or a wrong `out` length.
    ///
    /// Even-sized NV12 and I420 pictures (every picture a decoder hands over) take an integer path split into row bands
    /// across a few threads; the chroma taps are the bilinear ones of `to_rgba` (weights 1/4 and 3/4 at a 2:1 ratio),
    /// so the result is the same up to the rounding of the fixed-point matrix. A 2560x1440 picture costs about a
    /// tenth of `to_rgba` plus the byte swap the call view used to do. Odd sizes go through `to_rgba`.
    pub fn write_bgra(&self, out: &mut [u8]) -> Option<()> {
        if !self.is_valid() {
            return None;
        }
        let (width, height) = (self.width as usize, self.height as usize);
        if out.len() != width * height * 4 {
            return None;
        }
        if self.layout == Layout::Rgba {
            for (to, from) in out
                .as_chunks_mut::<4>()
                .0
                .iter_mut()
                .zip(self.data.as_chunks::<4>().0)
            {
                *to = [from[2], from[1], from[0], from[3]];
            }
            return Some(());
        }
        if !width.is_multiple_of(2) || !height.is_multiple_of(2) {
            let rgba = self.to_rgba()?;
            for (to, from) in out
                .as_chunks_mut::<4>()
                .0
                .iter_mut()
                .zip(rgba.as_chunks::<4>().0)
            {
                *to = [from[2], from[1], from[0], from[3]];
            }
            return Some(());
        }
        let luma_len = width * height;
        let interleaved = self.layout == Layout::Nv12;
        let bands = crate::band_pool::band_count(height);
        let band_rows = height.div_ceil(bands);
        let mut worker_tasks = Vec::with_capacity(bands.saturating_sub(1));
        let mut worker_targets = Vec::with_capacity(bands.saturating_sub(1));
        let mut caller: Option<(usize, &mut [u8])> = None;

        let mut rest = out;
        let mut first = 0;
        while first < height {
            let count = band_rows.min(height - first);
            let (band, tail) = std::mem::take(&mut rest).split_at_mut(count * width * 4);
            rest = tail;
            if first + count >= height {
                caller = Some((first, band));
            } else {
                worker_targets.push(band);
                worker_tasks.push(crate::band_pool::Task::Bgra(BgraBand {
                    width,
                    height,
                    interleaved,
                    first,
                    rows: count,
                }));
            }
            first += count;
        }
        let (caller_first, caller_band) = caller?;
        crate::band_pool::dispatch(
            crate::band_pool::Input::Slice(&self.data),
            worker_tasks,
            |source, scratch| {
                Planes {
                    luma: &source[..luma_len],
                    chroma: &source[luma_len..],
                    width,
                    height,
                    interleaved,
                }
                .rows_into(caller_first, caller_band, &mut scratch.chroma_line);
            },
            |index, bytes| worker_targets[index].copy_from_slice(bytes),
        );
        Some(())
    }
}

/// Geometry descriptor for one worker row band of BGRA conversion.
pub(crate) struct BgraBand {
    pub(crate) width: usize,
    pub(crate) height: usize,
    pub(crate) interleaved: bool,
    pub(crate) first: usize,
    pub(crate) rows: usize,
}

impl BgraBand {
    pub(crate) fn out_len(&self) -> usize {
        self.rows * self.width * 4
    }
}

pub(crate) fn run_band(
    source: &[u8],
    out: &mut [u8],
    scratch: &mut crate::band_pool::Scratch,
    job: &BgraBand,
) {
    let luma_len = job.width * job.height;
    Planes {
        luma: &source[..luma_len],
        chroma: &source[luma_len..],
        width: job.width,
        height: job.height,
        interleaved: job.interleaved,
    }
    .rows_into(job.first, out, &mut scratch.chroma_line);
}

/// The planes of an even-sized 4:2:0 picture, for the banded BGRA conversion.
struct Planes<'a> {
    luma: &'a [u8],
    chroma: &'a [u8],
    width: usize,
    height: usize,
    interleaved: bool,
}

impl Planes<'_> {
    /// Cb and Cr of chroma column `x` on chroma row `y`.
    fn sample(&self, x: usize, y: usize) -> (i32, i32) {
        let chroma_width = self.width / 2;
        if self.interleaved {
            let at = (y * chroma_width + x) * 2;
            (i32::from(self.chroma[at]), i32::from(self.chroma[at + 1]))
        } else {
            let plane = chroma_width * (self.height / 2);
            let at = y * chroma_width + x;
            (
                i32::from(self.chroma[at]),
                i32::from(self.chroma[plane + at]),
            )
        }
    }

    /// Converts the `out.len() / (width * 4)` rows from `first` on using caller-provided scratch storage.
    fn rows_into(&self, first: usize, out: &mut [u8], line: &mut Vec<(i32, i32)>) {
        let (width, chroma_width, chroma_height) = (self.width, self.width / 2, self.height / 2);
        // The vertically blended chroma row, times 4.
        line.resize(chroma_width, (0, 0));
        for (row, pixels) in out.chunks_exact_mut(width * 4).enumerate() {
            let y = first + row;
            let k = y / 2;
            // Pixel centre y maps to chroma position y / 2 - 1/4: rows (k - 1, k) at 1:3 for even y, (k, k + 1) at
            // 3:1 for odd y, clamped at the edges.
            let (near, far) = if y.is_multiple_of(2) {
                (k, k.saturating_sub(1))
            } else {
                (k, (k + 1).min(chroma_height - 1))
            };
            for (x, blended) in line.iter_mut().enumerate() {
                let (a, b) = self.sample(x, near);
                let (c, d) = self.sample(x, far);
                *blended = (3 * a + c, 3 * b + d);
            }
            let luma = &self.luma[y * width..][..width];
            for (x, (pixel, &value)) in pixels
                .as_chunks_mut::<4>()
                .0
                .iter_mut()
                .zip(luma)
                .enumerate()
            {
                let j = x / 2;
                let other = if x.is_multiple_of(2) {
                    j.saturating_sub(1)
                } else {
                    (j + 1).min(chroma_width - 1)
                };
                // Chroma times 16, centred.
                let cb = 3 * line[j].0 + line[other].0 - 128 * 16;
                let cr = 3 * line[j].1 + line[other].1 - 128 * 16;
                let luma = (i32::from(value) - 16) * FIXED_LUMA + (1 << 13);
                let channel = |sum: i32| (sum >> 14).clamp(0, 255) as u8;
                *pixel = [
                    channel(luma + ((cb * FIXED_CB_B) >> 4)),
                    channel(luma - ((cb * FIXED_CB_G + cr * FIXED_CR_G) >> 4)),
                    channel(luma + ((cr * FIXED_CR_R) >> 4)),
                    255,
                ];
            }
        }
    }
}

/// Bilinear taps (low, high, weight of high) mapping each of `output` pixel centres onto a `source`-texel plane,
/// clamped to its edges like a GPU sampler.
fn chroma_taps(output: usize, source: usize) -> Vec<(usize, usize, f32)> {
    (0..output)
        .map(|index| {
            let position = (index as f32 + 0.5) * source as f32 / output as f32 - 0.5;
            let base = position.floor();
            let clamp = |value: f32| (value.max(0.0) as usize).min(source - 1);
            (clamp(base), clamp(base + 1.0), position - base)
        })
        .collect()
}

/// Copies one plane out of a strided buffer into `out`, `row_bytes` per row.
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) fn pack_plane(
    out: &mut Vec<u8>,
    source: &[u8],
    stride: usize,
    row_bytes: usize,
    rows: usize,
) -> Option<()> {
    if stride < row_bytes || rows == 0 {
        return (rows == 0).then_some(());
    }
    let needed = stride.checked_mul(rows - 1)?.checked_add(row_bytes)?;
    let source = source.get(..needed)?;
    if stride == row_bytes {
        out.extend_from_slice(source);
    } else {
        for row in 0..rows {
            out.extend_from_slice(&source[row * stride..][..row_bytes]);
        }
    }
    Some(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flat(width: u32, height: u32, layout: Layout, y: u8, cb: u8, cr: u8) -> Picture {
        let luma = (width * height) as usize;
        let chroma = (width.div_ceil(2) * height.div_ceil(2)) as usize;
        let mut data = vec![y; luma];
        match layout {
            Layout::Nv12 => data.extend((0..chroma).flat_map(|_| [cb, cr])),
            _ => {
                data.extend(std::iter::repeat_n(cb, chroma));
                data.extend(std::iter::repeat_n(cr, chroma));
            }
        }
        Picture {
            width,
            height,
            layout,
            data,
        }
    }

    fn first_pixel(picture: &Picture) -> [u8; 4] {
        picture.to_rgba().unwrap()[..4].try_into().unwrap()
    }

    /// Known BT.601 limited-range studio values (the colour-bar references) against full-range RGB. Rounding in the
    /// 8-bit studio values allows a couple of levels.
    #[test]
    fn bt601_limited_range_matches_the_reference_colours() {
        let cases = [
            ((16, 128, 128), [0, 0, 0]),
            ((235, 128, 128), [255, 255, 255]),
            ((126, 128, 128), [128, 128, 128]),
            ((82, 90, 240), [255, 0, 0]),
            ((145, 54, 34), [0, 255, 0]),
            ((41, 240, 110), [0, 0, 255]),
            ((210, 16, 146), [255, 255, 0]),
            ((170, 166, 16), [0, 255, 255]),
            ((106, 202, 222), [255, 0, 255]),
        ];
        for layout in [Layout::Nv12, Layout::I420] {
            for ((y, cb, cr), want) in cases {
                let got = first_pixel(&flat(4, 4, layout, y, cb, cr));
                for channel in 0..3 {
                    assert!(
                        got[channel].abs_diff(want[channel]) <= 3,
                        "{layout:?} yuv {y},{cb},{cr}: got {got:?} want {want:?}"
                    );
                }
                assert_eq!(got[3], 255);
            }
        }
    }

    #[test]
    fn out_of_range_samples_clamp_and_rgba_passes_through() {
        // Below black and above white in limited range clamp to the ends instead of wrapping.
        assert_eq!(
            &first_pixel(&flat(2, 2, Layout::I420, 0, 128, 128))[..3],
            &[0, 0, 0]
        );
        assert_eq!(
            &first_pixel(&flat(2, 2, Layout::I420, 255, 128, 128))[..3],
            &[255, 255, 255]
        );
        let rgba = Picture::rgba(1, 2, vec![1, 2, 3, 255, 4, 5, 6, 255]).unwrap();
        assert_eq!(rgba.to_rgba().unwrap(), rgba.data);
    }

    #[test]
    fn nv12_and_i420_agree_and_odd_sizes_are_sized_right() {
        // 3x1: two chroma columns, one chroma row.
        let nv12 = Picture {
            width: 3,
            height: 1,
            layout: Layout::Nv12,
            data: vec![16, 126, 235, 128, 128, 90, 240],
        };
        let i420 = Picture {
            layout: Layout::I420,
            data: vec![16, 126, 235, 128, 90, 128, 240],
            ..nv12.clone()
        };
        assert!(nv12.is_valid() && i420.is_valid());
        assert_eq!(nv12.to_rgba().unwrap(), i420.to_rgba().unwrap());
        assert_eq!(&nv12.to_rgba().unwrap()[..4], &[0, 0, 0, 255]);
        let short = Picture {
            data: vec![0; 6],
            ..nv12
        };
        assert!(!short.is_valid());
        assert_eq!(Layout::Rgba.len(u32::MAX, u32::MAX), None);
        assert!(Picture::rgba(2, 2, vec![0; 15]).is_none());
    }

    #[test]
    fn chroma_is_interpolated_like_a_gpu_sampler() {
        // 4x1 mid-grey luma over two chroma columns, Cb 64 then 192: pixel centres map to chroma positions -0.25, 0.25,
        // 0.75 and 1.25, so the edges clamp and the middle pixels blend 3:1 and 1:3.
        assert_eq!(
            chroma_taps(4, 2),
            vec![(0, 0, 0.75), (0, 1, 0.25), (0, 1, 0.75), (1, 1, 0.25)]
        );
        let picture = Picture {
            width: 4,
            height: 1,
            layout: Layout::I420,
            data: vec![128, 128, 128, 128, 64, 192, 128, 128],
        };
        let blue: Vec<u8> = picture
            .to_rgba()
            .unwrap()
            .chunks_exact(4)
            .map(|pixel| pixel[2])
            .collect();
        assert!(
            blue[0] < blue[1] && blue[1] < blue[2] && blue[2] < blue[3],
            "{blue:?}"
        );
    }

    /// The fast BGRA path agrees with `to_rgba` (channels swapped) within one level on busy pictures of both 4:2:0
    /// layouts, at the edges too, and handles RGBA and odd sizes through the exact path.
    #[test]
    fn bgra_matches_rgba_within_one_level() {
        let mut state = 0x1234_5678u32;
        let mut next = || {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            state as u8
        };
        for (width, height) in [(2u32, 2u32), (8, 6), (130, 66), (320, 180)] {
            let len = Layout::I420.len(width, height).unwrap();
            let data: Vec<u8> = (0..len).map(|_| next()).collect();
            for layout in [Layout::Nv12, Layout::I420] {
                let picture = Picture {
                    width,
                    height,
                    layout,
                    data: data.clone(),
                };
                let rgba = picture.to_rgba().unwrap();
                let bgra = picture.to_bgra().unwrap();
                for (index, (a, b)) in rgba
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .zip(bgra.as_chunks::<4>().0)
                    .enumerate()
                {
                    for (channel, swapped) in [(0, 2), (1, 1), (2, 0), (3, 3)] {
                        assert!(
                            a[channel].abs_diff(b[swapped]) <= 1,
                            "{width}x{height} {layout:?} pixel {index}: rgba {a:?} bgra {b:?}"
                        );
                    }
                }
            }
        }
        let odd = flat(3, 3, Layout::I420, 82, 90, 240);
        assert_eq!(&odd.to_bgra().unwrap()[..4], &{
            let p = first_pixel(&odd);
            [p[2], p[1], p[0], p[3]]
        });
        let rgba = Picture::rgba(1, 1, vec![1, 2, 3, 255]).unwrap();
        assert_eq!(rgba.to_bgra().unwrap(), [3, 2, 1, 255]);
        let mut short = vec![0; 3];
        assert!(rgba.write_bgra(&mut short).is_none());
    }

    #[test]
    fn strided_planes_pack_tightly() {
        let mut out = Vec::new();
        pack_plane(&mut out, &[1, 2, 0, 3, 4], 3, 2, 2).unwrap();
        assert_eq!(out, [1, 2, 3, 4]);
        assert!(pack_plane(&mut out, &[1, 2, 0, 3], 3, 2, 2).is_none());
        assert!(pack_plane(&mut out, &[1, 2], 1, 2, 1).is_none());
    }
}
