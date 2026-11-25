//! Transport-wide congestion control feedback (RTPFB, FMT 15; `draft-holmer-rmcat-transport-wide-cc-extensions-01`).
//!
//! The native engine registers the transport-wide sequence number as one-byte extension 5 on audio and video
//! (`rtp::TRANSPORT_SEQUENCE_EXTENSION`) and runs libwebrtc's send-side estimator, so the receiving end of a media path
//! reports when each numbered packet arrived instead of estimating a bandwidth itself (no REMB). This is that receiving
//! end: arrival times of authenticated packets in 250 us units, reported every 100 ms (libwebrtc's default feedback
//! interval for the remote estimator proxy).
//!
//! Adapted from Serein `crates/discord-voice/src/transport_feedback.rs` (MIT OR Apache-2.0). Changes: the media SSRC is
//! the source of the newest numbered packet, as libwebrtc fills it, the report interval lives here, and reported
//! arrivals are kept for libwebrtc's back window so a packet that arrives after a message called it missing is
//! reported again as received (`TransportSequenceNumberFeedbackGenenerator::OnReceivedPacket` moves its
//! `periodic_window_start_seq_` back to it). Without that, every packet reordered across a report boundary reached the
//! sender as lost, and its loss-based estimator held the stream's rate down.
use std::{
    collections::VecDeque,
    time::{Duration, Instant},
};

const HISTORY: usize = 2048;
/// Packets one feedback message describes at most, so it stays well inside one datagram.
const REPORT: usize = 256;
/// One second, in 250 us units: older arrivals are never reported.
const AGE_TICKS: i64 = 4000;
/// How long reported arrivals are kept, in 250 us units: libwebrtc's `kBackWindow` of 500 ms
/// (`transport_sequence_number_feedback_generator.cc`).
const BACK_WINDOW_TICKS: i64 = 2000;
/// How often feedback goes out while numbered packets arrive.
pub(crate) const INTERVAL: Duration = Duration::from_millis(100);

pub(crate) struct TransportFeedback {
    started: Instant,
    /// The transport-wide sequence number of `arrivals[0]`.
    base: Option<u16>,
    /// Arrival times by sequence from `base`: the reported ones of the back window, then the unreported ones.
    arrivals: VecDeque<Option<i64>>,
    /// Where the next message starts in `arrivals` (libwebrtc's `periodic_window_start_seq_`).
    start: usize,
    count: u8,
    media_ssrc: u32,
    last: Option<i64>,
    next_report: Option<Instant>,
    /// Numbered packets seen, and packets that arrived without the extension, for the log.
    pub numbered: u64,
    pub unnumbered: u64,
    /// Feedback messages built, and packets that arrived after a message had reported them missing.
    pub reports: u64,
    pub late: u64,
    /// Statuses those messages reported as not received: the loss the sender's estimator sees.
    pub missing: u64,
    /// The behaviour before the back window, for the loopback comparison: a late packet stays lost.
    #[cfg(test)]
    pub drop_late: bool,
}

impl TransportFeedback {
    pub fn new(started: Instant) -> Self {
        Self {
            started,
            base: None,
            arrivals: VecDeque::new(),
            start: 0,
            count: 0,
            media_ssrc: 0,
            last: None,
            next_report: None,
            numbered: 0,
            unnumbered: 0,
            reports: 0,
            late: 0,
            missing: 0,
            #[cfg(test)]
            drop_late: false,
        }
    }

    fn ticks(&self, now: Instant) -> i64 {
        ((now.saturating_duration_since(self.started).as_nanos() + 125_000) / 250_000)
            .min(i64::MAX as u128) as i64
    }

    fn clear(&mut self, base: Option<u16>) {
        self.arrivals.clear();
        self.base = base;
        self.start = 0;
    }

    /// Forgets the first `count` kept arrivals, which are all reported.
    fn forget(&mut self, count: usize) {
        self.arrivals.drain(..count);
        self.base = self.base.map(|base| base.wrapping_add(count as u16));
        self.start -= count;
    }

