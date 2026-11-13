//! A Go Live stream of 2560x1440 at 60 fps through a simulated network, on a simulated clock: the sender's packetizer and
//! retransmission history, packet loss and reordering, and this crate's receiver (reorder window, NACK, RTX, PLI,
//! reassembly, the keyframe gate). Nothing is encoded or decoded: the access units are sized like the hardware
//! encoder's at 9 Mb/s (`dc-media`'s `bench_go_live_1440p60_picture_path`: 19 kB pictures, 161 kB keyframes) and carry
//! one NAL each, which is all the receive path looks at.
//!
//! It answers two questions with numbers: how many pictures of a lossy stream reach the decoder (and how long the
//! longest freeze is), and how evenly they are handed over, which is what a watcher sees as smooth or stuttering.
use super::{
    playout::Playout,
    receive::Receivers,
    send::{History, Pacing, Packet, SCREEN_PACING, packetize},
    sender::{CALL_START_BITRATE, Gate, KEYFRAME_INTERVAL},
};
use crate::rtp::{self, Rtp};
use std::{collections::BinaryHeap, time::Duration};
use tokio::time::Instant;

const SSRC: u32 = 4000;
const RTX: u32 = 4001;
const OWN: u32 = 3999;
const FPS: u64 = 60;
const DELTA_BYTES: usize = 19_000;
const KEY_BYTES: usize = 161_000;
const GOP: u64 = 120;
/// One way, sender to receiver and back again for feedback.
const DELAY_MS: u64 = 25;
const JITTER_MS: u64 = 4;
/// The sender's pacer at 2.5 times 9 Mb/s.
const PACING_BYTES_PER_MS: u64 = 9_000_000 * 5 / 2 / 8 / 1000;

struct Random(u32);

impl Random {
    fn next(&mut self) -> u32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 17;
        self.0 ^= self.0 << 5;
        self.0
    }
    fn chance(&mut self, percent: f64) -> bool {
        f64::from(self.next() % 100_000) < percent * 1000.0
    }
}

/// A datagram in flight, ordered by arrival time (earliest first).
#[derive(PartialEq, Eq)]
struct Flight {
    at: u64,
    order: u64,
    rtp: (u8, bool, u16, u32, u32, Vec<u8>),
}

impl Ord for Flight {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        (other.at, other.order).cmp(&(self.at, self.order))
    }
}

