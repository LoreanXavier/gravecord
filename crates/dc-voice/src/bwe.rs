//! Send-side bandwidth estimation from transport-wide congestion control feedback, as the native engine's libwebrtc runs
//! it (GoogCC): every RTP packet we send carries a transport-wide sequence number (`rtp::TRANSPORT_SEQUENCE_EXTENSION`),
//! the far end reports when each one arrived (RTPFB FMT 15), and the sender compares the spacing of departures with the
//! spacing of arrivals. A growing queue on the path shows as arrivals spreading out faster than departures; the
//! trendline of that delay drives an AIMD rate controller, whose estimate caps the loss-based target
//! (`video::send::Rate`).
//!
//! Ported from libwebrtc (`webrtc.googlesource.com/src`, BSD-3-Clause), file by file:
//! * the feedback message: `modules/rtp_rtcp/source/rtcp_packet/transport_feedback.cc`;
//! * the send history and the receive-time unwrapping: `modules/congestion_controller/rtp/transport_feedback_adapter.cc`;
//! * packet groups: `modules/congestion_controller/goog_cc/inter_arrival_delta.{h,cc}`;
//! * the trendline and the overuse detector: `modules/congestion_controller/goog_cc/trendline_estimator.{h,cc}`;
//! * the acknowledged rate: `modules/congestion_controller/goog_cc/bitrate_estimator.cc`;
//! * the link capacity: `modules/congestion_controller/goog_cc/link_capacity_estimator.cc`;
//! * the rate control: `modules/remote_bitrate_estimator/aimd_rate_control.cc`;
//! * the glue: `modules/congestion_controller/goog_cc/delay_based_bwe.cc`.
//!
//! Neither vendored plumbing reference has one: Serein receives transport feedback counts only, Yapfast links the whole
//! libwebrtc. Left out, as the native engine runs them only behind field trials or with probing this client does not
//! send: probing, ALR, the network state predictor, the separate audio detector, and loss-based BWE v2.
use std::{collections::VecDeque, time::Duration};
use tokio::time::Instant;

/// One transport-wide feedback message (`TransportFeedback::Parse`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct TransportCc {
    /// The transport-wide sequence number of the first packet described.
    pub base: u16,
    /// The reference time, in 64 ms units modulo 2^24 (`kBaseTimeTick`).
    pub reference: u32,
    /// The feedback packet count, which wraps.
    pub count: u8,
    /// Every packet from `base` on: its arrival in microseconds after the reference time, or `None` when not received.
    pub arrivals: Vec<Option<i64>>,
}

/// A feedback message (the whole RTCP packet, padding removed): reference time, status chunks (run length, one-bit and
/// two-bit vectors), then the receive deltas in 250 us ticks (`kDeltaTick`), one byte unsigned for "received, small
/// delta", two bytes signed for "received, large delta". A truncated message, or a reserved symbol, is refused.
pub(crate) fn parse_transport_cc(packet: &[u8]) -> Option<TransportCc> {
    // Header, sender and media SSRC, base, count, reference time and feedback count (`kMinPayloadSizeBytes` of 8 + the
    // 12-byte common feedback header).
    if packet.len() < 20 {
        return None;
    }
    let base = u16::from_be_bytes([packet[12], packet[13]]);
    let status_count = usize::from(u16::from_be_bytes([packet[14], packet[15]]));
    let reference =
        (u32::from(packet[16]) << 16) | (u32::from(packet[17]) << 8) | u32::from(packet[18]);
    let count = packet[19];
    let mut symbols = Vec::with_capacity(status_count);
    let mut at = 20;
    while symbols.len() < status_count {
        let chunk = u16::from_be_bytes(packet.get(at..at + 2)?.try_into().ok()?);
        at += 2;
        let remaining = status_count - symbols.len();
        if chunk & 0x8000 == 0 {
            // Run length: a two-bit symbol and a 13-bit length.
            let symbol = ((chunk >> 13) & 3) as u8;
            if symbol == 3 {
                return None;
            }
            let run = usize::from(chunk & 0x1fff).min(remaining);
            symbols.extend(std::iter::repeat_n(symbol, run));
        } else if chunk & 0x4000 == 0 {
            // Status vector of fourteen one-bit symbols: received with a small delta, or not received.
            for i in 0..14.min(remaining) {
                symbols.push(((chunk >> (13 - i)) & 1) as u8);
            }
        } else {
            // Status vector of seven two-bit symbols.
            for i in 0..7.min(remaining) {
                let symbol = ((chunk >> (12 - 2 * i)) & 3) as u8;
                if symbol == 3 {
                    return None;
                }
                symbols.push(symbol);
            }
        }
    }
    let mut arrivals = Vec::with_capacity(status_count);
    let mut ticks = 0i64;
    for symbol in symbols {
        let delta = match symbol {
            1 => {
                let small = *packet.get(at)?;
                at += 1;
                i64::from(small)
            }
            2 => {
                let large = i16::from_be_bytes(packet.get(at..at + 2)?.try_into().ok()?);
                at += 2;
                i64::from(large)
            }
            _ => {
                arrivals.push(None);
                continue;
            }
        };
        ticks += delta;
        arrivals.push(Some(ticks * 250));
    }
    Some(TransportCc {
        base,
        reference,
        count,
        arrivals,
    })
}