    /// One authenticated packet of `ssrc` with transport-wide `sequence`, arrived at `now`.
    pub fn receive(&mut self, ssrc: u32, sequence: u16, now: Instant) {
        self.numbered += 1;
        self.media_ssrc = ssrc;
        let at = self.ticks(now);
        if self
            .last
            .is_some_and(|last| at.saturating_sub(last) > AGE_TICKS)
        {
            self.clear(None);
        }
        let base = *self.base.get_or_insert(sequence);
        let mut offset = i32::from(sequence.wrapping_sub(base) as i16);
        if offset < 0 {
            // Older than anything kept: past the back window, or from before this sequence epoch.
            return;
        }
        if offset as usize >= HISTORY {
            // Room comes from reported arrivals first; only a jump past everything starts over.
            let forget = (offset as usize + 1 - HISTORY).min(self.start);
            self.forget(forget);
            offset -= forget as i32;
            if offset as usize >= HISTORY {
                self.clear(Some(sequence));
                offset = 0;
            }
        }
        let offset = offset as usize;
        #[cfg(test)]
        if self.drop_late && offset < self.start {
            return;
        }
        self.arrivals
            .resize(self.arrivals.len().max(offset + 1), None);
        if self.arrivals[offset].is_none() {
            // The first arrival counts; a duplicate never moves it.
            self.arrivals[offset] = Some(at);
            self.last = Some(self.last.map_or(at, |last| last.max(at)));
            if offset < self.start {
                // A message already called it missing: the next one starts here and reports it received.
                self.start = offset;
                self.late += 1;
            }
        }
    }

    /// The feedback messages due at `now` (none before the interval has passed), as (clear header, body) pairs.
    pub fn due(&mut self, sender: u32, now: Instant) -> Vec<([u8; 8], Vec<u8>)> {
        if self.next_report.is_some_and(|at| now < at) || self.start >= self.arrivals.len() {
            return Vec::new();
        }
        self.next_report = Some(now + INTERVAL);
        let mut out = Vec::new();
        while let Some(report) = self.report(sender, now) {
            out.push(report);
            if out.len() == 8 {
                break;
            }
        }
        out
    }

