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
        let packets = packetize(
            &encrypted,
            &mut self.sequence,
            frame.timestamp,
            self.video_ssrc,
        )?;
        self.pacer.queue(packets, now);
        if frame.keyframe {
            self.last_keyframe = Some(now);
        }
        Ok(true)
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
            // The original DAVE ciphertext is reused; the transport nonce is always fresh.
            send(&transport.seal(&packet.header, &packet.payload)?)?;
            self.pacer.charge_repair(packet.payload.len());
            self.repairs += 1;
        }
        let batch: Vec<Packet> = self.pacer.next_batch(now, self.rate.target).collect();
        for packet in batch {
            send(&transport.seal(&packet.header, &packet.payload)?)?;
            self.history.remember(packet, now);
        }
        Ok(())
    }

    /// Takes a viewer's feedback. True when the source should produce a keyframe now.
    pub fn feedback(&mut self, feedback: &Feedback, now: Instant) -> bool {
        self.rate.observe(feedback.loss, feedback.bitrate);
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
