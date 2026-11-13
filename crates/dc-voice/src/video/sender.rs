//! Sending one video source (the camera, or a Go Live screen): SPS normalization, DAVE, packetization, pacing,
//! retransmission from a history, and rate control from the viewers' RTCP.
//!
//! Serein's camera ignores its viewers' RTCP; this sender honours it as its Go Live sender does (PLI forces a keyframe
//! at most every 500 ms, NACK resends from the history, loss and REMB steer the bitrate). The pieces are adapted from
//! Serein `video.rs` and `transport.rs` (`run_stream`), MIT OR Apache-2.0.
use super::{
    send::{CAMERA_PACING, History, Pacer, Rate, SCREEN_PACING, packetize, payload_bitrate},
    sps,
};
use crate::{
    bwe::{AlrDetector, DelayBased, LimitedCause, ProbeCluster, ProbeController, Prober, Usage},
    dave::Dave,
    rtp::{Feedback, Transport},
};
use dc_media::EncodedFrame;
use std::time::Duration;
use tokio::time::Instant;

/// A viewer's keyframe request is honoured at most this often (libwebrtc's `EncoderRtcpFeedback`,
/// `kMinKeyframeSendInterval` of 300 ms).
pub(crate) const KEYFRAME_INTERVAL: Duration = Duration::from_millis(300);

pub(crate) struct VideoSender {
    video_ssrc: u32,
    rtx_ssrc: u32,
    sequence: u16,
    rtx_sequence: u16,
    pacer: Pacer,
    history: History,
    rate: Rate,
    next_keyframe: Instant,
    /// The server's `keyframe_interval` (voice op 4 or 14), in milliseconds; 0 leaves keyframes to the encoder and the
    /// viewers' requests.
    keyframe_every: Option<Duration>,
    last_keyframe: Option<Instant>,
    /// Frames dropped because the previous one had not left yet, for the log.
    pub dropped: u64,
    /// Retransmissions sent, for the log.
    pub repairs: u64,
    /// Send-side estimation from the transport-wide feedback of the connection.
    delay: DelayBased,
    delay_enabled: bool,
    /// Probing (`ProbeController`, and the pacer's `BitrateProber`) and the application-limited region.
    probes: ProbeController,
    prober: Prober,
    probing: bool,
    alr: AlrDetector,
    previously_in_alr: bool,
    /// Transport-wide feedback came at least once (`first_transport_feedback_received_`).
    feedback_seen: bool,
    /// A probe burst found nothing to send; it waits for the next media (`probing_send_failure_`).
    probe_failed: bool,
    /// What the probe controller last heard: the target and the cause.
    reported_network: Option<(u32, LimitedCause)>,
    /// Probe padding packets sent, for the log.
    pub padding: u64,
    /// Packets transport-wide feedback reported, and how many of them lost, since the last loss update.
    expected: u64,
    lost: u64,
    next_loss_update: Instant,
}

/// libwebrtc's `kLossUpdateInterval` (GoogCC, 1 s): how often the loss transport-wide feedback reports is handed to the
/// loss-based control, as GoogCC does when it runs on packet feedback.
const LOSS_UPDATE: Duration = Duration::from_secs(1);
/// `callBitRate` and `callMaxBitRate` of `getConnectionTransportOptions` (module 31595: 600 kb/s, and `us` of module
/// 731854, 10 Mb/s): the call's start and top rates that the client hands the native engine for every connection,
/// camera and Go Live alike. Inferred: they are the engine's `BitrateConstraints` (start, max), so the congestion
/// controller starts at 600 kb/s and probes up from there; `callMinBitRate` (16 kb/s) is below every stream's own floor.
pub(crate) const CALL_START_BITRATE: u32 = 600_000;
const CALL_MAX_BITRATE: u32 = 10_000_000;
/// `PacingController::kMaxEarlyProbeProcessing`: a probe burst may leave this much before it is due.
const MAX_EARLY_PROBE: Duration = Duration::from_millis(1);

/// What the send-side estimation knows, for the debug log.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Estimation {
    pub feedback_reports: u64,
    pub packets_acked: u64,
    pub packets_lost: u64,
    pub packets_unknown: u64,
    pub delay_estimate: u32,
    pub acknowledged: u32,
    pub overuses: u64,
    pub overusing: bool,
    pub probe_clusters_sent: u64,
    pub probe_clusters_dropped: u64,
    pub probe_results: u64,
    pub probe_refused: u64,
    pub last_probe: u32,
    pub application_limited: bool,
    pub padding_packets: u64,
}

