//! The target playout delay: libwebrtc's `Histogram` (`histogram.cc`), `UnderrunOptimizer` (`underrun_optimizer.cc`),
//! `ReorderOptimizer` (`reorder_optimizer.cc`), `DelayManager` (`delay_manager.cc`, defaults from `delay_manager.h`)
//! and `DelayConstraints` (`delay_constraints.cc`), all under `modules/audio_coding/neteq`. Fixed point as there: the
//! histogram holds Q30 probabilities, forget factors are Q15.

/// `histogram.cc`: one bucket per 20 ms of relative delay, an exponentially forgetting probability mass.
pub(crate) struct Histogram {
    buckets: Vec<i32>,
    forget_factor: i32,
    base_forget_factor: i32,
    add_count: i32,
    start_forget_weight: Option<f64>,
}

impl Histogram {
    pub(crate) fn new(
        num_buckets: usize,
        forget_factor: i32,
        start_forget_weight: Option<f64>,
    ) -> Self {
        debug_assert!(forget_factor < 1 << 15);
        let mut histogram = Self {
            buckets: vec![0; num_buckets],
            forget_factor: 0,
            base_forget_factor: forget_factor,
            add_count: 0,
            start_forget_weight,
        };
        histogram.reset();
        histogram
    }

    /// `Histogram::Add`.
    pub(crate) fn add(&mut self, value: usize) {
        debug_assert!(value < self.buckets.len());
        let mut vector_sum: i64 = 0;
        for bucket in &mut self.buckets {
            *bucket = ((i64::from(*bucket) * i64::from(self.forget_factor)) >> 15) as i32;
            vector_sum += i64::from(*bucket);
        }
        let added = ((1 << 15) - self.forget_factor) << 15;
        self.buckets[value] += added;
        vector_sum += i64::from(added);
        // The sum must be 1 in Q30: spread the rounding error, at most a sixteenth of each bucket.
        vector_sum -= 1 << 30;
        if vector_sum != 0 {
            let flip_sign: i64 = if vector_sum > 0 { -1 } else { 1 };
            for bucket in &mut self.buckets {
                let correction = flip_sign * vector_sum.abs().min(i64::from(*bucket >> 4));
                *bucket += correction as i32;
                vector_sum += correction;
                if vector_sum == 0 {
                    break;
                }
            }
        }
        debug_assert_eq!(vector_sum, 0);
        self.add_count += 1;
        if let Some(weight) = self.start_forget_weight {
            if self.forget_factor != self.base_forget_factor {
                let factor =
                    (f64::from(1 << 15) * (1.0 - weight / f64::from(self.add_count + 1))) as i32;
                self.forget_factor = factor.clamp(0, self.base_forget_factor);
            }
        } else {
            self.forget_factor += (self.base_forget_factor - self.forget_factor + 3) >> 2;
        }
    }

    /// `Histogram::Quantile`: the bucket at which the mass from below reaches `probability` (Q30).
    pub(crate) fn quantile(&self, probability: i32) -> usize {
        let inverse_probability = (1i64 << 30) - i64::from(probability);
        let mut index = 0;
        let mut sum = (1i64 << 30) - i64::from(self.buckets[0]);
        while sum > inverse_probability && index < self.buckets.len() - 1 {
            index += 1;
            sum -= i64::from(self.buckets[index]);
        }
        index
    }

    /// `Histogram::Reset`: a geometric start, 0.5^(i+1) in Q30 from a 16-bit seed.
    pub(crate) fn reset(&mut self) {
        let mut temp_prob: u16 = 0x4002;
        for bucket in &mut self.buckets {
            temp_prob >>= 1;
            *bucket = i32::from(temp_prob) << 16;
        }
        self.forget_factor = 0;
        self.add_count = 0;
    }

    pub(crate) fn buckets(&self) -> &[i32] {
        &self.buckets
    }

    pub(crate) fn num_buckets(&self) -> usize {
        self.buckets.len()
    }
}