/// One packet a feedback message described, with what the send history knew of it (`PacketResult`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct PacketResult {
    pub sent: Instant,
    /// Bytes on the wire (the whole datagram).
    pub size: usize,
    /// When it arrived, in microseconds of the far end's clock; `None` when it was reported lost.
    pub received: Option<i64>,
}

#[derive(Clone, Copy)]
struct Sent {
    at: Instant,
    size: u16,
    acked: bool,
}

/// Packets the history remembers. libwebrtc keeps 60 seconds (`kSendTimeHistoryWindow`); this bound is a memory limit
/// of ours, about eight seconds at 8 Mb/s, far longer than feedback takes.
const SENT_PACKETS: usize = 8192;
/// One reference time unit, in microseconds (`TransportFeedback::kBaseTimeTick`, 64 ms).
const BASE_TICK_US: i64 = 64_000;

/// What the transport numbered and sent, and the arrival clock its feedback is unwrapped on (the send-side part of
/// `TransportFeedbackAdapter`). One per media connection: the number is transport-wide, shared by audio, video and
/// retransmissions.
#[derive(Default)]
pub(crate) struct SentHistory {
    /// The sequence number of `entries[0]`.
    base: u16,
    entries: VecDeque<Sent>,
    /// The sequence number the next packet gets.
    next: u16,
    last_reference: Option<u32>,
    /// The far end's clock at the reference time of the newest message, in microseconds from the first one.
    offset_us: i64,
    /// Feedback messages, packets reported received and lost, and packets the history did not know, for the log.
    pub reports: u64,
    pub acked: u64,
    pub lost: u64,
    pub unknown: u64,
}

impl SentHistory {
    /// The number the next packet carries; it is taken only by [`Self::sent`].
    pub fn next(&self) -> u16 {
        self.next
    }

    /// The packet numbered [`Self::next`] left now, `size` bytes on the wire.
    pub fn sent(&mut self, size: usize, now: Instant) {
        if self.entries.is_empty() {
            self.base = self.next;
        }
        self.entries.push_back(Sent {
            at: now,
            size: size.min(usize::from(u16::MAX)) as u16,
            acked: false,
        });
        self.next = self.next.wrapping_add(1);
        if self.entries.len() > SENT_PACKETS {
            self.entries.pop_front();
            self.base = self.base.wrapping_add(1);
        }
    }

    /// The packets one feedback message reports, in sequence order. A packet already reported received is not repeated;
    /// one the history no longer knows is counted and skipped (`ProcessTransportFeedbackInner`).
    pub fn feedback(&mut self, report: &TransportCc) -> Vec<PacketResult> {
        self.reports += 1;
        // The reference time wraps every 2^24 ticks; the nearer of the two readings wins (`GetBaseDelta`).
        match self.last_reference {
            None => self.offset_us = 0,
            Some(last) => {
                let delta = ((report.reference.wrapping_sub(last) << 8) as i32) >> 8;
                self.offset_us += i64::from(delta) * BASE_TICK_US;
            }
        }
        self.last_reference = Some(report.reference);
        let mut out = Vec::with_capacity(report.arrivals.len());
        for (index, arrival) in report.arrivals.iter().enumerate() {
            let sequence = report.base.wrapping_add(index as u16);
            let offset = usize::from(sequence.wrapping_sub(self.base));
            let Some(sent) = self.entries.get_mut(offset) else {
                self.unknown += 1;
                continue;
            };
            match arrival {
                Some(at) => {
                    if sent.acked {
                        continue;
                    }
                    sent.acked = true;
                    self.acked += 1;
                    out.push(PacketResult {
                        sent: sent.at,
                        size: usize::from(sent.size),
                        received: Some(self.offset_us + at),
                    });
                }
                None => {
                    // A packet reported lost may still be reported received by a later message.
                    if sent.acked {
                        continue;
                    }
                    self.lost += 1;
                    out.push(PacketResult {
                        sent: sent.at,
                        size: usize::from(sent.size),
                        received: None,
                    });
                }
            }
        }
        out
    }
}

/// `kSendTimeGroupLength` of `delay_based_bwe.cc`: departures within 5 ms form one group.
const SEND_TIME_GROUP_US: i64 = 5_000;
/// `kBurstDeltaThreshold` and `kMaxBurstDuration` of `inter_arrival_delta.cc`.
const BURST_DELTA_US: i64 = 5_000;
const MAX_BURST_US: i64 = 100_000;
/// `InterArrivalDelta::kArrivalTimeOffsetThreshold` (3 s) and `kReorderedResetThreshold` (3).
const ARRIVAL_TIME_OFFSET_US: i64 = 3_000_000;
const REORDERED_RESET: u32 = 3;
/// `kStreamTimeOut` of `delay_based_bwe.cc`: after two seconds without feedback the detector starts over.
const STREAM_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Clone, Copy, Default)]
struct Group {
    size: usize,
    first_send: i64,
    send: i64,
    first_arrival: i64,
    /// The arrival of the group's last packet; `None` until the first packet (`IsFirstPacket`).
    complete: Option<i64>,
    last_system: i64,
}

