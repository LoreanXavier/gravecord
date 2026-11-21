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
//! * the glue: `modules/congestion_controller/goog_cc/delay_based_bwe.cc`;
//! * probing: `modules/congestion_controller/goog_cc/probe_controller.cc` (when to probe and at what rate),
//!   `modules/pacing/bitrate_prober.cc` (the clusters the pacer sends) and
//!   `modules/congestion_controller/goog_cc/probe_bitrate_estimator.cc` (what a cluster measured);
//! * the application-limited region: `modules/congestion_controller/goog_cc/alr_detector.cc` with
//!   `modules/pacing/interval_budget.cc`, and its settings in `rtc_base/experiments/alr_experiment.cc`.
//!
//! The pinned `discord_voice.node` carries all of these files (their paths and log lines are in it), together with the
//! default-on screen share settings string `1.0,2875,80,40,-60,3` of `alr_experiment.cc`.
//!
//! Neither vendored plumbing reference has one: Serein receives transport feedback counts only, Yapfast links the whole
//! libwebrtc. Left out, as the native engine runs them only behind field trials or with estimators this client does not
//! have: the network state predictor and network state estimate (and every probing rule that needs one), repeated
//! initial probing, the separate audio detector, and loss-based BWE v2.
use std::{
    collections::{BTreeMap, VecDeque},
    time::Duration,
};
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

/// The probe cluster a packet was sent in (the probe part of `PacedPacketInfo`): its id, and the packets and bytes the
/// cluster was asked to send.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ProbeTag {
    pub id: u32,
    pub min_probes: u32,
    pub min_bytes: u32,
}

/// One packet a feedback message described, with what the send history knew of it (`PacketResult`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct PacketResult {
    pub sent: Instant,
    /// Bytes on the wire (the whole datagram).
    pub size: usize,
    /// When it arrived, in microseconds of the far end's clock; `None` when it was reported lost.
    pub received: Option<i64>,
    /// The probe cluster it belonged to, if any.
    pub probe: Option<ProbeTag>,
}

#[derive(Clone, Copy)]
struct Sent {
    at: Instant,
    size: u16,
    acked: bool,
    probe: Option<ProbeTag>,
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
    /// The probe cluster the packets sealed now belong to; the pacer sets it around a probe.
    pub probe: Option<ProbeTag>,
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
            probe: self.probe,
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
                        probe: sent.probe,
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
                        probe: sent.probe,
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
    /// When the application-limited region last ended (`SetAlrEndedTime` of `AcknowledgedBitrateEstimator`).
    alr_ended: Option<Instant>,
}

impl Default for AckedRate {
    fn default() -> Self {
        Self {
            sum: 0,
            current_window_ms: 0,
            previous_ms: None,
            estimate_kbps: -1.0,
            estimate_var: INITIAL_ESTIMATE_VAR,
            alr_ended: None,
        }
    }
}

/// `BitrateEstimator::ExpectFastRateChange`: the variance added once the first packet sent after an application-limited
/// region is acknowledged, so the estimate follows the faster rate within a few windows.
const FAST_RATE_CHANGE_VAR: f32 = 200.0;

impl AckedRate {
    /// One acknowledged packet that left at `sent` (`AcknowledgedBitrateEstimator::IncomingPacketFeedbackVector`). The
    /// uncertainty scale inside the region (`scale_alr`) defaults to the one outside it, so being in it changes nothing
    /// here.
    fn packet(&mut self, sent: Instant, at_ms: i64, bytes: usize) {
        if self.alr_ended.is_some_and(|ended| sent > ended) {
            self.estimate_var += FAST_RATE_CHANGE_VAR;
            self.alr_ended = None;
        }
        self.update(at_ms, bytes);
    }

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
pub(crate) const DEFAULT_RTT: Duration = Duration::from_millis(200);
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
    /// `SetInApplicationLimitedRegion`.
    in_alr: bool,
    /// `WebRTC-DontIncreaseDelayBasedBweInAlr`: no increase while application limited (only a probe raises the estimate
    /// then). The pinned engine names the field trial, but it is off unless a field trial string switches it on, which
    /// is server side; its default, off, applies.
    no_increase_in_alr: bool,
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
            in_alr: false,
            no_increase_in_alr: false,
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
                let mut limit = INCREASE_LIMIT_FACTOR * estimated + INCREASE_LIMIT_TERM_BPS;
                if self.in_alr && self.no_increase_in_alr {
                    limit = self.current;
                }
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

/// What one feedback message did to the delay-based estimate (`DelayBasedBwe::Result`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct DelayUpdate {
    /// The new estimate, when it was updated.
    pub estimate: Option<u32>,
    /// The estimate is a probe's result.
    pub probe: bool,
    /// The detector went from under-using back to normal: the queue a drop drained is gone.
    pub recovered: bool,
    /// The detector's state after the message.
    pub usage: Usage,
}

/// `kProbeDropThroughputFraction` of `goog_cc_network_control.cc`: a probe result far below the acknowledged rate is
/// raised to this share of it (`WebRTC-Bwe-LimitProbesLowerThanThroughputEstimate`, on unless disabled).
const PROBE_DROP_THROUGHPUT_FRACTION: f64 = 0.85;

/// The delay-based estimator (`DelayBasedBwe`): every feedback message's received packets go through the packet
/// groups into the trendline; the AIMD controller moves on each message, and while over-using at most once per round
/// trip. With it, as `GoogCcNetworkController::OnTransportPacketsFeedback` runs them, the acknowledged rate and the
/// probe results.
pub(crate) struct DelayBased {
    epoch: Instant,
    inter_arrival: InterArrival,
    trendline: Trendline,
    last_seen: Option<Instant>,
    acked: AckedRate,
    probes: ProbeEstimator,
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
            probes: ProbeEstimator::default(),
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