impl PartialOrd for Flight {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

#[derive(Debug)]
pub(crate) struct Outcome {
    pub sent: u64,
    pub shown: u64,
    /// Pictures that arrived whole but were refused while waiting for a keyframe.
    pub frozen: u64,
    pub longest_freeze_ms: u64,
    /// Hand-over intervals of consecutive pictures: mean, the share further than 4 ms from 16.7 ms, and the largest.
    pub mean_interval_ms: f64,
    /// Pictures handed over within 4 ms of the one before: the call view keeps only the newest picture, so these are
    /// never seen.
    pub collapsed: u64,
    pub uneven_percent: f64,
    pub worst_interval_ms: u64,
    pub plis: u64,
    pub repairs: u64,
    /// The same pictures at their playout time (`playout.rs`): collapsed and uneven as above, and the mean delay it
    /// added.
    pub played_collapsed: u64,
    pub played_uneven_percent: f64,
    pub mean_delay_ms: f64,
}

/// Pictures within 4 ms of the one before, and the share of intervals further than 4 ms from a frame interval.
fn spacing(times: &[u64]) -> (u64, f64) {
    let intervals: Vec<u64> = times.windows(2).map(|w| w[1] - w[0]).collect();
    if intervals.is_empty() {
        return (0, 0.0);
    }
    let collapsed = intervals.iter().filter(|i| **i < 4).count() as u64;
    let uneven = intervals
        .iter()
        .filter(|i| (**i as f64 - 1000.0 / FPS as f64).abs() > 4.0)
        .count();
    (collapsed, uneven as f64 * 100.0 / intervals.len() as f64)
}

fn header_fields(packet: &Packet) -> (u8, bool, u16, u32, u32) {
    let h = packet.header;
    (
        h[1] & 0x7f,
        h[1] & 0x80 != 0,
        u16::from_be_bytes([h[2], h[3]]),
        u32::from_be_bytes([h[4], h[5], h[6], h[7]]),
        u32::from_be_bytes([h[8], h[9], h[10], h[11]]),
    )
}

/// Runs `seconds` of the stream with `loss_percent` independent loss on every datagram (media, repairs and feedback).
/// `drain_on_arrival` hands pictures over as their last packet arrives; otherwise only on the 20 ms media tick.
pub(crate) fn run(seconds: u64, loss_percent: f64, drain_on_arrival: bool) -> Outcome {
    run_with(seconds, loss_percent, drain_on_arrival, KEY_BYTES, true)
}

/// [`run`] with keyframes of `key_bytes`. `feedback_on_arrival` sends NACK and PLI as soon as they are due, as the call
/// and stream loops do; otherwise only on the 20 ms media tick, as they did before.
pub(crate) fn run_with(
    seconds: u64,
    loss_percent: f64,
    drain_on_arrival: bool,
    key_bytes: usize,
    feedback_on_arrival: bool,
) -> Outcome {
    run_seeded(
        seconds,
        loss_percent,
        drain_on_arrival,
        key_bytes,
        feedback_on_arrival,
        0x9E37_79B9 ^ (loss_percent * 1000.0) as u32,
    )
}

/// [`run_with`] on another random sequence of losses.
pub(crate) fn run_seeded(
    seconds: u64,
    loss_percent: f64,
    drain_on_arrival: bool,
    key_bytes: usize,
    feedback_on_arrival: bool,
    seed: u32,
) -> Outcome {
    let start = Instant::now();
    let at = |ms: u64| start + Duration::from_millis(ms);
    let mut random = Random(seed.max(1));
    let mut receivers = Receivers::default();
    receivers.announce(1, SSRC, RTX).unwrap();
    let mut history = History::default();
    let mut sequence = 1000u16;
    let mut rtx_sequence = 50u16;
    let mut air = BinaryHeap::new();
    let mut order = 0u64;
    let mut feedback_air: Vec<(u64, Vec<u8>)> = Vec::new();
    let mut force_key = false;
    let mut next_key_allowed = 0u64;
    let mut sender_busy_until = 0u64;
    let mut outcome = Outcome {
        sent: 0,
        shown: 0,
        frozen: 0,
        longest_freeze_ms: 0,
        mean_interval_ms: 0.0,
        collapsed: 0,
        uneven_percent: 0.0,
        worst_interval_ms: 0,
        plis: 0,
        repairs: 0,
        played_collapsed: 0,
        played_uneven_percent: 0.0,
        mean_delay_ms: 0.0,
    };
    let mut shown_at: Vec<u64> = Vec::new();
    let mut playout = Playout::default();
    let mut played: Vec<(u64, u64)> = Vec::new();
    let end = seconds * 1000;
    let mut frame = 0u64;
    let mut ready = Vec::new();
    let mut send = |air: &mut BinaryHeap<Flight>,
                    random: &mut Random,
                    departs: u64,
                    fields: (u8, bool, u16, u32, u32),
                    payload: Vec<u8>| {
        if random.chance(loss_percent) {
            return;
        }
        order += 1;
        let jitter = u64::from(random.next()) % (JITTER_MS + 1);
        air.push(Flight {
            at: departs + DELAY_MS + jitter,
            order,
            rtp: (fields.0, fields.1, fields.2, fields.3, fields.4, payload),
        });
    };
    let handover = |receivers: &mut Receivers,
                    ready: &mut Vec<super::receive::Received>,
                    now_ms: u64,
                    outcome: &mut Outcome,
                    shown_at: &mut Vec<u64>,
                    playout: &mut Playout,
                    played: &mut Vec<(u64, u64)>| {
        receivers.drain(at(now_ms), ready);
        for unit in ready.drain(..) {
            let keyframe = dc_media::h264::is_keyframe(&unit.data);
            if receivers.accept(unit.user, keyframe) {
                outcome.shown += 1;
                shown_at.push(now_ms);
                let due = playout.due(unit.timestamp, at(now_ms));
                played.push((due.duration_since(start).as_millis() as u64, now_ms));
            } else {
                outcome.frozen += 1;
            }
        }
    };
    for now_ms in 0..=end {
        // The sender: a picture every 1/60 s, paced out after the previous one.
        if now_ms * FPS >= frame * 1000 && now_ms < end - 500 {
            let keyframe = frame.is_multiple_of(GOP) || force_key;
            force_key = false;
            let size = if keyframe { key_bytes } else { DELTA_BYTES };
            let mut data = vec![0x55u8; size];
            data[..5].copy_from_slice(&[0, 0, 0, 1, if keyframe { 0x65 } else { 0x41 }]);
            let timestamp = (frame * 90_000 / FPS) as u32;
            let packets = packetize(&data, &mut sequence, timestamp, SSRC).unwrap();
            let first = sender_busy_until.max(now_ms);
            let mut departs = first;
            let mut bytes = 0u64;
            for packet in packets {
                bytes += packet.payload.len() as u64 + 32;
                let leaves = first + bytes / PACING_BYTES_PER_MS;
                send(
                    &mut air,
                    &mut random,
                    leaves,
                    header_fields(&packet),
                    packet.payload.clone(),
                );
                history.remember(packet, at(leaves));
                departs = departs.max(leaves);
            }
            sender_busy_until = departs;
            outcome.sent += 1;
            frame += 1;
        }
        // Feedback reaching the sender: NACKs resend from the history as RTX, PLIs force a keyframe (twice a second).
        let (due, later): (Vec<_>, Vec<_>) =
            feedback_air.drain(..).partition(|(t, _)| *t <= now_ms);
        feedback_air = later;
        for (_, compound) in due {
            let Some(feedback) = rtp::feedback(&compound, SSRC) else {
                continue;
            };
            let missing = history.request(&feedback.nacks, at(now_ms));
            if (feedback.keyframe || missing) && now_ms >= next_key_allowed {
                next_key_allowed = now_ms + KEYFRAME_INTERVAL.as_millis() as u64;
                force_key = true;
            }
            while let Some(repair) = history.repair(RTX, &mut rtx_sequence, at(now_ms)) {
                outcome.repairs += 1;
                send(
                    &mut air,
                    &mut random,
                    now_ms,
                    header_fields(&repair),
                    repair.payload,
                );
            }
        }
        // The receiver.
        while air.peek().is_some_and(|flight| flight.at <= now_ms) {
            let flight = air.pop().unwrap();
            let (payload_type, marker, seq, timestamp, ssrc, payload) = flight.rtp;
            receivers.push(
                Rtp {
                    payload_type,
                    marker,
                    ssrc,
                    sequence: seq,
                    timestamp,
                    transport_sequence: None,
                    payload,
                },
                at(now_ms),
            );
            if drain_on_arrival {
                handover(
                    &mut receivers,
                    &mut ready,
                    now_ms,
                    &mut outcome,
                    &mut shown_at,
                    &mut playout,
                    &mut played,
                );
                if feedback_on_arrival {
                    for (header, body) in receivers.feedback(OWN, at(now_ms)) {
                        if header[1] == 206 && body.len() == 4 {
                            outcome.plis += 1;
                        }
                        if random.chance(loss_percent) {
                            continue;
                        }
                        feedback_air.push((now_ms + DELAY_MS, [&header[..], &body].concat()));
                    }
                }
            }
        }
        // Requests that come due by time alone (a retry, a hole at the end of a picture) go at once, not on the tick.
        if drain_on_arrival && feedback_on_arrival {
            handover(
                &mut receivers,
                &mut ready,
                now_ms,
                &mut outcome,
                &mut shown_at,
                &mut playout,
                &mut played,
            );
            for (header, body) in receivers.feedback(OWN, at(now_ms)) {
                if header[1] == 206 && body.len() == 4 {
                    outcome.plis += 1;
                }
                if random.chance(loss_percent) {
                    continue;
                }
                feedback_air.push((now_ms + DELAY_MS, [&header[..], &body].concat()));
            }
        }
        if now_ms % 20 == 7 {
            handover(
                &mut receivers,
                &mut ready,
                now_ms,
                &mut outcome,
                &mut shown_at,
                &mut playout,
                &mut played,
            );
            for (header, body) in receivers.feedback(OWN, at(now_ms)) {
                if header[1] == 206 && body.len() == 4 {
                    outcome.plis += 1;
                }
                if random.chance(loss_percent) {
                    continue;
                }
                feedback_air.push((now_ms + DELAY_MS, [&header[..], &body].concat()));
            }
        }
    }
    let times: Vec<u64> = played.iter().map(|(due, _)| *due).collect();
    (outcome.played_collapsed, outcome.played_uneven_percent) = spacing(&times);
    if !played.is_empty() {
        outcome.mean_delay_ms = played
            .iter()
            .map(|(due, at)| (due - at) as f64)
            .sum::<f64>()
            / played.len() as f64;
    }
    let intervals: Vec<u64> = shown_at.windows(2).map(|w| w[1] - w[0]).collect();
    if !intervals.is_empty() {
        outcome.mean_interval_ms = intervals.iter().sum::<u64>() as f64 / intervals.len() as f64;
        let uneven = intervals
            .iter()
            .filter(|i| (**i as f64 - 1000.0 / FPS as f64).abs() > 4.0)
            .count();
        outcome.uneven_percent = uneven as f64 * 100.0 / intervals.len() as f64;
        outcome.worst_interval_ms = *intervals.iter().max().unwrap();
        outcome.collapsed = intervals.iter().filter(|i| **i < 4).count() as u64;
        outcome.longest_freeze_ms = outcome.worst_interval_ms;
    }
    outcome
}

/// The bottleneck's buffer: half a megabyte, half a second at 8 Mb/s and nearly three seconds at 1.5 Mb/s, the deep
/// buffer of a home router in which a sender that only reacts to loss builds seconds of delay.
const QUEUE_BYTES: f64 = 500_000.0;
/// The share's encoder: the top rate of a 1440p60 share and the official floor (`desktopBitrate.min`).
const SHARE_TOP: u32 = 9_000_000;
const SHARE_FLOOR: u32 = dc_media::screen::BITRATE_MIN;

/// A still screen's pictures: an encoder output of about 200 kb/s at 60 fps.
const STILL_BYTES: usize = 200_000 / 8 / FPS as usize;

/// Windows' default timer tick, to which tokio's timer rounds every wait up (DECISIONS.md, `clock.rs`).
const SYSTEM_TICK_US: u64 = 15_625;

/// What the far end sends back.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Returned {
    /// Everything the receiver makes: transport-wide feedback, receiver reports, NACK, PLI.
    All,
    /// Nothing at all.
    Nothing,
    /// Receiver reports only, one every five seconds.
    ReportsEvery5s,
}

