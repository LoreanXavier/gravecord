//! Sending one video source (the camera, or a Go Live screen): SPS normalization, DAVE, packetization, pacing,
//! retransmission from a history, and rate control from the viewers' RTCP.
//!
//! Serein's camera ignores its viewers' RTCP; this sender honours it as its Go Live sender does (PLI forces a keyframe
//! at most every 300 ms, NACK resends from the history, loss and REMB steer the bitrate). The pieces are adapted from
//! Serein `video.rs` and `transport.rs` (`run_stream`), MIT OR Apache-2.0.
use super::{
    send::{CAMERA_PACING, History, Pacer, Rate, SCREEN_PACING, packetize, payload_bitrate},
    sps,
};
use crate::{
    bwe::{
        AlrDetector, DEFAULT_RTT, DelayBased, LimitedCause, ProbeCluster, ProbeController, Prober,
        Usage,
    },
    dave::Dave,
    rtp::{Feedback, Transport},
};
use dc_media::EncodedFrame;
use std::time::Duration;
use tokio::time::Instant;

/// A viewer's keyframe request is honoured at most this often (libwebrtc's `EncoderRtcpFeedback`,
/// `kMinKeyframeSendInterval` of 300 ms).
pub(crate) const KEYFRAME_INTERVAL: Duration = Duration::from_millis(300);

/// How long after the first secure picture the sender waits for transport-wide feedback before it gives up on the
/// send-side estimator and steers by receiver reports and REMB alone. libwebrtc has no such fallback (its estimator
/// simply never limits without feedback, while the loss-based target starts low); this is our own, at the delay-based
/// estimator's `kStreamTimeOut` (2 s), the time after which it, too, treats the feedback stream as gone.
const FEEDBACK_WAIT: Duration = Duration::from_secs(2);

/// A picture is taken only while what the pacer holds leaves within two frame intervals at the pacing rate, at least
/// one burst interval (40 ms). Our own rule, standing in for libwebrtc's encoder frame dropper (`FrameDropper`, on for
/// encoders without a trusted rate controller), which is not ported: it keeps a backlog encoded at a higher target from
/// standing in the queue after the target falls. With the screen share's pacing at 2.5 times the target it hardly ever
/// holds a picture back; at 1 it skipped pictures behind every keyframe.
const QUEUE_ALLOWANCE_MIN: Duration = Duration::from_millis(40);

/// How long after a keyframe left the viewers' keyframe requests are taken to be about the picture before it: a round
/// trip (`kDefaultRtt`, as none is measured) and the request interval as a margin. Our own rule: a request that
/// crossed the keyframe in flight must not queue another one behind it.
const KEYFRAME_SETTLE: Duration = Duration::from_millis(DEFAULT_RTT.as_millis() as u64 + 300);

/// When the sender takes the next picture from the capture.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Gate {
    /// While what the pacer holds leaves within this many frame intervals, at least `QUEUE_ALLOWANCE_MIN`.
    Intervals(u32),
    /// Only once everything queued has left (Serein's sender), for the simulations' comparison.
    #[cfg(test)]
    Idle,
}

/// Frame intervals of queue the gate allows.
const GATE_INTERVALS: u32 = 2;

