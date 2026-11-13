//! `PacketArrivalHistory` (libwebrtc `modules/audio_coding/neteq/packet_arrival_history.{h,cc}`): when packets came,
//! measured on the playout tick timer, against their RTP timestamps, over a window of RTP time. A packet's delay is how
//! much later than the earliest-relative packet of the window it came. Everything is in samples, as there.
use std::collections::{BTreeMap, VecDeque};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Arrival {
    rtp_timestamp: i64,
    arrival_timestamp: i64,
    length_samples: i64,
}

impl Arrival {
    /// The key `PacketArrival::operator<=` and `>=` compare.
    fn key(&self) -> i64 {
        self.arrival_timestamp - self.rtp_timestamp
    }

    /// `PacketArrival::contains`.
    fn contains(&self, other: &Arrival) -> bool {
        self.rtp_timestamp <= other.rtp_timestamp
            && self.rtp_timestamp + self.length_samples
                >= other.rtp_timestamp + other.length_samples
    }
}

/// `RtpTimestampUnwrapper` (`SeqNumUnwrapper<uint32_t>`): the nearest 64-bit value to the last one.
#[derive(Default)]
pub(crate) struct Unwrapper {
    last: Option<i64>,
}

impl Unwrapper {
    pub(crate) fn peek(&self, value: u32) -> i64 {
        match self.last {
            None => i64::from(value),
            Some(last) => last + i64::from(value.wrapping_sub(last as u32) as i32),
        }
    }

    pub(crate) fn unwrap(&mut self, value: u32) -> i64 {
        let unwrapped = self.peek(value);
        self.last = Some(unwrapped);
        unwrapped
    }
}

pub(crate) struct PacketArrivalHistory {
    window_size_ms: i64,
    sample_rate_khz: i64,
    history: BTreeMap<i64, Arrival>,
    min_packet_arrivals: VecDeque<Arrival>,
    max_packet_arrivals: VecDeque<Arrival>,
    unwrapper: Unwrapper,
}

impl PacketArrivalHistory {
    pub(crate) fn new(window_size_ms: i64, sample_rate_khz: i64) -> Self {
        Self {
            window_size_ms,
            sample_rate_khz,
            history: BTreeMap::new(),
            min_packet_arrivals: VecDeque::new(),
            max_packet_arrivals: VecDeque::new(),
            unwrapper: Unwrapper::default(),
        }
    }

    fn arrival_now(&self, now: u64) -> i64 {
        now as i64 * super::MS_PER_TICK as i64 * self.sample_rate_khz
    }

    /// `PacketArrivalHistory::Insert`. False for an obsolete or duplicate packet.
    pub(crate) fn insert(&mut self, rtp_timestamp: u32, length_samples: usize, now: u64) -> bool {
        let packet = Arrival {
            rtp_timestamp: self.unwrapper.peek(rtp_timestamp),
            arrival_timestamp: self.arrival_now(now),
            length_samples: length_samples as i64,
        };
        if self.is_obsolete(&packet) || self.contains(&packet) {
            return false;
        }
        let _ = self.unwrapper.unwrap(rtp_timestamp);
        self.history.insert(packet.rtp_timestamp, packet);
        if self.history.keys().next_back() != Some(&packet.rtp_timestamp) {
            // Reordered packet.
            return true;
        }
        while let Some((&oldest_key, &oldest)) = self.history.iter().next()
            && self.is_obsolete(&oldest)
        {
            if self.min_packet_arrivals.front() == Some(&oldest) {
                self.min_packet_arrivals.pop_front();
            }
            if self.max_packet_arrivals.front() == Some(&oldest) {
                self.max_packet_arrivals.pop_front();
            }
            self.history.remove(&oldest_key);
        }
        while self
            .min_packet_arrivals
            .back()
            .is_some_and(|back| packet.key() <= back.key())
        {
            self.min_packet_arrivals.pop_back();
        }
        while self
            .max_packet_arrivals
            .back()
            .is_some_and(|back| packet.key() >= back.key())
        {
            self.max_packet_arrivals.pop_back();
        }
        self.min_packet_arrivals.push_back(packet);
        self.max_packet_arrivals.push_back(packet);
        true
    }

    fn is_obsolete(&self, packet: &Arrival) -> bool {
        match self.history.values().next_back() {
            None => false,
            Some(newest) => {
                packet.rtp_timestamp + self.window_size_ms * self.sample_rate_khz
                    < newest.rtp_timestamp
            }
        }
    }

