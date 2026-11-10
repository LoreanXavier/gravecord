//! Just enough of an H.264 sequence parameter set to describe the stream to an MP4 reader: the profile bytes, the
//! picture size after cropping and the chroma format (ITU-T H.264 section 7.3.2.1.1).

/// What the MP4 sample entry needs from an SPS.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SpsInfo {
    pub profile: u8,
    pub compatibility: u8,
    pub level: u8,
    pub width: u32,
    pub height: u32,
    pub chroma_format: u8,
    pub bit_depth_luma: u8,
    pub bit_depth_chroma: u8,
}

struct Bits {
    data: Vec<u8>,
    at: usize,
}

impl Bits {
    /// The RBSP of a NAL unit: the emulation prevention bytes (`00 00 03`) taken out.
    fn new(nal: &[u8]) -> Self {
        let mut data = Vec::with_capacity(nal.len());
        let mut zeros = 0;
        for &byte in nal {
            if zeros >= 2 && byte == 3 {
                zeros = 0;
                continue;
            }
            zeros = if byte == 0 { zeros + 1 } else { 0 };
            data.push(byte);
        }
        Self { data, at: 0 }
    }

    fn bit(&mut self) -> Option<u32> {
        let byte = *self.data.get(self.at / 8)?;
        let bit = (byte >> (7 - self.at % 8)) & 1;
        self.at += 1;
        Some(u32::from(bit))
    }

    fn bits(&mut self, count: u32) -> Option<u32> {
        let mut value = 0;
        for _ in 0..count {
            value = (value << 1) | self.bit()?;
        }
        Some(value)
    }

    fn ue(&mut self) -> Option<u32> {
        let mut zeros = 0;
        while self.bit()? == 0 {
            zeros += 1;
            if zeros > 31 {
                return None;
            }
        }
        let rest = self.bits(zeros)?;
        Some(((1u64 << zeros) - 1 + u64::from(rest)) as u32)
    }

    fn se(&mut self) -> Option<i32> {
        let k = self.ue()?;
        let magnitude = k.div_ceil(2) as i32;
        Some(if k % 2 == 1 { magnitude } else { -magnitude })
    }

    fn skip_scaling_list(&mut self, size: usize) -> Option<()> {
        let (mut last, mut next) = (8i32, 8i32);
        for _ in 0..size {
            if next != 0 {
                next = (last + self.se()? + 256) % 256;
            }
            if next != 0 {
                last = next;
            }
        }
        Some(())
    }
}

/// Parses the SPS NAL unit (with its one-byte header). `None` when it is malformed or cut short.
pub fn parse(nal: &[u8]) -> Option<SpsInfo> {
    if nal.len() < 4 || nal[0] & 31 != 7 {
        return None;
    }
    let (profile, compatibility, level) = (nal[1], nal[2], nal[3]);
    let mut r = Bits::new(&nal[4..]);
    r.ue()?; // seq_parameter_set_id
    let (mut chroma_format, mut luma, mut chroma, mut separate) = (1u32, 8u32, 8u32, false);
    if matches!(
        profile,
        100 | 110 | 122 | 244 | 44 | 83 | 86 | 118 | 128 | 138 | 139 | 134 | 135
    ) {
        chroma_format = r.ue()?;
        if chroma_format == 3 {
            separate = r.bit()? == 1;
        }
        luma = 8 + r.ue()?;
        chroma = 8 + r.ue()?;
        r.bit()?; // qpprime_y_zero_transform_bypass_flag
        if r.bit()? == 1 {
            let lists = if chroma_format != 3 { 8 } else { 12 };
            for index in 0..lists {
                if r.bit()? == 1 {
                    r.skip_scaling_list(if index < 6 { 16 } else { 64 })?;
                }
            }
        }
    }
    r.ue()?; // log2_max_frame_num_minus4
    match r.ue()? {
        0 => {
            r.ue()?;
        }
        1 => {
            r.bit()?;
            r.se()?;
            r.se()?;
            for _ in 0..r.ue()? {
                r.se()?;
            }
        }
        _ => {}
    }
    r.ue()?; // max_num_ref_frames
    r.bit()?; // gaps_in_frame_num_value_allowed_flag
    let width_mbs = r.ue()? + 1;
    let height_map_units = r.ue()? + 1;
    let frame_mbs_only = r.bit()?;
    if frame_mbs_only == 0 {
        r.bit()?; // mb_adaptive_frame_field_flag
    }
    r.bit()?; // direct_8x8_inference_flag
    let (mut left, mut right, mut top, mut bottom) = (0, 0, 0, 0);
    if r.bit()? == 1 {
        left = r.ue()?;
        right = r.ue()?;
        top = r.ue()?;
        bottom = r.ue()?;
    }
    let array = if separate { 0 } else { chroma_format };
    let (unit_x, unit_y) = match array {
        0 => (1, 1),
        1 => (2, 2),
        2 => (2, 1),
        _ => (1, 1),
    };
    let unit_y = unit_y * (2 - frame_mbs_only);
    let width = (width_mbs * 16).checked_sub((left + right) * unit_x)?;
    let height =
        (height_map_units * 16 * (2 - frame_mbs_only)).checked_sub((top + bottom) * unit_y)?;
    Some(SpsInfo {
        profile,
        compatibility,
        level,
        width,
        height,
        chroma_format: chroma_format as u8,
        bit_depth_luma: luma as u8,
        bit_depth_chroma: chroma as u8,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_size_of_an_encoder_stream_including_the_crop() {
        // 320 x 180 is not a multiple of 16 in height: the SPS crops 192 down to 180.
        for (width, height) in [(320, 180), (640, 360), (256, 144)] {
            let mut generator =
                dc_media::synthetic::Generator::new(width, height, 10, 400_000).unwrap();
            let frame = generator.next(true).unwrap().unwrap();
            let sps = dc_media::h264::nals(&frame.data)
                .find(|nal| nal[0] & 31 == 7)
                .expect("a keyframe carries its SPS");
            let info = parse(sps).unwrap();
            assert_eq!((info.width, info.height), (width, height));
            assert_eq!(info.chroma_format, 1);
            assert_eq!((info.bit_depth_luma, info.bit_depth_chroma), (8, 8));
        }
    }

    #[test]
    fn rejects_other_nal_types_and_truncation() {
        assert_eq!(parse(&[0x68, 1, 2, 3, 4]), None);
        assert_eq!(parse(&[0x67, 0x42, 0xC0, 0x1E]), None);
    }
}
