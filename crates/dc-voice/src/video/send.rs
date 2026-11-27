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

/// How fast the pacer drains against the target, and how long a queue may take to leave, as the pinned engine sets them.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Pacing {
    pub factor: f64,
    pub queue_limit: Duration,
}

/// A camera: `kStrictPacingMultiplier` (1.1, the default `factor` of `PacingConfig` in `video_send_stream_impl.cc`,
/// which `GetConfiguredPacingFactor` hands the transport with send-side estimation) and
/// `PacingController::kMaxExpectedQueueLength` (2 s, the default `max_delay`).
pub(crate) const CAMERA_PACING: Pacing = Pacing {
    factor: 1.1,
    queue_limit: Duration::from_millis(2000),
};
/// A screen share: the paced queue time limit (2875 ms) of the default-on `WebRTC-ProbingScreenshareBwe` settings
/// ("1.0,2875,80,40,-60,3", `kDefaultProbingScreenshareBweSettings` of `alr_experiment.cc`), but a pacing factor of 2.5
/// (`kDefaultPaceMultiplier` of `goog_cc_network_control.cc`, and Serein's sender), not the settings' 1.0. A deviation:
/// libwebrtc pairs 1.0 with an encoder frame dropper and tight encoder buffers, which this client does
/// not have; at 1.0 every keyframe of the hardware encoder (8.5 pictures at 1440p60) held the next pictures back and
/// the capture skipped some two a second, at 2.5 none (`video::lossy::smoothness_table`).
pub(crate) const SCREEN_PACING: Pacing = Pacing {
    factor: 2.5,
    queue_limit: Duration::from_millis(2875),
};
/// `PacingController::kMinSleepTime`: the least time left that the long queue rule divides by.
const MIN_QUEUE_TIME_LEFT: Duration = Duration::from_millis(1);
/// `kMaxDebtInTime` of `pacing_controller.cc`: the media debt is capped at half a second of the pacing rate.
const MAX_DEBT: Duration = Duration::from_millis(500);
/// Retransmissions may use up to the target rate over this window (libwebrtc's retransmission rate limiter,
/// `kRetransmitWindowSizeMs`).
const REPAIR_WINDOW: Duration = Duration::from_millis(500);

/// The most packets the pacer holds, a memory bound of ours: two of the largest access units.
const MAX_QUEUED_PACKETS: usize = 2 * MAX_FRAGMENTS;
/// The pacer may run up a debt of this much time at its rate before it waits for the debt to drain
/// (`send_burst_interval_`, from the pacer config's `PacerConfig::kDefaultTimeInterval`, 40 ms), but never more than
/// `kMaxBurstSize` (63 000 bytes) in one burst.
const BURST_INTERVAL: Duration = Duration::from_millis(40);
const MAX_BURST_BYTES: f64 = 63_000.0;
/// `kMaxElapsedTime` of `pacing_controller.cc`: the longest gap one update accounts for.
const MAX_ELAPSED: Duration = Duration::from_secs(2);

/// What every packet adds to the encoder's bytes: the RTP header and the transport's overhead, as
/// `ExpectedPerPacketOverhead` and the transport overhead count it, and the two bytes of the FU-A indicator and header
/// our packetizer puts in each fragment's payload (libwebrtc's packetizer counts its own headers inside the payload
/// limit; ours does too, so the encoder must leave room for them).
const PACKET_OVERHEAD: usize = RTP_HEADER + TRANSPORT_OVERHEAD + 2;

/// The encoder's share of a target (`RtpVideoSender::OnBitrateUpdated` with transport-wide feedback): the target less
/// what every packet adds, at as many packets a second as `fps` pictures of the target fill at full size
/// (`CalculateOverheadRate` as `WebRTC-Video-UseFrameRateForOverhead` computes it; that field trial is in the pinned
/// engine but off by default, where the packet rate is the target over the packet size). The frame-based count is
/// used because the pacer drains exactly the target at a pacing factor of 1: the per-second count misses each
/// picture's last, partly filled packet, and the queue would grow by that much for as long as the share runs.
pub(crate) fn payload_bitrate(target: u32, fps: u32) -> u32 {
    let fps = f64::from(fps.max(1));
    let frame = f64::from(target) / 8.0 / fps;
    let packets_per_second = ((frame / MTU as f64).ceil() * fps).ceil();
    let overhead = packets_per_second * (PACKET_OVERHEAD * 8) as f64;
    target.saturating_sub(overhead as u32)
}

