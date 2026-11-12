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
    send::{History, Packet, packetize},
    sender::KEYFRAME_INTERVAL,
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