    /// Probe clusters that gave a result and that were refused, and the last result in bits per second, for the log.
    pub fn probe_counts(&self) -> (u64, u64, u32) {
        (
            self.probes.successes,
            self.probes.failures,
            self.probes.latest.unwrap_or(0.0) as u32,
        )
    }

    /// The application-limited region ended at `now` (`SetAlrEndedTime`).
    pub fn alr_ended(&mut self, now: Instant) {
        self.acked.alr_ended = Some(now);
    }

    /// One feedback message's packets, which reached us at `now`; `in_alr` whether the sender is application limited.
    pub fn feedback(
        &mut self,
        packets: &[PacketResult],
        now: Instant,
        in_alr: bool,
    ) -> DelayUpdate {
        let mut received: Vec<(i64, &PacketResult)> = packets
            .iter()
            .filter_map(|packet| Some((packet.received?, packet)))
            .collect();
        if received.is_empty() {
            return DelayUpdate {
                usage: self.trendline.state,
                ..DelayUpdate::default()
            };
        }
        received.sort_by_key(|(at, _)| *at);
        // The acknowledged rate first, then the probes, as `GoogCcNetworkController::OnTransportPacketsFeedback` orders
        // them.
        for (at, packet) in &received {
            self.acked
                .packet(packet.sent, at.div_euclid(1000), packet.size);
        }
        let acked = self.acked.bitrate();
        for (at, packet) in &received {
            // Cluster ids start again with each estimator; a probe of an earlier one on this transport (a camera
            // turned off and on) is not this one's.
            if let Some(tag) = packet.probe
                && packet.sent >= self.epoch
            {
                self.probes.packet(packet, tag, *at);
            }
        }
        let mut probe = self.probes.latest_new.take();
        if let (Some(result), Some(acked)) = (probe.as_mut(), acked) {
            // "Slightly below" the acknowledged rate, to drain the queues if we are actually over-using; never above the
            // current estimate.
            let limit = self
                .rate
                .current
                .min(acked * PROBE_DROP_THROUGHPUT_FRACTION);
            *result = result.max(limit);
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
        let mut previous = before;
        let mut recovered = false;
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
            if previous == Usage::Underusing && self.trendline.state == Usage::Normal {
                recovered = true;
            }
            previous = self.trendline.state;
        }
        let usage = self.trendline.state;
        if usage == Usage::Overusing && before != Usage::Overusing {
            self.overuses += 1;
        }
        self.rate.in_alr = in_alr;
        let mut update = DelayUpdate {
            usage,
            ..DelayUpdate::default()
        };
        // `MaybeUpdateEstimate`.
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
                _ => return update,
            }
        } else if let Some(result) = probe {
            self.rate.set_estimate(result, now);
            update.probe = true;
        } else {
            self.rate.update(usage, acked, now);
            update.recovered = recovered;
        }
        update.estimate = Some(self.estimate());
        update
    }
}

/// `probe_bitrate_estimator.cc`: `kMinReceivedProbesRatio` and `kMinReceivedBytesRatio` (0.80 each), `kMaxValidRatio`
/// (2.0), `kMinRatioForUnsaturatedLink` (0.9), `kTargetUtilizationFraction` (0.95), `kMaxClusterHistory` and
/// `kMaxProbeInterval` (1 s each).
const MIN_RECEIVED_PROBES_RATIO: f64 = 0.80;
const MIN_RECEIVED_BYTES_RATIO: f64 = 0.80;
const MAX_VALID_RATIO: f64 = 2.0;
const MIN_RATIO_FOR_UNSATURATED_LINK: f64 = 0.9;
const TARGET_UTILIZATION_FRACTION: f64 = 0.95;
const MAX_CLUSTER_HISTORY_US: i64 = 1_000_000;
const MAX_PROBE_INTERVAL: Duration = Duration::from_secs(1);

/// What feedback reported of one cluster (`AggregatedCluster`).
#[derive(Clone, Copy)]
struct AggregatedCluster {
    num_probes: u32,
    first_send: Instant,
    last_send: Instant,
    first_receive: i64,
    last_receive: i64,
    size_last_send: usize,
    size_first_receive: usize,
    size_total: usize,
}

/// The rate a probe cluster measured, from its send and receive intervals (`ProbeBitrateEstimator`).
#[derive(Default)]
struct ProbeEstimator {
    clusters: BTreeMap<u32, AggregatedCluster>,
    /// `estimated_data_rate_`, taken by each feedback message (`FetchAndResetLastEstimatedBitrate`).
    latest_new: Option<f64>,
    /// For the log: clusters measured, refused, and the last result.
    successes: u64,
    failures: u64,
    latest: Option<f64>,
}