/// When the pacer's wake comes after its deadline.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Wakes {
    /// The high-resolution pacer clock (`clock::PacerClock`): within a millisecond, up to a millisecond late.
    PacerClock,
    /// A tokio timer on Windows: on the next 15.6 ms system tick, up to two milliseconds late beyond it.
    SystemTick,
}

/// How [`run_capacity`] sets the sender up.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Setup {
    /// The send-side delay-based estimator caps the target.
    pub delay_based: bool,
    /// Probing and the application-limited region.
    pub probing: bool,
    /// From which second until which the shared screen is still: pictures of `STILL_BYTES`, and a keyframe every
    /// `GOP` pictures, of the size a picture at the target would have as a keyframe.
    pub still: Option<(u64, u64)>,
    /// Where the estimation starts.
    pub start: u32,
    /// The encoder's top rate.
    pub top: u32,
    pub pacing: Pacing,
    pub returned: Returned,
    pub wakes: Wakes,
    /// When the first secure picture goes (the DAVE handshake before it), in milliseconds.
    pub media_from_ms: u64,
    pub pictures: Pictures,
    pub gate: Gate,
}

/// What the encoder makes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Pictures {
    /// Every picture at the encoder rate, a keyframe only when asked for, of `KEY_BYTES / DELTA_BYTES` (8.5) pictures.
    Target,
    /// A constant rate encoder: a keyframe every `GOP` pictures and when asked for, `key` times an average picture
    /// (capped at `vbv` average pictures when the encoder's buffer is limited), the pictures between it smaller so the
    /// group of pictures averages the rate, each within 10% of its size. The 1440p60 measurement at 9 Mb/s
    /// (`dc-media`'s `bench_go_live_1440p60_picture_path`) gave 161 kB keyframes for 19 kB pictures, 8.5 times; the
    /// other ratios are assumptions.
    Cbr { key: f64, vbv: Option<f64> },
}

/// `HOLD_AT` and `FRAME_QUEUE` of `stream_conn.rs`: encoded pictures wait in a channel of eight for the sender; from two
/// waiting, checked on the 20 ms tick, the capture skips pictures before they are encoded.
const HOLD_AT: usize = 2;
const FRAME_QUEUE: usize = 8;

/// The sender as it was before the official start rate and pacing: estimation from the top rate, and a pacer at 2.5
/// times the target without a queue time limit, woken by tokio's timer.
const ORIGINAL: Setup = Setup {
    delay_based: true,
    probing: false,
    still: None,
    start: SHARE_TOP,
    top: SHARE_TOP,
    pacing: Pacing {
        factor: 2.5,
        queue_limit: Duration::MAX,
    },
    returned: Returned::All,
    wakes: Wakes::SystemTick,
    media_from_ms: 0,
    pictures: Pictures::Target,
    gate: Gate::Idle,
};
/// As it is now: probing, the call's start rate, the screen share's pacing and the pacer clock.
const NOW: Setup = Setup {
    delay_based: true,
    probing: true,
    still: None,
    start: CALL_START_BITRATE,
    top: SHARE_TOP,
    pacing: SCREEN_PACING,
    returned: Returned::All,
    wakes: Wakes::PacerClock,
    media_from_ms: 0,
    pictures: Pictures::Target,
    gate: Gate::Intervals(2),
};

/// What [`run_capacity`] saw.
pub(crate) struct LinkRun {
    pub seconds: Vec<LinkSecond>,
    /// The target every 20 ms: (millisecond, bits per second).
    pub samples: Vec<(u64, u32)>,
    /// The sender's estimation counters at the end.
    pub estimation: super::sender::Estimation,
    /// When each picture was skipped before encoding (or lost after it), and when each was handed to the decoder, in
    /// milliseconds.
    pub skipped_at: Vec<u64>,
    pub shown_at: Vec<u64>,
    /// Each media packet's arrival less its picture's capture and the one-way delay: (arrival ms, delay ms).
    pub delays: Vec<(u64, u64)>,
}

/// One second of [`run_capacity`].
#[derive(Debug)]
pub(crate) struct LinkSecond {
    pub second: u64,
    pub capacity: u32,
    /// The sender's target at the end of the second.
    pub target: u32,
    /// What reached the receiver, bits per second.
    pub delivered: u32,
    /// The longest a packet that left in this second waited in the bottleneck.
    pub queue_ms: u64,
    /// The longest a media packet that arrived in this second took from its picture's hand-over to its arrival, less
    /// the one-way delay: the sender's queue and the bottleneck's together.
    pub sender_ms: u64,
    pub dropped: u64,
}