/// Departures and arrivals in groups of one send burst; the deltas between consecutive groups (`InterArrivalDelta`).
#[derive(Default)]
struct InterArrival {
    current: Group,
    previous: Group,
    reordered: u32,
}

impl InterArrival {
    /// One received packet: departure and arrival in microseconds, `system` our clock when its feedback came. Gives the
    /// departure and arrival deltas of the group that just completed.
    fn compute(&mut self, send: i64, arrival: i64, system: i64, size: usize) -> Option<(i64, i64)> {
        let mut deltas = None;
        if self.current.complete.is_none() {
            self.current.send = send;
            self.current.first_send = send;
            self.current.first_arrival = arrival;
        } else if self.current.first_send > send {
            // Reordered before the group began: ignored.
            return None;
        } else if self.new_group(arrival, send) {
            if let Some(previous_complete) = self.previous.complete {
                let send_delta = self.current.send - self.previous.send;
                let arrival_delta = self.current.complete.unwrap_or(arrival) - previous_complete;
                let system_delta = self.current.last_system - self.previous.last_system;
                if arrival_delta - system_delta >= ARRIVAL_TIME_OFFSET_US {
                    // The far end's clock jumped.
                    *self = Self::default();
                    return None;
                }
                if arrival_delta < 0 {
                    self.reordered += 1;
                    if self.reordered >= REORDERED_RESET {
                        *self = Self::default();
                    }
                    return None;
                }
                self.reordered = 0;
                deltas = Some((send_delta, arrival_delta));
            }
            self.previous = self.current;
            self.current.first_send = send;
            self.current.send = send;
            self.current.first_arrival = arrival;
            self.current.size = 0;
        } else {
            self.current.send = self.current.send.max(send);
        }
        self.current.size += size;
        self.current.complete = Some(arrival);
        self.current.last_system = system;
        deltas
    }

    fn new_group(&self, arrival: i64, send: i64) -> bool {
        if self.current.complete.is_none() || self.belongs_to_burst(arrival, send) {
            return false;
        }
        send - self.current.first_send > SEND_TIME_GROUP_US
    }

    fn belongs_to_burst(&self, arrival: i64, send: i64) -> bool {
        let Some(complete) = self.current.complete else {
            return false;
        };
        let arrival_delta = arrival - complete;
        let send_delta = send - self.current.send;
        if send_delta == 0 {
            return true;
        }
        let propagation_delta = arrival_delta - send_delta;
        propagation_delta < 0
            && arrival_delta <= BURST_DELTA_US
            && arrival - self.current.first_arrival < MAX_BURST_US
    }
}

/// What the overuse detector concludes (`BandwidthUsage`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Usage {
    #[default]
    Normal,
    Underusing,
    Overusing,
}

/// `trendline_estimator.{h,cc}`: `kDefaultTrendlineWindowSize` (20 groups), `kDefaultTrendlineSmoothingCoeff` (0.9),
/// `kDefaultTrendlineThresholdGain` (4.0). The pinned `discord_voice.node` carries Discord's own trendline window
/// experiments (`trendline-window-size`, `trendline-window-duration-*`); which one a client is in is server side, so
/// the libwebrtc default applies.
const TRENDLINE_WINDOW: usize = 20;
const SMOOTHING: f64 = 0.9;
const THRESHOLD_GAIN: f64 = 4.0;
/// `kMaxAdaptOffsetMs`, `kOverUsingTimeThreshold`, `kMinNumDeltas`, `kDeltaCounterMax`.
const MAX_ADAPT_OFFSET_MS: f64 = 15.0;
const OVERUSING_TIME_MS: f64 = 10.0;
const MIN_NUM_DELTAS: u32 = 60;
const DELTA_COUNTER_MAX: u32 = 1000;
/// The threshold's adaptation rates `k_up_` and `k_down_`, its start `threshold_` and its clamp in `UpdateThreshold`.
const K_UP: f64 = 0.0087;
const K_DOWN: f64 = 0.039;
const INITIAL_THRESHOLD: f64 = 12.5;
const THRESHOLD_MIN: f64 = 6.0;
const THRESHOLD_MAX: f64 = 600.0;
/// `kMaxTimeDeltaMs` of `UpdateThreshold`.
const MAX_THRESHOLD_STEP_MS: i64 = 100;

struct Trendline {
    num_of_deltas: u32,
    first_arrival_ms: Option<i64>,
    accumulated_delay: f64,
    smoothed_delay: f64,
    /// (arrival since the first, smoothed delay) of the last `TRENDLINE_WINDOW` groups.
    history: VecDeque<(f64, f64)>,
    threshold: f64,
    previous_trend: f64,
    last_update_ms: Option<i64>,
    time_over_using: f64,
    overuse_counter: u32,
    state: Usage,
}

