//! NetEq's controller: `BufferLevelFilter` (`buffer_level_filter.cc`) and `DecisionLogic` (`decision_logic.{h,cc}`,
//! libwebrtc `modules/audio_coding/neteq`, the version with `delay_constraints.cc` that the pinned engine carries).
//! Every 10 ms it chooses what the next output is: decoded audio as it is, sped up, slowed down, or concealment.
use super::arrival::PacketArrivalHistory;
use super::delay::{DelayConstraints, DelayManager};

/// `NetEq::Mode`: what the last output was. The comfort-noise packet mode (RFC 3389) and DTMF are not carried by
/// Discord's Opus stream and are left out; `Expand` is the concealment that never runs here (see `CodecPlc`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Mode {
    Normal,
    Expand,
    CodecPlc,
    AccelerateSuccess,
    AccelerateLowEnergy,
    AccelerateFail,
    PreemptiveExpandSuccess,
    PreemptiveExpandLowEnergy,
    PreemptiveExpandFail,
    CodecInternalCng,
    Error,
}

/// `NetEq::Operation`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Operation {
    Normal,
    Merge,
    Expand,
    Accelerate,
    FastAccelerate,
    PreemptiveExpand,
    CodecInternalCng,
    Undefined,
}

fn is_timestretch(mode: Mode) -> bool {
    matches!(
        mode,
        Mode::AccelerateSuccess
            | Mode::AccelerateLowEnergy
            | Mode::PreemptiveExpandSuccess
            | Mode::PreemptiveExpandLowEnergy
    )
}

fn is_cng(mode: Mode) -> bool {
    mode == Mode::CodecInternalCng
}

pub(crate) fn is_expand(mode: Mode) -> bool {
    matches!(mode, Mode::Expand | Mode::CodecPlc)
}

/// `buffer_level_filter.cc`: the packet buffer's level, smoothed in Q8 samples.
pub(crate) struct BufferLevelFilter {
    level_factor: i32,
    filtered_current_level: i32,
}

impl BufferLevelFilter {
    fn new() -> Self {
        Self {
            level_factor: 253,
            filtered_current_level: 0,
        }
    }

    fn reset(&mut self) {
        self.filtered_current_level = 0;
        self.level_factor = 253;
    }

    fn update(&mut self, buffer_size_samples: usize, time_stretched_samples: i32) {
        let filtered = ((i64::from(self.level_factor) * i64::from(self.filtered_current_level))
            >> 8)
            + i64::from(256 - self.level_factor) * buffer_size_samples as i64;
        self.filtered_current_level = (filtered - i64::from(time_stretched_samples) * 256)
            .clamp(0, i64::from(i32::MAX)) as i32;
    }

    fn set_filtered_buffer_level(&mut self, buffer_size_samples: usize) {
        self.filtered_current_level =
            (buffer_size_samples as i64 * 256).min(i64::from(i32::MAX)) as i32;
    }

    fn set_target_buffer_level(&mut self, target_buffer_level_ms: i32) {
        self.level_factor = match target_buffer_level_ms {
            ..=20 => 251,
            21..=60 => 252,
            61..=140 => 253,
            _ => 254,
        };
    }

    /// `filtered_current_level()`, in samples.
    #[cfg(test)]
    pub(crate) fn filtered_current_level(&self) -> i32 {
        self.filtered_current_level >> 8
    }
}

/// `kPostponeDecodingLevel`: the engine's `postponeDecodeLevel`, which the client sets to 100 (`setPostponeDecodeLevel(
/// 100)` in module 754712, the transport option of module 31595); upstream's constant is 50. Inferred from the name:
/// the snapshot has no source for what the engine does with it.
pub(crate) const POSTPONE_DECODING_LEVEL: usize = 100;
/// `kTargetLevelWindowMs`.
const TARGET_LEVEL_WINDOW_MS: i32 = 100;
/// `kDelayAdjustmentGranularityMs`.
const DELAY_ADJUSTMENT_GRANULARITY_MS: i64 = 20;
/// `kPacketHistorySizeMs`.
const PACKET_HISTORY_SIZE_MS: i64 = 2000;
/// `kCngTimeoutMs`.
pub(crate) const CNG_TIMEOUT_MS: usize = 1000;
/// `kMinTimescaleInterval` (`decision_logic.h`), in ticks: at most about 100 ms of stretching a second.
const MIN_TIMESCALE_INTERVAL: u64 = 5;