/// A Go Live sender through a bandwidth-limited link, every piece real but the encoder and the clock: the sender
/// (`VideoSender`: pacer, retransmissions, the loss-based control and, with `delay_based`, the send-side estimator), the
/// transport seal with its transport-wide numbers, a FIFO bottleneck whose rate follows `phases` (until which second,
/// at what rate) with a drop-tail buffer of `QUEUE_BYTES`, 25 ms each way, the transport open, and this crate's receiver
/// sending back what `returned` says. The encoder makes pictures of the sender's encoder rate at 60 fps (keyframes on
/// request, 8.5 times a picture, the ratio of the 1440p60 measurement) and skips one only while the pacer's queue is
/// over its limit. The pacer is woken as `wakes` says.
pub(crate) fn run_capacity(phases: &[(u64, u32)], setup: Setup) -> LinkRun {
    use super::{receive::Received, sender::VideoSender};
    use crate::rtp::{Mode, Transport};
    let key = [9u8; 32];
    let mut tx = Transport::new(Mode::Aes256GcmRtpSize, &key);
    let mut rx = Transport::new(Mode::Aes256GcmRtpSize, &key);
    let mut sender = VideoSender::new(SSRC, RTX, SHARE_FLOOR, setup.start, setup.top);
    sender.set_delay_based(setup.delay_based);
    sender.set_probing(setup.probing);
    sender.set_screen_share(true);
    sender.set_pacing(setup.pacing);
    sender.set_framerate(FPS as u32);
    sender.set_gate(setup.gate);
    let (mut skipped_at, mut shown_at, mut delays) = (Vec::new(), Vec::new(), Vec::new());
    let mut waiting: std::collections::VecDeque<(Vec<u8>, u32, bool)> = Default::default();
    let mut held = false;
    let mut encoded = 0u64;
    let mut samples = Vec::new();
    let start = Instant::now();
    let at = |ms: u64| start + Duration::from_millis(ms);
    let mut random = Random(0x2545_F491);
    let mut receivers = Receivers::default();
    receivers.announce(1, SSRC, RTX).unwrap();
    let end = phases.last().map_or(0, |phase| phase.0) * 1000;
    let capacity_at = |ms: u64| {
        phases
            .iter()
            .find(|(until, _)| ms < until * 1000)
            .map_or(0, |phase| phase.1)
    };
    // Datagrams on the way, by arrival in microseconds; the feedback path has no bottleneck.
    let mut air: BinaryHeap<std::cmp::Reverse<(u64, u64, Vec<u8>)>> = BinaryHeap::new();
    let mut back: Vec<(u64, Vec<u8>)> = Vec::new();
    let mut order = 0u64;
    // When the bottleneck is free again, in microseconds.
    let mut link_free = 0f64;
    let mut seconds = Vec::new();
    let (mut delivered, mut queue_us, mut sender_ms, mut dropped) = (0u64, 0f64, 0u64, 0u64);
    let mut frame = 0u64;
    let mut force_key = true;
    let mut ready: Vec<Received> = Vec::new();
    // Hand-over time of each picture by RTP timestamp, for the sender's delay.
    let mut handed: std::collections::HashMap<u32, u64> = std::collections::HashMap::new();
    let mut last_report = None::<u64>;
    // The pacer's next wake, in microseconds, for the deadline it was computed from.
    let mut wake: Option<(Instant, u64)> = None;
    for now_ms in 0..end {
        let now = at(now_ms);
        let now_us = now_ms as f64 * 1000.0;
        let capacity = f64::from(capacity_at(now_ms));
        if now_ms * FPS >= frame * 1000 {
            frame += 1;
            let still = setup
                .still
                .is_some_and(|(from, until)| now_ms >= from * 1000 && now_ms < until * 1000);
            if still && frame.is_multiple_of(GOP) {
                force_key = true;
            }
            if now_ms >= setup.media_from_ms {
                if held {
                    skipped_at.push(now_ms);
                } else {
                    let average = f64::from(sender.encoder_bitrate()) / 8.0 / FPS as f64;
                    let (keyframe, size) = match setup.pictures {
                        Pictures::Target => {
                            let picture = average as usize;
                            let size = if force_key {
                                picture * KEY_BYTES / DELTA_BYTES
                            } else if still {
                                STILL_BYTES
                            } else {
                                picture
                            };
                            (force_key, size)
                        }
                        Pictures::Cbr { key, vbv } => {
                            let keyframe = force_key || encoded.is_multiple_of(GOP);
                            let key = vbv.map_or(key, |vbv| key.min(vbv));
                            let delta = (GOP as f64 - key) / (GOP as f64 - 1.0);
                            let spread = 0.9 + f64::from(random.next() % 2001) / 10_000.0;
                            let size = if keyframe {
                                average * key
                            } else {
                                average * delta * spread
                            };
                            (keyframe, size as usize)
                        }
                    };
                    encoded += 1;
                    let mut data = vec![0x55u8; size.max(200)];
                    data[..5].copy_from_slice(&[0, 0, 0, 1, if keyframe { 0x65 } else { 0x41 }]);
                    let timestamp = (frame * 90_000 / FPS) as u32;
                    handed.insert(timestamp, now_ms);
                    force_key = false;
                    if waiting.len() == FRAME_QUEUE {
                        // Lost after encoding: the chain needs a keyframe.
                        skipped_at.push(now_ms);
                        force_key = true;
                    } else {
                        waiting.push_back((data, timestamp, keyframe));
                    }
                }
            }
        }
        while !waiting.is_empty() && sender.accepts(now) {
            let (data, timestamp, keyframe) = waiting.pop_front().unwrap();
            if !sender.offer_plain(&data, timestamp, keyframe, now).unwrap() {
                force_key = true;
            }
        }
        // The pacer's wake for its current deadline.
        let deadline = sender.deadline();
        let woken = match deadline {
            None => {
                wake = None;
                false
            }
            Some(deadline) => {
                if wake.is_none_or(|(known, _)| known != deadline) {
                    let due_us = deadline.saturating_duration_since(start).as_micros() as u64;
                    let wake_us = match setup.wakes {
                        Wakes::PacerClock => due_us + u64::from(random.next() % 1000),
                        Wakes::SystemTick => {
                            due_us.div_ceil(SYSTEM_TICK_US) * SYSTEM_TICK_US
                                + u64::from(random.next() % 2000)
                        }
                    };
                    wake = Some((deadline, wake_us));
                }
                wake.is_some_and(|(_, wake_us)| wake_us <= now_ms * 1000)
            }
        };
        if woken {
            wake = None;
            sender
                .flush(now, &mut tx, |wire| {
                    let queued_bytes = (link_free - now_us).max(0.0) * capacity / 8e6;
                    if queued_bytes + wire.len() as f64 > QUEUE_BYTES {
                        dropped += 1;
                        return Ok(());
                    }
                    let departs = link_free.max(now_us) + wire.len() as f64 * 8e6 / capacity;
                    queue_us = queue_us.max(departs - now_us);
                    link_free = departs;
                    order += 1;
                    let arrives = departs as u64 + DELAY_MS * 1000;
                    air.push(std::cmp::Reverse((arrives, order, wire.to_vec())));
                    Ok(())
                })
                .unwrap();
        }
        // The receiver: open, number, reorder, hand over, and its feedback.
        while air.peek().is_some_and(|flight| flight.0.0 <= now_ms * 1000) {
            let std::cmp::Reverse((_, _, wire)) = air.pop().unwrap();
            delivered += wire.len() as u64 * 8;
            let rtp = rx.open(&wire).unwrap();
            if rtp.payload_type == rtp::H264_PAYLOAD_TYPE
                && let Some(&handed_ms) = handed.get(&rtp.timestamp)
            {
                let delay = now_ms.saturating_sub(handed_ms + DELAY_MS);
                sender_ms = sender_ms.max(delay);
                delays.push((now_ms, delay));
            }
            receivers.observe_transport(rtp.ssrc, rtp.transport_sequence, now);
            receivers.push(rtp, now);
        }
        receivers.drain(now, &mut ready);
        for unit in ready.drain(..) {
            let keyframe = dc_media::h264::is_keyframe(&unit.data);
            if receivers.accept(unit.user, keyframe) {
                shown_at.push(now_ms);
            }
        }
        for (header, body) in receivers.feedback(OWN, now) {
            let keep = match setup.returned {
                Returned::All => true,
                Returned::Nothing => false,
                Returned::ReportsEvery5s => {
                    header[1] == 201 && last_report.is_none_or(|last| now_ms >= last + 5000)
                }
            };
            if !keep {
                continue;
            }
            if header[1] == 201 {
                last_report = Some(now_ms);
            }
            back.push((now_ms + DELAY_MS, rx.seal_rtcp(&header, &body).unwrap()));
        }
        // Feedback reaching the sender.
        let (due, later): (Vec<_>, Vec<_>) = back.drain(..).partition(|(t, _)| *t <= now_ms);
        back = later;
        for (_, wire) in due {
            let compound = tx.open_rtcp(&wire).unwrap();
            if let Some(feedback) = rtp::feedback(&compound, SSRC)
                && sender.feedback(&feedback, &mut tx, now)
            {
                force_key = true;
            }
        }
        if now_ms % 20 == 0 {
            held = waiting.len() >= HOLD_AT;
            sender.tick_rate(now);
            samples.push((now_ms, sender.bitrate()));
            if sender.stale(now) {
                sender.clear();
                force_key = true;
            }
        }
        if (now_ms + 1) % 1000 == 0 {
            seconds.push(LinkSecond {
                second: now_ms / 1000,
                capacity: capacity as u32,
                target: sender.bitrate(),
                delivered: delivered as u32,
                queue_ms: (queue_us / 1000.0) as u64,
                sender_ms,
                dropped,
            });
            (delivered, queue_us, sender_ms, dropped) = (0, 0.0, 0, 0);
            handed.retain(|_, handed_ms| *handed_ms + 10_000 > now_ms);
        }
    }
    let estimation = sender.estimation(Some(&tx));
    println!("{setup:?}: {estimation:?}");
    LinkRun {
        seconds,
        samples,
        estimation,
        skipped_at,
        shown_at,
        delays,
    }
}