impl Default for Trendline {
    fn default() -> Self {
        Self {
            num_of_deltas: 0,
            first_arrival_ms: None,
            accumulated_delay: 0.0,
            smoothed_delay: 0.0,
            history: VecDeque::with_capacity(TRENDLINE_WINDOW + 1),
            threshold: INITIAL_THRESHOLD,
            previous_trend: 0.0,
            last_update_ms: None,
            time_over_using: -1.0,
            overuse_counter: 0,
            state: Usage::Normal,
        }
    }
}

impl Trendline {
    /// `UpdateTrendline`: one group's deltas in milliseconds, and its arrival.
    fn update(&mut self, recv_delta_ms: f64, send_delta_ms: f64, arrival_ms: i64) {
        let delta_ms = recv_delta_ms - send_delta_ms;
        self.num_of_deltas = (self.num_of_deltas + 1).min(DELTA_COUNTER_MAX);
        let first = *self.first_arrival_ms.get_or_insert(arrival_ms);
        self.accumulated_delay += delta_ms;
        self.smoothed_delay =
            SMOOTHING * self.smoothed_delay + (1.0 - SMOOTHING) * self.accumulated_delay;
        self.history
            .push_back(((arrival_ms - first) as f64, self.smoothed_delay));
        if self.history.len() > TRENDLINE_WINDOW {
            self.history.pop_front();
        }
        let mut trend = self.previous_trend;
        if self.history.len() == TRENDLINE_WINDOW
            && let Some(slope) = linear_fit_slope(&self.history)
        {
            trend = slope;
        }
        self.detect(trend, send_delta_ms, arrival_ms);
    }

    fn detect(&mut self, trend: f64, send_delta_ms: f64, now_ms: i64) {
        if self.num_of_deltas < 2 {
            self.state = Usage::Normal;
            return;
        }
        let modified_trend =
            f64::from(self.num_of_deltas.min(MIN_NUM_DELTAS)) * trend * THRESHOLD_GAIN;
        if modified_trend > self.threshold {
            if self.time_over_using == -1.0 {
                // Assume half the time since the last sample was spent over-using.
                self.time_over_using = send_delta_ms / 2.0;
            } else {
                self.time_over_using += send_delta_ms;
            }
            self.overuse_counter += 1;
            if self.time_over_using > OVERUSING_TIME_MS
                && self.overuse_counter > 1
                && trend >= self.previous_trend
            {
                self.time_over_using = 0.0;
                self.overuse_counter = 0;
                self.state = Usage::Overusing;
            }
        } else if modified_trend < -self.threshold {
            self.time_over_using = -1.0;
            self.overuse_counter = 0;
            self.state = Usage::Underusing;
        } else {
            self.time_over_using = -1.0;
            self.overuse_counter = 0;
            self.state = Usage::Normal;
        }
        self.previous_trend = trend;
        self.update_threshold(modified_trend, now_ms);
    }

    fn update_threshold(&mut self, modified_trend: f64, now_ms: i64) {
        let last = *self.last_update_ms.get_or_insert(now_ms);
        if modified_trend.abs() > self.threshold + MAX_ADAPT_OFFSET_MS {
            // A spike, for instance a route change, is not adapted to.
            self.last_update_ms = Some(now_ms);
            return;
        }
        let k = if modified_trend.abs() < self.threshold {
            K_DOWN
        } else {
            K_UP
        };
        let time_delta_ms = (now_ms - last).min(MAX_THRESHOLD_STEP_MS);
        self.threshold += k * (modified_trend.abs() - self.threshold) * time_delta_ms as f64;
        self.threshold = self.threshold.clamp(THRESHOLD_MIN, THRESHOLD_MAX);
        self.last_update_ms = Some(now_ms);
    }
}

/// `LinearFitSlope`: least squares slope of the smoothed delay over the arrival time.
fn linear_fit_slope(points: &VecDeque<(f64, f64)>) -> Option<f64> {
    let n = points.len() as f64;
    let (sum_x, sum_y) = points
        .iter()
        .fold((0.0, 0.0), |(x, y), point| (x + point.0, y + point.1));
    let (x_avg, y_avg) = (sum_x / n, sum_y / n);
    let (mut numerator, mut denominator) = (0.0, 0.0);
    for (x, y) in points {
        numerator += (x - x_avg) * (y - y_avg);
        denominator += (x - x_avg) * (x - x_avg);
    }
    (denominator != 0.0).then(|| numerator / denominator)
}

/// `bitrate_estimator.cc`: `kInitialRateWindowMs` (500), `kRateWindowMs` (150), the uncertainty scale (10.0), the start
/// variance (50) and its growth per update (5).
const INITIAL_RATE_WINDOW_MS: i64 = 500;
const RATE_WINDOW_MS: i64 = 150;
const UNCERTAINTY_SCALE: f32 = 10.0;
const INITIAL_ESTIMATE_VAR: f32 = 50.0;
const ESTIMATE_VAR_GROWTH: f32 = 5.0;

/// The rate the far end acknowledged receiving, a Bayesian estimate over 150 ms windows (`BitrateEstimator`, fed by
/// `AcknowledgedBitrateEstimator` with each received packet's size at its arrival).
struct AckedRate {
    sum: i64,
    current_window_ms: i64,
    previous_ms: Option<i64>,
    estimate_kbps: f32,
    estimate_var: f32,
}