impl VideoSender {
    /// `minimum` and `bitrate` bound the rate control: the official `encodingVideoMinBitRate` and the top rate
    /// (`encodingVideoMaxBitRate`); the estimation starts at `start` (`CALL_START_BITRATE` for a real connection).
    pub fn new(video_ssrc: u32, rtx_ssrc: u32, minimum: u32, start: u32, bitrate: u32) -> Self {
        let now = Instant::now();
        let mut sender = Self {
            video_ssrc,
            rtx_ssrc,
            sequence: rand::random(),
            rtx_sequence: rand::random(),
            pacer: Pacer::new(CAMERA_PACING),
            history: History::default(),
            rate: Rate::new(minimum, bitrate, now),
            next_keyframe: now,
            keyframe_every: None,
            last_keyframe: None,
            dropped: 0,
            repairs: 0,
            delay: DelayBased::new(start.clamp(minimum, bitrate), minimum, now),
            delay_enabled: true,
            probes: ProbeController::default(),
            prober: Prober::default(),
            probing: true,
            alr: AlrDetector::default(),
            previously_in_alr: false,
            feedback_seen: false,
            probe_failed: false,
            reported_network: None,
            padding: 0,
            expected: 0,
            lost: 0,
            next_loss_update: now + LOSS_UPDATE,
        };
        sender.rate.set_start(start);
        // The network is up when a sender exists: `SetBitrates` with the call's start and top rates starts the
        // exponential probing, then the stream's allocation is known (`OnMaxTotalAllocatedBitrate`), which caps the
        // probes at twice it.
        let start = sender.rate.target;
        let clusters = sender.probes.set_bitrates(
            f64::from(start),
            f64::from(CALL_MAX_BITRATE.max(bitrate)),
            now,
        );
        sender.add_probes(&clusters, now);
        let allocated = sender
            .probes
            .on_max_total_allocated(f64::from(bitrate), now);
        sender.add_probes(&allocated, now);
        sender.alr.set_target(start);
        sender
    }

    /// A screen share probes periodically while application limited and paces at its own factor (the default-on
    /// `WebRTC-ProbingScreenshareBwe` settings make `VideoSendStreamImpl` call `EnablePeriodicAlrProbing(true)` and set
    /// their pacing factor and queue time limit); a camera does neither.
    pub fn set_screen_share(&mut self, screen: bool) {
        self.probes.enable_periodic_alr_probing(screen);
        self.pacer
            .set_pacing(if screen { SCREEN_PACING } else { CAMERA_PACING });
    }

    /// The simulation's "before" runs: another pacing.
    #[cfg(test)]
    pub fn set_pacing(&mut self, pacing: super::send::Pacing) {
        self.pacer.set_pacing(pacing);
    }

    /// The simulation's "before" run: no probing and no application-limited region, as this sender was.
    #[cfg(test)]
    pub fn set_probing(&mut self, enabled: bool) {
        self.probing = enabled;
        if !enabled {
            self.prober = Prober::default();
        }
    }

    fn add_probes(&mut self, clusters: &[ProbeCluster], now: Instant) {
        if !self.probing {
            return;
        }
        for cluster in clusters {
            self.prober.create(cluster, now);
        }
    }

    /// `MaybeTriggerOnNetworkChanged`: the target or the detector's verdict moved; the probe controller and the
    /// application-limited budget follow.
    fn network_changed(&mut self, now: Instant) {
        let cause = LimitedCause::of(self.delay.usage());
        let target = self.rate.target;
        if self.reported_network == Some((target, cause)) {
            return;
        }
        self.reported_network = Some((target, cause));
        self.alr.set_target(target);
        let clusters = self
            .probes
            .set_estimated_bitrate(f64::from(target), cause, now);
        self.add_probes(&clusters, now);
    }

    /// The simulation's "before" run: loss-based control only, as this sender was without the delay-based estimator.
    #[cfg(test)]
    pub fn set_delay_based(&mut self, enabled: bool) {
        self.delay_enabled = enabled;
    }

    /// The send-side estimation's counters; the counters of `transport` are the connection's.
    pub fn estimation(&self, transport: Option<&Transport>) -> Estimation {
        let sent = transport.map(|transport| &transport.sent);
        Estimation {
            feedback_reports: sent.map_or(0, |sent| sent.reports),
            packets_acked: sent.map_or(0, |sent| sent.acked),
            packets_lost: sent.map_or(0, |sent| sent.lost),
            packets_unknown: sent.map_or(0, |sent| sent.unknown),
            delay_estimate: self.rate.delay_limit().unwrap_or(0),
            acknowledged: self.delay.acknowledged().unwrap_or(0),
            overuses: self.delay.overuses,
            overusing: self.delay.usage() == Usage::Overusing,
            probe_clusters_sent: self.prober.sent,
            probe_clusters_dropped: self.prober.dropped,
            probe_results: self.delay.probe_counts().0,
            probe_refused: self.delay.probe_counts().1,
            last_probe: self.delay.probe_counts().2,
            application_limited: self.alr.start().is_some(),
            padding_packets: self.padding,
        }
    }

