//! Snapshot types for official WebRTC / voice transport telemetry (`PopoutWindowRTCDebug` / `RTCStatsStore`).
//!
//! Call and stream tasks refresh [`RtcStatsSnapshot`] on periodic 200 ms–1 s intervals into shared state read by
//! [`crate::Controls::rtc_stats`]. Audio callbacks never touch or allocate inside these structs.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// Complete call + active Go Live stream telemetry snapshot.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RtcStatsSnapshot {
    pub connected: bool,
    pub transport: TransportStats,
    pub outbound: OutboundStats,
    pub camera: Option<CameraStats>,
    pub inbound: HashMap<u64, InboundUserStats>,
    pub streams: Vec<StreamStats>,
}

impl RtcStatsSnapshot {
    /// Returns a copy with personal network fields (`local_address`) cleared for Streamer Mode display.
    pub fn redacted_for_streamer(&self) -> Self {
        let mut copy = self.clone();
        copy.transport.local_address = None;
        for stream in &mut copy.streams {
            stream.transport.local_address = None;
        }
        copy
    }
}

/// Transport-layer connection counters and BWE state.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TransportStats {
    pub hostname: Option<String>,
    /// Local UDP socket address; mark sensitive so UI hides it when Streamer Mode (`hidePersonalInformation`) is on.
    pub local_address: Option<String>,
    pub media_session_id: Option<String>,
    pub ping_ms: Option<u64>,
    pub available_outgoing_bitrate: u32,
    pub outbound_bitrate_estimate: u32,
    pub inbound_bitrate_estimate: u32,
    pub outbound_bytes: u64,
    pub inbound_bytes: u64,
    pub outbound_packets: u64,
    pub inbound_packets: u64,
    pub delay_estimate_ms: u32,
    pub pacer_delay_ms: u32,
    pub decryption_failures: u64,
    pub routing_time_ms: Option<u64>,
    pub tls_version: Option<String>,
    pub mode: Option<String>,
    pub secure_frames_protocol_version: u32,
    pub output_device_underruns: u64,
    pub receiver_reports: Vec<ReceiverReportStats>,
}

/// Per-receiver RTCP Receiver Report summary attached to transport telemetry.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ReceiverReportStats {
    pub user_id: u64,
    pub ssrc: u32,
    pub bitrate_bps: u32,
    pub fraction_lost: f32,
    pub rtt_ms: Option<u64>,
}

/// Local outbound RTP audio + video streams.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct OutboundStats {
    pub audio: Option<OutboundAudioStats>,
    pub video: Option<OutboundVideoStats>,
}

/// Local outbound Opus audio stats.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct OutboundAudioStats {
    pub ssrc: u32,
    pub codec: &'static str,
    pub payload_type: u8,
    pub bitrate: u32,
    pub bytes_sent: u64,
    pub packets_sent: u64,
    pub packets_lost: u64,
    pub fraction_lost: f32,
    pub frames_captured: u64,
    /// Normalized RMS level (`0.0..=1.0`).
    pub audio_level: f32,
    pub audio_detected: Option<bool>,
    pub encrypt_success_count: u64,
    pub encrypt_failure_count: u64,
    pub secure_frames_protocol_version: u32,
}

/// Local outbound H.264 video stats.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct OutboundVideoStats {
    pub ssrc: u32,
    pub rtx_ssrc: u32,
    pub codec: &'static str,
    pub payload_type: u8,
    pub bitrate_target: u32,
    pub bitrate_actual: u32,
    pub bytes_sent: u64,
    pub packets_sent: u64,
    pub width: u32,
    pub height: u32,
    pub fps_encoded: u32,
    pub frames_encoded: u64,
    pub frames_dropped: u64,
    pub key_frames_encoded: u64,
    pub qp_sum: u64,
    pub nacks: u64,
    pub plis: u64,
    pub firs: u64,
    pub encoder_implementation_name: Option<String>,
    pub average_encode_time_ms: f32,
    pub encrypt_success_count: u64,
    pub encrypt_failure_count: u64,
    pub secure_frames_protocol_version: u32,
}

/// Local camera capture/encoding state.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CameraStats {
    pub device_name: Option<String>,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub frames_captured: u64,
    pub frames_dropped: u64,
    pub captured: bool,
    pub encoded: bool,
}

/// Remote user inbound audio and/or video statistics.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct InboundUserStats {
    pub audio: Option<InboundAudioStats>,
    pub video: Option<InboundVideoStats>,
}