/// The first time from `from_ms` on at which the target is at least `level` and stays there for `hold_ms`.
fn reached(samples: &[(u64, u32)], level: u32, from_ms: u64, hold_ms: u64) -> Option<u64> {
    let mut since = None;
    for &(ms, target) in samples.iter().filter(|(ms, _)| *ms >= from_ms) {
        if target < level {
            since = None;
            continue;
        }
        let start = *since.get_or_insert(ms);
        if ms - start >= hold_ms {
            return Some(start - from_ms);
        }
    }
    None
}

/// Delivery from `from_ms` on: pictures skipped a second, the longest gap between pictures shown, and the sender's
/// delay at the median and the 95th percentile.
#[derive(Debug)]
pub(crate) struct Smoothness {
    pub skipped_per_second: f64,
    pub longest_gap_ms: u64,
    pub delay_p50_ms: u64,
    pub delay_p95_ms: u64,
}

fn smoothness(run: &LinkRun, from_ms: u64) -> Smoothness {
    let end = run.seconds.len() as u64 * 1000;
    let skipped = run.skipped_at.iter().filter(|at| **at >= from_ms).count();
    let shown: Vec<u64> = run
        .shown_at
        .iter()
        .copied()
        .filter(|at| *at >= from_ms)
        .collect();
    let longest_gap_ms = shown
        .windows(2)
        .map(|w| w[1] - w[0])
        .max()
        .unwrap_or(end - from_ms);
    let mut delays: Vec<u64> = run
        .delays
        .iter()
        .filter(|(at, _)| *at >= from_ms)
        .map(|(_, delay)| *delay)
        .collect();
    delays.sort_unstable();
    let percentile = |p: usize| {
        delays
            .get(delays.len().saturating_sub(1) * p / 100)
            .copied()
            .unwrap_or(0)
    };
    Smoothness {
        skipped_per_second: skipped as f64 * 1000.0 / (end - from_ms) as f64,
        longest_gap_ms,
        delay_p50_ms: percentile(50),
        delay_p95_ms: percentile(95),
    }
}

fn print_link(label: &str, seconds: &[LinkSecond]) {
    println!("{label}");
    println!("  s  capacity   target  delivered  queue ms  sender ms  dropped");
    for s in seconds {
        println!(
            "{:3} {:9} {:8} {:10} {:9} {:10} {:8}",
            s.second, s.capacity, s.target, s.delivered, s.queue_ms, s.sender_ms, s.dropped
        );
    }
}