impl ProbeEstimator {
    /// `HandleProbeAndEstimateBitrate`: one received packet of a cluster, `received` in microseconds of the far end.
    fn packet(&mut self, packet: &PacketResult, tag: ProbeTag, received: i64) {
        self.clusters
            .retain(|_, cluster| cluster.last_receive + MAX_CLUSTER_HISTORY_US >= received);
        let cluster = self.clusters.entry(tag.id).or_insert(AggregatedCluster {
            num_probes: 0,
            first_send: packet.sent,
            last_send: packet.sent,
            first_receive: received,
            last_receive: received,
            size_last_send: packet.size,
            size_first_receive: packet.size,
            size_total: 0,
        });
        if packet.sent < cluster.first_send {
            cluster.first_send = packet.sent;
        }
        if packet.sent > cluster.last_send {
            cluster.last_send = packet.sent;
            cluster.size_last_send = packet.size;
        }
        if received < cluster.first_receive {
            cluster.first_receive = received;
            cluster.size_first_receive = packet.size;
        }
        if received > cluster.last_receive {
            cluster.last_receive = received;
        }
        cluster.size_total += packet.size;
        cluster.num_probes += 1;
        let min_probes = (f64::from(tag.min_probes) * MIN_RECEIVED_PROBES_RATIO) as u32;
        let min_size = f64::from(tag.min_bytes) * MIN_RECEIVED_BYTES_RATIO;
        if cluster.num_probes < min_probes || (cluster.size_total as f64) < min_size {
            return;
        }
        let cluster = *cluster;
        let send_interval = cluster
            .last_send
            .saturating_duration_since(cluster.first_send);
        let receive_interval_us = cluster.last_receive - cluster.first_receive;
        if send_interval.is_zero()
            || send_interval > MAX_PROBE_INTERVAL
            || receive_interval_us <= 0
            || receive_interval_us > MAX_PROBE_INTERVAL.as_micros() as i64
            || cluster.size_total <= cluster.size_last_send
            || cluster.size_total <= cluster.size_first_receive
        {
            tracing::debug!(
                cluster = tag.id,
                send_us = send_interval.as_micros() as u64,
                receive_us = receive_interval_us,
                "probe refused: invalid send or receive interval"
            );
            self.failures += 1;
            return;
        }
        // The send interval does not include the last packet's own time on the wire, the receive interval not the
        // first packet's.
        let send_rate = (cluster.size_total - cluster.size_last_send) as f64 * 8.0
            / send_interval.as_secs_f64();
        let receive_rate = (cluster.size_total - cluster.size_first_receive) as f64 * 8.0
            / (receive_interval_us as f64 / 1e6);
        if receive_rate / send_rate > MAX_VALID_RATIO {
            tracing::debug!(
                cluster = tag.id,
                send_bps = send_rate as u64,
                receive_bps = receive_rate as u64,
                "probe refused: receive to send ratio too high"
            );
            self.failures += 1;
            return;
        }
        let mut result = send_rate.min(receive_rate);
        // Received clearly slower than sent: the link's capacity was found; aim a little under it.
        if receive_rate < MIN_RATIO_FOR_UNSATURATED_LINK * send_rate {
            result = TARGET_UTILIZATION_FRACTION * receive_rate;
        }
        tracing::debug!(
            cluster = tag.id,
            send_bps = send_rate as u64,
            receive_bps = receive_rate as u64,
            result_bps = result as u64,
            "probe measured"
        );
        self.successes += 1;
        self.latest = Some(result);
        self.latest_new = Some(result);
    }
}

/// `interval_budget.cc`: `kWindowMs`, the budget's window.
const BUDGET_WINDOW_MS: i64 = 500;

/// `IntervalBudget` as the ALR detector builds it, with `can_build_up_underuse` on.
#[derive(Default)]
struct IntervalBudget {
    target_kbps: i64,
    max_bytes: i64,
    remaining: i64,
}

impl IntervalBudget {
    fn set_target_kbps(&mut self, kbps: i64) {
        self.target_kbps = kbps;
        self.max_bytes = BUDGET_WINDOW_MS * kbps / 8;
        self.remaining = self.remaining.max(-self.max_bytes).min(self.max_bytes);
    }
    fn increase(&mut self, delta_ms: i64) {
        let bytes = self.target_kbps * delta_ms / 8;
        self.remaining = (self.remaining + bytes).min(self.max_bytes);
    }
    fn spend(&mut self, bytes: usize) {
        self.remaining = (self.remaining - bytes as i64).max(-self.max_bytes);
    }
    fn ratio(&self) -> f64 {
        if self.max_bytes == 0 {
            return 0.0;
        }
        self.remaining as f64 / self.max_bytes as f64
    }
}

/// `AlrDetectorConfig`: the budget is this share of the target, the region starts above this share of the budget left
/// and ends below that one. The struct's own defaults (0.65, 0.80, 0.50) are replaced in this libwebrtc by the
/// `WebRTC-ProbingScreenshareBwe` settings, which are on by default (`kDefaultProbingScreenshareBweSettings`,
/// "1.0,2875,80,40,-60,3": usage 80%, start 40%, stop -60%), for every kind of stream: the detector reads them first.
const ALR_BANDWIDTH_USAGE: f64 = 0.80;
const ALR_START_BUDGET: f64 = 0.40;
const ALR_STOP_BUDGET: f64 = -0.60;

/// Whether the sender sends less than its target allows: the application, not the network, limits it (`AlrDetector`).
#[derive(Default)]
pub(crate) struct AlrDetector {
    budget: IntervalBudget,
    last_send: Option<Instant>,
    started: Option<Instant>,
}

impl AlrDetector {
    /// `OnBytesSent`: `bytes` on the wire left at `now`.
    pub fn sent(&mut self, bytes: usize, now: Instant) {
        let Some(last) = self.last_send.replace(now) else {
            return;
        };
        let delta_ms = (now.saturating_duration_since(last).as_micros() as i64 + 500) / 1000;
        self.budget.spend(bytes);
        self.budget.increase(delta_ms);
        if self.budget.ratio() > ALR_START_BUDGET && self.started.is_none() {
            self.started = Some(now);
            tracing::debug!("application limited: region started");
        } else if self.budget.ratio() < ALR_STOP_BUDGET && self.started.is_some() {
            self.started = None;
            tracing::debug!("application limited: region ended");
        }
    }