impl Default for AckedRate {
    fn default() -> Self {
        Self {
            sum: 0,
            current_window_ms: 0,
            previous_ms: None,
            estimate_kbps: -1.0,
            estimate_var: INITIAL_ESTIMATE_VAR,
        }
    }
}

impl AckedRate {
    fn update(&mut self, at_ms: i64, bytes: usize) {
        let window = if self.estimate_kbps < 0.0 {
            INITIAL_RATE_WINDOW_MS
        } else {
            RATE_WINDOW_MS
        };
        let Some(sample) = self.update_window(at_ms, bytes as i64, window) else {
            return;
        };
        if self.estimate_kbps < 0.0 {
            self.estimate_kbps = sample;
            return;
        }
        // `uncertainty_symmetry_cap` is 0, so the denominator is the estimate alone.
        let uncertainty =
            UNCERTAINTY_SCALE * (self.estimate_kbps - sample).abs() / self.estimate_kbps;
        let sample_var = uncertainty * uncertainty;
        let predicted_var = self.estimate_var + ESTIMATE_VAR_GROWTH;
        self.estimate_kbps = (sample_var * self.estimate_kbps + predicted_var * sample)
            / (sample_var + predicted_var);
        self.estimate_kbps = self.estimate_kbps.max(0.0);
        self.estimate_var = sample_var * predicted_var / (sample_var + predicted_var);
    }

    fn update_window(&mut self, now_ms: i64, bytes: i64, window_ms: i64) -> Option<f32> {
        if self.previous_ms.is_some_and(|previous| now_ms < previous) {
            self.previous_ms = None;
            self.sum = 0;
            self.current_window_ms = 0;
        }
        if let Some(previous) = self.previous_ms {
            self.current_window_ms += now_ms - previous;
            if now_ms - previous > window_ms {
                self.sum = 0;
                self.current_window_ms %= window_ms;
            }
        }
        self.previous_ms = Some(now_ms);
        let mut sample = None;
        if self.current_window_ms >= window_ms {
            sample = Some(8.0 * self.sum as f32 / window_ms as f32);
            self.current_window_ms -= window_ms;
            self.sum = 0;
        }
        self.sum += bytes;
        sample
    }

    /// Bits per second, once a window completed.
    fn bitrate(&self) -> Option<f64> {
        (self.estimate_kbps >= 0.0).then(|| f64::from(self.estimate_kbps) * 1000.0)
    }
}

/// `link_capacity_estimator.cc`: the smoothing on overuse (0.05), the deviation's clamp (0.4 to 2.5) and start (0.4),
/// bounds at three deviations.
const CAPACITY_ALPHA: f64 = 0.05;
const DEVIATION_MIN: f64 = 0.4;
const DEVIATION_MAX: f64 = 2.5;

/// What the link carried each time it overused (`LinkCapacityEstimator`).
struct LinkCapacity {
    estimate_kbps: Option<f64>,
    deviation_kbps: f64,
}

impl Default for LinkCapacity {
    fn default() -> Self {
        Self {
            estimate_kbps: None,
            deviation_kbps: DEVIATION_MIN,
        }
    }
}

impl LinkCapacity {
    fn deviation_estimate_kbps(&self, estimate: f64) -> f64 {
        (self.deviation_kbps * estimate).sqrt()
    }
    fn upper_bound(&self) -> f64 {
        self.estimate_kbps.map_or(f64::INFINITY, |estimate| {
            (estimate + 3.0 * self.deviation_estimate_kbps(estimate)) * 1000.0
        })
    }
    fn lower_bound(&self) -> f64 {
        self.estimate_kbps.map_or(0.0, |estimate| {
            (estimate - 3.0 * self.deviation_estimate_kbps(estimate)).max(0.0) * 1000.0
        })
    }
    fn on_overuse(&mut self, acknowledged_bps: f64) {
        let sample = acknowledged_bps / 1000.0;
        let estimate = match self.estimate_kbps {
            None => sample,
            Some(estimate) => (1.0 - CAPACITY_ALPHA) * estimate + CAPACITY_ALPHA * sample,
        };
        self.estimate_kbps = Some(estimate);
        let norm = estimate.max(1.0);
        let error = estimate - sample;
        self.deviation_kbps =
            (1.0 - CAPACITY_ALPHA) * self.deviation_kbps + CAPACITY_ALPHA * error * error / norm;
        self.deviation_kbps = self.deviation_kbps.clamp(DEVIATION_MIN, DEVIATION_MAX);
    }
}

