//! Annex B inspection shared by the encoders, the decoder and the callers that packetize.
//!
//! The NAL walk and the IDR / parameter-set checks are adapted from Serein's `video_codec.rs` and `video.rs`
//! (`crates/discord-voice/src`, MIT OR Apache-2.0), reduced to H.264.

/// Largest access unit any live entry point here accepts, in bytes.
pub const MAX_ACCESS_UNIT: usize = 2 * 1024 * 1024;

/// Largest access unit accepted when decoding a local/VOD MP4 sample (1080p/4K IDR frames can exceed 2 MiB).
pub const MAX_VOD_ACCESS_UNIT: usize = 32 * 1024 * 1024;

/// NAL unit types the callers care about.
pub const NAL_IDR: u8 = 5;
pub const NAL_SPS: u8 = 7;
pub const NAL_PPS: u8 = 8;

/// The NAL units of an Annex B buffer, each without its start code. Four-byte and three-byte start codes both work and
/// the zero bytes before the next start code are trimmed. Bytes before the first start code are ignored.
pub fn nals(data: &[u8]) -> impl Iterator<Item = &[u8]> {
    let mut starts = Vec::new();
    let mut at = 0;
    while at + 3 <= data.len() {
        if data[at] == 0 && data[at + 1] == 0 && data[at + 2] == 1 {
            starts.push(at + 3);
            at += 3;
        } else {
            at += 1;
        }
    }
    let ends: Vec<usize> = starts
        .iter()
        .skip(1)
        .map(|&next| next - 3)
        .chain(std::iter::once(data.len()))
        .collect();
    starts
        .into_iter()
        .zip(ends)
        .filter_map(move |(start, end)| {
            let mut nal = &data[start..end.max(start)];
            while let [rest @ .., 0] = nal {
                nal = rest;
            }
            (!nal.is_empty()).then_some(nal)
        })
}

/// The NAL unit type (low five bits of its first byte) of each unit in order.
pub fn nal_types(data: &[u8]) -> Vec<u8> {
    nals(data).map(|nal| nal[0] & 31).collect()
}

/// Whether the access unit holds an IDR slice, so a decoder can start from it.
pub fn is_keyframe(data: &[u8]) -> bool {
    nals(data).any(|nal| nal[0] & 31 == NAL_IDR)
}

/// Whether the access unit carries both an SPS and a PPS, so a freshly created decoder can start from it.
pub fn has_parameter_sets(data: &[u8]) -> bool {
    let mut found = 0u8;
    for nal in nals(data) {
        match nal[0] & 31 {
            NAL_SPS => found |= 1,
            NAL_PPS => found |= 2,
            _ => {}
        }
    }
    found == 3
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn walks_mixed_start_codes_and_trims_padding() {
        let data = [
            9, 9, // junk before the first start code
            0, 0, 0, 1, 0x67, 1, 2, // SPS
            0, 0, 1, 0x68, 3, // PPS
            0, 0, 0, 0, 1, 0x65, 4, 0, 5, 0, // IDR with an inner zero and trailing padding
        ];
        let units: Vec<&[u8]> = nals(&data).collect();
        assert_eq!(
            units,
            [&[0x67, 1, 2][..], &[0x68, 3][..], &[0x65, 4, 0, 5][..]]
        );
        assert_eq!(nal_types(&data), [7, 8, 5]);
        assert!(is_keyframe(&data));
        assert!(has_parameter_sets(&data));
    }

    #[test]
    fn inter_frames_and_partial_parameter_sets_are_not_starts() {
        let delta = [0, 0, 0, 1, 0x41, 1, 2, 3];
        assert!(!is_keyframe(&delta));
        assert!(!has_parameter_sets(&delta));
        let only_sps = [0, 0, 0, 1, 0x67, 1, 0, 0, 0, 1, 0x65, 2];
        assert!(is_keyframe(&only_sps));
        assert!(!has_parameter_sets(&only_sps));
        assert_eq!(nals(&[]).count(), 0);
        assert_eq!(nals(&[1, 2, 3, 4]).count(), 0);
        assert_eq!(nals(&[0, 0, 1]).count(), 0);
    }
}