    /// `SetEstimatedBitrate`: the target, in bits per second.
    pub fn set_target(&mut self, bitrate: u32) {
        let kbps = (f64::from(bitrate) * ALR_BANDWIDTH_USAGE / 1000.0).round() as i64;
        if kbps != self.budget.target_kbps {
            self.budget.set_target_kbps(kbps);
        }
    }

    /// `GetApplicationLimitedRegionStartTime`.
    pub fn start(&self) -> Option<Instant> {
        self.started
    }
}

/// One probe the pacer is asked to send (`ProbeClusterConfig`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct ProbeCluster {
    pub at: Instant,
    /// Bits per second.
    pub bitrate: f64,
    pub duration: Duration,
    pub min_delta: Duration,
    pub min_probes: u32,
    pub id: u32,
}

/// `probe_controller.cc`, with the `ProbeControllerConfig` defaults: `kMaxWaitingTimeForProbingResult` (1 s),
/// `kBitrateDropThreshold` (0.66), `kBitrateDropTimeout` (5 s), `kProbeFractionAfterDrop` (0.85), `kAlrEndedTimeout`
/// (3 s), `kMinTimeBetweenAlrProbes` (5 s), `kProbeUncertainty` (0.05); `p1` (3), `p2` (6), `step_size` (2),
/// `further_probe_threshold` (0.7), `alr_interval` (5 s), `alr_scale` (2), `alloc_p1` (1), `alloc_p2` (2),
/// `alloc_current_bwe_limit` (2), `min_probe_packets_sent` (5), `min_probe_duration` (15 ms), `min_probe_delta`
/// (2 ms). `initial_probe_duration` and `initial_min_probe_delta` apply only with repeated initial probing, which
/// `allow_probe_without_media` switches on and which is off by default.
const MAX_WAITING_FOR_PROBING_RESULT: Duration = Duration::from_secs(1);
const BITRATE_DROP_THRESHOLD: f64 = 0.66;
const BITRATE_DROP_TIMEOUT: Duration = Duration::from_secs(5);
const PROBE_FRACTION_AFTER_DROP: f64 = 0.85;
const ALR_ENDED_TIMEOUT: Duration = Duration::from_secs(3);
const MIN_TIME_BETWEEN_ALR_PROBES: Duration = Duration::from_secs(5);
const PROBE_UNCERTAINTY: f64 = 0.05;
const FIRST_EXPONENTIAL_PROBE_SCALE: f64 = 3.0;
const SECOND_EXPONENTIAL_PROBE_SCALE: f64 = 6.0;
const FURTHER_EXPONENTIAL_PROBE_SCALE: f64 = 2.0;
const FURTHER_PROBE_THRESHOLD: f64 = 0.7;
const ALR_PROBING_INTERVAL: Duration = Duration::from_secs(5);
const ALR_PROBE_SCALE: f64 = 2.0;
const FIRST_ALLOCATION_PROBE_SCALE: f64 = 1.0;
const SECOND_ALLOCATION_PROBE_SCALE: f64 = 2.0;
const ALLOCATION_PROBE_LIMIT_BY_CURRENT_SCALE: f64 = 2.0;
const MIN_PROBE_PACKETS_SENT: u32 = 5;
const MIN_PROBE_DURATION: Duration = Duration::from_millis(15);
const MIN_PROBE_DELTA: Duration = Duration::from_millis(2);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ProbeState {
    /// Nothing probed yet.
    Init,
    /// Probing further when a result shows more room.
    WaitingForResult,
    Complete,
}

/// Why the estimate is where it is, for probing (`BandwidthLimitedCause`). Without loss-based BWE v2 and without the
/// RTT back-off (no round trip is measured here), only the delay-based causes occur.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LimitedCause {
    DelayBasedLimited,
    /// The detector over- or under-uses: no probe now.
    DelayIncreased,
}

impl LimitedCause {
    /// `GetBandwidthLimitedCause` with loss-based BWE v2 off.
    pub fn of(usage: Usage) -> Self {
        if usage == Usage::Normal {
            Self::DelayBasedLimited
        } else {
            Self::DelayIncreased
        }
    }
}

/// When to probe and at what rate (`ProbeController`). Times it has not seen yet (`Timestamp::Zero()` after `Reset`)
/// are `None`: long ago.
pub(crate) struct ProbeController {
    periodic_alr: bool,
    state: ProbeState,
    min_bitrate_to_probe_further: f64,
    time_last_probing_initiated: Option<Instant>,
    estimated: f64,
    start: f64,
    max: f64,
    max_total_allocated: f64,
    alr_start: Option<Instant>,
    alr_end: Option<Instant>,
    time_of_last_large_drop: Option<Instant>,
    bitrate_before_last_large_drop: f64,
    last_bwe_drop_probing_time: Option<Instant>,
    cause: LimitedCause,
    next_id: u32,
}

impl Default for ProbeController {
    fn default() -> Self {
        Self {
            periodic_alr: false,
            state: ProbeState::Init,
            min_bitrate_to_probe_further: f64::INFINITY,
            time_last_probing_initiated: None,
            estimated: 0.0,
            start: 0.0,
            max: 0.0,
            max_total_allocated: 0.0,
            alr_start: None,
            alr_end: None,
            time_of_last_large_drop: None,
            bitrate_before_last_large_drop: 0.0,
            last_bwe_drop_probing_time: None,
            cause: LimitedCause::DelayBasedLimited,
            next_id: 1,
        }
    }
}

/// Time since `then`; a time not seen yet is long ago.
fn since(now: Instant, then: Option<Instant>) -> Duration {
    then.map_or(Duration::MAX, |then| now.saturating_duration_since(then))
}

