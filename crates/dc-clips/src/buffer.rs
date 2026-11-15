//! The rolling buffer: the last stretch of the stream's encoded video and audio, indexed by time.
//!
//! Video is kept as access units grouped by keyframe, so a window cut for a clip always starts on a picture a decoder
//! can start from. Audio is kept as Opus packets of 20 ms. Times are milliseconds on one steady clock ([`now_ms`]),
//! taken when a unit arrives from the encoder or the audio capture.
//!
//! The native module's own ring is not in the bundle (plan section 4); this one meets the observable contract: the
//! buffer holds `clipsLength` of the stream, a save cuts the newest window of that length.
use std::collections::VecDeque;
use std::sync::OnceLock;
use std::time::Instant;

/// Milliseconds on the steady clock the buffer and the metadata timeline use (the media engine's
/// `getSystemSteadyClockNowMs`, module 31048). The origin is the first call.
pub fn now_ms() -> u64 {
    static ORIGIN: OnceLock<Instant> = OnceLock::new();
    ORIGIN.get_or_init(Instant::now).elapsed().as_millis() as u64
}

/// The most video the buffer holds in bytes, whatever the length: a clip length of two minutes at the stream's top
/// bit rate fits well inside it.
pub const MAX_VIDEO_BYTES: usize = 384 * 1024 * 1024;

/// An SPS and a PPS NAL unit (without start codes).
pub type ParameterSets = (Vec<u8>, Vec<u8>);

#[derive(Clone, Debug)]
pub struct VideoFrame {
    /// When the access unit arrived.
    pub arrival_ms: u64,
    /// 90 kHz ticks since the first unit, with the encoder's 32-bit wrap undone.
    pub pts: u64,
    pub keyframe: bool,
    /// Annex B, as the encoder made it.
    pub data: Vec<u8>,
    /// The parameter sets this unit carries, if it carries both.
    pub params: Option<ParameterSets>,
}

#[derive(Default)]
pub struct VideoRing {
    frames: VecDeque<VideoFrame>,
    bytes: usize,
    last_timestamp: Option<u32>,
    pts: u64,
    last_params: Option<ParameterSets>,
}

impl VideoRing {
    pub fn len(&self) -> usize {
        self.frames.len()
    }

    pub fn is_empty(&self) -> bool {
        self.frames.is_empty()
    }

    pub fn bytes(&self) -> usize {
        self.bytes
    }

    pub fn has_keyframe(&self) -> bool {
        self.frames.iter().any(|frame| frame.keyframe)
    }

    pub fn clear(&mut self) {
        *self = Self::default();
    }

    /// Adds an access unit. `timestamp` is the encoder's 90 kHz value (it wraps like an RTP timestamp).
    pub fn push(&mut self, arrival_ms: u64, timestamp: u32, keyframe: bool, data: Vec<u8>) {
        let delta = match self.last_timestamp {
            // A step backwards (more than half the range) is a reordering or a restart: it adds nothing.
            Some(last) => {
                let step = timestamp.wrapping_sub(last);
                if step > u32::MAX / 2 { 0 } else { step }
            }
            None => 0,
        };
        self.last_timestamp = Some(timestamp);
        self.pts += u64::from(delta);
        let params = parameter_sets(&data);
        if let Some(found) = &params {
            self.last_params = Some(found.clone());
        }
        self.bytes += data.len();
        self.frames.push_back(VideoFrame {
            arrival_ms,
            pts: self.pts,
            keyframe,
            data,
            params,
        });
    }

    /// Drops what no clip of `length_ms` ending at `now_ms` can use: everything before the last keyframe at or before
    /// `now_ms - length_ms` (the window starts there), and, when the bytes are over `max_bytes`, whole groups of pictures
    /// from the front, never the newest one.
    pub fn evict(&mut self, now_ms: u64, length_ms: u64, max_bytes: usize) {
        let cutoff = now_ms.saturating_sub(length_ms);
        let keep_from = self
            .frames
            .iter()
            .rposition(|frame| frame.keyframe && frame.arrival_ms <= cutoff)
            .unwrap_or(0);
        self.drop_front(keep_from);
        while self.bytes > max_bytes {
            // The next keyframe after the front one; without one the buffer is a single group and stays.
            let Some(next) = self.frames.iter().skip(1).position(|frame| frame.keyframe) else {
                break;
            };
            self.drop_front(next + 1);
        }
    }