pub(crate) struct VideoSender {
    video_ssrc: u32,
    rtx_ssrc: u32,
    sequence: u16,
    rtx_sequence: u16,
    pacer: Pacer,
    history: History,
    rate: Rate,
    minimum: u32,
    maximum: u32,
    /// Pictures a second, for the encoder's share of the target.
    fps: u32,
    /// When the next picture is taken.
    gate: Gate,
    next_keyframe: Instant,
    /// The server's `keyframe_interval` (voice op 4 or 14), in milliseconds; 0 leaves keyframes to the encoder and the
    /// viewers' requests.
    keyframe_every: Option<Duration>,
    last_keyframe: Option<Instant>,
    /// The last packet of a keyframe still in the pacer, and when the last keyframe left.
    keyframe_queued: Option<u16>,
    keyframe_left: Option<Instant>,
    /// Frames refused because the pacer's queue was full, for the log.
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
    /// When the first secure picture was queued: the estimation and its probing start there.
    started: Option<Instant>,
    /// Transport-wide feedback came at least once (`first_transport_feedback_received_`).
    feedback_seen: bool,
    /// No transport-wide feedback in time: receiver reports and REMB alone steer the target.
    fallback: bool,
    /// A probe burst found nothing to send; it waits for the next media (`probing_send_failure_`).
    probe_failed: bool,
    /// What the probe controller last heard: the target and the cause.
    reported_network: Option<(u32, LimitedCause)>,
    /// Probe padding packets sent, receiver reports and REMB messages taken, for the log.
    pub padding: u64,
    receiver_reports: u64,
    rembs: u64,
    /// Packets transport-wide feedback reported, and how many of them lost, since the last loss update.
    expected: u64,
    lost: u64,
    next_loss_update: Instant,
    packets_sent: u64,
    bytes_sent: u64,
    frames_encoded: u64,
    keyframes_encoded: u64,
    nacks: u64,
    plis: u64,
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

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct VideoSenderSnapshot {
    pub video_ssrc: u32,
    pub rtx_ssrc: u32,
    pub packets_sent: u64,
    pub bytes_sent: u64,
    pub frames_encoded: u64,
    pub keyframes_encoded: u64,
    pub nacks: u64,
    pub plis: u64,
}

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
    pub transport_cc: bool,
    pub receiver_reports: u64,
    pub rembs: u64,
    pub fallback: bool,
    pub target: u32,
    pub encoder_rate: u32,
    pub dropped_pictures: u64,
}

impl VideoSender {
    /// `minimum` and `bitrate` bound the rate control: the official `encodingVideoMinBitRate` and the top rate
    /// (`encodingVideoMaxBitRate`); the estimation starts at `start` (`CALL_START_BITRATE` for a real connection) once
    /// the first secure picture is queued.
    pub fn new(video_ssrc: u32, rtx_ssrc: u32, minimum: u32, start: u32, bitrate: u32) -> Self {
        let now = Instant::now();
        let start = start.clamp(minimum.min(bitrate), bitrate);
        let mut sender = Self {
            video_ssrc,
            rtx_ssrc,
            sequence: rand::random(),
            rtx_sequence: rand::random(),
            pacer: Pacer::new(CAMERA_PACING),
            history: History::default(),
            rate: Rate::new(minimum, bitrate, now),
            minimum,
            maximum: bitrate,
            fps: 30,
            gate: Gate::Intervals(GATE_INTERVALS),
            next_keyframe: now,
            keyframe_every: None,
            last_keyframe: None,
            keyframe_queued: None,
            keyframe_left: None,
            dropped: 0,
            repairs: 0,
            delay: DelayBased::new(start, minimum, now),
            delay_enabled: true,
            probes: ProbeController::default(),
            prober: Prober::default(),
            probing: true,
            alr: AlrDetector::default(),
            previously_in_alr: false,
            started: None,
            feedback_seen: false,
            fallback: false,
            probe_failed: false,
            reported_network: None,
            padding: 0,
            receiver_reports: 0,
            rembs: 0,
            expected: 0,
            lost: 0,
            next_loss_update: now + LOSS_UPDATE,
            packets_sent: 0,
            bytes_sent: 0,
            frames_encoded: 0,
            keyframes_encoded: 0,
            nacks: 0,
            plis: 0,
        };
        sender.rate.set_start(start);
        sender
    }

    /// The first secure picture: the network is up for this source. `SetBitrates` with the call's start and top rates
    /// starts the exponential probing, then the stream's allocation is known (`OnMaxTotalAllocatedBitrate`), which caps
    /// the probes at twice it. Starting here, not when the source is announced, keeps a slow DAVE handshake from
    /// running out the probe controller's wait for results before anything was sent.
    fn start_estimation(&mut self, now: Instant) {
        self.started = Some(now);
        let start = self.rate.target;
        let clusters = self.probes.set_bitrates(
            f64::from(start),
            f64::from(CALL_MAX_BITRATE.max(self.maximum)),
            now,
        );
        self.add_probes(&clusters, now);
        let allocated = self
            .probes
            .on_max_total_allocated(f64::from(self.maximum), now);
        self.add_probes(&allocated, now);
        self.alr.set_target(start);
    }