/// What one packet costs on the wire.
fn wire_bytes(packet: &Packet) -> usize {
    RTP_HEADER + packet.payload.len() + TRANSPORT_OVERHEAD
}

/// The pacer's queue of access units, drained as libwebrtc's `PacingController` drains its queue: every packet sent
/// adds to a media debt that decays at the pacing rate (`UpdateBudgetWithSentData`, `UpdateBudgetWithElapsedTime`),
/// packets leave while the debt is under one burst interval, and then the pacer waits until it is paid. A late wake
/// sends what the time since the last one paid for, so the rate holds whatever the timer's granularity. The encoder is
/// not held back while a picture leaves; the queue is, by the long-queue rule.
pub(crate) struct Pacer {
    pacing: Pacing,
    /// Packets waiting, with when they were queued.
    packets: VecDeque<(Packet, Instant)>,
    /// Their bytes on the wire, and the sum of their queue times in microseconds after `base`.
    bytes: usize,
    queued_us: u128,
    base: Instant,
    /// `media_debt_`, in bytes.
    debt: f64,
    /// `last_process_time_`.
    last_process: Instant,
    /// Repairs sent within `REPAIR_WINDOW`: when, and their size on the wire.
    repairs: VecDeque<(Instant, usize)>,
    progress: Instant,
    pub deadline: Instant,
}
impl Pacer {
    pub fn new(pacing: Pacing) -> Self {
        let now = Instant::now();
        Self {
            pacing,
            packets: VecDeque::new(),
            bytes: 0,
            queued_us: 0,
            base: now,
            debt: 0.0,
            last_process: now,
            repairs: VecDeque::new(),
            progress: now,
            deadline: now,
        }
    }
    pub fn is_empty(&self) -> bool {
        self.packets.is_empty()
    }
    /// Whether another access unit may join the queue: it is empty, or what waits leaves at the pacing rate within
    /// `allowance`, its oldest packet is younger than the queue time limit (which the long-queue rule keeps it to) and
    /// the memory bound is not reached. A refused picture is not encoded: the capture holds.
    pub fn accepts(&self, now: Instant, bitrate: u32, allowance: Duration) -> bool {
        self.packets.front().is_none_or(|(_, queued)| {
            let pacing = f64::from(bitrate.max(1)) / 8.0 * self.pacing.factor;
            self.bytes as f64 / pacing < allowance.as_secs_f64()
                && now.saturating_duration_since(*queued) < self.pacing.queue_limit
                && self.packets.len() < MAX_QUEUED_PACKETS
        })
    }
    /// Whether the memory bound leaves room for `packets` more.
    pub fn has_room(&self, packets: usize) -> bool {
        self.packets.len() + packets <= MAX_QUEUED_PACKETS
    }
    pub fn set_pacing(&mut self, pacing: Pacing) {
        self.pacing = pacing;
    }
    pub fn clear(&mut self) {
        self.packets.clear();
        self.bytes = 0;
        self.queued_us = 0;
    }
    pub fn stale(&self, now: Instant) -> bool {
        !self.is_empty() && now.duration_since(self.progress) >= Duration::from_millis(500)
    }
    pub fn queue(&mut self, packets: Vec<Packet>, now: Instant, bitrate: u32) {
        if self.packets.is_empty() {
            // The pacer processed while idle (`kPausedProcessInterval`): its clock is now, and idle time paid the debt
            // but banked nothing.
            self.step_to(now, bitrate);
            self.deadline = now;
            self.progress = now;
        }
        let at = now.saturating_duration_since(self.base).as_micros();
        for packet in packets {
            self.bytes += wire_bytes(&packet);
            self.queued_us += at;
            self.packets.push_back((packet, now));
        }
    }
    fn pop(&mut self) -> Option<Packet> {
        let (packet, queued) = self.packets.pop_front()?;
        self.bytes -= wire_bytes(&packet);
        self.queued_us -= queued.saturating_duration_since(self.base).as_micros();
        Some(packet)
    }
    /// The drain rate in bytes a second (`adjusted_media_rate_`): the target times the pacing factor, or faster when
    /// what waits would not leave within the queue time limit at the packets' average time in the queue
    /// (`MaybeUpdateMediaRateDueToLongQueue`, on unless `WebRTC-Pacer-DrainQueue` is disabled).
    fn rate(&self, now: Instant, bitrate: u32) -> f64 {
        let pacing = f64::from(bitrate.max(1)) / 8.0 * self.pacing.factor;
        if self.packets.is_empty() {
            return pacing;
        }
        let average_queued = self.queued_us / self.packets.len() as u128;
        let now_us = now.saturating_duration_since(self.base).as_micros();
        let average_age = Duration::from_micros(now_us.saturating_sub(average_queued) as u64);
        let left = self
            .pacing
            .queue_limit
            .saturating_sub(average_age)
            .max(MIN_QUEUE_TIME_LEFT);
        pacing.max(self.bytes as f64 / left.as_secs_f64())
    }
    /// `UpdateTimeAndGetElapsed` and `UpdateBudgetWithElapsedTime`: the pacer's own clock moves to `to` (never back),
    /// and the debt decays for the time it moved, at most `MAX_ELAPSED`.
    fn step_to(&mut self, to: Instant, bitrate: u32) {
        let elapsed = to
            .saturating_duration_since(self.last_process)
            .min(MAX_ELAPSED);
        if to > self.last_process {
            self.last_process = to;
        }
        let rate = self.rate(to, bitrate);
        self.debt -= self.debt.min(rate * elapsed.as_secs_f64());
    }
    /// `NextSendTime` with packets waiting: at once while the debt is under one burst interval, else when it is paid.
    fn next_send_time(&self, now: Instant, bitrate: u32) -> Instant {
        let rate = self.rate(now, bitrate);
        let burst = BURST_INTERVAL.as_secs_f64().min(MAX_BURST_BYTES / rate);
        let drain = self.debt / rate;
        if drain < burst {
            self.last_process
        } else {
            self.last_process + Duration::from_secs_f64(drain)
        }
    }
    fn expire_repairs(&mut self, now: Instant) {
        while self
            .repairs
            .front()
            .is_some_and(|(at, _)| now.saturating_duration_since(*at) >= REPAIR_WINDOW)
        {
            self.repairs.pop_front();
        }
    }
    /// `UpdateBudgetWithSentData`: `bytes` left; the debt is capped at `MAX_DEBT` at the drain rate.
    pub fn charge(&mut self, now: Instant, bytes: usize, bitrate: u32) {
        let cap = self.rate(now, bitrate) * MAX_DEBT.as_secs_f64();
        self.debt = (self.debt + bytes as f64).min(cap);
    }
    /// Whether one more repair may leave now: its send time has come, and repairs in the last half second stay within
    /// the target rate. Repairs go before media, so they wait for nothing but these two limits.
    pub fn allow_repair(&mut self, now: Instant, bitrate: u32) -> bool {
        self.expire_repairs(now);
        let due = self.next_send_time(now, bitrate);
        let window = f64::from(bitrate) / 8.0 * REPAIR_WINDOW.as_secs_f64();
        let sent: usize = self.repairs.iter().map(|(_, bytes)| bytes).sum();
        if due > now || (sent + MTU) as f64 > window {
            return false;
        }
        self.step_to(due, bitrate);
        self.repairs.push_back((now, 0));
        true
    }
    /// The size of the repair `allow_repair` just let through.
    pub fn charge_repair(&mut self, now: Instant, payload: usize, bitrate: u32) {
        let bytes = RTP_HEADER + payload + TRANSPORT_OVERHEAD;
        self.charge(now, bytes, bitrate);
        if let Some(last) = self.repairs.back_mut() {
            last.1 = bytes;
        }
    }
    /// The next media packet of a probe burst. Probes leave whatever the debt says; what they send is charged to it.
    pub fn pop_probe(&mut self, now: Instant, bitrate: u32) -> Option<Packet> {
        self.step_to(now, bitrate);
        let packet = self.pop()?;
        self.progress = now;
        Some(packet)
    }
    /// The largest media packet waiting, header and payload, as `BitrateProber::OnIncomingPacket` sees packets.
    pub fn largest(&self) -> usize {
        self.packets
            .iter()
            .map(|(packet, _)| RTP_HEADER + packet.payload.len())
            .max()
            .unwrap_or(0)
    }
    /// The packets whose send time has come by `now`, each charged at its own send time as `ProcessPackets` replays a
    /// late wake (the pacer's clock moves from send time to send time, so a wake 15 ms late sends what those 15 ms paid
    /// for); then the deadline of the next one, or of a pending repair.
    pub fn next_batch(&mut self, now: Instant, bitrate: u32) -> Vec<Packet> {
        self.expire_repairs(now);
        let mut batch = Vec::new();
        while !self.packets.is_empty() {
            let due = self.next_send_time(now, bitrate);
            if due > now {
                break;
            }
            self.step_to(due, bitrate);
            let Some(packet) = self.pop() else { break };
            self.charge(due, wire_bytes(&packet), bitrate);
            batch.push(packet);
        }
        let next = if !self.packets.is_empty() || self.repairs.is_empty() {
            self.next_send_time(now, bitrate)
        } else {
            let target = f64::from(bitrate.max(1)) / 8.0;
            let sent: usize = self.repairs.iter().map(|(_, bytes)| bytes).sum();
            let full = (sent + MTU) as f64 > target * REPAIR_WINDOW.as_secs_f64();
            let window = match self.repairs.front() {
                Some((at, _)) if full => *at + REPAIR_WINDOW,
                _ => now,
            };
            self.next_send_time(now, bitrate).max(window)
        };
        self.deadline = next
            .max(now + MIN_QUEUE_TIME_LEFT)
            .min(now + Duration::from_millis(100));
        // A large IDR at a reduced bitrate needs time to drain. Only a lack of
        // progress expires it, otherwise every replacement IDR could be cut off too.
        if !batch.is_empty() {
            self.progress = now;
        }
        batch
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
    /// No delay-based estimate any more: only the loss-based control and REMB bound the target.
    pub fn clear_delay_based(&mut self) {
        self.delay = None;
    }
    /// The rate the estimation starts from (`SetBitrates` with a start rate): the target until feedback moves it. The
    /// encoder hears it on the next [`Self::tick`].
    pub fn set_start(&mut self, start: u32) {
        self.target = start.clamp(self.minimum, self.maximum);
    }
    /// A new delay-based estimate caps the target at once (`UpdateDelayBasedEstimate`); the target grows back towards it
    /// only as the loss-based control lets it.
    pub fn delay_based(&mut self, estimate: u32) {
        self.delay = Some(estimate);
        self.target = self.target.min(estimate).clamp(self.minimum, self.maximum);
    }
    /// A probe's result (`SetSendBitrate`, then `UpdateDelayBasedEstimate` with the same rate): the target is set to it
    /// at once, up or down, and it caps the target from then on.
    pub fn set_send_bitrate(&mut self, bitrate: u32) {
        self.delay = Some(bitrate);
        self.target = bitrate.clamp(self.minimum, self.maximum);
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
    /// The packet padding resends (`PaddingMode::kRecentLargePacket`, which libwebrtc's RTP module uses): the newest one
    /// about as large as the largest seen, kept after it left the history.
    large: Option<Packet>,
    /// The RTP timestamp of the newest media packet, for padding packets.
    last_timestamp: Option<u32>,
}
/// `rtp_packet_history.cc`: a packet replaces the padding packet when it is within `kOldPayloadPaddingSizeHysteresis`
/// bytes of it, or when the padding packet is `kMaxOldPayloadPaddingSequenceNumber` packets old.
const PADDING_SIZE_HYSTERESIS: usize = 100;
const MAX_OLD_PADDING_SEQUENCE: u16 = 1 << 13;
/// `rtp_sender.cc`: `kMinPayloadPaddingBytes` (padding from the history needs at least this much to send) and
/// `kMaxPaddingSizeFactor` (a padding packet may be this many times the size asked for); `kRtxHeaderSize`, the original
/// sequence number RTX adds.
const MIN_PAYLOAD_PADDING_BYTES: usize = 50;
const MAX_PADDING_SIZE_FACTOR: f64 = 3.0;
const RTX_HEADER: usize = 2;
/// `kMaxPaddingLength` of `rtp_sender.h`: the padding of one padding-only packet (`PaddingBytesInPacket` always uses
/// all of it for video).
const MAX_PADDING_LENGTH: usize = 255;
impl History {
    pub fn clear(&mut self) {
        self.packets.clear();
        self.pending.clear();
        self.bytes = 0;
        self.large = None;
    }
    /// `RTPSender::GeneratePadding` with RTX on, for about `target` bytes on the RTX stream `ssrc`, numbered from
    /// `sequence`: RTX copies of the padding packet first (`AppendPayloadPadding`), then padding-only packets of
    /// `MAX_PADDING_LENGTH` (`BuildPaddingPacket`), which carry no payload, only RTP padding. The session negotiates RTX
    /// (`rtx_payload_type` in ops 1 and 14, `rtx_ssrc` in op 12), so both are RTX packets of this connection.
    pub fn padding(&self, target: usize, ssrc: u32, sequence: &mut u16) -> Vec<Packet> {
        let mut out = Vec::new();
        if ssrc == 0 {
            return out;
        }
        let mut left = target;
        if let Some(large) = &self.large {
            let max_overshoot = ((MAX_PADDING_SIZE_FACTOR - 1.0) * target as f64 + 0.5) as usize;
            while left >= MIN_PAYLOAD_PADDING_BYTES {
                if large.payload.len() + RTX_HEADER > max_overshoot + left {
                    break;
                }
                let packet = rtx(large, ssrc, sequence);
                left -= left.min(packet.payload.len());
                out.push(packet);
            }
        }
        // A padding packet goes with the last media timestamp; none before any media.
        let Some(timestamp) = self.last_timestamp else {
            return out;
        };
        while left > 0 {
            let mut header = crate::rtp::video_header(
                crate::rtp::H264_RTX_PAYLOAD_TYPE,
                false,
                *sequence,
                timestamp,
                ssrc,
            );
            header[0] |= 0x20;
            *sequence = sequence.wrapping_add(1);
            // RFC 3550: the last padding byte counts the padding, itself included.
            let mut payload = vec![0; MAX_PADDING_LENGTH];
            payload[MAX_PADDING_LENGTH - 1] = MAX_PADDING_LENGTH as u8;
            out.push(Packet { header, payload });
            left -= left.min(MAX_PADDING_LENGTH);
        }
        out
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
        self.last_timestamp = Some(u32::from_be_bytes([
            packet.header[4],
            packet.header[5],
            packet.header[6],
            packet.header[7],
        ]));
        let replace = self.large.as_ref().is_none_or(|large| {
            let newer = u16::from_be_bytes([packet.header[2], packet.header[3]]).wrapping_sub(
                u16::from_be_bytes([large.header[2], large.header[3]])
                    .wrapping_add(MAX_OLD_PADDING_SEQUENCE),
            );
            packet.payload.len() + PADDING_SIZE_HYSTERESIS > large.payload.len()
                || (newer != 0 && newer < 0x8000)
        });
        if replace {
            self.large = Some(Packet {
                header: packet.header,
                payload: packet.payload.clone(),
            });
        }
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
            return Some(rtx(&sent.packet, ssrc, sequence));
        }
        None
    }
}

/// RFC 4588: `packet` again on the RTX stream, its sequence number leading the payload.
fn rtx(packet: &Packet, ssrc: u32, sequence: &mut u16) -> Packet {
    let mut header = packet.header;
    header[1] = (header[1] & 0x80) | crate::rtp::H264_RTX_PAYLOAD_TYPE;
    header[2..4].copy_from_slice(&sequence.to_be_bytes());
    header[8..12].copy_from_slice(&ssrc.to_be_bytes());
    *sequence = sequence.wrapping_add(1);
    let mut payload = Vec::with_capacity(packet.payload.len() + RTX_HEADER);
    payload.extend_from_slice(&packet.header[2..4]);
    payload.extend_from_slice(&packet.payload);
    Packet { header, payload }
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
        let mut pacer = Pacer::new(CAMERA_PACING);
        // 1 Mb/s: 62.5 kB of repairs per 500 ms, about 52 packets of 1200 bytes.
        let mut sent = 0;
        for ms in 0..400u64 {
            let now = start + Duration::from_millis(ms);
            while pacer.allow_repair(now, 1_000_000) {
                pacer.charge_repair(now, MTU - RTP_HEADER - TRANSPORT_OVERHEAD, 1_000_000);
                sent += 1;
            }
        }
        assert!((45..=52).contains(&sent), "{sent} repairs");
        // Half a second after the first, the window has room again.
        let later = start + Duration::from_millis(900);
        assert!(pacer.allow_repair(later, 1_000_000));
    }