    fn drop_front(&mut self, count: usize) {
        for frame in self.frames.drain(..count) {
            self.bytes -= frame.data.len();
        }
    }

    /// The units of the window ending at `end_ms` and reaching back `length_ms`: from the last keyframe at or before
    /// the start (or, when the buffer does not reach back that far, the first keyframe it has) to the last unit that
    /// arrived by `end_ms`. `None` when no keyframe is held yet.
    pub fn cut(&self, end_ms: u64, length_ms: u64) -> Option<Vec<&VideoFrame>> {
        let start = end_ms.saturating_sub(length_ms);
        let first = self
            .frames
            .iter()
            .rposition(|frame| frame.keyframe && frame.arrival_ms <= start)
            .or_else(|| self.frames.iter().position(|frame| frame.keyframe))?;
        let window: Vec<&VideoFrame> = self
            .frames
            .iter()
            .skip(first)
            .take_while(|frame| frame.arrival_ms <= end_ms)
            .collect();
        (!window.is_empty()).then_some(window)
    }

    /// Arrival time of the oldest and the newest unit held.
    pub fn span(&self) -> Option<(u64, u64)> {
        Some((
            self.frames.front()?.arrival_ms,
            self.frames.back()?.arrival_ms,
        ))
    }

    /// The newest parameter sets any held unit carries, or the last parameter sets seen before eviction.
    pub fn latest_params(&self) -> Option<&ParameterSets> {
        self.frames
            .iter()
            .rev()
            .find_map(|frame| frame.params.as_ref())
            .or(self.last_params.as_ref())
    }
}

/// The SPS and PPS of an access unit, when it holds both.
pub fn parameter_sets(data: &[u8]) -> Option<ParameterSets> {
    let (mut sps, mut pps) = (None, None);
    for nal in dc_media::h264::nals(data) {
        match nal[0] & 31 {
            dc_media::h264::NAL_SPS => sps = Some(nal.to_vec()),
            dc_media::h264::NAL_PPS => pps = Some(nal.to_vec()),
            _ => {}
        }
    }
    Some((sps?, pps?))
}

/// One Opus packet.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AudioPacket {
    /// When the first sample of the packet was captured.
    pub start_ms: u64,
    pub data: Vec<u8>,
}

#[derive(Default)]
pub struct AudioRing {
    packets: VecDeque<AudioPacket>,
}

impl AudioRing {
    pub fn len(&self) -> usize {
        self.packets.len()
    }

    pub fn is_empty(&self) -> bool {
        self.packets.is_empty()
    }

    pub fn clear(&mut self) {
        self.packets.clear();
    }

    pub fn push(&mut self, packet: AudioPacket) {
        self.packets.push_back(packet);
    }

    /// Drops the packets that end before `now_ms - keep_ms` (a packet is 20 ms).
    pub fn evict(&mut self, now_ms: u64, keep_ms: u64) {
        let cutoff = now_ms.saturating_sub(keep_ms);
        while self
            .packets
            .front()
            .is_some_and(|packet| packet.start_ms + PACKET_MS < cutoff)
        {
            self.packets.pop_front();
        }
    }

    /// The packets that start inside `[start_ms, end_ms]`.
    pub fn cut(&self, start_ms: u64, end_ms: u64) -> Vec<AudioPacket> {
        self.packets
            .iter()
            .filter(|packet| packet.start_ms + PACKET_MS > start_ms && packet.start_ms <= end_ms)
            .cloned()
            .collect()
    }
}

/// One Opus packet is 20 ms of 48 kHz audio.
pub const PACKET_MS: u64 = 20;
/// Samples per channel in one packet.
pub const PACKET_SAMPLES: usize = 960;

#[cfg(test)]
mod tests {
    use super::*;

    /// A fake access unit: an IDR slice (NAL type 5) or a P slice (type 1) behind a four byte start code.
    fn unit(keyframe: bool, size: usize) -> Vec<u8> {
        let mut data = vec![0, 0, 0, 1, if keyframe { 0x65 } else { 0x41 }];
        data.resize(size.max(5), 7);
        data
    }

    /// One picture every 100 ms (a keyframe each second), `count` of them, from time 0.
    fn make(count: u64) -> VideoRing {
        let mut ring = VideoRing::default();
        for index in 0..count {
            ring.push(
                index * 100,
                (index * 9000) as u32,
                index % 10 == 0,
                unit(index % 10 == 0, 100),
            );
        }
        ring
    }