/// `aimd_rate_control.cc`: `kDefaultRtt` (200 ms, the round trip it assumes without RTCP round trip times, which this
/// client never measures since it sends no sender reports), `kDefaultBackoffFactor` (0.85), the 5 kb/s extra backoff,
/// the increase limit of 1.5 times the acknowledged rate plus 10 kb/s, the 8% a second multiplicative increase with its
/// 1 kb/s minimum, and `GetNearMaxIncreaseRateBpsPerSecond` (`kFrameInterval` 1/30 s, `kPacketSize` 1200 bytes, a 100 ms
/// detector delay, twice the response time, `kMinIncreaseRateBpsPerSecond` 4000).
const DEFAULT_RTT: Duration = Duration::from_millis(200);
const BACKOFF: f64 = 0.85;
const BACKOFF_TERM_BPS: f64 = 5_000.0;
const INCREASE_LIMIT_FACTOR: f64 = 1.5;
const INCREASE_LIMIT_TERM_BPS: f64 = 10_000.0;
const MULTIPLICATIVE_ALPHA: f64 = 1.08;
const MIN_MULTIPLICATIVE_BPS: f64 = 1_000.0;
const FRAME_INTERVAL_S: f64 = 1.0 / 30.0;
const PACKET_BYTES: f64 = 1200.0;
const DETECTOR_DELAY: Duration = Duration::from_millis(100);
const MIN_INCREASE_BPS_PER_S: f64 = 4_000.0;
/// `TimeToReduceFurther`: the round trip clamped to 10 to 200 ms, and half the estimate.
const REDUCTION_INTERVAL_MIN: Duration = Duration::from_millis(10);
const REDUCTION_INTERVAL_MAX: Duration = Duration::from_millis(200);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RateState {
    Hold,
    Increase,
    Decrease,
}

/// `AimdRateControl` on the send side, started at the encoder's top rate (`SetStartBitrate`) and floored at the
/// official minimum.
pub(crate) struct Aimd {
    minimum: f64,
    current: f64,
    latest_throughput: f64,
    link: LinkCapacity,
    state: RateState,
    last_change: Option<Instant>,
    rtt: Duration,
}

impl Aimd {
    fn new(start: u32, minimum: u32) -> Self {
        Self {
            minimum: f64::from(minimum),
            current: f64::from(start),
            latest_throughput: f64::from(start),
            link: LinkCapacity::default(),
            state: RateState::Hold,
            last_change: None,
            rtt: DEFAULT_RTT,
        }
    }

    fn update(&mut self, usage: Usage, throughput: Option<f64>, now: Instant) -> f64 {
        let estimated = throughput.unwrap_or(self.latest_throughput);
        if let Some(throughput) = throughput {
            self.latest_throughput = throughput;
        }
        match usage {
            Usage::Normal if self.state == RateState::Hold => {
                self.last_change = Some(now);
                self.state = RateState::Increase;
            }
            Usage::Overusing => self.state = RateState::Decrease,
            Usage::Underusing => self.state = RateState::Hold,
            Usage::Normal => {}
        }
        let mut new = None;
        match self.state {
            RateState::Hold => {}
            RateState::Increase => {
                if estimated > self.link.upper_bound() {
                    self.link.estimate_kbps = None;
                }
                let limit = INCREASE_LIMIT_FACTOR * estimated + INCREASE_LIMIT_TERM_BPS;
                if self.current < limit {
                    let increased = if self.link.estimate_kbps.is_some() {
                        self.current + self.additive_increase(now)
                    } else {
                        self.current + self.multiplicative_increase(now)
                    };
                    new = Some(increased.min(limit));
                }
                self.last_change = Some(now);
            }
            RateState::Decrease => {
                let mut decreased = estimated * BACKOFF;
                if decreased > BACKOFF_TERM_BPS {
                    decreased -= BACKOFF_TERM_BPS;
                }
                if decreased > self.current
                    && let Some(capacity) = self.link.estimate_kbps
                {
                    decreased = BACKOFF * capacity * 1000.0;
                }
                if decreased < self.current {
                    new = Some(decreased);
                }
                if estimated < self.link.lower_bound() {
                    self.link.estimate_kbps = None;
                }
                self.link.on_overuse(estimated);
                self.state = RateState::Hold;
                self.last_change = Some(now);
            }
        }
        self.current = new.unwrap_or(self.current).max(self.minimum);
        self.current
    }

    fn multiplicative_increase(&self, now: Instant) -> f64 {
        let alpha = match self.last_change {
            Some(last) => MULTIPLICATIVE_ALPHA
                .powf(now.saturating_duration_since(last).as_secs_f64().min(1.0)),
            None => MULTIPLICATIVE_ALPHA,
        };
        (self.current * (alpha - 1.0)).max(MIN_MULTIPLICATIVE_BPS)
    }

    fn additive_increase(&self, now: Instant) -> f64 {
        let period = self.last_change.map_or(0.0, |last| {
            now.saturating_duration_since(last).as_secs_f64()
        });
        self.near_max_increase_per_second() * period
    }

    fn near_max_increase_per_second(&self) -> f64 {
        let frame_bits = self.current * FRAME_INTERVAL_S;
        let packets_per_frame = (frame_bits / 8.0 / PACKET_BYTES).ceil().max(1.0);
        let average_packet_bits = frame_bits / packets_per_frame;
        let response = (self.rtt + DETECTOR_DELAY) * 2;
        (average_packet_bits / response.as_secs_f64()).max(MIN_INCREASE_BPS_PER_S)
    }

