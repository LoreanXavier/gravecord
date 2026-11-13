//! Sending one video source (the camera, or a Go Live screen): SPS normalization, DAVE, packetization, pacing,
//! retransmission from a history, and rate control from the viewers' RTCP.
//!
//! Serein's camera ignores its viewers' RTCP; this sender honours it as its Go Live sender does (PLI forces a keyframe
//! at most every 500 ms, NACK resends from the history, loss and REMB steer the bitrate). The pieces are adapted from
//! Serein `video.rs` and `transport.rs` (`run_stream`), MIT OR Apache-2.0.
use super::{
    send::{History, Pacer, Packet, Rate, packetize},
    sps,
};
use crate::{
    bwe::{DelayBased, Usage},
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
    /// Packets transport-wide feedback reported, and how many of them lost, since the last loss update.
    expected: u64,
    lost: u64,
    next_loss_update: Instant,
}

/// libwebrtc's `kLossUpdateInterval` (GoogCC, 1 s): how often the loss transport-wide feedback reports is handed to the
/// loss-based control, as GoogCC does when it runs on packet feedback.
const LOSS_UPDATE: Duration = Duration::from_secs(1);

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
}

impl VideoSender {
    /// `minimum` and `bitrate` bound the rate control: the official `encodingVideoMinBitRate` and the top rate.
    pub fn new(video_ssrc: u32, rtx_ssrc: u32, minimum: u32, bitrate: u32) -> Self {
        let now = Instant::now();
        Self {
            video_ssrc,
            rtx_ssrc,
            sequence: rand::random(),
            rtx_sequence: rand::random(),
            pacer: Pacer::new(),
            history: History::default(),
            rate: Rate::new(minimum, bitrate, now),
            next_keyframe: now,
            keyframe_every: None,
            last_keyframe: None,
            dropped: 0,
            repairs: 0,
            delay: DelayBased::new(bitrate, minimum, now),
            delay_enabled: true,
            expected: 0,
            lost: 0,
            next_loss_update: now + LOSS_UPDATE,
        }
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

    /// Whether the next encoded frame can be taken (the previous one has left).
    pub fn ready(&self) -> bool {
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
        if !self.ready() {
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
        if !self.ready() {
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
        if keyframe {
            self.last_keyframe = Some(now);
        }
        Ok(())
    }

    /// When the pacer or a repair wants the loop to wake, if it does.
    pub fn deadline(&self) -> Option<Instant> {
        (!self.pacer.is_empty() || self.history.has_pending()).then_some(self.pacer.deadline)
    }

    /// Sends the repairs the pacer allows now, ahead of media as libwebrtc's pacer orders them, then the media it allows.
    pub fn flush(
        &mut self,
        now: Instant,
        transport: &mut Transport,
        mut send: impl FnMut(&[u8]) -> Result<(), &'static str>,
    ) -> Result<(), &'static str> {
        while self.history.has_pending() && self.pacer.allow_repair(now, self.rate.target) {
            let Some(packet) = self
                .history
                .repair(self.rtx_ssrc, &mut self.rtx_sequence, now)
            else {
                break;
            };
            // The original DAVE ciphertext is reused; the transport nonce and the transport-wide number are fresh.
            send(&transport.seal(&packet.header, &packet.payload, now)?)?;
            self.pacer.charge_repair(packet.payload.len());
            self.repairs += 1;
        }
        let batch: Vec<Packet> = self.pacer.next_batch(now, self.rate.target).collect();
        for packet in batch {
            send(&transport.seal(&packet.header, &packet.payload, now)?)?;
            self.history.remember(packet, now);
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
            if let Some(estimate) = self.delay.feedback(&packets, now)
                && self.delay_enabled
            {
                self.rate.delay_based(estimate);
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

    /// The new target bit rate when rate control moved it.
    pub fn tick_rate(&mut self, now: Instant) -> Option<u32> {
        self.rate.tick(now)
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