    #[test]
    fn padding_resends_a_recent_large_packet_then_fills_with_padding_only_packets() {
        let start = Instant::now();
        let mut history = History::default();
        let mut sequence = 7u16;
        assert!(
            history.padding(2000, 99, &mut sequence).is_empty(),
            "no media yet"
        );
        let header = crate::rtp::video_header(crate::rtp::H264_PAYLOAD_TYPE, false, 500, 9000, 5);
        history.remember(
            Packet {
                header,
                payload: vec![1; 1100],
            },
            start,
        );
        // 2250 bytes: two copies of the 1100-byte packet on RTX, then one padding-only packet for the last 46.
        let padding = history.padding(2250, 99, &mut sequence);
        assert_eq!(padding.len(), 3);
        for (n, packet) in padding[..2].iter().enumerate() {
            assert_eq!(packet.header[1], crate::rtp::H264_RTX_PAYLOAD_TYPE);
            assert_eq!(&packet.header[8..12], &99u32.to_be_bytes());
            assert_eq!(
                u16::from_be_bytes([packet.header[2], packet.header[3]]),
                7 + n as u16
            );
            assert_eq!(&packet.payload[..2], &500u16.to_be_bytes());
            assert_eq!(packet.payload.len(), 1102);
        }
        let pad = &padding[2];
        assert_eq!(pad.header[0] & 0x20, 0x20, "padding bit");
        assert_eq!(&pad.header[4..8], &9000u32.to_be_bytes());
        assert_eq!(pad.payload.len(), 255);
        assert_eq!(pad.payload[254], 255);
        assert_eq!(sequence, 10);
        // A burst smaller than the packet allows no copy (it would overshoot three times): padding only.
        let small = history.padding(250, 99, &mut sequence);
        assert_eq!(small.len(), 1);
        assert_eq!(small[0].payload.len(), 255);
        // A newer packet much smaller than the padding packet does not replace it; the old one stays after expiry.
        let header = crate::rtp::video_header(crate::rtp::H264_PAYLOAD_TYPE, false, 501, 9100, 5);
        history.remember(
            Packet {
                header,
                payload: vec![2; 300],
            },
            start + Duration::from_secs(2),
        );
        let later = history.padding(1200, 99, &mut sequence);
        assert_eq!(&later[0].payload[..3], &[1, 244, 1]);
        // After an encryption transition there is nothing to resend.
        history.clear();
        assert!(
            history
                .padding(1200, 99, &mut sequence)
                .iter()
                .all(|packet| packet.payload.len() == 255)
        );
    }

    #[test]
    fn a_probe_result_sets_the_target_at_once() {
        let start = Instant::now();
        let mut rate = Rate::new(500_000, 9_000_000, start);
        rate.delay_based(1_000_000);
        assert_eq!(rate.target, 1_000_000);
        // A delay-based estimate only lowers the target; a probe raises it too, within the bounds.
        rate.delay_based(4_000_000);
        assert_eq!(rate.target, 1_000_000);
        rate.set_send_bitrate(7_600_000);
        assert_eq!(
            (rate.target, rate.delay_limit()),
            (7_600_000, Some(7_600_000))
        );
        rate.set_send_bitrate(20_000_000);
        assert_eq!(rate.target, 9_000_000);
    }
}