    #[test]
    fn eviction_keeps_the_group_the_window_starts_in() {
        let mut ring = make(100); // 0 to 9900 ms
        ring.evict(9_900, 3_000, usize::MAX);
        // The window starts at 6900: the last keyframe at or before it is the one at 6000.
        let (first, last) = ring.span().unwrap();
        assert_eq!((first, last), (6_000, 9_900));
        assert!(ring.frames.front().unwrap().keyframe);
        assert_eq!(ring.len(), 40);
    }

    #[test]
    fn eviction_by_bytes_drops_whole_groups_but_never_the_last() {
        let mut ring = make(100);
        ring.evict(9_900, 60_000, 2_500);
        assert!(ring.bytes() <= 2_500 + 1_000);
        assert!(ring.frames.front().unwrap().keyframe);
        let mut one_group = make(5);
        one_group.evict(500, 60_000, 10);
        assert_eq!(one_group.len(), 5);
    }

    #[test]
    fn a_cut_starts_on_a_keyframe_at_or_before_the_start() {
        let ring = make(100);
        let window = ring.cut(9_900, 3_000).unwrap();
        // Start 6900 -> keyframe at 6000, through the last unit.
        assert_eq!(window.first().unwrap().arrival_ms, 6_000);
        assert!(window.first().unwrap().keyframe);
        assert_eq!(window.last().unwrap().arrival_ms, 9_900);
        // Every cut begins on a keyframe, whatever the start.
        for end in [1_000u64, 2_550, 4_321, 9_999] {
            for length in [500u64, 1_000, 2_000] {
                if let Some(window) = ring.cut(end, length) {
                    assert!(window[0].keyframe, "end {end} length {length}");
                    assert!(window.last().unwrap().arrival_ms <= end);
                }
            }
        }
    }

    #[test]
    fn a_short_buffer_gives_what_it_has_from_its_first_keyframe() {
        let ring = make(25);
        let window = ring.cut(2_400, 30_000).unwrap();
        assert_eq!(window.len(), 25);
        assert!(VideoRing::default().cut(1_000, 1_000).is_none());
        // Delta frames alone are not a clip.
        let mut only_delta = VideoRing::default();
        only_delta.push(0, 0, false, unit(false, 10));
        assert!(only_delta.cut(0, 1_000).is_none());
    }

    #[test]
    fn the_timestamp_wrap_is_undone() {
        let mut ring = VideoRing::default();
        ring.push(0, u32::MAX - 4_000, true, unit(true, 10));
        ring.push(33, 5_000, false, unit(false, 10));
        let window = ring.cut(100, 1_000).unwrap();
        assert_eq!(window[1].pts - window[0].pts, 9_001);
    }

    #[test]
    fn audio_eviction_and_cuts_follow_the_clock() {
        let mut ring = AudioRing::default();
        for index in 0..500u64 {
            ring.push(AudioPacket {
                start_ms: index * 20,
                data: vec![index as u8],
            });
        }
        ring.evict(9_980, 2_000);
        assert_eq!(ring.cut(0, 20_000).first().unwrap().start_ms, 7_960);
        let cut = ring.cut(9_000, 9_500);
        assert_eq!(cut.first().unwrap().start_ms, 9_000);
        assert_eq!(cut.last().unwrap().start_ms, 9_500);
    }

    #[test]
    fn latest_params_survives_keyframe_eviction() {
        let mut ring = VideoRing::default();
        // First keyframe carries SPS + PPS + IDR.
        let kf_with_params = vec![
            0, 0, 0, 1, 0x67, 0x42, 0x00, 0x1e, // SPS
            0, 0, 0, 1, 0x68, 0xce, // PPS
            0, 0, 0, 1, 0x65, 0x88, // IDR
        ];
        ring.push(0, 0, true, kf_with_params);
        // Newer keyframes omit repeated SPS/PPS (as some encoders do mid-stream).
        for idx in 1..=5u64 {
            let idr_only = vec![0, 0, 0, 1, 0x65, idx as u8];
            ring.push(idx * 1_000, (idx as u32) * 90_000, true, idr_only);
        }
        ring.evict(5_000, 2_000, usize::MAX);
        assert_eq!(ring.span().unwrap().0, 3_000);
        assert_eq!(
            ring.latest_params(),
            Some(&(vec![0x67, 0x42, 0x00, 0x1e], vec![0x68, 0xce]))
        );
        ring.clear();
        assert!(ring.latest_params().is_none());
    }
}