/// Remote speaker Opus/NetEq inbound stats.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct InboundAudioStats {
    pub ssrc: u32,
    pub codec: &'static str,
    pub payload_type: u8,
    pub bytes_received: u64,
    pub packets_received: u64,
    pub packets_lost: u64,
    pub packets_late: u64,
    pub packets_discarded: u64,
    pub fec_recovered: u64,
    pub jitter_ms: f32,
    pub jitter_buffer_ms: u32,
    pub jitter_buffer_preferred_ms: i32,
    /// Normalized RMS voice level (`0.0..=1.0`).
    pub audio_level: f32,
    pub decoding_muted_output: bool,
    pub concealed_samples: u64,
    pub concealment_events: u64,
    pub accelerated_samples: u64,
    pub expanded_samples: u64,
    pub underrun_samples: u64,
    pub flushes: u64,
    pub op_normal: u64,
    pub op_expand: u64,
    pub op_accelerate: u64,
    pub op_preemptive_expand: u64,
    pub op_cng: u64,
    pub secure_frames_protocol_version: u32,
}

/// Remote peer inbound H.264 video stats.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct InboundVideoStats {
    pub ssrc: u32,
    pub rtx_ssrc: u32,
    pub codec: &'static str,
    pub payload_type: u8,
    pub width: u32,
    pub height: u32,
    pub bytes_received: u64,
    pub packets_received: u64,
    pub packets_lost: u64,
    pub frames_received: u64,
    pub frames_decoded: u64,
    pub key_frames_decoded: u64,
    pub fps_decoded: u32,
    pub jitter_ms: f32,
    pub nacks: u64,
    pub plis: u64,
    pub firs: u64,
    pub freeze_count: u64,
    pub total_freezes_duration_ms: u64,
    pub decoder_implementation_name: Option<String>,
    pub secure_frames_protocol_version: u32,
}

/// Active Go Live stream connection telemetry (outbound when broadcasting, inbound when watching).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct StreamStats {
    pub stream_key: String,
    pub owner_id: u64,
    pub sending: bool,
    pub connected: bool,
    pub transport: TransportStats,
    pub outbound: OutboundStats,
    pub inbound: HashMap<u64, InboundUserStats>,
}

/// Shared holder updated by `session` (and merged with `VideoHub` active stream snapshots on read).
#[derive(Default)]
pub(crate) struct RtcStatsStore {
    inner: Mutex<RtcStatsSnapshot>,
}

impl RtcStatsStore {
    pub(crate) fn update(&self, f: impl FnOnce(&mut RtcStatsSnapshot)) {
        let mut guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        f(&mut guard);
    }

    pub(crate) fn snapshot(&self, streams: Vec<StreamStats>) -> RtcStatsSnapshot {
        let mut snap = self.inner.lock().unwrap_or_else(|e| e.into_inner()).clone();
        snap.streams = streams;
        snap
    }
}

/// Shared map of active Go Live stream telemetry maintained via `VideoHub`.
#[derive(Default)]
pub(crate) struct StreamStatsMap {
    inner: Arc<Mutex<HashMap<String, StreamStats>>>,
}

impl Clone for StreamStatsMap {
    fn clone(&self) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
        }
    }
}

impl StreamStatsMap {
    pub(crate) fn upsert(&self, stats: StreamStats) {
        self.inner
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(stats.stream_key.clone(), stats);
    }

    pub(crate) fn remove(&self, stream_key: &str) {
        self.inner
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(stream_key);
    }

    pub(crate) fn all(&self) -> Vec<StreamStats> {
        let mut entries: Vec<_> = self
            .inner
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .values()
            .cloned()
            .collect();
        entries.sort_by(|a, b| a.stream_key.cmp(&b.stream_key));
        entries
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn streamer_redaction_clears_local_addresses_without_dropping_metrics() {
        let snap = RtcStatsSnapshot {
            connected: true,
            transport: TransportStats {
                hostname: Some("ams01.discord.media".into()),
                local_address: Some("192.168.1.42:55000".into()),
                ping_ms: Some(24),
                ..TransportStats::default()
            },
            streams: vec![StreamStats {
                stream_key: "guild:1:2:3".into(),
                transport: TransportStats {
                    hostname: Some("fra02.discord.media".into()),
                    local_address: Some("192.168.1.42:55001".into()),
                    ..TransportStats::default()
                },
                ..StreamStats::default()
            }],
            ..RtcStatsSnapshot::default()
        };

        let safe = snap.redacted_for_streamer();
        assert_eq!(
            safe.transport.hostname.as_deref(),
            Some("ams01.discord.media")
        );
        assert_eq!(safe.transport.local_address, None);
        assert_eq!(safe.transport.ping_ms, Some(24));
        assert_eq!(safe.streams[0].transport.local_address, None);
    }
}