    /// No transport-wide feedback in time: as before the estimator, the target goes to the top and only the
    /// loss-based control (receiver reports, REMB) moves it.
    fn enter_fallback(&mut self) {
        tracing::debug!("no transport-wide feedback; loss-based control only");
        self.fallback = true;
        self.rate.clear_delay_based();
        self.rate.set_start(self.maximum);
        self.prober = Prober::default();
    }

    /// Transport-wide feedback after the fallback: the estimator starts again from the target the loss-based control
    /// reached, on a fresh clock, so nothing sent before counts.
    fn leave_fallback(&mut self, now: Instant) {
        tracing::debug!("transport-wide feedback arrived late; send-side estimation runs");
        self.fallback = false;
        self.delay = DelayBased::new(self.rate.target, self.minimum, now);
    }

    /// A screen share probes periodically while application limited and paces at its own factor (the default-on
    /// `WebRTC-ProbingScreenshareBwe` settings make `VideoSendStreamImpl` call `EnablePeriodicAlrProbing(true)` and set
    /// their pacing factor and queue time limit); a camera does neither.
    pub fn set_screen_share(&mut self, screen: bool) {
        self.probes.enable_periodic_alr_probing(screen);
        self.pacer
            .set_pacing(if screen { SCREEN_PACING } else { CAMERA_PACING });
    }

    /// The source's pictures a second, for the encoder's share of the target.
    pub fn set_framerate(&mut self, fps: u32) {
        self.fps = fps.max(1);
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
        if !self.probing || self.fallback {
            return;
        }
        for cluster in clusters {
            self.prober.create(cluster, now);
        }
    }

    /// `MaybeTriggerOnNetworkChanged`: the target or the detector's verdict moved; the probe controller and the
    /// application-limited budget follow.
    fn network_changed(&mut self, now: Instant) {
        if self.started.is_none() {
            return;
        }
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
        if !enabled {
            self.rate.set_start(self.maximum);
        }
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
            transport_cc: self.feedback_seen,
            receiver_reports: self.receiver_reports,
            rembs: self.rembs,
            fallback: self.fallback,
            target: self.rate.target,
            encoder_rate: self.encoder_bitrate(),
            dropped_pictures: self.dropped,
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

    pub fn stats_snapshot(&self) -> VideoSenderSnapshot {
        VideoSenderSnapshot {
            video_ssrc: self.video_ssrc,
            rtx_ssrc: self.rtx_ssrc,
            packets_sent: self.packets_sent,
            bytes_sent: self.bytes_sent,
            frames_encoded: self.frames_encoded,
            keyframes_encoded: self.keyframes_encoded,
            nacks: self.nacks,
            plis: self.plis,
        }
    }

    /// The rate control's current target, in bits per second.
    pub fn bitrate(&self) -> u32 {
        self.rate.target
    }

    /// The encoder's rate: the target less the packets' overhead.
    pub fn encoder_bitrate(&self) -> u32 {
        payload_bitrate(self.rate.target, self.fps)
    }

    /// Whether the next encoded frame can be taken: the pacer's queue is within its time limit. A loop that checked
    /// this may hand the picture over later: [`Self::offer`] takes it then whatever the queue's age.
    pub fn accepts(&self, now: Instant) -> bool {
        match self.gate {
            #[cfg(test)]
            Gate::Idle => self.pacer.is_empty(),
            Gate::Intervals(intervals) => {
                let allowance =
                    (Duration::from_secs(u64::from(intervals)) / self.fps).max(QUEUE_ALLOWANCE_MIN);
                self.pacer.accepts(now, self.rate.target, allowance)
            }
        }
    }

    /// The simulations' comparisons: another gate.
    #[cfg(test)]
    pub fn set_gate(&mut self, gate: Gate) {
        self.gate = gate;
    }

    /// Everything queued has left.
    pub fn idle(&self) -> bool {
        self.pacer.is_empty()
    }

    /// Normalizes, encrypts and queues one access unit. `Ok(false)` only when the pacer's memory bound refused it;
    /// the caller must then restart the chain with a keyframe.
    pub fn offer(
        &mut self,
        frame: &EncodedFrame,
        dave: &mut Dave,
        now: Instant,
    ) -> Result<bool, &'static str> {
        let normalized = sps::normalize(&frame.data)?;
        let encrypted = dave
            .encrypt(davey::MediaType::VIDEO, davey::Codec::H264, &normalized)
            .map_err(|_| "DAVE H264 encryption failed")?;
        self.queue(&encrypted, frame.timestamp, frame.keyframe, now)
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
        self.queue(data, timestamp, keyframe, now)
    }

