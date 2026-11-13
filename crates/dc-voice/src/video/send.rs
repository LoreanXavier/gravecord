//! H.264 RTP sending: packetizer, pacer, retransmission history and the loss-based rate control, shared by the camera
//! and Go Live. Adapted from Serein `crates/discord-voice/src/video.rs` (MIT OR Apache-2.0).
use std::{collections::VecDeque, time::Duration};
use tokio::time::Instant;

const MTU: usize = 1200;
const RTP_HEADER: usize = 12;
// The AEAD tag plus the rtpsize nonce trailer, and the transport-wide sequence extension (a four-byte preamble and one
// word of elements, `rtp::Transport::seal`).
const TRANSPORT_OVERHEAD: usize = 20 + 8;
// Reserve the original sequence number added by RFC 4588 retransmission.
const MAX_PAYLOAD: usize = MTU - RTP_HEADER - TRANSPORT_OVERHEAD - 2;
pub(crate) const MAX_FRAGMENTS: usize = 2048;
const MAX_DAVE_FRAME: usize = 2 * 1024 * 1024 + 64 * 1024;

pub(crate) struct Packet {
    pub header: [u8; RTP_HEADER],
    pub payload: Vec<u8>,
}

/// libwebrtc's default: the pacer drains at 2.5x the encoder target, so an average frame
/// leaves in well under its frame interval while keyframes still avoid a line-rate burst.
const PACING_FACTOR: f64 = 2.5;
/// Longest sleep the pacer accounts for. A coarse OS timer (Windows' 15.6 ms default tick)
/// or a delayed task sends proportionally more on its next wake instead of losing frames.
const MAX_CATCH_UP: Duration = Duration::from_millis(100);
/// Budget a fresh access unit may send before the first pacing sleep.
const INITIAL_BURST: f64 = (4 * MTU) as f64;
/// Retransmissions may use up to the target rate over this window (libwebrtc's retransmission rate limiter,
/// `kRetransmitWindowSizeMs`).
const REPAIR_WINDOW: Duration = Duration::from_millis(500);