impl ProbeController {
    /// `EnablePeriodicAlrProbing`: on for a screen share (`GetAlrSettings` finds the default-on
    /// `WebRTC-ProbingScreenshareBwe` settings), off for a camera (`RateControlSettings::UseAlrProbing`, off by default).
    pub fn enable_periodic_alr_probing(&mut self, enable: bool) {
        self.periodic_alr = enable;
    }

    /// `SetBitrates` with the network available: the first call starts the exponential probing.
    pub fn set_bitrates(&mut self, start: f64, max: f64, now: Instant) -> Vec<ProbeCluster> {
        if start > 0.0 {
            self.start = start;
            self.estimated = start;
        }
        let old_max = self.max;
        self.max = max;
        match self.state {
            ProbeState::Init => self.initiate_exponential_probing(now),
            ProbeState::WaitingForResult => Vec::new(),
            ProbeState::Complete => {
                if self.estimated > 0.0 && old_max < self.max && self.estimated < self.max {
                    self.initiate_probing(now, &[self.max], false)
                } else {
                    Vec::new()
                }
            }
        }
    }

    /// `OnMaxTotalAllocatedBitrate`: the streams may now use `total`.
    pub fn on_max_total_allocated(&mut self, total: f64, now: Instant) -> Vec<ProbeCluster> {
        // `probe_max_allocation` and `probe_max_allocation_without_alr` are on by default.
        if self.state == ProbeState::Complete
            && total > self.max_total_allocated
            && self.estimated < self.max
            && self.estimated < total
        {
            self.max_total_allocated = total;
            let limit = ALLOCATION_PROBE_LIMIT_BY_CURRENT_SCALE * self.estimated;
            let mut first = total * FIRST_ALLOCATION_PROBE_SCALE;
            let mut limited = limit < first;
            if limited {
                first = limit;
            }
            let mut probes = vec![first];
            if !limited {
                let mut second = total * SECOND_ALLOCATION_PROBE_SCALE;
                limited = limit < second;
                if limited {
                    second = limit;
                }
                if second > first {
                    probes.push(second);
                }
            }
            return self.initiate_probing(now, &probes, limited);
        }
        self.max_total_allocated = total;
        Vec::new()
    }

    fn initiate_exponential_probing(&mut self, now: Instant) -> Vec<ProbeCluster> {
        let probes = [
            FIRST_EXPONENTIAL_PROBE_SCALE * self.start,
            SECOND_EXPONENTIAL_PROBE_SCALE * self.start,
        ];
        self.initiate_probing(now, &probes, true)
    }

    /// `SetEstimatedBitrate`: the loss-based target moved (`MaybeTriggerOnNetworkChanged`).
    pub fn set_estimated_bitrate(
        &mut self,
        bitrate: f64,
        cause: LimitedCause,
        now: Instant,
    ) -> Vec<ProbeCluster> {
        self.cause = cause;
        if bitrate < BITRATE_DROP_THRESHOLD * self.estimated {
            self.time_of_last_large_drop = Some(now);
            self.bitrate_before_last_large_drop = self.estimated;
        }
        self.estimated = bitrate;
        if self.state == ProbeState::WaitingForResult && bitrate > self.min_bitrate_to_probe_further
        {
            return self.initiate_probing(now, &[FURTHER_EXPONENTIAL_PROBE_SCALE * bitrate], true);
        }
        Vec::new()
    }

    pub fn set_alr_start_time(&mut self, start: Option<Instant>) {
        self.alr_start = start;
    }

    pub fn set_alr_ended_time(&mut self, end: Instant) {
        self.alr_end = Some(end);
    }

    /// `RequestProbe`: back to normal after a large drop; while application limited (or just after) a probe at most of
    /// the rate before the drop finds out whether it was real.
    pub fn request_probe(&mut self, now: Instant) -> Vec<ProbeCluster> {
        let in_alr = self.alr_start.is_some();
        let alr_ended_recently =
            self.alr_end.is_some() && since(now, self.alr_end) < ALR_ENDED_TIMEOUT;
        // `WebRTC-BweRapidRecoveryExperiment` would probe outside the region too; it is off by default.
        if (in_alr || alr_ended_recently) && self.state == ProbeState::Complete {
            let suggested = PROBE_FRACTION_AFTER_DROP * self.bitrate_before_last_large_drop;
            let min_expected = (1.0 - PROBE_UNCERTAINTY) * suggested;
            if min_expected > self.estimated
                && since(now, self.time_of_last_large_drop) < BITRATE_DROP_TIMEOUT
                && since(now, self.last_bwe_drop_probing_time) > MIN_TIME_BETWEEN_ALR_PROBES
            {
                tracing::debug!("large drop of the estimate; probing");
                self.last_bwe_drop_probing_time = Some(now);
                return self.initiate_probing(now, &[suggested], false);
            }
        }
        Vec::new()
    }

    fn time_for_alr_probe(&self, now: Instant) -> bool {
        let (true, Some(alr_start)) = (self.periodic_alr, self.alr_start) else {
            return false;
        };
        let last = self
            .time_last_probing_initiated
            .map_or(alr_start, |last| last.max(alr_start));
        now >= last + ALR_PROBING_INTERVAL
    }

    /// `Process`, every process interval.
    pub fn process(&mut self, now: Instant) -> Vec<ProbeCluster> {
        if since(now, self.time_last_probing_initiated) > MAX_WAITING_FOR_PROBING_RESULT
            && self.state == ProbeState::WaitingForResult
        {
            tracing::debug!("probing: no result in time");
            self.update_state(ProbeState::Complete);
        }
        if self.estimated <= 0.0 || self.state != ProbeState::Complete {
            return Vec::new();
        }
        if self.time_for_alr_probe(now) {
            return self.initiate_probing(now, &[self.estimated * ALR_PROBE_SCALE], true);
        }
        Vec::new()
    }

