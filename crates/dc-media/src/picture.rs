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

    #[test]
    fn strided_planes_pack_tightly() {
        let mut out = Vec::new();
        pack_plane(&mut out, &[1, 2, 0, 3, 4], 3, 2, 2).unwrap();
        assert_eq!(out, [1, 2, 3, 4]);
        assert!(pack_plane(&mut out, &[1, 2, 0, 3], 3, 2, 2).is_none());
        assert!(pack_plane(&mut out, &[1, 2], 1, 2, 1).is_none());
    }
}