/// One bounded access unit, paced by elapsed time against the encoder target.
pub(crate) struct Pacer {
    packets: std::vec::IntoIter<Packet>,
    credit: f64,
    /// Repairs sent within `REPAIR_WINDOW`: when, and their size on the wire.
    repairs: VecDeque<(Instant, usize)>,
    updated: Instant,
    progress: Instant,
    pub deadline: Instant,
}
impl Pacer {
    pub fn new() -> Self {
        Self {
            packets: Vec::new().into_iter(),
            credit: INITIAL_BURST,
            repairs: VecDeque::new(),
            updated: Instant::now(),
            progress: Instant::now(),
            deadline: Instant::now(),
        }
    }
    pub fn is_empty(&self) -> bool {
        self.packets.len() == 0
    }
    pub fn clear(&mut self) {
        self.packets = Vec::new().into_iter();
    }
    pub fn stale(&self, now: Instant) -> bool {
        !self.is_empty() && now.duration_since(self.progress) >= Duration::from_millis(500)
    }
    pub fn queue(&mut self, packets: Vec<Packet>, now: Instant) {
        // Idle time is not banked into a burst; only the small initial allowance carries over.
        self.credit = self.credit.min(INITIAL_BURST);
        self.updated = now;
        self.packets = packets.into_iter();
        self.deadline = now;
        self.progress = now;
    }
    fn refill(&mut self, now: Instant, bitrate: u32) {
        let elapsed = now
            .saturating_duration_since(self.updated)
            .min(MAX_CATCH_UP)
            .as_secs_f64();
        self.updated = now;
        let target = f64::from(bitrate) / 8.0;
        let added = elapsed * target * PACING_FACTOR;
        // Keep a 5 ms bucket on precise timers; a longer wake may spend what it accrued.
        let limit = (target * PACING_FACTOR * 0.005)
            .max(INITIAL_BURST)
            .max(added);
        self.credit = (self.credit + added).min(limit);
        while self
            .repairs
            .front()
            .is_some_and(|(at, _)| now.saturating_duration_since(*at) >= REPAIR_WINDOW)
        {
            self.repairs.pop_front();
        }
    }
    /// Whether one more repair may leave now: the pacer has a packet's budget, and repairs in the last half second stay
    /// within the target rate. Repairs go before media, so they wait for nothing but these two limits.
    pub fn allow_repair(&mut self, now: Instant, bitrate: u32) -> bool {
        self.refill(now, bitrate);
        let window = f64::from(bitrate) / 8.0 * REPAIR_WINDOW.as_secs_f64();
        let sent: usize = self.repairs.iter().map(|(_, bytes)| bytes).sum();
        if self.credit < MTU as f64 || (sent + MTU) as f64 > window {
            return false;
        }
        self.repairs.push_back((now, 0));
        true
    }
    /// The size of the repair `allow_repair` just let through.
    pub fn charge_repair(&mut self, payload: usize) {
        let bytes = RTP_HEADER + payload + TRANSPORT_OVERHEAD;
        self.credit -= bytes as f64;
        if let Some(last) = self.repairs.back_mut() {
            last.1 = bytes;
        }
    }
    pub fn next_batch(&mut self, now: Instant, bitrate: u32) -> impl Iterator<Item = Packet> + '_ {
        self.refill(now, bitrate);
        let mut count = 0;
        let mut shortfall = 0.0;
        for packet in self.packets.as_slice() {
            let bytes = (RTP_HEADER + packet.payload.len() + TRANSPORT_OVERHEAD) as f64;
            if bytes > self.credit {
                shortfall = bytes - self.credit;
                break;
            }
            self.credit -= bytes;
            count += 1;
        }
        // Sleep until the next media packet, or else a pending repair, is affordable.
        let target = f64::from(bitrate.max(1)) / 8.0;
        let wait = if count < self.packets.len() {
            shortfall / (target * PACING_FACTOR)
        } else {
            let sent: usize = self.repairs.iter().map(|(_, bytes)| bytes).sum();
            let full = (sent + MTU) as f64 > target * REPAIR_WINDOW.as_secs_f64();
            let window = match self.repairs.front() {
                Some((at, _)) if full => (*at + REPAIR_WINDOW)
                    .saturating_duration_since(now)
                    .as_secs_f64(),
                _ => 0.0,
            };
            ((MTU as f64 - self.credit) / (target * PACING_FACTOR))
                .max(window)
                .max(0.0)
        };
        self.deadline = now + Duration::from_secs_f64(wait.min(0.1)).max(Duration::from_millis(1));
        // A large IDR at a reduced bitrate needs time to drain. Only a lack of
        // progress expires it, otherwise every replacement IDR could be cut off too.
        if count > 0 {
            self.progress = now;
        }
        self.packets.by_ref().take(count)
    }
}