#[test]
fn the_sender_settles_under_a_narrowed_link_and_recovers_when_it_widens() {
    // 8 Mb/s for 15 s, 1.5 Mb/s until 45 s, 8 Mb/s again until 120 s.
    let phases = [(15, 8_000_000), (45, 1_500_000), (120, 8_000_000)];
    let loss_only = Setup {
        delay_based: false,
        ..ORIGINAL
    };
    let before = run_capacity(&phases, loss_only).seconds;
    let unprobed = run_capacity(
        &phases,
        Setup {
            delay_based: true,
            ..loss_only
        },
    )
    .seconds;
    let after = run_capacity(&phases, NOW).seconds;
    print_link("loss-based control only:", &before);
    print_link(
        "with the delay-based estimator, without probing:",
        &unprobed,
    );
    print_link("with the delay-based estimator and probing:", &after);
    // Nothing is dropped. The queue the drop leaves grows with the rate sent before it: from the call's start rate the
    // sender found the link and sends about 6.9 Mb/s into the narrowing, without probing it was still at the 4.4 Mb/s
    // of the start's halving. Both stay under the loss-only sender's full buffer.
    let worst_queue = |run: &[LinkSecond]| run.iter().map(|s| s.queue_ms).max().unwrap();
    let drops = |run: &[LinkSecond]| run.iter().map(|s| s.dropped).sum::<u64>();
    println!(
        "worst queue {} ms, drops {}, rate into the narrowing {}; without probing {} ms, {}, {}; loss only {} ms, {}",
        worst_queue(&after),
        drops(&after),
        after[14].target,
        worst_queue(&unprobed),
        drops(&unprobed),
        unprobed[14].target,
        worst_queue(&before),
        drops(&before)
    );
    // At 6.9 Mb/s into 1.5 the detection's second overfills the 500 kB buffer: drops in the first two seconds of
    // the narrowing only (some 65 packets at a pacing factor of 2.5, 56 at 1.0; the sender skipping pictures behind the
    // one leaving, as before, sends less into it and drops some 30).
    assert!(drops(&after) < 100, "{}", drops(&after));
    assert_eq!(after[17..].iter().map(|s| s.dropped).sum::<u64>(), 0);
    assert!(drops(&after) * 20 < drops(&before));
    // A second after the drop the target is under the capacity, and stays there, but for the additive increase that
    // reaches it again after half a minute.
    let narrow = &after[16..45];
    assert!(narrow.iter().all(|s| s.target < 1_530_000), "{narrow:#?}");
    // What the half second of detection queued drains (the target holds while the delay falls) and the queue stays
    // short; without the estimator the buffer stays full and drops.
    let worst = narrow.iter().map(|s| s.queue_ms).max().unwrap();
    let worst_before = before[16..45].iter().map(|s| s.queue_ms).max().unwrap();
    assert!(
        worst <= worst_before,
        "{worst} ms against {worst_before} ms"
    );
    // The sender now delivers its whole target (no picture skipped behind the one leaving), so it meets the narrowing
    // at 6.9 Mb/s and the buffer is full for the second the detection takes; at a target some 15% under the link the
    // queue is gone 19 s after the drop.
    let settled = after[34..45].iter().map(|s| s.queue_ms).max().unwrap();
    assert!(settled < 100, "{settled} ms; {narrow:#?}");
    assert!(before[16..45].iter().map(|s| s.dropped).sum::<u64>() > 0);
    // Most of the narrowed link is still used.
    let used = narrow.iter().map(|s| u64::from(s.delivered)).sum::<u64>() / narrow.len() as u64;
    assert!(used > 1_200_000, "{used} b/s");
    // Capacity back: the rate climbs back towards it at 8% a second, and the queue stays short.
    let last = after.last().unwrap();
    assert!(last.target > 7_000_000, "{last:?}");
    // When the climb reaches the link the detector over-uses once and backs off: a queue of some 200 ms for two
    // seconds, as at the start, and nothing dropped.
    assert!(
        after[60..]
            .iter()
            .all(|s| s.queue_ms < 250 && s.dropped == 0),
        "{:?}",
        &after[60..]
    );
}

#[test]
fn a_clean_stream_is_handed_over_evenly_only_when_drained_on_arrival() {
    let tick = run(20, 0.0, false);
    let arrival = run(20, 0.0, true);
    println!("clean, drained on the 20 ms tick: {tick:?}");
    println!("clean, drained on arrival: {arrival:?}");
    // At most the start: a first packet overtaken by its successor is behind the window's first sequence number.
    assert!(arrival.frozen <= 1, "{arrival:?}");
    assert!(arrival.shown + 2 >= arrival.sent);
    // On the 20 ms tick a 60 fps stream is handed over at 50 Hz: one picture in six lands on the same tick as the one
    // before it and is never shown.
    assert!(tick.collapsed * 7 > tick.sent, "{tick:?}");
    // On arrival only network jitter (0 to 4 ms here) moves them.
    assert!(arrival.collapsed * 50 < arrival.sent, "{arrival:?}");
}

#[test]
fn bursts_behind_repairs_are_spread_out_at_playout_time() {
    let outcome = run(20, 1.0, true);
    println!("1% loss: {outcome:?}");
    // Handed over as they complete, a large share of pictures come in bursts behind a repaired packet.
    assert!(outcome.collapsed * 10 > outcome.sent, "{outcome:?}");
    // At their playout time almost none do, for a delay of about a repair's round trip.
    assert!(
        outcome.played_collapsed * 10 < outcome.collapsed,
        "{outcome:?}"
    );
    assert!(outcome.mean_delay_ms < 200.0, "{outcome:?}");
}

#[test]
fn three_percent_loss_never_freezes_the_picture_for_300_ms() {
    for loss in [1.0, 2.0, 3.0] {
        let prompt = run(60, loss, true);
        let ticked = run_with(60, loss, true, KEY_BYTES, false);
        println!("{loss}% loss, feedback when due: {prompt:?}");
        println!("{loss}% loss, feedback on the 20 ms tick: {ticked:?}");
        assert!(prompt.longest_freeze_ms < 300, "{loss}%: {prompt:?}");
        assert!(
            prompt.shown * 100 >= prompt.sent * 98,
            "{loss}%: {prompt:?}"
        );
    }
}