    fn time_to_reduce_further(&self, now: Instant, throughput: f64) -> bool {
        let interval = self
            .rtt
            .clamp(REDUCTION_INTERVAL_MIN, REDUCTION_INTERVAL_MAX);
        if self
            .last_change
            .is_none_or(|last| now.saturating_duration_since(last) >= interval)
        {
            return true;
        }
        throughput < 0.5 * self.current
    }

    fn set_estimate(&mut self, bitrate: f64, now: Instant) {
        self.current = bitrate.max(self.minimum);
        self.last_change = Some(now);
    }
}

/// The delay-based estimator (`DelayBasedBwe`): every feedback message's received packets go through the packet
/// groups into the trendline; the AIMD controller moves on each message, and while over-using at most once per round
/// trip.
pub(crate) struct DelayBased {
    epoch: Instant,
    inter_arrival: InterArrival,
    trendline: Trendline,
    last_seen: Option<Instant>,
    acked: AckedRate,
    rate: Aimd,
    /// Overuse detections, for the log.
    pub overuses: u64,
}

impl DelayBased {
    pub fn new(start: u32, minimum: u32, now: Instant) -> Self {
        Self {
            epoch: now,
            inter_arrival: InterArrival::default(),
            trendline: Trendline::default(),
            last_seen: None,
            acked: AckedRate::default(),
            rate: Aimd::new(start, minimum),
            overuses: 0,
        }
    }

    pub fn usage(&self) -> Usage {
        self.trendline.state
    }

    /// The acknowledged rate in bits per second, once known.
    pub fn acknowledged(&self) -> Option<u32> {
        self.acked.bitrate().map(|rate| rate as u32)
    }

    /// The current delay-based estimate in bits per second.
    pub fn estimate(&self) -> u32 {
        self.rate.current.min(f64::from(u32::MAX)) as u32
    }