/// `kDelayBuckets` of `underrun_optimizer.cc` and `reorder_optimizer.cc`: 2000 ms at most.
const DELAY_BUCKETS: usize = 100;
/// `kBucketSizeMs`.
const BUCKET_SIZE_MS: i32 = 20;

/// `underrun_optimizer.cc`: the delay that a quantile of the relative arrival delays fits under.
pub(crate) struct UnderrunOptimizer {
    histogram: Histogram,
    histogram_quantile: i32,
    resample_interval_ms: Option<i32>,
    /// The tick the resample stopwatch started at (`resample_stopwatch_`).
    resample_start: Option<u64>,
    max_delay_in_interval_ms: i32,
    optimal_delay_ms: Option<i32>,
}

impl UnderrunOptimizer {
    fn new(
        histogram_quantile: i32,
        forget_factor: i32,
        start_forget_weight: Option<f64>,
        resample_interval_ms: Option<i32>,
    ) -> Self {
        Self {
            histogram: Histogram::new(DELAY_BUCKETS, forget_factor, start_forget_weight),
            histogram_quantile,
            resample_interval_ms,
            resample_start: None,
            max_delay_in_interval_ms: 0,
            optimal_delay_ms: None,
        }
    }

    /// `UnderrunOptimizer::Update`; `now` is the tick timer (10 ms per tick).
    fn update(&mut self, relative_delay_ms: i32, now: u64) {
        let mut histogram_update = None;
        if let Some(interval) = self.resample_interval_ms {
            let start = *self.resample_start.get_or_insert(now);
            if ((now - start) * super::MS_PER_TICK) as i32 > interval {
                histogram_update = Some(self.max_delay_in_interval_ms);
                self.resample_start = Some(now);
                self.max_delay_in_interval_ms = 0;
            }
            self.max_delay_in_interval_ms = self.max_delay_in_interval_ms.max(relative_delay_ms);
        } else {
            histogram_update = Some(relative_delay_ms);
        }
        let Some(update) = histogram_update else {
            return;
        };
        let index = update / BUCKET_SIZE_MS;
        if index >= 0 && (index as usize) < self.histogram.num_buckets() {
            self.histogram.add(index as usize);
        }
        let bucket_index = self.histogram.quantile(self.histogram_quantile);
        self.optimal_delay_ms = Some((1 + bucket_index as i32) * BUCKET_SIZE_MS);
    }

    fn reset(&mut self) {
        self.histogram.reset();
        self.resample_start = None;
        self.max_delay_in_interval_ms = 0;
        self.optimal_delay_ms = None;
    }
}

/// `reorder_optimizer.cc`: the delay that best trades waiting against losing reordered packets.
pub(crate) struct ReorderOptimizer {
    histogram: Histogram,
    ms_per_loss_percent: i32,
    optimal_delay_ms: Option<i32>,
}

impl ReorderOptimizer {
    fn new(forget_factor: i32, ms_per_loss_percent: i32, start_forget_weight: Option<f64>) -> Self {
        Self {
            histogram: Histogram::new(DELAY_BUCKETS, forget_factor, start_forget_weight),
            ms_per_loss_percent,
            optimal_delay_ms: None,
        }
    }

    fn update(&mut self, relative_delay_ms: i32, reordered: bool, base_delay_ms: i32) {
        let index = if reordered {
            relative_delay_ms / BUCKET_SIZE_MS
        } else {
            0
        };
        if index >= 0 && (index as usize) < self.histogram.num_buckets() {
            self.histogram.add(index as usize);
        }
        let bucket_index = self.minimize_cost_function(base_delay_ms);
        self.optimal_delay_ms = Some((1 + bucket_index as i32) * BUCKET_SIZE_MS);
    }

    fn reset(&mut self) {
        self.histogram.reset();
        self.optimal_delay_ms = None;
    }