    fn queue(
        &mut self,
        data: &[u8],
        timestamp: u32,
        keyframe: bool,
        now: Instant,
    ) -> Result<bool, &'static str> {
        let mut sequence = self.sequence;
        let packets = packetize(data, &mut sequence, timestamp, self.video_ssrc)?;
        if !self.pacer.has_room(packets.len()) {
            self.dropped += 1;
            return Ok(false);
        }
        self.sequence = sequence;
        if self.started.is_none() {
            self.start_estimation(now);
        }
        self.pacer.queue(packets, now, self.rate.target);
        self.probe_failed = false;
        if self.probing {
            self.prober.incoming(self.pacer.largest(), now);
        }
        if keyframe {
            self.last_keyframe = Some(now);
            self.keyframe_queued = Some(self.sequence.wrapping_sub(1));
            self.keyframes_encoded += 1;
        }
        self.frames_encoded += 1;
        Ok(true)
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
            .filter(|_| self.probing && !self.probe_failed && !self.fallback)
    }

    /// A media packet left: it goes to the history, and the keyframe it may end has left.
    fn media_sent(&mut self, packet: super::send::Packet, now: Instant) {
        let sequence = u16::from_be_bytes([packet.header[2], packet.header[3]]);
        if self.keyframe_queued == Some(sequence) {
            self.keyframe_queued = None;
            self.keyframe_left = Some(now);
        }
        self.history.remember(packet, now);
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
            self.packets_sent += 1;
            self.bytes_sent += wire.len() as u64;
            self.pacer
                .charge_repair(now, packet.payload.len(), self.rate.target);
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
            self.packets_sent += 1;
            self.bytes_sent += wire.len() as u64;
            self.media_sent(packet, now);
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
            self.media_sent(packet, now);
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
        self.receiver_reports += u64::from(feedback.loss.is_some());
        self.rembs += u64::from(feedback.bitrate.is_some());
        self.plis += u64::from(feedback.keyframe);
        self.nacks += feedback.nacks.len() as u64;
        self.rate.observe(feedback.loss, feedback.bitrate);
        for report in &feedback.transport {
            let packets = transport.sent.feedback(report);
            self.expected += packets.len() as u64;
            self.lost += packets
                .iter()
                .filter(|packet| packet.received.is_none())
                .count() as u64;
            if !self.feedback_seen && !self.fallback {
                tracing::debug!("transport-wide feedback received; send-side estimation runs");
            }
            self.feedback_seen = true;
            if self.fallback {
                self.leave_fallback(now);
            }
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
        // A keyframe still leaving, or one that left too recently for the viewer to have seen it, already answers.
        let answered = self.keyframe_queued.is_some()
            || self
                .keyframe_left
                .is_some_and(|left| now.saturating_duration_since(left) < KEYFRAME_SETTLE);
        if (feedback.keyframe || missing) && !answered && now >= self.next_keyframe {
            self.next_keyframe = now + KEYFRAME_INTERVAL;
            return true;
        }
        false
    }

    /// The encoder's new rate ([`Self::encoder_bitrate`]) when rate control moved the target; also the probe
    /// controller's process interval, and the wait for transport-wide feedback.
    pub fn tick_rate(&mut self, now: Instant) -> Option<u32> {
        if self.delay_enabled
            && !self.feedback_seen
            && !self.fallback
            && self
                .started
                .is_some_and(|started| now.saturating_duration_since(started) >= FEEDBACK_WAIT)
        {
            self.enter_fallback();
        }
        let fps = self.fps;
        let moved = self
            .rate
            .tick(now)
            .map(|target| payload_bitrate(target, fps));
        if self.probing && self.started.is_some() {
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
        self.keyframe_queued = None;
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
        sender.set_pacing(super::super::send::Pacing {
            factor: 1.0,
            queue_limit: Duration::from_millis(2875),
        });
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
        // The pacer sends in bursts of up to 40 ms of its rate and then waits for the debt to drain: the repair leaves
        // with the next burst, within one burst interval and one packet (16 ms at 75 kB/s).
        assert!(took <= Duration::from_millis(60), "{took:?}");
        assert!(!sender.idle());
    }

    /// A viewer asks again and again while a large keyframe drains at a low rate; the sender answers once.
    #[test]
    fn keyframe_requests_while_a_keyframe_is_leaving_do_not_queue_more_keyframes() {
        let key = [4u8; 32];
        let mut wire = Transport::new(Mode::Aes256GcmRtpSize, &key);
        let mut sender = VideoSender::new(10, 11, 500_000, CALL_START_BITRATE, 9_000_000);
        let start = Instant::now();
        sender.set_screen_share(true);
        sender.set_probing(false);
        let pli = Feedback {
            keyframe: true,
            ..Feedback::default()
        };
        // The first request is honoured: nothing is leaving.
        assert!(sender.feedback(&pli, &mut wire, start));
        let mut keyframe = vec![0x55u8; 160_000];
        keyframe[..5].copy_from_slice(&[0, 0, 0, 1, 0x65]);
        assert!(sender.offer_plain(&keyframe, 0, true, start).unwrap());
        // 160 kB at 600 kb/s takes about two seconds; a request every 200 ms meanwhile asks for nothing new.
        let mut now = start;
        let mut answered = 0;
        while !sender.idle() {
            now += Duration::from_millis(1);
            sender.flush(now, &mut wire, |_| Ok(())).unwrap();
            if (now - start).as_millis().is_multiple_of(200)
                && sender.feedback(&pli, &mut wire, now)
            {
                answered += 1;
            }
            assert!(now - start < Duration::from_secs(5));
        }
        assert_eq!(answered, 0);
        let left = now;
        // Nor for a round trip and a margin after it left; later, a request is a new loss.
        assert!(!sender.feedback(&pli, &mut wire, left + Duration::from_millis(300)));
        assert!(sender.feedback(&pli, &mut wire, left + Duration::from_millis(600)));
    }

    /// No transport-wide feedback two seconds after the first picture: the target goes to the top, and feedback that
    /// comes later brings the estimator back.
    #[test]
    fn without_transport_wide_feedback_the_loss_based_control_steers_from_the_top() {
        let mut wire = Transport::new(Mode::Aes256GcmRtpSize, &[5u8; 32]);
        let start = Instant::now();
        let mut sender = VideoSender::new(10, 11, 500_000, CALL_START_BITRATE, 9_000_000);
        sender.set_screen_share(true);
        // Nothing sent yet: no clock runs, however long the handshake takes.
        sender.tick_rate(start + Duration::from_secs(5));
        assert!(!sender.estimation(None).fallback);
        let first = start + Duration::from_secs(5);
        assert!(
            sender
                .offer_plain(&[0, 0, 0, 1, 0x65, 1, 2, 3], 0, true, first)
                .unwrap()
        );
        sender.tick_rate(first + Duration::from_millis(1900));
        assert_eq!(sender.bitrate(), CALL_START_BITRATE);
        sender.tick_rate(first + Duration::from_secs(2));
        assert!(sender.estimation(None).fallback);
        assert_eq!(sender.bitrate(), 9_000_000);
        // A report of 20% loss steers it, as the loss-based control did before the estimator.
        let report = Feedback {
            loss: Some(51),
            ..Feedback::default()
        };
        sender.feedback(&report, &mut wire, first + Duration::from_millis(2500));
        sender.tick_rate(first + Duration::from_secs(4));
        assert!(sender.bitrate() < 9_000_000);
        // Transport-wide feedback after all: the estimator runs again.
        let late = Feedback {
            transport: vec![crate::bwe::TransportCc::default()],
            ..Feedback::default()
        };
        sender.feedback(&late, &mut wire, first + Duration::from_secs(5));
        assert!(!sender.estimation(None).fallback);
        assert!(sender.estimation(None).transport_cc);
    }
}