    fn update_state(&mut self, state: ProbeState) {
        self.state = state;
        if state == ProbeState::Complete {
            self.min_bitrate_to_probe_further = f64::INFINITY;
        }
    }

    fn initiate_probing(
        &mut self,
        now: Instant,
        bitrates: &[f64],
        mut probe_further: bool,
    ) -> Vec<ProbeCluster> {
        let mut max_probe = self.max;
        if self.max_total_allocated > 0.0 {
            // Up to twice the allocation, for bursty streams and for probes received a little under their rate.
            max_probe = max_probe.min(self.max_total_allocated * 2.0);
        }
        if self.cause == LimitedCause::DelayIncreased {
            tracing::debug!("probing: not while the delay rises");
            return Vec::new();
        }
        let mut clusters = Vec::with_capacity(bitrates.len());
        for &bitrate in bitrates {
            let mut bitrate = bitrate;
            if bitrate >= max_probe {
                bitrate = max_probe;
                probe_further = false;
            }
            clusters.push(ProbeCluster {
                at: now,
                bitrate,
                duration: MIN_PROBE_DURATION,
                min_delta: MIN_PROBE_DELTA,
                min_probes: MIN_PROBE_PACKETS_SENT,
                id: self.next_id,
            });
            self.next_id += 1;
        }
        self.time_last_probing_initiated = Some(now);
        if probe_further {
            self.update_state(ProbeState::WaitingForResult);
            // A result is not expected above a share of the probe's rate.
            self.min_bitrate_to_probe_further = clusters
                .last()
                .map_or(f64::INFINITY, |cluster| cluster.bitrate)
                * FURTHER_PROBE_THRESHOLD;
        } else {
            self.update_state(ProbeState::Complete);
        }
        clusters
    }
}

/// `bitrate_prober.cc`: `kProbeClusterTimeout` (5 s), `kMaxPendingProbeClusters` (5), and the `BitrateProberConfig`
/// defaults `max_probe_delay` (10 ms) and `min_packet_size` (200 bytes).
const PROBE_CLUSTER_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_PENDING_PROBE_CLUSTERS: usize = 5;
const MAX_PROBE_DELAY: Duration = Duration::from_millis(10);
const PROBE_MIN_PACKET_BYTES: f64 = 200.0;

struct PendingCluster {
    tag: ProbeTag,
    bitrate: f64,
    min_delta: Duration,
    requested_at: Instant,
    started_at: Option<Instant>,
    sent_probes: u32,
    sent_bytes: usize,
}

/// The clusters the pacer sends, one burst after another at the cluster's rate (`BitrateProber`, enabled; probing waits
/// for a media packet, as `allow_start_probing_immediately` is off by default).
#[derive(Default)]
pub(crate) struct Prober {
    clusters: VecDeque<PendingCluster>,
    active: bool,
    /// `next_probe_time_` while active; `None` is "at once" (minus infinity).
    next: Option<Instant>,
    /// When it became active: what "at once" means to a loop that sleeps until a deadline.
    activated: Option<Instant>,
    /// Clusters sent whole and dropped for lateness, for the log.
    pub sent: u64,
    pub dropped: u64,
}

impl Prober {
    /// `CreateProbeCluster`.
    pub fn create(&mut self, config: &ProbeCluster, now: Instant) {
        while self.clusters.front().is_some_and(|front| {
            config.at.saturating_duration_since(front.requested_at) > PROBE_CLUSTER_TIMEOUT
                || self.clusters.len() > MAX_PENDING_PROBE_CLUSTERS
        }) {
            self.clusters.pop_front();
        }
        let min_bytes = (config.bitrate * config.duration.as_secs_f64() / 8.0).round() as u32;
        self.clusters.push_back(PendingCluster {
            tag: ProbeTag {
                id: config.id,
                min_probes: config.min_probes,
                min_bytes,
            },
            bitrate: config.bitrate,
            min_delta: config.min_delta,
            requested_at: config.at,
            started_at: None,
            sent_probes: 0,
            sent_bytes: 0,
        });
        tracing::debug!(
            cluster = config.id,
            bitrate = config.bitrate as u64,
            min_bytes,
            "probe cluster requested"
        );
        self.maybe_activate(0, now);
    }

    /// `OnIncomingPacket`: a media packet of `bytes` reached the pacer.
    pub fn incoming(&mut self, bytes: usize, now: Instant) {
        self.maybe_activate(bytes, now);
    }

    /// `MaybeSetActiveState`: a cluster waits for a packet of at least the smaller of one burst and `min_packet_size`.
    fn maybe_activate(&mut self, bytes: usize, now: Instant) {
        if self.clusters.is_empty() || self.active {
            return;
        }
        if bytes as f64
            >= self
                .recommended_min_probe_size()
                .min(PROBE_MIN_PACKET_BYTES)
        {
            self.next = None;
            self.active = true;
            self.activated = Some(now);
        }
    }

    /// `NextProbeTime`: when the next burst is due, if probing.
    pub fn next_probe_time(&self) -> Option<Instant> {
        if !self.active || self.clusters.is_empty() {
            return None;
        }
        self.next.or(self.activated)
    }