    /// One feedback message for the oldest unreported arrivals, or `None` when there are none.
    pub fn report(&mut self, sender: u32, now: Instant) -> Option<([u8; 8], Vec<u8>)> {
        let now_ticks = self.ticks(now);
        let oldest = now_ticks.saturating_sub(AGE_TICKS);
        while let Some(index) = self
            .arrivals
            .iter()
            .skip(self.start)
            .position(Option::is_some)
        {
            let index = self.start + index;
            if self.arrivals[index]? >= oldest {
                break;
            }
            self.start = index + 1;
        }
        // A message starts where the last one ended, so packets lost in between are reported as such; its reference
        // time is the first arrival in it.
        let anchor = self
            .arrivals
            .iter()
            .skip(self.start)
            .flatten()
            .next()
            .copied()?;
        // The reference time is in 64 ms units, modulo 24 bits.
        let reference = anchor / 256;
        let mut previous = reference * 256;
        let count = (self.arrivals.len() - self.start).min(REPORT);
        let mut symbols = Vec::<u8>::with_capacity(count);
        let mut deltas = Vec::with_capacity(count * 2);
        for arrival in self.arrivals.iter().skip(self.start).take(count) {
            let Some(at) = arrival else {
                symbols.push(0);
                self.missing += 1;
                continue;
            };
            let delta = i16::try_from(at - previous).ok()?;
            if let Ok(small) = u8::try_from(delta) {
                symbols.push(1);
                deltas.push(small);
            } else {
                symbols.push(2);
                deltas.extend_from_slice(&delta.to_be_bytes());
            }
            previous = *at;
        }
        let first = self.base?.wrapping_add(self.start as u16);
        let mut body = Vec::with_capacity(608);
        body.extend_from_slice(&self.media_ssrc.to_be_bytes());
        body.extend_from_slice(&first.to_be_bytes());
        body.extend_from_slice(&(count as u16).to_be_bytes());
        let reference = (reference as u32).to_be_bytes();
        body.extend_from_slice(&reference[1..]);
        body.push(self.count);
        for chunk in symbols.chunks(7) {
            // Status vector chunks of two-bit symbols: no run-length ambiguity.
            let mut word = 0xc000u16;
            for (i, symbol) in chunk.iter().enumerate() {
                word |= u16::from(*symbol) << (12 - i * 2);
            }
            body.extend_from_slice(&word.to_be_bytes());
        }
        body.extend_from_slice(&deltas);
        while !body.len().is_multiple_of(4) {
            body.push(0);
        }
        let mut header = [0x8f, 205, 0, 0, 0, 0, 0, 0];
        header[2..4].copy_from_slice(&((body.len() / 4 + 1) as u16).to_be_bytes());
        header[4..].copy_from_slice(&sender.to_be_bytes());
        self.start += count;
        self.count = self.count.wrapping_add(1);
        self.reports += 1;
        // Reported arrivals older than the back window go, with the holes before them.
        let cutoff = now_ticks.saturating_sub(BACK_WINDOW_TICKS);
        let mut forget = 0;
        for (index, arrival) in self.arrivals.iter().take(self.start).enumerate() {
            match arrival {
                Some(at) if *at < cutoff => forget = index + 1,
                Some(_) => break,
                None => {}
            }
        }
        self.forget(forget);
        Some((header, body))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn feedback_keeps_wrap_reordering_loss_and_signed_arrival_deltas() {
        let start = Instant::now();
        let mut feedback = TransportFeedback::new(start);
        for (seq, ms) in [(65534, 10), (0, 11), (65535, 12), (65535, 13), (1, 82)] {
            feedback.receive(77, seq, start + Duration::from_millis(ms));
        }
        let (header, body) = feedback
            .report(9, start + Duration::from_millis(100))
            .unwrap();
        assert_eq!(header, [0x8f, 205, 0, 6, 0, 0, 0, 9]);
        // Media SSRC 77, base 65534, four packets, reference 0, count 0; then the symbols and the deltas in 250 us
        // ticks: 40 and 8 small, -4 large (65535 arrived after 0), 284 large.
        assert_eq!(
            body,
            [
                0, 0, 0, 77, 255, 254, 0, 4, 0, 0, 0, 0, 0xd6, 0x80, 40, 8, 255, 252, 1, 28
            ]
        );
        feedback.receive(77, 65535, start + Duration::from_millis(101));
        assert!(
            feedback
                .report(9, start + Duration::from_millis(102))
                .is_none(),
            "a duplicate of a reported packet"
        );
        feedback.receive(77, 3, start + Duration::from_millis(110));
        let (_, body) = feedback
            .report(9, start + Duration::from_millis(120))
            .unwrap();
        // Sequence 2 never came: the message starts at it and reports it lost, then 3 received.
        assert_eq!(body, [0, 0, 0, 77, 0, 2, 0, 2, 0, 0, 1, 1, 0xc4, 0, 184, 0]);
        assert_eq!(feedback.late, 0);
    }

    #[test]
    fn a_packet_reordered_past_a_report_is_reported_again_as_received() {
        let start = Instant::now();
        let at = |ms: u64| start + Duration::from_millis(ms);
        let mut feedback = TransportFeedback::new(start);
        for seq in [10u16, 11, 13] {
            feedback.receive(5, seq, at(u64::from(seq)));
        }
        let first = feedback.due(9, at(20));
        assert_eq!(first.len(), 1);
        // 10, 11 received, 12 missing, 13 received.
        assert_eq!(&first[0].1[4..8], &[0, 10, 0, 4]);
        assert_eq!(first[0].1[12..14], [0xd4, 0x40]);
        // 12 turns up late, before 14: the next message starts at 12 and says both were received.
        feedback.receive(5, 12, at(25));
        feedback.receive(5, 14, at(30));
        assert_eq!(feedback.late, 1);
        let second = feedback.due(9, at(130));
        assert_eq!(second.len(), 1);
        let body = &second[0].1;
        assert_eq!(&body[4..8], &[0, 12, 0, 3], "base 12, three statuses");
        assert_eq!(body[11], 1, "feedback count");
        // 12 received, 13 received (again), 14 received.
        assert_eq!(body[12..14], [0xd9, 0x00]);
        // Reference 0; 12 at 100 ticks, 13 at 52 (-48 large), 14 at 120 (68).
        assert_eq!(&body[14..18], &[100, 0xff, 0xd0, 68]);
        assert_eq!(feedback.reports, 2);
    }

    #[test]
    fn reported_arrivals_are_forgotten_after_the_back_window() {
        let start = Instant::now();
        let at = |ms: u64| start + Duration::from_millis(ms);
        let mut feedback = TransportFeedback::new(start);
        feedback.receive(5, 100, at(0));
        feedback.receive(5, 102, at(1));
        assert_eq!(feedback.due(9, at(10)).len(), 1);
        feedback.receive(5, 103, at(600));
        assert_eq!(feedback.due(9, at(600)).len(), 1);
        // 101 is older than the back window now: it is not reported again.
        feedback.receive(5, 101, at(610));
        assert_eq!(feedback.late, 0);
        assert!(feedback.due(9, at(800)).is_empty());
    }

    #[test]
    fn feedback_is_due_every_interval_and_bounded() {
        let start = Instant::now();
        let mut feedback = TransportFeedback::new(start);
        for seq in 0..2048u16 {
            feedback.receive(1, seq, start + Duration::from_micros(u64::from(seq) * 200));
        }
        let first = feedback.due(9, start + Duration::from_millis(500));
        assert_eq!(first.len(), 8, "2048 packets in 256-packet messages");
        assert!(first.iter().all(|(_, body)| body.len() + 8 + 20 <= 1200));
        feedback.receive(1, 2048, start + Duration::from_millis(510));
        assert!(
            feedback
                .due(9, start + Duration::from_millis(550))
                .is_empty()
        );
        assert_eq!(feedback.due(9, start + Duration::from_millis(600)).len(), 1);
        // A restarted sequence after a silence is a new epoch, not stale packets.
        feedback.receive(1, 7, start + Duration::from_secs(5));
        assert_eq!(feedback.due(9, start + Duration::from_secs(5)).len(), 1);
    }
}