    /// The server's keyframe interval in milliseconds (`setKeyframeInterval` of module 31595 hands it to the native
    /// engine as `keyframeInterval`, logged there as `keyFrameIntervalMs`). 0 switches periodic keyframes off.
    pub fn set_keyframe_interval(&mut self, milliseconds: u64) {
        self.keyframe_every = (milliseconds > 0).then(|| Duration::from_millis(milliseconds));
    }

    /// Whether the server's keyframe interval has passed since the last keyframe left.
    pub fn periodic_keyframe_due(&self, now: Instant) -> bool {
        match (self.keyframe_every, self.last_keyframe) {
            (Some(every), Some(last)) => now.saturating_duration_since(last) >= every,
            _ => false,
        }
    }

    pub fn video_ssrc(&self) -> u32 {
        self.video_ssrc
    }

    /// The rate control's current target, in bits per second.
    pub fn bitrate(&self) -> u32 {
        self.rate.target
    }

    /// Whether the next encoded frame can be taken: the pacer's queue is within its time limit.
    pub fn accepts(&self, now: Instant) -> bool {
        self.pacer.accepts(now)
    }

    /// Everything queued has left.
    pub fn idle(&self) -> bool {
        self.pacer.is_empty()
    }

    /// Normalizes, encrypts and queues one access unit. `Ok(false)` when it was dropped because the previous picture
    /// is still leaving the pacer.
    pub fn offer(
        &mut self,
        frame: &EncodedFrame,
        dave: &mut Dave,
        now: Instant,
    ) -> Result<bool, &'static str> {
        if !self.accepts(now) {
            self.dropped += 1;
            return Ok(false);
        }
        let normalized = sps::normalize(&frame.data)?;
        let encrypted = dave
            .session
            .encrypt(davey::MediaType::VIDEO, davey::Codec::H264, &normalized)
            .map_err(|_| "DAVE H264 encryption failed")?;
        self.queue(&encrypted, frame.timestamp, frame.keyframe, now)?;
        Ok(true)
    }

    /// An access unit without SPS normalization and DAVE, for the simulations.
    #[cfg(test)]
    pub fn offer_plain(
        &mut self,
        data: &[u8],
        timestamp: u32,
        keyframe: bool,
        now: Instant,
    ) -> Result<bool, &'static str> {
        if !self.accepts(now) {
            self.dropped += 1;
            return Ok(false);
        }
        self.queue(data, timestamp, keyframe, now)?;
        Ok(true)
    }

    fn queue(
        &mut self,
        data: &[u8],
        timestamp: u32,
        keyframe: bool,
        now: Instant,
    ) -> Result<(), &'static str> {
        let packets = packetize(data, &mut self.sequence, timestamp, self.video_ssrc)?;
        self.pacer.queue(packets, now);
        self.probe_failed = false;
        if self.probing {
            self.prober.incoming(self.pacer.largest(), now);
        }
        if keyframe {
            self.last_keyframe = Some(now);
        }
        Ok(())
    }

    /// When the pacer, a repair or a probe wants the loop to wake, if it does. While probing, the probe's timing is
    /// the pacer's (`NextSendTime`).
    pub fn deadline(&self) -> Option<Instant> {
        self.probe_due().or_else(|| {
            (!self.pacer.is_empty() || self.history.has_pending()).then_some(self.pacer.deadline)
        })
    }

    /// When the next probe burst is due, while probing.
    fn probe_due(&self) -> Option<Instant> {
        self.prober
            .next_probe_time()
            .filter(|_| self.probing && !self.probe_failed)
    }

    /// Sends the repairs the pacer allows now, ahead of media as libwebrtc's pacer orders them, then the media it allows.
    pub fn flush(
        &mut self,
        now: Instant,
        transport: &mut Transport,
        mut send: impl FnMut(&[u8]) -> Result<(), &'static str>,
    ) -> Result<(), &'static str> {
        // A probe cluster lasts some 15 ms; repairs wait for its end rather than share the link with it.
        while self.probe_due().is_none()
            && self.history.has_pending()
            && self.pacer.allow_repair(now, self.rate.target)
        {
            let Some(packet) = self
                .history
                .repair(self.rtx_ssrc, &mut self.rtx_sequence, now)
            else {
                break;
            };
            // The original DAVE ciphertext is reused; the transport nonce and the transport-wide number are fresh.
            let wire = transport.seal(&packet.header, &packet.payload, now)?;
            self.alr.sent(wire.len(), now);
            send(&wire)?;
            self.pacer.charge_repair(packet.payload.len());
            self.repairs += 1;
        }
        if let Some(due) = self.probe_due() {
            // While probing, everything leaves in the bursts: other packets between them would share the link with the
            // probe and spoil its measurement.
            if due <= now + MAX_EARLY_PROBE {
                let result = self.probe(now, transport, &mut send);
                transport.sent.probe = None;
                result?;
            }
            if self.probe_due().is_some() {
                return Ok(());
            }
        }
        let batch = self.pacer.next_batch(now, self.rate.target);
        for packet in batch {
            let wire = transport.seal(&packet.header, &packet.payload, now)?;
            self.alr.sent(wire.len(), now);
            send(&wire)?;
            self.history.remember(packet, now);
        }
        Ok(())
    }

    /// One probe burst (`PacingController::ProcessPackets` while probing): the media waiting first, then RTX padding of
    /// a recent packet, until the burst's size; every packet is marked with the cluster for the probe estimator.
    fn probe(
        &mut self,
        now: Instant,
        transport: &mut Transport,
        send: &mut impl FnMut(&[u8]) -> Result<(), &'static str>,
    ) -> Result<(), &'static str> {
        let Some(tag) = self.prober.current(now) else {
            return Ok(());
        };
        let recommended = self.prober.recommended_min_probe_size() as usize;
        transport.sent.probe = Some(tag);
        let target = self.rate.target;
        let mut sent = 0;
        // `GetPendingPacket`: a cluster starts with a small padding packet, for a cleaner start of its measurement.
        if self.prober.starting() {
            for packet in self
                .history
                .padding(1, self.rtx_ssrc, &mut self.rtx_sequence)
            {
                let wire = transport.seal(&packet.header, &packet.payload, now)?;
                self.alr.sent(wire.len(), now);
                send(&wire)?;
                self.pacer.charge(now, wire.len(), target);
                sent += wire.len();
                self.padding += 1;
            }
        }
        while sent < recommended {
            let Some(packet) = self.pacer.pop_probe(now, target) else {
                break;
            };
            let wire = transport.seal(&packet.header, &packet.payload, now)?;
            self.alr.sent(wire.len(), now);
            send(&wire)?;
            self.pacer.charge(now, wire.len(), target);
            sent += wire.len();
            self.history.remember(packet, now);
        }
        // `PaddingToAdd`: what the burst still lacks once the queue is empty.
        if sent < recommended && self.pacer.is_empty() {
            let padding =
                self.history
                    .padding(recommended - sent, self.rtx_ssrc, &mut self.rtx_sequence);
            for packet in padding {
                let wire = transport.seal(&packet.header, &packet.payload, now)?;
                self.alr.sent(wire.len(), now);
                send(&wire)?;
                self.pacer.charge(now, wire.len(), target);
                sent += wire.len();
                self.padding += 1;
            }
        }
        transport.sent.probe = None;
        self.probe_failed = sent == 0;
        if sent > 0 {
            self.prober.probe_sent(now, sent);
        }
        Ok(())
    }

    /// Takes a viewer's feedback, and the transport-wide feedback of the connection, matched against what `transport`
    /// sent. True when the source should produce a keyframe now.
    pub fn feedback(
        &mut self,
        feedback: &Feedback,
        transport: &mut Transport,
        now: Instant,
    ) -> bool {
        self.rate.observe(feedback.loss, feedback.bitrate);
        for report in &feedback.transport {
            let packets = transport.sent.feedback(report);
            self.expected += packets.len() as u64;
            self.lost += packets
                .iter()
                .filter(|packet| packet.received.is_none())
                .count() as u64;
            self.feedback_seen = true;
            // `OnTransportPacketsFeedback`: the region ending first, then the estimators.
            let alr_start = self.alr.start().filter(|_| self.probing);
            if self.previously_in_alr && alr_start.is_none() {
                self.delay.alr_ended(now);
                self.probes.set_alr_ended_time(now);
            }
            self.previously_in_alr = alr_start.is_some();
            let update = self.delay.feedback(&packets, now, alr_start.is_some());
            if !self.delay_enabled {
                continue;
            }
            if let Some(estimate) = update.estimate {
                if update.probe {
                    self.rate.set_send_bitrate(estimate);
                } else {
                    self.rate.delay_based(estimate);
                }
            }
            self.network_changed(now);
            if update.recovered && self.probing {
                self.probes.set_alr_start_time(alr_start);
                let clusters = self.probes.request_probe(now);
                self.add_probes(&clusters, now);
            }
        }
        if now >= self.next_loss_update && self.expected > 0 {
            // Q8 like a report block's fraction lost (`UpdatePacketsLost`).
            let fraction = (self.lost * 256 / self.expected).min(255) as u8;
            self.rate.observe(Some(fraction), None);
            self.expected = 0;
            self.lost = 0;
            self.next_loss_update = now + LOSS_UPDATE;
        }
        let missing = self.history.request(&feedback.nacks, now);
        if (feedback.keyframe || missing) && now >= self.next_keyframe {
            self.next_keyframe = now + KEYFRAME_INTERVAL;
            return true;
        }
        false
    }

    /// The encoder's rate: the target less the packets' overhead, for the simulations (the loops take it from
    /// [`Self::tick_rate`]).
    #[cfg(test)]
    pub fn encoder_bitrate(&self) -> u32 {
        payload_bitrate(self.rate.target)
    }

    /// The encoder's new rate ([`Self::encoder_bitrate`]) when rate control moved the target; also the probe
    /// controller's process interval.
    pub fn tick_rate(&mut self, now: Instant) -> Option<u32> {
        let moved = self.rate.tick(now).map(payload_bitrate);
        if self.probing {
            // No periodic probe in the region before feedback shows the far end measures it.
            let alr_start = self.alr.start().filter(|_| self.feedback_seen);
            self.probes.set_alr_start_time(alr_start);
            let clusters = self.probes.process(now);
            self.add_probes(&clusters, now);
        }
        self.network_changed(now);
        moved
    }

    /// Drops everything in flight: the encryption epoch changed, or the pacer stalled.
    pub fn clear(&mut self) {
        self.pacer.clear();
        self.history.clear();
    }

    /// The pacer made no progress for half a second.
    pub fn stale(&self, now: Instant) -> bool {
        self.pacer.stale(now)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rtp::{H264_PAYLOAD_TYPE, H264_RTX_PAYLOAD_TYPE, Mode};

    /// At a pacing factor of 1 a keyframe takes many frame intervals to leave; a repair still goes ahead of it within
    /// about one packet's time, and the pictures behind it queue instead of being dropped.
    #[test]
    fn a_repair_overtakes_a_keyframe_draining_at_pacing_factor_one() {
        let key = [3u8; 32];
        let mut wire = Transport::new(Mode::Aes256GcmRtpSize, &key);
        let reader = Transport::new(Mode::Aes256GcmRtpSize, &key);
        let start = Instant::now();
        let mut sender = VideoSender::new(10, 11, 500_000, CALL_START_BITRATE, 9_000_000);
        sender.set_screen_share(true);
        sender.set_probing(false);
        let mut keyframe = vec![0x55u8; 160_000];
        keyframe[..5].copy_from_slice(&[0, 0, 0, 1, 0x65]);
        assert!(sender.offer_plain(&keyframe, 0, true, start).unwrap());
        let mut sent = Vec::new();
        let mut now = start;
        for _ in 0..100 {
            now += Duration::from_millis(1);
            sender
                .flush(now, &mut wire, |data| {
                    sent.push(data.to_vec());
                    Ok(())
                })
                .unwrap();
        }
        // 100 ms at 600 kb/s is some 7.5 kB of the 160: the keyframe is still leaving, and the next picture is taken.
        assert!(!sender.idle());
        assert!(
            sender
                .offer_plain(&keyframe[..2000], 1500, false, now)
                .unwrap()
        );
        let first = reader.open(&sent[0]).unwrap();
        assert_eq!(first.payload_type, H264_PAYLOAD_TYPE);
        let nack = Feedback {
            nacks: vec![first.sequence],
            ..Feedback::default()
        };
        sender.feedback(&nack, &mut wire, now);
        let asked = now;
        let mut repaired = None;
        for _ in 0..100 {
            now += Duration::from_millis(1);
            sender
                .flush(now, &mut wire, |data| {
                    let rtp = reader.open(data).unwrap();
                    if rtp.payload_type == H264_RTX_PAYLOAD_TYPE && repaired.is_none() {
                        repaired = Some(now);
                    }
                    Ok(())
                })
                .unwrap();
            if repaired.is_some() {
                break;
            }
        }
        let took = repaired.expect("the repair left") - asked;
        // One 1200-byte packet at 75 kB/s is 16 ms.
        assert!(took <= Duration::from_millis(20), "{took:?}");
        assert!(!sender.idle());
    }
}