    /// `ReorderOptimizer::MinimizeCostFunction`.
    fn minimize_cost_function(&self, base_delay_ms: i32) -> usize {
        let mut loss_probability: i64 = 1 << 30;
        let mut min_cost = i64::MAX;
        let mut min_bucket = 0;
        for (i, bucket) in self.histogram.buckets().iter().enumerate() {
            loss_probability -= i64::from(*bucket);
            let delay_ms = i64::from(0.max(i as i32 * BUCKET_SIZE_MS - base_delay_ms)) << 30;
            let cost = delay_ms + 100 * i64::from(self.ms_per_loss_percent) * loss_probability;
            if cost < min_cost {
                min_cost = cost;
                min_bucket = i;
            }
            if loss_probability == 0 {
                break;
            }
        }
        min_bucket
    }
}

/// `DelayManager::Config` defaults (`delay_manager.h`); `WebRTC-Audio-NetEqDelayManagerConfig` can override them, and
/// the pinned engine carries that field trial's name, but no value for it is in the snapshot.
pub(crate) const QUANTILE: f64 = 0.95;
pub(crate) const FORGET_FACTOR: f64 = 0.983;
pub(crate) const START_FORGET_WEIGHT: f64 = 2.0;
pub(crate) const RESAMPLE_INTERVAL_MS: i32 = 500;
pub(crate) const REORDER_FORGET_FACTOR: f64 = 0.9993;
pub(crate) const MS_PER_LOSS_PERCENT: i32 = 20;
/// `kStartDelayMs` of `delay_manager.cc`.
pub(crate) const START_DELAY_MS: i32 = 80;

/// `delay_manager.cc` with `use_reorder_optimizer` true, its default.
pub(crate) struct DelayManager {
    underrun: UnderrunOptimizer,
    reorder: ReorderOptimizer,
    target_level_ms: i32,
}

impl DelayManager {
    pub(crate) fn new() -> Self {
        let mut manager = Self {
            underrun: UnderrunOptimizer::new(
                (f64::from(1 << 30) * QUANTILE) as i32,
                (f64::from(1 << 15) * FORGET_FACTOR) as i32,
                Some(START_FORGET_WEIGHT),
                Some(RESAMPLE_INTERVAL_MS),
            ),
            reorder: ReorderOptimizer::new(
                (f64::from(1 << 15) * REORDER_FORGET_FACTOR) as i32,
                MS_PER_LOSS_PERCENT,
                Some(START_FORGET_WEIGHT),
            ),
            target_level_ms: START_DELAY_MS,
        };
        manager.reset();
        manager
    }

    /// `DelayManager::Update`.
    pub(crate) fn update(&mut self, arrival_delay_ms: i32, reordered: bool, now: u64) {
        if !reordered {
            self.underrun.update(arrival_delay_ms, now);
        }
        self.target_level_ms = self.underrun.optimal_delay_ms.unwrap_or(START_DELAY_MS);
        self.reorder
            .update(arrival_delay_ms, reordered, self.target_level_ms);
        self.target_level_ms = self
            .target_level_ms
            .max(self.reorder.optimal_delay_ms.unwrap_or(0));
    }

    pub(crate) fn reset(&mut self) {
        self.underrun.reset();
        self.target_level_ms = START_DELAY_MS;
        self.reorder.reset();
    }

    pub(crate) fn target_delay_ms(&self) -> i32 {
        self.target_level_ms
    }
}

/// `kMaxBaseMinimumDelayMs` of `delay_constraints.cc`.
const MAX_BASE_MINIMUM_DELAY_MS: i32 = 10_000;

/// `delay_constraints.cc`: the minimum and maximum the target is clamped to.
pub(crate) struct DelayConstraints {
    max_packets_in_buffer: i32,
    base_minimum_delay_ms: i32,
    effective_minimum_delay_ms: i32,
    minimum_delay_ms: i32,
    maximum_delay_ms: i32,
    packet_len_ms: i32,
}