/// `NetEqController::NetEqStatus`, the fields this controller reads.
pub(crate) struct Status {
    pub target_timestamp: u32,
    pub expand_mutefactor: i32,
    pub last_mode: Mode,
    pub next_packet: Option<NextPacket>,
    pub generated_noise_samples: usize,
    pub span_samples: usize,
    pub span_samples_wait_time: usize,
    pub dtx_or_cng: bool,
    pub sync_buffer_samples: usize,
}

#[derive(Clone, Copy)]
pub(crate) struct NextPacket {
    pub timestamp: u32,
    pub is_dtx: bool,
}

/// `NetEqController::PacketArrivedInfo`.
pub(crate) struct PacketArrivedInfo {
    pub packet_length_samples: usize,
    pub main_timestamp: u32,
    pub buffer_flush: bool,
}

/// `PacketBuffer::IsObsoleteTimestamp`: `timestamp` is older than `limit`, and within `horizon` of it (0: any age).
pub(crate) fn is_obsolete_timestamp(timestamp: u32, limit: u32, horizon_samples: u32) -> bool {
    is_newer_timestamp(limit, timestamp)
        && (horizon_samples == 0
            || is_newer_timestamp(timestamp, limit.wrapping_sub(horizon_samples)))
}

/// `IsNewerTimestamp` (`module_common_types_public.h`).
pub(crate) fn is_newer_timestamp(timestamp: u32, prev_timestamp: u32) -> bool {
    if timestamp.wrapping_sub(prev_timestamp) == 0x8000_0000 {
        return timestamp > prev_timestamp;
    }
    timestamp != prev_timestamp && timestamp.wrapping_sub(prev_timestamp) < 0x8000_0000
}

pub(crate) struct DecisionLogic {
    delay_manager: DelayManager,
    delay_constraints: DelayConstraints,
    buffer_level_filter: BufferLevelFilter,
    packet_arrival_history: PacketArrivalHistory,
    sample_rate_khz: i32,
    /// The tick a time stretch may happen again at (`timescale_countdown_`).
    timescale_countdown: Option<u64>,
    noise_fast_forward: usize,
    packet_length_samples: usize,
    sample_memory: i32,
    prev_time_scale: bool,
    time_stretched_cn_samples: i32,
    buffer_flush: bool,
}

impl DecisionLogic {
    /// `max_packets_in_buffer` and `base_min_delay_ms` from `NetEq::Config`; the minimum delay is the engine's
    /// `minimumJitterBufferLevel`.
    pub(crate) fn new(max_packets_in_buffer: i32, minimum_delay_ms: i32, now: u64) -> Self {
        let mut constraints = DelayConstraints::new(max_packets_in_buffer, 0);
        constraints.set_minimum_delay(minimum_delay_ms);
        Self {
            delay_manager: DelayManager::new(),
            delay_constraints: constraints,
            buffer_level_filter: BufferLevelFilter::new(),
            packet_arrival_history: PacketArrivalHistory::new(PACKET_HISTORY_SIZE_MS, 48),
            sample_rate_khz: 48,
            timescale_countdown: Some(now + MIN_TIMESCALE_INTERVAL + 1),
            noise_fast_forward: 0,
            packet_length_samples: 0,
            sample_memory: 0,
            prev_time_scale: false,
            time_stretched_cn_samples: 0,
            buffer_flush: false,
        }
    }

    /// `DecisionLogic::SoftReset`.
    pub(crate) fn soft_reset(&mut self, now: u64) {
        self.packet_length_samples = 0;
        self.sample_memory = 0;
        self.prev_time_scale = false;
        self.timescale_countdown = Some(now + MIN_TIMESCALE_INTERVAL + 1);
        self.time_stretched_cn_samples = 0;
        self.delay_manager.reset();
        self.buffer_level_filter.reset();
        self.packet_arrival_history.reset();
    }

    pub(crate) fn add_sample_memory(&mut self, value: i32) {
        self.sample_memory += value;
    }

    pub(crate) fn set_sample_memory(&mut self, value: i32) {
        self.sample_memory = value;
    }

    pub(crate) fn set_prev_time_scale(&mut self, value: bool) {
        self.prev_time_scale = value;
    }

    pub(crate) fn noise_fast_forward(&self) -> usize {
        self.noise_fast_forward
    }

    fn timescale_allowed(&self, now: u64) -> bool {
        self.timescale_countdown.is_none_or(|end| now >= end)
    }