    /// `CurrentCluster`: the cluster a burst now belongs to; one more than `MAX_PROBE_DELAY` late is dropped.
    pub fn current(&mut self, now: Instant) -> Option<ProbeTag> {
        if self.clusters.is_empty() || !self.active {
            return None;
        }
        if let Some(next) = self.next
            && now.saturating_duration_since(next) > MAX_PROBE_DELAY
        {
            tracing::debug!("probe cluster dropped: the pacer was late");
            self.dropped += 1;
            self.clusters.pop_front();
            if self.clusters.is_empty() {
                self.active = false;
                self.next = None;
                self.activated = None;
                return None;
            }
            // Start the queued successor cleanly at `now` instead of inheriting the expired cluster's `next`.
            self.next = None;
            self.activated = Some(now);
        }
        self.clusters.front().map(|cluster| cluster.tag)
    }

    /// Nothing of the current cluster has left yet (`probe_cluster_bytes_sent` is 0).
    pub fn starting(&self) -> bool {
        self.clusters
            .front()
            .is_some_and(|cluster| cluster.sent_bytes == 0)
    }

    /// `RecommendedMinProbeSize`: bytes for one burst.
    pub fn recommended_min_probe_size(&self) -> f64 {
        self.clusters.front().map_or(0.0, |cluster| {
            cluster.bitrate * cluster.min_delta.as_secs_f64() / 8.0
        })
    }