/// The target as libwebrtc's `SendSideBandwidthEstimation` keeps it: loss-based control (below 2% loss the rate grows
/// 8% a second, above 10% it falls by half the loss, in between it holds), capped by the delay-based estimate
/// (`delay_based_limit_`, from `crate::bwe::DelayBased`) and by REMB. Nothing grows without feedback, and the rate stays
/// between the official floor and the encoder's top.
pub(crate) struct Rate {
    pub target: u32,
    minimum: u32,
    maximum: u32,
    at: Instant,
    loss: Option<u8>,
    limit: Option<(u32, Instant)>,
    estimate: Option<u32>,
    /// The delay-based estimate, once transport-wide feedback produced one.
    delay: Option<u32>,
    /// The target last handed to the encoder.
    reported: u32,
}
/// Fractions lost over 256: libwebrtc's low (2%) and high (10%) loss thresholds.
const LOW_LOSS: u8 = 5;
const HIGH_LOSS: u8 = 25;
impl Rate {
    pub fn new(minimum: u32, maximum: u32, now: Instant) -> Self {
        Self {
            target: maximum,
            minimum: minimum.min(maximum),
            maximum,
            at: now,
            loss: None,
            limit: None,
            estimate: None,
            delay: None,
            reported: maximum,
        }
    }
    /// A new delay-based estimate caps the target at once (`UpdateDelayBasedEstimate`); the target grows back towards it
    /// only as the loss-based control lets it.
    pub fn delay_based(&mut self, estimate: u32) {
        self.delay = Some(estimate);
        self.target = self.target.min(estimate).clamp(self.minimum, self.maximum);
    }
    /// The delay-based estimate, for the log.
    pub fn delay_limit(&self) -> Option<u32> {
        self.delay
    }
    pub fn observe(&mut self, loss: Option<u8>, estimate: Option<u32>) {
        if let Some(loss) = loss {
            self.loss = Some(self.loss.map_or(loss, |old| old.max(loss)));
        }
        if let Some(estimate) = estimate {
            // REMB describes the wire rate; leave room for RTP/AEAD and bounded repairs.
            let estimate = (u64::from(estimate) * 4 / 5) as u32;
            self.estimate = Some(self.estimate.map_or(estimate, |old| old.min(estimate)));
        }
    }
    /// The new target when it moved since the last call: the loss step once a second, a delay-based cap at any time.
    pub fn tick(&mut self, now: Instant) -> Option<u32> {
        if now.duration_since(self.at) >= Duration::from_secs(1) {
            self.at = now;
            self.loss_step(now);
        }
        if self.target == self.reported {
            return None;
        }
        self.reported = self.target;
        Some(self.target)
    }
    fn loss_step(&mut self, now: Instant) {
        let loss = self.loss.take();
        let estimate = self.estimate.take();
        if let Some(estimate) = estimate {
            self.limit = Some((estimate, now));
        }
        let ceiling = self
            .limit
            .filter(|(_, at)| now.duration_since(*at) < Duration::from_secs(5))
            .map_or(self.maximum, |(limit, _)| limit.min(self.maximum))
            .min(self.delay.unwrap_or(u32::MAX))
            .max(self.minimum);
        let mut target = self.target.min(ceiling);
        match loss {
            Some(loss) if loss > HIGH_LOSS => {
                // bitrate * (1 - loss / 2), loss over 256.
                target = (u64::from(target) * (512 - u64::from(loss)) / 512) as u32;
            }
            Some(loss) if loss > LOW_LOSS => {}
            _ if loss.is_some() || estimate.is_some() => {
                target = (u64::from(target) * 108 / 100 + 1000).min(u64::from(ceiling)) as u32;
            }
            _ => {}
        }
        self.target = target.clamp(self.minimum, self.maximum);
    }
}

const HISTORY_PACKETS: usize = 2048;
const HISTORY_BYTES: usize = 2 * 1024 * 1024;
const HISTORY_AGE: Duration = Duration::from_secs(1);
/// Resends of one packet; a request past this costs a keyframe instead. The receiver asks at most ten times
/// (libwebrtc's `kMaxNackRetries`), and libwebrtc's sender sets no lower bound of its own.
const MAX_RESENDS: u8 = 10;
/// A request this soon after the last resend of the same packet is the receiver's retry crossing that resend in
/// flight, not a second loss.
const RESEND_SPACING: Duration = Duration::from_millis(20);
struct Sent {
    packet: Packet,
    at: Instant,
    retries: u8,
    last_retry: Option<Instant>,
}
impl Sent {
    fn bytes(&self) -> usize {
        std::mem::size_of::<Self>() + self.packet.payload.capacity()
    }
}