#[test]
#[ignore = "prints the loss table; takes a few seconds"]
fn loss_table() {
    for loss in [0.0, 1.0, 2.0, 3.0, 5.0] {
        for drain in [false, true] {
            let outcome = run(60, loss, drain);
            println!(
                "161 kB keyframes, loss {loss}% drain_on_arrival={drain}: sent {} shown {} ({:.1}%) frozen {} collapsed {} plis {} \
                 repairs {} longest gap {} ms, uneven {:.1}%; at playout time: collapsed {} uneven {:.1}% delay {:.0} ms",
                outcome.sent,
                outcome.shown,
                outcome.shown as f64 * 100.0 / outcome.sent as f64,
                outcome.frozen,
                outcome.collapsed,
                outcome.plis,
                outcome.repairs,
                outcome.longest_freeze_ms,
                outcome.uneven_percent,
                outcome.played_collapsed,
                outcome.played_uneven_percent,
                outcome.mean_delay_ms
            );
        }
    }
    // The same loss on other random sequences: the worst gap of ten runs a minute each.
    for loss in [1.0, 2.0, 3.0, 5.0] {
        let runs: Vec<Outcome> = (1..=10u32)
            .map(|seed| {
                run_seeded(
                    60,
                    loss,
                    true,
                    KEY_BYTES,
                    true,
                    seed.wrapping_mul(0x2545_F491),
                )
            })
            .collect();
        println!(
            "ten seeds, loss {loss}%: worst gap {} ms, gaps {:?}, shown {:.2}% at worst, repairs {} on average",
            runs.iter().map(|o| o.longest_freeze_ms).max().unwrap(),
            runs.iter().map(|o| o.longest_freeze_ms).collect::<Vec<_>>(),
            runs.iter()
                .map(|o| o.shown as f64 * 100.0 / o.sent as f64)
                .fold(100.0, f64::min),
            runs.iter().map(|o| o.repairs).sum::<u64>() / 10
        );
    }
    // Busier pictures: keyframes of 400 kB, as a detailed desktop gives at this rate.
    for loss in [1.0, 3.0, 5.0] {
        let outcome = run_with(60, loss, true, 400_000, true);
        println!(
            "400 kB keyframes, loss {loss}%: sent {} shown {} ({:.1}%) frozen {} plis {} longest gap {} ms",
            outcome.sent,
            outcome.shown,
            outcome.shown as f64 * 100.0 / outcome.sent as f64,
            outcome.frozen,
            outcome.plis,
            outcome.longest_freeze_ms
        );
    }
}

#[test]
fn from_the_call_start_rate_probing_reaches_the_link_rate_with_any_feedback() {
    let phases = [(60, 8_000_000)];
    let level = 8_000_000 * 85 / 100;
    let held = |run: &LinkRun| reached(&run.samples, level, 0, 5_000);
    let first = |run: &LinkRun| reached(&run.samples, level, 0, 0);
    let original = run_capacity(&phases, ORIGINAL);
    let now = run_capacity(&phases, NOW);
    let tokio_wakes = run_capacity(
        &phases,
        Setup {
            wakes: Wakes::SystemTick,
            ..NOW
        },
    );
    let silent = run_capacity(
        &phases,
        Setup {
            returned: Returned::Nothing,
            ..NOW
        },
    );
    let reports = run_capacity(
        &phases,
        Setup {
            returned: Returned::ReportsEvery5s,
            ..NOW
        },
    );
    print_link("start on 8 Mb/s, before:", &original.seconds);
    print_link("start on 8 Mb/s, now:", &now.seconds);
    print_link(
        "start on 8 Mb/s, now but with tokio's timer for the pacer:",
        &tokio_wakes.seconds,
    );
    print_link("start on 8 Mb/s, no feedback at all:", &silent.seconds);
    print_link(
        "start on 8 Mb/s, receiver reports every 5 s only:",
        &reports.seconds,
    );
    for (label, run) in [
        ("before", &original),
        ("now", &now),
        ("now, tokio timer", &tokio_wakes),
        ("no feedback", &silent),
        ("reports only", &reports),
    ] {
        println!(
            "85% of 8 Mb/s, {label}: first at {:?} ms, held 5 s from {:?} ms; {:?}",
            first(run),
            held(run),
            run.estimation
        );
    }
    // From the call's start rate the exponential probes (1.8 and 3.6 Mb/s, then twice each result) find the link
    // within half a second; the first overuse at the link's rate backs off to 85% of the acknowledged rate less
    // 5 kb/s, just under the line, and the additive increase crosses it again within a few seconds.
    assert!(first(&now).is_some_and(|ms| ms <= 500), "{:?}", first(&now));
    assert!(
        held(&now).is_some_and(|ms| ms <= 10_000),
        "{:?}",
        held(&now)
    );
    assert_eq!(now.estimation.probe_refused, 0);
    assert!(now.seconds.iter().all(|s| s.dropped == 0));
    // Without transport-wide feedback the sender gives up on the estimator two seconds after its first picture and
    // runs at the top rate under the loss-based control alone, as it did before the estimator.
    for run in [&silent, &reports] {
        assert!(run.estimation.fallback);
        assert!(first(run).is_some_and(|ms| ms <= 2_500), "{:?}", first(run));
    }
    assert!(!now.estimation.fallback);
}

#[test]
fn the_pacer_clock_keeps_probes_at_their_rate_where_the_system_tick_would_not() {
    let phases = [(30, 8_000_000)];
    let level = 8_000_000 * 85 / 100;
    let clock = run_capacity(&phases, NOW);
    let tick = run_capacity(
        &phases,
        Setup {
            wakes: Wakes::SystemTick,
            ..NOW
        },
    );
    let first = |run: &LinkRun| reached(&run.samples, level, 0, 0);
    println!(
        "85% first at {:?} ms with the pacer clock, {:?} ms with tokio's 15.6 ms timer; probes {} measured, {} dropped          against {} and {}",
        first(&clock),
        first(&tick),
        clock.estimation.probe_results,
        clock.estimation.probe_clusters_dropped,
        tick.estimation.probe_results,
        tick.estimation.probe_clusters_dropped
    );
    assert!(
        first(&clock).is_some_and(|ms| ms <= 500),
        "{:?}",
        first(&clock)
    );
    assert_eq!(clock.estimation.probe_clusters_dropped, 0);
    // On the system tick a burst is up to 15.6 ms late: clusters are dropped for lateness or measure the tick.
    assert!(
        tick.estimation.probe_clusters_dropped > 0
            || first(&tick).is_none_or(|ms| ms > 2 * first(&clock).unwrap()),
        "{:?}",
        tick.estimation
    );
}

#[test]
fn probing_starts_with_the_first_secure_picture_after_a_slow_handshake() {
    let phases = [(20, 8_000_000)];
    let level = 8_000_000 * 85 / 100;
    let slow = run_capacity(
        &phases,
        Setup {
            media_from_ms: 1_500,
            ..NOW
        },
    );
    let first = reached(&slow.samples, level, 1_500, 0);
    println!(
        "85% first {first:?} ms after media began 1.5 s late; {:?}",
        slow.estimation
    );
    assert!(first.is_some_and(|ms| ms <= 500), "{first:?}");
}

#[test]
fn the_pacer_holds_the_queue_short_at_the_top_rate() {
    for top in [3_000_000, 5_000_000, 9_000_000] {
        for wakes in [Wakes::PacerClock, Wakes::SystemTick] {
            let run = run_capacity(
                &[(40, 20_000_000)],
                Setup {
                    top,
                    start: top,
                    wakes,
                    ..NOW
                },
            );
            let steady = &run.seconds[20..];
            let worst = steady.iter().map(|s| s.sender_ms).max().unwrap();
            let delivered =
                steady.iter().map(|s| u64::from(s.delivered)).sum::<u64>() / steady.len() as u64;
            println!(
                "top {top}, {wakes:?}: steady queue delay {worst} ms, {delivered} b/s delivered"
            );
            assert!(worst < 100, "{top} {wakes:?}: {worst} ms");
            assert!(steady.iter().all(|s| s.target == top), "{top} {wakes:?}");
        }
    }
}