    /// `DecisionLogic::GetDecision`.
    pub(crate) fn get_decision(&mut self, status: &Status, now: u64) -> Operation {
        self.prev_time_scale = self.prev_time_scale && is_timestretch(status.last_mode);
        if self.prev_time_scale {
            self.timescale_countdown = Some(now + MIN_TIMESCALE_INTERVAL);
        }
        if !is_cng(status.last_mode) && !is_expand(status.last_mode) {
            self.filter_buffer_level(status.span_samples);
        }
        if status.last_mode == Mode::Error {
            return if status.next_packet.is_none() {
                Operation::Expand
            } else {
                Operation::Undefined
            };
        }
        let Some(next) = status.next_packet else {
            return self.no_packet(status);
        };
        if self.postpone_decode(status) {
            return self.no_packet(status);
        }
        let five_seconds_samples = (5000 * self.sample_rate_khz) as u32;
        if status.target_timestamp == next.timestamp {
            return self.expected_packet_available(status, now);
        }
        if !is_obsolete_timestamp(
            next.timestamp,
            status.target_timestamp,
            five_seconds_samples,
        ) {
            return self.future_packet_available(status, next);
        }
        Operation::Undefined
    }

    /// `TargetLevelMs`: the delay manager's target within the constraints.
    pub(crate) fn target_level_ms(&self) -> i32 {
        self.delay_constraints
            .clamp(self.delay_manager.target_delay_ms())
    }

    /// `DecisionLogic::PacketArrived`: the arrival delay it measured, once two packets have come.
    pub(crate) fn packet_arrived(
        &mut self,
        should_update_stats: bool,
        info: &PacketArrivedInfo,
        now: u64,
    ) -> Option<i32> {
        self.buffer_flush |= info.buffer_flush;
        if !should_update_stats {
            return None;
        }
        if info.packet_length_samples > 0
            && info.packet_length_samples != self.packet_length_samples
        {
            self.packet_length_samples = info.packet_length_samples;
            self.delay_constraints.set_packet_audio_length(
                (self.packet_length_samples / self.sample_rate_khz as usize) as i32,
            );
        }
        let inserted = self.packet_arrival_history.insert(
            info.main_timestamp,
            info.packet_length_samples,
            now,
        );
        if !inserted || self.packet_arrival_history.size() < 2 {
            return None;
        }
        let arrival_delay_ms = self
            .packet_arrival_history
            .delay_ms(info.main_timestamp, now);
        let reordered = !self
            .packet_arrival_history
            .is_newest_rtp_timestamp(info.main_timestamp);
        self.delay_manager.update(arrival_delay_ms, reordered, now);
        Some(arrival_delay_ms)
    }

    fn filter_buffer_level(&mut self, buffer_size_samples: usize) {
        self.buffer_level_filter
            .set_target_buffer_level(self.target_level_ms());
        let mut time_stretched_samples = self.time_stretched_cn_samples;
        if self.prev_time_scale {
            time_stretched_samples += self.sample_memory;
        }
        if self.buffer_flush {
            self.buffer_level_filter
                .set_filtered_buffer_level(buffer_size_samples);
            self.buffer_flush = false;
        } else {
            self.buffer_level_filter
                .update(buffer_size_samples, time_stretched_samples);
        }
        self.prev_time_scale = false;
        self.time_stretched_cn_samples = 0;
    }

    fn no_packet(&self, status: &Status) -> Operation {
        match status.last_mode {
            Mode::CodecInternalCng => {
                if status.generated_noise_samples > CNG_TIMEOUT_MS * self.sample_rate_khz as usize {
                    Operation::Expand
                } else {
                    Operation::CodecInternalCng
                }
            }
            _ => Operation::Expand,
        }
    }

    fn expected_packet_available(&self, status: &Status, now: u64) -> Operation {
        if status.last_mode != Mode::Expand {
            let playout_delay_ms = i64::from(self.playout_delay_ms(status, now));
            let low_limit = i64::from(self.target_level_ms());
            let high_limit = low_limit
                + i64::from(self.packet_arrival_history.max_delay_ms())
                + DELAY_ADJUSTMENT_GRANULARITY_MS;
            if playout_delay_ms >= high_limit * 4 {
                return Operation::FastAccelerate;
            }
            if self.timescale_allowed(now) {
                if playout_delay_ms >= high_limit {
                    return Operation::Accelerate;
                }
                if playout_delay_ms < low_limit {
                    return Operation::PreemptiveExpand;
                }
            }
        }
        Operation::Normal
    }