/// DAVE ciphertext only, cleared across every encryption transition.
#[derive(Default)]
pub(crate) struct History {
    packets: VecDeque<Sent>,
    bytes: usize,
    pending: VecDeque<u16>,
}
impl History {
    pub fn clear(&mut self) {
        self.packets.clear();
        self.pending.clear();
        self.bytes = 0;
    }
    pub fn expire(&mut self, now: Instant) {
        while self
            .packets
            .front()
            .is_some_and(|sent| now.duration_since(sent.at) >= HISTORY_AGE)
        {
            self.bytes -= self.packets.pop_front().unwrap().bytes();
        }
        if self.packets.is_empty() {
            self.pending.clear();
        }
    }
    pub fn remember(&mut self, packet: Packet, now: Instant) {
        self.expire(now);
        let sent = Sent {
            packet,
            at: now,
            retries: 0,
            last_retry: None,
        };
        let bytes = sent.bytes();
        if bytes > HISTORY_BYTES {
            return;
        }
        while self.packets.len() >= HISTORY_PACKETS || self.bytes + bytes > HISTORY_BYTES {
            self.bytes -= self.packets.pop_front().unwrap().bytes();
        }
        self.bytes += bytes;
        self.packets.push_back(sent);
    }
    /// A cache miss needs an IDR instead of resending media outside the repair window.
    pub fn request(&mut self, sequences: &[u16], now: Instant) -> bool {
        self.expire(now);
        let mut missing = false;
        for &seq in sequences.iter().take(128) {
            let Some(sent) = self
                .packets
                .iter()
                .find(|sent| sent.packet.header[2..4] == seq.to_be_bytes())
            else {
                missing = true;
                continue;
            };
            if sent.retries >= MAX_RESENDS {
                missing = true;
                continue;
            }
            if self.pending.len() < 128
                && !self.pending.contains(&seq)
                && sent
                    .last_retry
                    .is_none_or(|at| now.duration_since(at) >= RESEND_SPACING)
            {
                self.pending.push_back(seq);
            }
        }
        missing
    }
    pub fn has_pending(&self) -> bool {
        !self.pending.is_empty()
    }
    pub fn repair(&mut self, ssrc: u32, sequence: &mut u16, now: Instant) -> Option<Packet> {
        self.expire(now);
        if ssrc == 0 {
            return None;
        }
        // Drain stale requests in this bounded pass, rather than charging an MTU
        // of repair budget for each one and blocking newer useful retransmissions.
        while let Some(seq) = self.pending.pop_front() {
            let Some(sent) = self
                .packets
                .iter_mut()
                .find(|sent| sent.packet.header[2..4] == seq.to_be_bytes())
            else {
                continue;
            };
            if sent.retries >= MAX_RESENDS {
                continue;
            }
            sent.retries += 1;
            sent.last_retry = Some(now);
            let mut header = sent.packet.header;
            header[1] = (header[1] & 0x80) | crate::rtp::H264_RTX_PAYLOAD_TYPE;
            header[2..4].copy_from_slice(&sequence.to_be_bytes());
            header[8..12].copy_from_slice(&ssrc.to_be_bytes());
            *sequence = sequence.wrapping_add(1);
            let mut payload = Vec::with_capacity(sent.packet.payload.len() + 2);
            payload.extend_from_slice(&seq.to_be_bytes());
            payload.extend_from_slice(&sent.packet.payload);
            return Some(Packet { header, payload });
        }
        None
    }
}

/// Packetize an already DAVE-encrypted Annex-B H.264 frame as RFC 6184 NAL/FU-A RTP.
pub(crate) fn packetize(
    frame: &[u8],
    sequence: &mut u16,
    timestamp: u32,
    ssrc: u32,
) -> Result<Vec<Packet>, &'static str> {
    if frame.len() > MAX_DAVE_FRAME {
        return Err("DAVE H264 frame exceeds the sharing limit");
    }
    let nalus = nalus(frame)?;
    let mut packets = Vec::new();
    for (index, nalu) in nalus.iter().enumerate() {
        if nalu.is_empty() {
            continue;
        }
        let last_nalu = index + 1 == nalus.len();
        if nalu.len() <= MAX_PAYLOAD {
            push(
                &mut packets,
                sequence,
                timestamp,
                ssrc,
                nalu.to_vec(),
                last_nalu,
            )?;
            continue;
        }
        if nalu.len() < 2 {
            return Err("Invalid H264 NAL unit");
        }
        let indicator = (nalu[0] & 0xe0) | 28;
        let kind = nalu[0] & 0x1f;
        let mut offset = 1;
        while offset < nalu.len() {
            let take = (nalu.len() - offset).min(MAX_PAYLOAD - 2);
            let mut payload = Vec::with_capacity(take + 2);
            payload.push(indicator);
            payload.push(
                kind | if offset == 1 { 0x80 } else { 0 }
                    | if offset + take == nalu.len() { 0x40 } else { 0 },
            );
            payload.extend_from_slice(&nalu[offset..offset + take]);
            let last = last_nalu && offset + take == nalu.len();
            push(&mut packets, sequence, timestamp, ssrc, payload, last)?;
            offset += take;
        }
    }
    if packets.is_empty() {
        return Err("H264 frame has no NAL units");
    }
    Ok(packets)
}