impl DelayConstraints {
    pub(crate) fn new(max_packets_in_buffer: i32, base_minimum_delay_ms: i32) -> Self {
        Self {
            max_packets_in_buffer,
            base_minimum_delay_ms,
            effective_minimum_delay_ms: base_minimum_delay_ms,
            minimum_delay_ms: 0,
            maximum_delay_ms: 0,
            packet_len_ms: 0,
        }
    }

    /// `DelayConstraints::Clamp`: never more than three quarters of what the packet buffer holds.
    pub(crate) fn clamp(&self, delay_ms: i32) -> i32 {
        let mut delay_ms = delay_ms.max(self.effective_minimum_delay_ms);
        if self.maximum_delay_ms > 0 {
            delay_ms = delay_ms.min(self.maximum_delay_ms);
        }
        if self.packet_len_ms > 0 {
            delay_ms = delay_ms.min(3 * self.max_packets_in_buffer * self.packet_len_ms / 4);
        }
        delay_ms
    }

    pub(crate) fn set_packet_audio_length(&mut self, length_ms: i32) -> bool {
        if length_ms <= 0 {
            return false;
        }
        self.packet_len_ms = length_ms;
        true
    }

    /// `SetMinimumDelay`: the engine's `minimumJitterBufferLevel` (0 in module 31595).
    pub(crate) fn set_minimum_delay(&mut self, delay_ms: i32) -> bool {
        if !(0..=self.minimum_delay_upper_bound()).contains(&delay_ms) {
            return false;
        }
        self.minimum_delay_ms = delay_ms;
        self.update_effective_minimum_delay();
        true
    }

    fn update_effective_minimum_delay(&mut self) {
        let base = self
            .base_minimum_delay_ms
            .clamp(0, self.minimum_delay_upper_bound());
        self.effective_minimum_delay_ms = self.minimum_delay_ms.max(base);
    }

    fn minimum_delay_upper_bound(&self) -> i32 {
        let mut q75 = self.max_packets_in_buffer * self.packet_len_ms * 3 / 4;
        if q75 <= 0 {
            q75 = MAX_BASE_MINIMUM_DELAY_MS;
        }
        let maximum = if self.maximum_delay_ms > 0 {
            self.maximum_delay_ms
        } else {
            MAX_BASE_MINIMUM_DELAY_MS
        };
        maximum.min(q75)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn histogram_mass_stays_one_and_moves_to_new_values() {
        let mut histogram =
            Histogram::new(100, (f64::from(1 << 15) * FORGET_FACTOR) as i32, Some(2.0));
        for _ in 0..200 {
            histogram.add(7);
            assert_eq!(
                histogram
                    .buckets()
                    .iter()
                    .map(|b| i64::from(*b))
                    .sum::<i64>(),
                1 << 30
            );
        }
        assert_eq!(histogram.quantile((f64::from(1 << 30) * 0.95) as i32), 7);
    }

    #[test]
    fn target_follows_the_95th_percentile_of_relative_delay() {
        let mut manager = DelayManager::new();
        assert_eq!(manager.target_delay_ms(), START_DELAY_MS);
        // A 20 ms packet every two ticks; every tenth arrives 150 ms late, the rest on time.
        for packet in 0..1500u64 {
            let delay = if packet % 10 == 0 { 150 } else { 0 };
            manager.update(delay, false, packet * 2);
        }
        let target = manager.target_delay_ms();
        assert!((140..=180).contains(&target), "{target}");
        // Steady arrivals bring it back down over time.
        for packet in 1500..6000u64 {
            manager.update(0, false, packet * 2);
        }
        assert!(
            manager.target_delay_ms() <= 40,
            "{}",
            manager.target_delay_ms()
        );
    }

    #[test]
    fn constraints_cap_at_three_quarters_of_the_packet_buffer() {
        let mut constraints = DelayConstraints::new(200, 0);
        assert_eq!(constraints.clamp(5000), 5000);
        constraints.set_packet_audio_length(20);
        assert_eq!(constraints.clamp(5000), 3000);
        assert!(constraints.set_minimum_delay(0));
        assert_eq!(constraints.clamp(10), 10);
    }
}