    fn contains(&self, packet: &Arrival) -> bool {
        self.history
            .range(..=packet.rtp_timestamp)
            .next_back()
            .is_some_and(|(_, earlier)| earlier.contains(packet))
    }

    /// `GetPacketArrivalDelayMs`: each term divided on its own, as there.
    fn packet_arrival_delay_ms(&self, packet: &Arrival) -> i32 {
        let Some(min) = self.min_packet_arrivals.front() else {
            return 0;
        };
        let k = self.sample_rate_khz;
        let delay = (packet.arrival_timestamp / k - min.arrival_timestamp / k)
            - (packet.rtp_timestamp / k - min.rtp_timestamp / k);
        delay.max(0) as i32
    }

    /// `GetDelayMs`: the delay of `rtp_timestamp` if it were played (or arrived) now.
    pub(crate) fn delay_ms(&self, rtp_timestamp: u32, now: u64) -> i32 {
        let packet = Arrival {
            rtp_timestamp: self.unwrapper.peek(rtp_timestamp),
            arrival_timestamp: self.arrival_now(now),
            length_samples: 0,
        };
        self.packet_arrival_delay_ms(&packet)
    }

    /// `GetMaxDelayMs`: the largest delay in the window.
    pub(crate) fn max_delay_ms(&self) -> i32 {
        self.max_packet_arrivals
            .front()
            .map_or(0, |packet| self.packet_arrival_delay_ms(packet))
    }

    pub(crate) fn is_newest_rtp_timestamp(&self, rtp_timestamp: u32) -> bool {
        match self.history.keys().next_back() {
            None => true,
            Some(newest) => self.unwrapper.peek(rtp_timestamp) == *newest,
        }
    }

    pub(crate) fn size(&self) -> usize {
        self.history.len()
    }

    pub(crate) fn reset(&mut self) {
        self.history.clear();
        self.min_packet_arrivals.clear();
        self.max_packet_arrivals.clear();
        self.unwrapper = Unwrapper::default();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delay_is_relative_to_the_fastest_packet_in_the_window() {
        let mut history = PacketArrivalHistory::new(2000, 48);
        // Packets of 20 ms every two ticks; the third comes 60 ms late.
        assert!(history.insert(1000, 960, 0));
        assert!(history.insert(1960, 960, 2));
        assert!(history.insert(2920, 960, 10));
        assert_eq!(history.delay_ms(2920, 10), 60);
        assert_eq!(history.max_delay_ms(), 60);
        // A duplicate and a packet older than the window are refused.
        assert!(!history.insert(1960, 960, 11));
        assert!(history.insert(2920 + 960 * 200, 960, 400));
        assert!(!history.insert(1000, 960, 401));
        // Reordering: an older packet arriving after a newer one is kept but is not the newest.
        let newest = 2920 + 960 * 200;
        assert!(history.insert(newest - 960, 960, 402));
        assert!(!history.is_newest_rtp_timestamp(newest - 960));
        assert!(history.is_newest_rtp_timestamp(newest));
    }

    #[test]
    fn timestamps_unwrap_across_the_32_bit_edge() {
        let mut history = PacketArrivalHistory::new(2000, 48);
        let start = u32::MAX - 960;
        assert!(history.insert(start, 960, 0));
        assert!(history.insert(start.wrapping_add(960), 960, 2));
        assert!(history.insert(start.wrapping_add(1920), 960, 4));
        assert!(history.is_newest_rtp_timestamp(start.wrapping_add(1920)));
        assert_eq!(history.delay_ms(start.wrapping_add(1920), 4), 0);
        assert!(!history.is_newest_rtp_timestamp(start));
    }

    #[test]
    fn obsolete_stray_packet_does_not_poison_unwrapper() {
        let mut history = PacketArrivalHistory::new(2000, 48);
        let newest = 2_200_000_000u32;
        assert!(history.insert(newest, 960, 0));
        // Just over half a 32-bit cycle behind `newest`: rejected as obsolete.
        let stray = newest.wrapping_sub(0x8000_0000 - 1_000_000);
        assert!(!history.insert(stray, 960, 1));
        assert!(history.insert(newest + 960, 960, 2));
        assert!(history.is_newest_rtp_timestamp(newest + 960));
        assert!(history.delay_ms(newest + 960, 2) < 100);
    }
}