#[test]
fn probing_while_application_limited_keeps_the_rate_for_motion_after_a_still_screen() {
    // 8 Mb/s throughout: motion for 10 s, a still screen (200 kb/s, a keyframe every 2 s) for 20 s, motion again.
    let phases = [(80, 8_000_000)];
    let still = Some((10, 30));
    let before = run_capacity(&phases, Setup { still, ..ORIGINAL });
    let after = run_capacity(&phases, Setup { still, ..NOW });
    print_link(
        "still screen 10 to 30 s on 8 Mb/s, before:",
        &before.seconds,
    );
    print_link("still screen 10 to 30 s on 8 Mb/s, now:", &after.seconds);
    let before_ms = reached(&before.samples, 4_000_000, 30_000, 0);
    let after_ms = reached(&after.samples, 4_000_000, 30_000, 0);
    println!("4 Mb/s after motion resumed: {before_ms:?} ms before, {after_ms:?} ms now");
    // The periodic probes of the application-limited region keep finding the link while the screen is still.
    assert!(after_ms.is_some_and(|ms| ms <= 3_000), "{after_ms:?}");
    assert!(after.estimation.probe_clusters_sent > before.estimation.probe_clusters_sent);
    assert_eq!(after.seconds.iter().map(|s| s.dropped).sum::<u64>(), 0);
}

#[test]
fn a_share_with_hardware_sized_keyframes_skips_no_picture() {
    // 9 Mb/s top, keyframes of 8.5 pictures every two seconds (the 1440p60 measurement), on three links.
    for (link, gap_limit) in [(20_000_000, 100), (8_000_000, 160), (3_000_000, 160)] {
        let run = run_capacity(
            &[(40, link)],
            Setup {
                pictures: Pictures::Cbr {
                    key: 8.5,
                    vbv: None,
                },
                ..NOW
            },
        );
        let smooth = smoothness(&run, 15_000);
        println!("{} Mb/s link: {smooth:?}", link / 1_000_000);
        assert_eq!(smooth.skipped_per_second, 0.0, "{link}");
        // A keyframe still takes its time on the link itself: the gap behind it is that long, never a skipped picture.
        assert!(smooth.longest_gap_ms < gap_limit, "{link}: {smooth:?}");
        assert!(smooth.delay_p95_ms < 130, "{link}: {smooth:?}");
    }
    // The engine's 1.0 skips pictures behind the same keyframes.
    let engine = run_capacity(
        &[(40, 20_000_000)],
        Setup {
            pictures: Pictures::Cbr {
                key: 8.5,
                vbv: None,
            },
            pacing: PACING_10,
            ..NOW
        },
    );
    assert!(smoothness(&engine, 15_000).skipped_per_second > 1.0);
}

/// The Go Live pacings the comparisons try: the engine's 1.0, and 2.5 (now `SCREEN_PACING`), with the screen share's
/// queue time limit.
#[cfg(test)]
const PACING_10: Pacing = Pacing {
    factor: 1.0,
    queue_limit: Duration::from_millis(2875),
};
#[cfg(test)]
const PACING_25: Pacing = SCREEN_PACING;

#[test]
#[ignore = "prints the comparison of sender configurations; takes a minute"]
fn smoothness_table() {
    let configurations: Vec<(String, Setup)> = vec![
        (
            "A: 1.0, gate 2".into(),
            Setup {
                pacing: PACING_10,
                ..NOW
            },
        ),
        ("B: 2.5, gate 2 (chosen)".into(), NOW),
        (
            "B: 2.5, gate 4".into(),
            Setup {
                pacing: PACING_25,
                gate: Gate::Intervals(4),
                ..NOW
            },
        ),
        (
            "B: 2.5, gate 6".into(),
            Setup {
                pacing: PACING_25,
                gate: Gate::Intervals(6),
                ..NOW
            },
        ),
        (
            "C: top, 2.5, idle".into(),
            Setup {
                start: SHARE_TOP,
                pacing: PACING_25,
                gate: Gate::Idle,
                ..NOW
            },
        ),
    ];
    for (name, config) in &configurations {
        for vbv in [None, Some(3.0)] {
            if vbv.is_some() && name.starts_with('C') {
                continue;
            }
            let label = if vbv.is_some() {
                format!("D: {} + 3-picture buffer", &name[3..])
            } else {
                name.clone()
            };
            for (link, top) in [
                (20_000_000, SHARE_TOP),
                (8_000_000, SHARE_TOP),
                (3_000_000, SHARE_TOP),
            ] {
                for key in [3.0, 6.0, 8.5, 10.0] {
                    let run = run_capacity(
                        &[(40, link)],
                        Setup {
                            top,
                            pictures: Pictures::Cbr { key, vbv },
                            ..*config
                        },
                    );
                    let smooth = smoothness(&run, 15_000);
                    println!(
                        "TABLE {label} | link {} Mb/s | key {key}x | skipped {:.2}/s | longest gap {} ms | delay p50 {} ms p95 {} ms | target {}",
                        link / 1_000_000,
                        smooth.skipped_per_second,
                        smooth.longest_gap_ms,
                        smooth.delay_p50_ms,
                        smooth.delay_p95_ms,
                        run.seconds.last().unwrap().target
                    );
                }
            }
            let realistic = Pictures::Cbr { key: 8.5, vbv };
            let start = run_capacity(
                &[(40, 8_000_000)],
                Setup {
                    pictures: realistic,
                    ..*config
                },
            );
            let level = 8_000_000 * 85 / 100;
            let narrowing = run_capacity(
                &[(15, 8_000_000), (45, 1_500_000), (120, 8_000_000)],
                Setup {
                    pictures: realistic,
                    ..*config
                },
            );
            println!(
                "TABLE {label} | start 85% first {:?} ms held {:?} ms | narrowing worst queue {} ms drops {} | settled {} ms",
                reached(&start.samples, level, 0, 0),
                reached(&start.samples, level, 0, 5_000),
                narrowing.seconds.iter().map(|s| s.queue_ms).max().unwrap(),
                narrowing.seconds.iter().map(|s| s.dropped).sum::<u64>(),
                narrowing.seconds[33..45]
                    .iter()
                    .map(|s| s.queue_ms)
                    .max()
                    .unwrap()
            );
        }
    }
}