    /// One feedback message's packets, which reached us at `now`. The new estimate when it moved.
    pub fn feedback(&mut self, packets: &[PacketResult], now: Instant) -> Option<u32> {
        let mut received: Vec<(i64, &PacketResult)> = packets
            .iter()
            .filter_map(|packet| Some((packet.received?, packet)))
            .collect();
        if received.is_empty() {
            return None;
        }
        received.sort_by_key(|(at, _)| *at);
        // The acknowledged rate first, as `GoogCcNetworkController::OnTransportPacketsFeedback` orders them.
        for (at, packet) in &received {
            self.acked.update(at.div_euclid(1000), packet.size);
        }
        if self
            .last_seen
            .is_none_or(|last| now.saturating_duration_since(last) > STREAM_TIMEOUT)
        {
            self.inter_arrival = InterArrival::default();
            self.trendline = Trendline::default();
        }
        self.last_seen = Some(now);
        let system = now.saturating_duration_since(self.epoch).as_micros() as i64;
        let before = self.trendline.state;
        for (at, packet) in &received {
            // Packets from before this estimator (audio sent before the video source) carry no usable departure.
            let Some(sent) = packet.sent.checked_duration_since(self.epoch) else {
                continue;
            };
            let sent = sent.as_micros() as i64;
            if let Some((send_delta, recv_delta)) =
                self.inter_arrival.compute(sent, *at, system, packet.size)
            {
                self.trendline.update(
                    recv_delta as f64 / 1000.0,
                    send_delta as f64 / 1000.0,
                    at.div_euclid(1000),
                );
            }
        }
        let usage = self.trendline.state;
        if usage == Usage::Overusing && before != Usage::Overusing {
            self.overuses += 1;
        }
        let acked = self.acked.bitrate();
        if usage == Usage::Overusing {
            match acked {
                Some(acked) if self.rate.time_to_reduce_further(now, acked) => {
                    self.rate.update(usage, Some(acked), now);
                }
                // No acknowledged rate yet: halve, at most every reduction interval (`InitialTimeToReduceFurther`).
                None if self
                    .rate
                    .time_to_reduce_further(now, self.rate.current / 2.0 - 1.0) =>
                {
                    self.rate.set_estimate(self.rate.current / 2.0, now);
                }
                _ => return None,
            }
        } else {
            self.rate.update(usage, acked, now);
        }
        Some(self.estimate())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport_cc::TransportFeedback;

    #[test]
    fn our_own_feedback_parses_back_with_loss_wrap_and_negative_deltas() {
        let start = std::time::Instant::now();
        let mut feedback = TransportFeedback::new(start);
        for (seq, ms) in [(65534, 10), (0, 11), (65535, 12), (1, 82)] {
            feedback.receive(77, seq, start + Duration::from_millis(ms));
        }
        let (header, body) = feedback
            .report(9, start + Duration::from_millis(100))
            .unwrap();
        let packet = [&header[..], &body].concat();
        let parsed = parse_transport_cc(&packet).unwrap();
        assert_eq!((parsed.base, parsed.reference, parsed.count), (65534, 0, 0));
        assert_eq!(
            parsed.arrivals,
            [Some(10_000), Some(12_000), Some(11_000), Some(82_000)]
        );
        // Run-length and one-bit chunks, as libwebrtc's own sender of feedback writes them: 3 received (small deltas
        // of 1, 2, 3 ticks), 2 lost by a run of zeros, then a one-bit vector with one received.
        let mut packet = vec![0x8f, 205, 0, 0, 0, 0, 0, 9, 0, 0, 0, 1, 0, 10, 0, 6];
        packet.extend([0, 0, 2, 5]);
        packet.extend((0x2000u16 | 3).to_be_bytes());
        packet.extend(2u16.to_be_bytes());
        packet.extend((0x8000u16 | 1 << 13).to_be_bytes());
        packet.extend([1, 2, 3, 4]);
        let parsed = parse_transport_cc(&packet).unwrap();
        assert_eq!((parsed.base, parsed.reference, parsed.count), (10, 2, 5));
        assert_eq!(
            parsed.arrivals,
            [Some(250), Some(750), Some(1500), None, None, Some(2500)]
        );
        // Truncated deltas or a reserved symbol: nothing.
        assert!(parse_transport_cc(&packet[..packet.len() - 1]).is_none());
        let mut reserved = packet.clone();
        reserved[20] = 0x60;
        assert!(parse_transport_cc(&reserved).is_none());
    }

    #[test]
    fn the_history_matches_feedback_once_and_unwraps_the_reference_time() {
        let start = Instant::now();
        let mut history = SentHistory::default();
        for n in 0..4u64 {
            assert_eq!(history.next(), n as u16);
            history.sent(1000 + n as usize, start + Duration::from_millis(n));
        }
        let report = TransportCc {
            base: 1,
            reference: (1 << 24) - 1,
            count: 0,
            arrivals: vec![Some(0), None, Some(500), Some(900)],
        };
        let results = history.feedback(&report);
        assert_eq!(results.len(), 3, "sequence 4 was never sent");
        assert_eq!(results[0].size, 1001);
        assert_eq!(results[1].received, None);
        assert_eq!((history.acked, history.lost, history.unknown), (2, 1, 1));
        // The next message wraps the 24-bit reference forwards by one tick (64 ms) and reports 2 after all.
        let report = TransportCc {
            base: 2,
            reference: 0,
            count: 1,
            arrivals: vec![Some(100), Some(0)],
        };
        let results = history.feedback(&report);
        assert_eq!(results.len(), 1, "3 was already acknowledged");
        assert_eq!(results[0].received, Some(64_100));
    }

    /// Packets sent at a steady 1 ms apart that arrive further and further apart: the queue grows, the detector says so.
    #[test]
    fn a_growing_queue_is_overuse_and_a_steady_one_is_not() {
        for (stretch, expected) in [(0, Usage::Normal), (300, Usage::Overusing)] {
            let start = Instant::now();
            let mut estimator = DelayBased::new(2_000_000, 100_000, start);
            let mut arrival = 0i64;
            let mut state = Usage::Normal;
            for batch in 0..30u64 {
                let packets: Vec<PacketResult> = (0..100u64)
                    .map(|n| {
                        let index = batch * 100 + n;
                        arrival += 1000 + stretch;
                        PacketResult {
                            sent: start + Duration::from_millis(index),
                            size: 1200,
                            received: Some(arrival),
                        }
                    })
                    .collect();
                let now = start + Duration::from_millis(batch * 100 + 150);
                estimator.feedback(&packets, now);
                if estimator.usage() == Usage::Overusing {
                    state = Usage::Overusing;
                }
            }
            assert_eq!(state, expected, "stretch {stretch} us");
        }
    }

    #[test]
    fn aimd_backs_off_to_085_of_the_acknowledged_rate_then_grows() {
        let start = Instant::now();
        let mut aimd = Aimd::new(5_000_000, 150_000);
        let after = aimd.update(Usage::Overusing, Some(2_000_000.0), start);
        assert_eq!(after, 2_000_000.0 * 0.85 - 5_000.0);
        // Hold, then increase: additively now that a link capacity is known (about 14 kb/s a second here).
        aimd.update(
            Usage::Normal,
            Some(1_700_000.0),
            start + Duration::from_millis(100),
        );
        let grown = aimd.update(
            Usage::Normal,
            Some(1_700_000.0),
            start + Duration::from_millis(1100),
        );
        let step = grown - after;
        assert!((10_000.0..20_000.0).contains(&step), "{step}");
        // Without a capacity estimate it grows 8% a second, up to 1.5 times the acknowledged rate plus 10 kb/s.
        let mut aimd = Aimd::new(1_000_000, 150_000);
        aimd.update(Usage::Normal, Some(1_000_000.0), start);
        let grown = aimd.update(
            Usage::Normal,
            Some(1_000_000.0),
            start + Duration::from_secs(1),
        );
        // The first step after the hold is the 1 kb/s minimum, since no time has passed.
        assert!((grown - 1_001_000.0 * 1.08).abs() < 1.0, "{grown}");
        for n in 2..60 {
            aimd.update(
                Usage::Normal,
                Some(1_000_000.0),
                start + Duration::from_secs(n),
            );
        }
        assert_eq!(aimd.current, 1_510_000.0);
    }
}