    /// `ProbeSent`: one burst of `bytes` left now.
    pub fn probe_sent(&mut self, now: Instant, bytes: usize) {
        let Some(cluster) = self.clusters.front_mut() else {
            return;
        };
        let started = *cluster.started_at.get_or_insert(now);
        cluster.sent_bytes += bytes;
        cluster.sent_probes += 1;
        // `CalculateNextProbeTime`: the cluster stays at its rate from its start.
        self.next = Some(
            started + Duration::from_secs_f64(cluster.sent_bytes as f64 * 8.0 / cluster.bitrate),
        );
        if cluster.sent_bytes as u64 >= u64::from(cluster.tag.min_bytes)
            && cluster.sent_probes >= cluster.tag.min_probes
        {
            tracing::debug!(
                cluster = cluster.tag.id,
                bytes = cluster.sent_bytes,
                packets = cluster.sent_probes,
                "probe cluster sent"
            );
            self.sent += 1;
            self.clusters.pop_front();
        }
        if self.clusters.is_empty() {
            self.active = false;
        }
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
                            probe: None,
                        }
                    })
                    .collect();
                let now = start + Duration::from_millis(batch * 100 + 150);
                estimator.feedback(&packets, now, false);
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

    /// `count` packets of 1200 bytes of cluster 1 (asked for 5 packets, 6000 bytes), sent `send_us` apart and received
    /// `receive_us` apart.
    fn cluster(start: Instant, count: u64, send_us: u64, receive_us: i64) -> Vec<PacketResult> {
        let tag = ProbeTag {
            id: 1,
            min_probes: 5,
            min_bytes: 6000,
        };
        (0..count)
            .map(|n| PacketResult {
                sent: start + Duration::from_micros(n * send_us),
                size: 1200,
                received: Some(1_000_000 + n as i64 * receive_us),
                probe: Some(tag),
            })
            .collect()
    }

    fn measure(packets: &[PacketResult]) -> (Option<f64>, u64) {
        let mut estimator = ProbeEstimator::default();
        for packet in packets {
            estimator.packet(packet, packet.probe.unwrap(), packet.received.unwrap());
        }
        (estimator.latest, estimator.failures)
    }

    #[test]
    fn a_probe_cluster_measures_the_slower_of_its_send_and_receive_rates() {
        let start = Instant::now();
        // Sent at 9.6 Mb/s, received at 8 Mb/s: the link was found, and 95% of it is the result.
        let (result, _) = measure(&cluster(start, 10, 1000, 1200));
        assert!(
            (result.unwrap() - 0.95 * 8_000_000.0).abs() < 1.0,
            "{result:?}"
        );
        // Received nearly as fast as sent: the smaller of the two.
        let (result, _) = measure(&cluster(start, 10, 1000, 1050));
        assert!(
            (result.unwrap() - 1200.0 * 8.0 / 0.00105).abs() < 1.0,
            "{result:?}"
        );
        // Too few packets for 80% of the cluster: nothing yet.
        assert_eq!(measure(&cluster(start, 3, 1000, 1200)), (None, 0));
        // All received at once, or received five times faster than sent: refused.
        assert_eq!(measure(&cluster(start, 10, 1000, 0)).0, None);
        assert!(measure(&cluster(start, 10, 1000, 0)).1 > 0);
        assert_eq!(measure(&cluster(start, 10, 1000, 200)).0, None);
    }

    #[test]
    fn the_probe_controller_probes_exponentially_then_further_while_results_show_room() {
        let start = Instant::now();
        let mut probes = ProbeController::default();
        let first = probes.set_bitrates(300_000.0, 9_000_000.0, start);
        let rates: Vec<f64> = first.iter().map(|probe| probe.bitrate).collect();
        assert_eq!(rates, [900_000.0, 1_800_000.0]);
        assert_eq!((first[0].id, first[1].id), (1, 2));
        assert_eq!(first[0].duration, Duration::from_millis(15));
        assert_eq!(first[0].min_probes, 5);
        // More than 70% of the last probe: twice the result.
        let further =
            probes.set_estimated_bitrate(1_500_000.0, LimitedCause::DelayBasedLimited, start);
        assert_eq!(further.len(), 1);
        assert_eq!(further[0].bitrate, 3_000_000.0);
        // Less than 70% of it: done.
        assert!(
            probes
                .set_estimated_bitrate(2_000_000.0, LimitedCause::DelayBasedLimited, start)
                .is_empty()
        );
        // Capped at the top rate, which ends the probing.
        let mut capped = ProbeController::default();
        let first = capped.set_bitrates(9_000_000.0, 9_000_000.0, start);
        assert!(first.iter().all(|probe| probe.bitrate == 9_000_000.0));
        assert!(
            capped
                .set_estimated_bitrate(8_000_000.0, LimitedCause::DelayBasedLimited, start)
                .is_empty()
        );
        // Without a result within a second the wait ends; a delay increase allows no probe.
        let later = start + Duration::from_millis(1100);
        assert!(probes.process(later).is_empty());
        probes.enable_periodic_alr_probing(true);
        probes.set_alr_start_time(Some(later));
        assert!(probes.process(later + Duration::from_secs(4)).is_empty());
        probes.set_estimated_bitrate(2_000_000.0, LimitedCause::DelayIncreased, later);
        assert!(probes.process(later + Duration::from_secs(5)).is_empty());
        // Application limited for five seconds: a probe at twice the estimate.
        probes.set_estimated_bitrate(2_000_000.0, LimitedCause::DelayBasedLimited, later);
        let alr = probes.process(later + Duration::from_secs(5));
        assert_eq!(alr.len(), 1);
        assert_eq!(alr[0].bitrate, 4_000_000.0);
    }

    #[test]
    fn a_large_drop_is_probed_back_only_in_or_just_after_an_application_limited_region() {
        let start = Instant::now();
        for (in_alr, ended, expected) in [
            (true, false, true),
            (false, true, true),
            (false, false, false),
        ] {
            let mut probes = ProbeController::default();
            probes.set_bitrates(9_000_000.0, 9_000_000.0, start);
            let cause = LimitedCause::DelayBasedLimited;
            probes.set_estimated_bitrate(6_000_000.0, cause, start);
            // From 6 to 1 Mb/s is a large drop (under 66%).
            let drop = start + Duration::from_secs(10);
            probes.set_estimated_bitrate(1_000_000.0, cause, drop);
            probes.set_alr_start_time(in_alr.then_some(start));
            if ended {
                probes.set_alr_ended_time(drop + Duration::from_millis(500));
            }
            let probe = probes.request_probe(drop + Duration::from_secs(1));
            assert_eq!(!probe.is_empty(), expected, "in {in_alr}, ended {ended}");
            if expected {
                assert_eq!(probe[0].bitrate, 0.85 * 6_000_000.0);
                // At most every five seconds.
                assert!(
                    probes
                        .request_probe(drop + Duration::from_secs(2))
                        .is_empty()
                );
            }
        }
    }

    #[test]
    fn the_region_starts_when_little_of_the_budget_is_used_and_ends_when_it_is_overspent() {
        let start = Instant::now();
        let mut alr = AlrDetector::default();
        // 1 Mb/s target: a budget of 800 kb/s, 50 kB over its half-second window.
        alr.set_target(1_000_000);
        let mut at = start;
        for _ in 0..30 {
            at += Duration::from_millis(10);
            alr.sent(200, at);
        }
        assert!(
            alr.start().is_some(),
            "160 kb/s of 800 is application limited"
        );
        let started = alr.start();
        // Sending at 2 Mb/s spends the budget down past -60%.
        for _ in 0..100 {
            at += Duration::from_millis(10);
            alr.sent(2500, at);
            if alr.start().is_none() {
                break;
            }
        }
        assert_eq!(alr.start(), None);
        assert!(started.is_some());
    }

    #[test]
    fn the_prober_waits_for_media_then_sends_bursts_at_the_cluster_rate() {
        let start = Instant::now();
        let mut prober = Prober::default();
        let config = ProbeCluster {
            at: start,
            bitrate: 8_000_000.0,
            duration: Duration::from_millis(15),
            min_delta: Duration::from_millis(2),
            min_probes: 5,
            id: 7,
        };
        prober.create(&config, start);
        assert_eq!(prober.next_probe_time(), None, "no media yet");
        prober.incoming(100, start);
        assert_eq!(prober.next_probe_time(), None, "too small to start a probe");
        prober.incoming(1200, start);
        assert_eq!(prober.next_probe_time(), Some(start));
        // 2 ms at 8 Mb/s is 2000 bytes a burst; the cluster wants 15 000 bytes in 5 bursts at least.
        assert_eq!(prober.recommended_min_probe_size(), 2000.0);
        let tag = prober.current(start).unwrap();
        assert_eq!((tag.id, tag.min_bytes, tag.min_probes), (7, 15_000, 5));
        let mut at = start;
        for burst in 1..=8 {
            assert!(prober.current(at).is_some(), "burst {burst}");
            prober.probe_sent(at, 2000);
            if burst < 8 {
                let next = prober.next_probe_time().unwrap();
                let expected = start + Duration::from_millis(2 * burst);
                assert!(next.max(expected) - next.min(expected) < Duration::from_micros(1));
                at = next;
            }
        }
        assert_eq!(prober.next_probe_time(), None, "the cluster was sent");
        assert_eq!(prober.sent, 1);
        // A burst more than 10 ms late drops its cluster, and a queued successor starts cleanly at `now`.
        prober.create(&ProbeCluster { id: 8, ..config }, at);
        prober.create(&ProbeCluster { id: 9, ..config }, at);
        prober.incoming(1200, at);
        prober.probe_sent(at, 2000);
        let late = at + Duration::from_millis(13);
        let next_tag = prober
            .current(late)
            .expect("queued successor survives drop");
        assert_eq!(next_tag.id, 9);
        assert_eq!(prober.dropped, 1);
        assert_eq!(prober.next_probe_time(), Some(late));
        // Calling `current` again right after must not re-drop the successor on cluster 8's stale deadline.
        assert_eq!(prober.current(late).map(|tag| tag.id), Some(9));
        assert_eq!(prober.dropped, 1);
    }
}