fn push(
    packets: &mut Vec<Packet>,
    sequence: &mut u16,
    timestamp: u32,
    ssrc: u32,
    payload: Vec<u8>,
    marker: bool,
) -> Result<(), &'static str> {
    if packets.len() == MAX_FRAGMENTS {
        return Err("H264 frame exceeds fragment limit");
    }
    let header = crate::rtp::video_header(
        crate::rtp::H264_PAYLOAD_TYPE,
        marker,
        *sequence,
        timestamp,
        ssrc,
    );
    *sequence = sequence.wrapping_add(1);
    packets.push(Packet { header, payload });
    Ok(())
}

fn nalus(frame: &[u8]) -> Result<Vec<&[u8]>, &'static str> {
    let mut starts = Vec::new();
    let mut at = 0;
    while at + 3 <= frame.len() {
        let size = if frame[at..].starts_with(&[0, 0, 0, 1]) {
            4
        } else if frame[at..].starts_with(&[0, 0, 1]) {
            3
        } else {
            at += 1;
            continue;
        };
        if starts.len() == MAX_FRAGMENTS {
            return Err("H264 frame exceeds fragment limit");
        }
        starts.push((at, size));
        at += size;
    }
    if starts.is_empty() || starts[0].0 != 0 {
        return Err("H264 frame is not Annex-B");
    }
    let mut out = Vec::with_capacity(starts.len());
    for (index, (start, size)) in starts.iter().enumerate() {
        let end = starts.get(index + 1).map_or(frame.len(), |next| next.0);
        if start + size >= end {
            return Err("H264 frame contains an empty NAL unit");
        }
        out.push(&frame[start + size..end]);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loss_control_follows_libwebrtc_thresholds() {
        let start = Instant::now();
        let second = |n: u64| start + Duration::from_secs(n);
        let mut rate = Rate::new(500_000, 9_000_000, start);
        // 3 and 8 percent hold the rate: a lossy line is not driven to the floor.
        for (n, loss) in [(1, 8u8), (2, 20)] {
            rate.observe(Some(loss), None);
            assert_eq!(rate.tick(second(n)), None);
        }
        assert_eq!(rate.target, 9_000_000);
        // 20 percent: down by half of it.
        rate.observe(Some(51), None);
        assert_eq!(rate.tick(second(3)), Some(8_103_515));
        // Under 2 percent: 8 percent a second plus 1 kb/s, up to the top.
        rate.observe(Some(2), None);
        assert_eq!(rate.tick(second(4)), Some(8_752_796));
        rate.observe(Some(0), None);
        assert_eq!(rate.tick(second(5)), Some(9_000_000));
        // No feedback, no growth; a REMB ceiling holds for five seconds; the floor is the official minimum.
        let mut rate = Rate::new(500_000, 9_000_000, start);
        rate.observe(None, Some(1_000_000));
        assert_eq!(rate.tick(second(1)), Some(800_000));
        for n in 2..=5 {
            rate.observe(Some(0), None);
            assert_eq!(rate.tick(second(n)), None, "held under the ceiling");
        }
        for n in 6..=40 {
            rate.observe(Some(200), None);
            rate.tick(second(n));
        }
        assert_eq!(rate.target, 500_000);
    }

    #[test]
    fn repairs_go_within_the_target_rate_over_half_a_second() {
        let start = Instant::now();
        let mut pacer = Pacer::new();
        // 1 Mb/s: 62.5 kB of repairs per 500 ms, about 52 packets of 1200 bytes.
        let mut sent = 0;
        for ms in 0..400u64 {
            let now = start + Duration::from_millis(ms);
            while pacer.allow_repair(now, 1_000_000) {
                pacer.charge_repair(MTU - RTP_HEADER - TRANSPORT_OVERHEAD);
                sent += 1;
            }
        }
        assert!((45..=52).contains(&sent), "{sent} repairs");
        // Half a second after the first, the window has room again.
        let later = start + Duration::from_millis(900);
        assert!(pacer.allow_repair(later, 1_000_000));
    }
}