    fn future_packet_available(&mut self, status: &Status, next: NextPacket) -> Operation {
        let buffer_delay_samples = status.span_samples_wait_time;
        let buffer_delay_ms = (buffer_delay_samples / self.sample_rate_khz as usize) as i32;
        let high_limit = self.target_level_ms() + TARGET_LEVEL_WINDOW_MS / 2;
        let above_target_delay = buffer_delay_ms > high_limit;
        if Self::packet_too_early(status, next) && !above_target_delay {
            return self.no_packet(status);
        }
        let timestamp_leap = next.timestamp.wrapping_sub(status.target_timestamp);
        if timestamp_leap as usize != status.generated_noise_samples {
            self.buffer_level_filter
                .set_filtered_buffer_level(buffer_delay_samples);
        }
        match status.last_mode {
            Mode::Expand => Operation::Merge,
            Mode::CodecPlc | Mode::CodecInternalCng => Operation::Normal,
            _ => Operation::Expand,
        }
    }

    fn postpone_decode(&self, status: &Status) -> bool {
        let min_buffer_level_samples = self.target_level_ms() as usize
            * self.sample_rate_khz as usize
            * POSTPONE_DECODING_LEVEL
            / 100;
        if status.span_samples_wait_time >= min_buffer_level_samples {
            return false;
        }
        if status.dtx_or_cng {
            return false;
        }
        if is_cng(status.last_mode) {
            return true;
        }
        // The mute factor is Q14: below a half, concealment was long enough to be noticed.
        is_expand(status.last_mode) && status.expand_mutefactor < 16384 / 2
    }

    fn packet_too_early(status: &Status, next: NextPacket) -> bool {
        next.timestamp.wrapping_sub(status.target_timestamp) as usize
            > status.generated_noise_samples
    }

    /// `GetPlayoutDelayMs`: the delay of the sample about to be played, against the packet arrival history.
    pub(crate) fn playout_delay_ms(&self, status: &Status, now: u64) -> i32 {
        let playout_timestamp = status
            .target_timestamp
            .wrapping_sub(status.sync_buffer_samples as u32);
        self.packet_arrival_history.delay_ms(playout_timestamp, now)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn status(
        target: u32,
        next: Option<u32>,
        last_mode: Mode,
        span: usize,
        future: usize,
    ) -> Status {
        Status {
            target_timestamp: target,
            expand_mutefactor: 16384,
            last_mode,
            next_packet: next.map(|timestamp| NextPacket {
                timestamp,
                is_dtx: false,
            }),
            generated_noise_samples: 0,
            span_samples: span,
            span_samples_wait_time: span,
            dtx_or_cng: false,
            sync_buffer_samples: future,
        }
    }

    #[test]
    fn filter_follows_the_level_with_its_target_dependent_factor() {
        let mut filter = BufferLevelFilter::new();
        filter.set_target_buffer_level(100);
        for _ in 0..2000 {
            filter.update(4800, 0);
        }
        assert!((filter.filtered_current_level() - 4800).abs() <= 2);
        filter.update(4800, 4800);
        assert!(filter.filtered_current_level() < 100);
        filter.set_filtered_buffer_level(960);
        assert_eq!(filter.filtered_current_level(), 960);
    }

    #[test]
    fn no_packet_conceals_and_a_lost_packet_is_skipped_after_its_duration() {
        let mut logic = DecisionLogic::new(200, 0, 0);
        assert_eq!(
            logic.get_decision(&status(960, None, Mode::Normal, 0, 0), 10),
            Operation::Expand
        );
        // The packet after a lost one waits while concealment has not covered the gap.
        let mut early = status(960, Some(1920), Mode::CodecPlc, 960, 0);
        assert_eq!(logic.get_decision(&early, 11), Operation::Expand);
        early.generated_noise_samples = 960;
        assert_eq!(logic.get_decision(&early, 12), Operation::Normal);
    }

    #[test]
    fn obsolete_timestamps_are_older_within_the_horizon() {
        assert!(is_obsolete_timestamp(100, 200, 0));
        assert!(!is_obsolete_timestamp(200, 200, 0));
        assert!(!is_obsolete_timestamp(300, 200, 0));
        assert!(is_obsolete_timestamp(u32::MAX - 10, 5, 100));
        assert!(!is_obsolete_timestamp(0, 1000, 100));
    }
}
