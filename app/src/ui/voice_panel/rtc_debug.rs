//! Out-of-process / secondary RTC Debug window (`PopoutWindowRTCDebug` in modules `596339`, `950040`, `412780`,
//! `233545`, `378885`, and `229659`).
//!
//! Draws the 854x480 secondary window with the 218px `--background-base-lower` sidebar and scrollable
//! `--background-base-lowest` content region, two-column key-value stat grids, `300x100` metric graphs for the 25
//! `RTC_DEBUG` graph keys (`v.iA`), active video preview slot (`320x180`), and the Streamer Mode guard on
//! `localAddress`.

use super::super::{Root, avatar_source, color};
use super::ping_graph::{self, Sample};
use super::{now_ms, short_hostname};
use dc_state::Id;
use gpui::{
    AnyElement, AnyWindowHandle, App, Bounds, Context, FontWeight, Global, IntoElement, Render,
    TitlebarOptions, WeakEntity, Window, WindowBounds, WindowHandle, WindowOptions, div, img,
    prelude::*, px, rgb, size,
};
use std::collections::{BTreeMap, HashMap};
use std::fmt;

pub(crate) const DEFAULT_SECTION: &str = "default:transport:0";
pub(crate) const WINDOW_WIDTH: f32 = 854.;
pub(crate) const WINDOW_HEIGHT: f32 = 480.;
pub(crate) const SIDEBAR_WIDTH: f32 = 218.;
pub(crate) const MAX_GRAPH_POINTS: usize = 600;

/// Metrics tracked as 600-point time-series graphs (`v.iA` in module `412780`).
const GRAPH_KEYS: &[&str] = &[
    "availableOutgoingBitrate",
    "bitrate",
    "bitrateTarget",
    "bytesReceived",
    "bytesSent",
    "encoderQualityPsnr",
    "encoderQualityVmaf",
    "encodeUsage",
    "frameRateDecode",
    "frameRateEncode",
    "frameRateInput",
    "frameRateNetwork",
    "frameRateRender",
    "keyFramesEncoded",
    "keyFramesDecoded",
    "inboundBitrateEstimate",
    "packetsLost",
    "packetsReceived",
    "packetsSent",
    "ping",
    "qpSum",
    "videoEntropy",
    "audioLevel",
    "screenshareCapturedFps",
    "screenshareCapturedFpsUnique",
];

/// Internal or noisy telemetry fields suppressed in the grid (`er` in module `950040`).
const IGNORED_KEYS: &[&str] = &[
    "apmFrameCount",
    "apmProcessTimeMs",
    "audioJitterBuffer",
    "audioJitterBufferSamples",
    "audioJitterDelay",
    "audioJitterDelaySamples",
    "audioJitterTarget",
    "audioJitterTargetSamples",
    "audioPlayoutUnderruns",
    "audioReceiverDelayMs",
    "audioReceiverPacketCount",
    "captureProcessingDelayMs",
    "captureProcessingFrameCount",
    "consecutiveStaticColorFrames",
    "decryptAttempts",
    "decryptDuration",
    "decryptInvalidNonceCount",
    "decryptMissingKeyCount",
    "encryptAttempts",
    "encryptDuration",
    "encryptMaxAttempts",
    "encryptMissingKeyCount",
    "fractionLost",
    "framesCaptured",
    "framesDecodeErrorsFallback",
    "framesDecodeErrorsGeneric",
    "framesDecodeErrorsNoOutput",
    "framesDecodeErrorsOther",
    "framesDecodeErrorsParameter",
    "framesDecodeErrorsTimeout",
    "framesRendered",
    "hqSimulcastStreamEncoded",
    "lqSimulcastStreamEncoded",
    "noiseCancellerFrames",
    "noiseCancellerProcessTime",
    "sendDelayMs",
    "sendPacketCount",
    "sinkWantAsInt",
    "sinkWantLocalAsInt",
    "skipHistoryJson",
    "sumOfSquaredFramesDurations",
    "targetWindowElevated",
    "timestamp",
    "totalDecodeTime",
    "totalPacketSendDelayMs",
    "transportDelayMs",
    "transportPacketCount",
    "type",
    "videoJitterBuffer",
    "videoJitterDelay",
    "videoJitterTarget",
    "voiceActivityDetectorProcessTime",
    "voiceQuality",
];

/// Keys marked with the `14x14` `#f0b232` warning icon (`V` + module `603349` in `950040`).
const WARNING_KEYS: &[&str] = &["firCount", "nackCount"];

/// Exact field display titles (`K` in module `950040`).
const STATS_LABELS: &[(&str, &str)] = &[
    ("accelerateRate", "Accelerate Rate"),
    ("activityFrames", "Activity GPU Frames"),
    ("activityFramesDropped", "Activity GPU Frames Dropped"),
    ("audioDetected", "Audio Detected"),
    ("audioLevel", "Audio Level"),
    ("availableOutgoingBitrate", "Available Outgoing Bitrate"),
    ("averageDecodeTime", "Average Decode Time"),
    ("averageEncodeTime", "Average Encode Time"),
    ("bandwidthLimitedFrameRate", "Bandwidth Limited Frame Rate"),
    ("bandwidthLimitedResolution", "Bandwidth Limited Resolution"),
    ("bitrate", "Bitrate"),
    ("bitrateTarget", "Bitrate (Target)"),
    ("bytesReceived", "Bytes Received"),
    ("bytesSent", "Bytes Sent"),
    ("capturedFramesCount", "Captured Frames per Second"),
    ("capturedFramesDropped", "Captured Frames Dropped"),
    ("capturedFramesMean", "Captured Frames Mean"),
    ("capturedFramesStdev", "Captured Frames Standard Deviation"),
    ("codec", "Codec"),
    ("cpuLimitedResolution", "CPU Limited Resolution"),
    ("currentDelay", "Current Delay"),
    ("currentSampleRate", "Current Sample Rate"),
    ("decoderImplementationName", "Decoder"),
    ("decodingCNG", "Decoding CNG"),
    ("decodingMutedOutput", "Decoding Muted Output"),
    ("decodingNormal", "Decoding Normal"),
    ("decodingPLC", "Decoding PLC"),
    ("decodingPLCCNG", "Decoding PLC CNG"),
    ("decryptFailureCount", "Decrypt Failures"),
    ("decryptionFailures", "Decryption Failures"),
    ("decryptSuccessCount", "Decrypt Successes"),
    ("delayEstimate", "Delay Estimate"),
    ("encoderImplementationName", "Encoder"),
    ("encoderQualityPsnr", "Encoder PSNR"),
    ("encoderQualityVmaf", "Encoder VMAF"),
    ("encodeUsage", "Encode Usage"),
    ("encryptFailureCount", "Encrypt Failures"),
    ("encryptSuccessCount", "Encrypt Successes"),
    ("expandRate", "Expand Rate"),
    ("fecPacketsDiscarded", "FEC Packets Discarded"),
    ("fecPacketsReceived", "FEC Packets Received"),
    ("filter", "Filter"),
    ("firCount", "FIR"),
    ("fractionLost", "Packet Loss"),
    ("frameRateDecode", "Frame Rate (Decode)"),
    ("frameRateEncode", "Frame Rate (Encode)"),
    ("frameRateInput", "Frame Rate (Input)"),
    ("frameRateNetwork", "Frame Rate (Network)"),
    ("frameRateRender", "Frame Rate (Render)"),
    ("framesDecoded", "Frames Decoded"),
    ("framesDecodeErrors", "Decoder Error Count"),
    ("framesDropped", "Frames Dropped"),
    (
        "framesDroppedCongestionWindow",
        "Frames Dropped by Congestion Window",
    ),
    ("framesDroppedEncoder", "Frames Dropped by Encoder"),
    (
        "framesDroppedEncoderQueue",
        "Frames Dropped by Encoder Queue",
    ),
    (
        "framesDroppedRateLimiter",
        "Frames Dropped by Bitrate Limiter",
    ),
    ("framesEncoded", "Frames Encoded"),
    ("framesReceived", "Frames Received"),
    ("framesSent", "Frames Sent"),
    ("freezeCount", "Freeze Count"),
    ("hdrFrames", "HDR Frames"),
    ("hostname", "Hostname"),
    (
        "hybridCaptureMethodSwitches",
        "Hybrid Capture Method Switches",
    ),
    ("hybridDxgiFrames", "Hybrid DXGI Frames"),
    ("hybridExternalFrames", "Hybrid External Frames"),
    ("hybridGdiBitBltFrames", "Hybrid GDI BitBlt Frames"),
    ("hybridGdiFrames", "Hybrid GDI Frames"),
    (
        "hybridGdiPrintWindowFrames",
        "Hybrid GDI PrintWindow Frames",
    ),
    (
        "hybridGraphicsCaptureFrames",
        "Hybrid Graphics Capture Frames",
    ),
    ("hybridVideohookFrames", "Hybrid Videohook Frames"),
    ("inboundBitrateEstimate", "Inbound Bitrate Estimate"),
    ("jitter", "Jitter"),
    ("jitterBuffer", "Jitter Buffer"),
    ("jitterBufferPreferred", "Jitter Buffer (Preferred)"),
    ("keyFrameInterval", "Key Frame Interval"),
    ("keyFramesDecoded", "Key Frames Decoded"),
    ("keyFramesEncoded", "Key Frames Encoded"),
    ("localAddress", "Local Address"),
    ("minPlayoutDelay", "Minimum Playout Delay"),
    ("nackCount", "NACK"),
    ("networkFramesDropped", "Frames Dropped By Network"),
    ("opAccelerate", "Accelerated Frames"),
    ("opCNG", "CNG Frames"),
    ("opExpand", "Expand Frames"),
    ("opMerge", "Merge Frames"),
    ("opNormal", "Normal Frames"),
    ("opPreemptiveExpand", "Preemptive Expand Frames"),
    ("opSilence", "Silent Frames"),
    ("outboundBitrateEstimate", "Outbound Bitrate Estimate"),
    ("outputDeviceUnderruns", "Output Device Underruns"),
    ("pacerDelay", "Pacer Delay"),
    ("packetsLost", "Packets Lost"),
    ("packetsReceived", "Packets Received"),
    ("packetsSent", "Packets Sent"),
    ("passthroughCount", "Passthrough"),
    ("pauseCount", "Pause Count"),
    ("ping", "Ping"),
    ("pipewireFrames", "Pipewire Frames"),
    ("pliCount", "PLI"),
    ("powerEfficientDecoder", "Power Efficient Decoder"),
    ("powerEfficientEncoder", "Power Efficient Encoder"),
    ("preemptiveExpandRate", "Pre-emptive Expand Rate"),
    ("qpSum", "QP Sum"),
    ("qualityDecodeErrors", "Encoder Quality Decode Errors"),
    ("qualityDecoderReboots", "Encoder Quality Decoder Reboots"),
    ("qualityFrameDrops", "Encoder Quality Frame Drops"),
    ("qualityScoreErrors", "Encoder Quality Score Errors"),
    ("qualitySizeMismatches", "Encoder Quality Size Mismatches"),
    ("quartzFrames", "Quartz Frames"),
    (
        "receiverBitrateEstimate",
        "Receiver Bitrate Estimate (REMB)",
    ),
    ("relativePlayoutDelay", "Relative Playout Delay"),
    ("relativeReceptionDelay", "Relative Reception Delay"),
    ("renderDelay", "Render Delay"),
    ("resolution", "Resolution"),
    ("routingFailures", "Routing Failures"),
    ("sampleRateMismatchPercent", "Sample Rate Mismatch"),
    ("screenCaptureKitFrames", "ScreenCaptureKit frames"),
    ("screenshareCapturedFps", "Captured FPS"),
    ("screenshareCapturedFpsUnique", "Unique Captured FPS"),
    ("screenshareFrames", "WebRTC Frames"),
    ("secondaryDecodedRate", "Secondary Decode Rate"),
    ("secureFramesProtocolVersion", "DAVE Protocol"),
    ("sinkWant", "Sink Quality Level (Remote)"),
    ("sinkWantLocal", "Sink Quality Level (Local)"),
    ("speechExpandRate", "Speech Expand Rate"),
    ("ssrc", "SSRC"),
    ("targetDelay", "Target Delay"),
    ("totalFramesDuration", "Frames Duration"),
    ("totalFreezesDuration", "Freezes Duration"),
    ("totalPausesDuration", "Pauses Duration"),
    ("videoEntropy", "Video Entropy"),
    ("videohookBackend", "Videohook Backend"),
    ("videohookFrames", "Videohook Frames"),
    ("voiceQualityMos", "Voice Quality (mean DNSMOS)"),
    ("x11Frames", "X11 Frames"),
];

const VIDEOHOOK_BACKENDS: &[&str] = &[
    "N/A",
    "Direct3D 9",
    "Direct3D 10",
    "Direct3D 11",
    "Direct3D 12",
    "OpenGL",
    "Vulkan",
];

/// Ordered screenshare pairs (`ev` in `950040`).
const SCREENSHARE_PAIRED: &[(&str, &str, &str)] = &[
    ("activityFrames", "activityFramesConsumed", "Consumed"),
    ("hdrFrames", "hdrFramesCapable", "Capable"),
    ("hybridDxgiFrames", "hybridDxgiFramesUnique", "Unique"),
    (
        "hybridExternalFrames",
        "hybridExternalFramesUnique",
        "Unique",
    ),
    (
        "hybridGdiBitBltFrames",
        "hybridGdiBitBltFramesUnique",
        "Unique",
    ),
    (
        "hybridGdiPrintWindowFrames",
        "hybridGdiPrintWindowFramesUnique",
        "Unique",
    ),
    (
        "hybridGraphicsCaptureFrames",
        "hybridGraphicsCaptureFramesUnique",
        "Unique",
    ),
    (
        "hybridVideohookFrames",
        "hybridVideohookFramesUnique",
        "Unique",
    ),
];

/// Display order for primary screenshare fields (`eE` in `950040`).
const SCREENSHARE_ORDER: &[&str] = &[
    "activityFrames",
    "activityFramesDropped",
    "hdrFrames",
    "hybridCaptureMethodSwitches",
    "hybridDxgiFrames",
    "hybridExternalFrames",
    "hybridGdiBitBltFrames",
    "hybridGdiPrintWindowFrames",
    "hybridGraphicsCaptureFrames",
    "hybridVideohookFrames",
    "hybridGdiFrames",
    "quartzFrames",
    "screenCaptureKitFrames",
    "screenshareFrames",
    "videohookFrames",
    "videohookBackend",
    "screenshareCapturedFps",
    "screenshareCapturedFpsUnique",
    "x11Frames",
    "pipewireFrames",
];

/// Priority order for Clips stats (`eF` in `950040`).
const CLIPS_ORDER: &[(&str, usize)] = &[
    ("processRunning", 1),
    ("encoderReady", 2),
    ("encodeWidth", 3),
    ("encodeHeight", 4),
    ("encodeFps", 5),
    ("measuredFps", 6),
    ("encoderImplementationName", 7),
    ("referenceBitrateKbps", 8),
    ("bitrateKbps", 9),
    ("maxBitrateKbps", 10),
    ("submittedFrames", 11),
    ("encodedFrames", 12),
    ("droppedFrames", 13),
    ("recentEncodedFrames", 14),
    ("recentDroppedFrames", 15),
    ("recentDurationMs", 16),
    ("clipDurationMs", 17),
    ("totalSavedKB", 18),
];

/// Priority order for Clips buffer entries (`ef` in `950040`).
const BUFFER_ORDER: &[(&str, usize)] = &[
    ("userId", 1),
    ("dataType", 2),
    ("savedKB", 3),
    ("numFrames", 4),
    ("startTimestampMs", 5),
    ("endTimestampMs", 6),
    ("width", 7),
    ("height", 8),
    ("audioChannels", 9),
    ("sampleRate", 10),
];

const SIMULCAST_OPTIONS: &[(SimulcastQuality, &str)] = &[
    (SimulcastQuality::NoOverride, "None"),
    (SimulcastQuality::Low, "Low Quality Stream"),
    (SimulcastQuality::High, "High Quality Stream"),
];

const RECEIVER_LABEL_BITRATE: &str = "Bitrate:";
const RECEIVER_LABEL_LOSS: &str = "Packet Loss:";
const RECEIVER_LABEL_RTT: &str = "RTT:";
const SIMULCAST_HEADER: &str = "Simulcast Override";
const FPS_COMPUTED_LABEL: &str = "Frames Per Second";
const BUFFER_PREFIX: &str = "Buffer: ";
const VIDEO_SOURCE_PREVIEW: &str = "Stream ";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub(crate) enum ContextKind {
    #[default]
    Default,
    Stream,
}

impl ContextKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Default => "default",
            Self::Stream => "stream",
        }
    }

    fn from_str(text: &str) -> Option<Self> {
        match text {
            "default" => Some(Self::Default),
            "stream" => Some(Self::Stream),
            _ => None,
        }
    }
}

impl fmt::Display for ContextKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub(crate) enum SimulcastQuality {
    #[default]
    NoOverride,
    Low,
    High,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum StatValue {
    Text(String),
    Int(i64),
    Uint(u64),
    Float(f64),
    Bool(bool),
    Codec { id: u32, name: String },
    Resolution { width: u32, height: u32 },
    Series(Vec<Sample>),
}

impl StatValue {
    fn as_f64(&self) -> Option<f64> {
        match self {
            Self::Float(v) => Some(*v),
            Self::Int(v) => Some(*v as f64),
            Self::Uint(v) => Some(*v as f64),
            Self::Series(points) => points.last().map(|point| point.value),
            _ => None,
        }
    }

    fn as_series(&self) -> Option<&[Sample]> {
        match self {
            Self::Series(points) if !points.is_empty() => Some(points.as_slice()),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct RtpStreamDebug {
    pub kind: String,
    pub ssrc: u32,
    pub resolution: Option<(u32, u32)>,
    pub codec: Option<(u32, String)>,
    pub stats: BTreeMap<String, StatValue>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct ReceiverReportDebug {
    pub id: u64,
    pub bitrate: Option<f64>,
    pub fraction_lost: Option<f64>,
    pub rtt: Option<f64>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct TransportDebug {
    pub local_address: Option<String>,
    pub hostname: Option<String>,
    pub receiver_reports: Vec<ReceiverReportDebug>,
    pub stats: BTreeMap<String, StatValue>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct ClipsBufferDebug {
    pub key: String,
    pub stats: BTreeMap<String, StatValue>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct ClipsDebug {
    pub stats: BTreeMap<String, StatValue>,
    pub recent_encoded_frames: Option<u64>,
    pub recent_duration_ms: Option<u64>,
    pub buffers: Vec<ClipsBufferDebug>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct RtcConnectionDebug {
    pub media_engine_connection_id: String,
    pub transport: Option<TransportDebug>,
    pub output_device_underruns: Option<u64>,
    pub outbound: Vec<RtpStreamDebug>,
    pub inbound: BTreeMap<u64, Vec<RtpStreamDebug>>,
    pub camera: Option<BTreeMap<String, StatValue>>,
    pub screenshare: Option<BTreeMap<String, StatValue>>,
    pub clips: Option<ClipsDebug>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct RtcDebugData {
    pub channel_name: Option<String>,
    pub guild_id: Option<Id>,
    pub connected: bool,
    pub media_session_id: Option<String>,
    pub hide_personal_information: bool,
    pub default_stats: Vec<RtcConnectionDebug>,
    pub stream_stats: Vec<RtcConnectionDebug>,
}

/// Summary selectors matching `RTCDebugStore.getInboundStats` / `getOutboundStats` (`412780`).
#[cfg(test)]
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct SummaryStats {
    pub codec: Option<String>,
    pub resolution: Option<(u32, u32)>,
    pub bitrate_estimate: Option<f64>,
    pub fps: Option<f64>,
}

#[derive(Clone, Copy, Debug, Default)]
struct ScreenshareCounts {
    primary: u64,
    unique: u64,
}

/// Fallback call state when [`dc_voice::RtcStatsSnapshot`] has not yet populated transport or peer rows.
pub(crate) struct FallbackCallStats<'a> {
    pub(crate) media_engine_connection_id: &'a str,
    pub(crate) hostname: Option<String>,
    pub(crate) pings: &'a [Sample],
    pub(crate) loss: Option<f32>,
    pub(crate) dave_version: u16,
    pub(crate) camera: bool,
    pub(crate) self_muted: bool,
    pub(crate) connected: bool,
    pub(crate) rtc_users: &'a [u64],
    pub(crate) me: Option<Id>,
}

/// Ported `RTCDebugStore` (`412780`) plus the open secondary window handle.
pub(crate) struct RtcDebugStore {
    pub(crate) section: String,
    pub(crate) default_stats: Vec<RtcConnectionDebug>,
    pub(crate) stream_stats: Vec<RtcConnectionDebug>,
    pub(crate) video_streams: HashMap<(String, u64, u32), String>,
    pub(crate) simulcast_overrides: HashMap<(u64, ContextKind), SimulcastQuality>,
    pub(crate) selected_tabs: HashMap<String, usize>,
    screenshare_prev: HashMap<String, (f64, ScreenshareCounts)>,
    pub(crate) modal_open: bool,
    pub(crate) override_data: Option<RtcDebugData>,
}

impl Default for RtcDebugStore {
    fn default() -> Self {
        Self {
            section: DEFAULT_SECTION.to_owned(),
            default_stats: Vec::new(),
            stream_stats: Vec::new(),
            video_streams: HashMap::new(),
            simulcast_overrides: HashMap::new(),
            selected_tabs: HashMap::new(),
            screenshare_prev: HashMap::new(),
            modal_open: false,
            override_data: None,
        }
    }
}

impl RtcDebugStore {
    /// `RTC_DEBUG_MODAL_OPEN` + `RTC_DEBUG_POPOUT_WINDOW_OPEN` (`233545` `ho`, `412780`).
    pub fn open(&mut self, section: Option<String>) {
        self.section = section.unwrap_or_else(|| DEFAULT_SECTION.to_owned());
        self.modal_open = true;
    }

    /// `RTC_DEBUG_MODAL_CLOSE` (`233545` `VN`, `412780`).
    pub fn close(&mut self) {
        self.modal_open = false;
    }

    /// `RTC_DEBUG_MODAL_SET_SECTION` (`233545` `c4`, `412780`).
    pub fn set_section(&mut self, section: impl Into<String>) {
        self.section = section.into();
    }

    /// `RTC_DEBUG_MODAL_UPDATE_VIDEO_OUTPUT` (`412780`).
    pub fn update_video_output(
        &mut self,
        media_engine_connection_id: &str,
        user_id: u64,
        video_ssrc: u32,
        stream_id: &str,
    ) {
        let key = (media_engine_connection_id.to_owned(), user_id, video_ssrc);
        if stream_id.is_empty() {
            self.video_streams.remove(&key);
        } else {
            self.video_streams.insert(key, stream_id.to_owned());
        }
    }

    /// `RTC_DEBUG_SET_SIMULCAST_OVERRIDE` (`233545` `z0`, `412780`).
    pub fn set_simulcast_override(
        &mut self,
        user_id: u64,
        context: ContextKind,
        quality: SimulcastQuality,
    ) {
        self.simulcast_overrides.insert((user_id, context), quality);
    }

    #[cfg(test)]
    pub fn simulcast_override(&self, user_id: u64, context: ContextKind) -> SimulcastQuality {
        self.simulcast_overrides
            .get(&(user_id, context))
            .copied()
            .unwrap_or(SimulcastQuality::NoOverride)
    }

    /// `RTC_CONNECTION_VIDEO` when `streamId == null` resets the user's override (`412780`).
    pub fn clear_user_simulcast_on_video_end(&mut self, user_id: u64, context: ContextKind) {
        self.simulcast_overrides
            .insert((user_id, context), SimulcastQuality::NoOverride);
    }

    /// `VOICE_CHANNEL_SELECT` when selecting a channel clears accumulated telemetry (`412780`).
    pub fn on_voice_channel_select(&mut self, channel_id: Option<Id>) {
        if channel_id.is_some() {
            self.default_stats.clear();
            self.stream_stats.clear();
            self.simulcast_overrides.clear();
            self.screenshare_prev.clear();
            self.section = DEFAULT_SECTION.to_owned();
        }
    }

    /// `MEDIA_ENGINE_CONNECTION_STATS`: merges incoming snapshot and caps `v.iA` series at 600 points.
    pub fn ingest_stats(
        &mut self,
        context: ContextKind,
        index: usize,
        now: f64,
        mut next: RtcConnectionDebug,
    ) {
        let parsed = parse_section_id(&self.section);
        if let Some((sec_ctx, sec_user, sec_idx)) = parsed
            && sec_ctx == context
            && sec_idx == index
            && let Some(user_id) = sec_user
            && !next.inbound.contains_key(&user_id)
        {
            self.section = DEFAULT_SECTION.to_owned();
        }

        let prev_screenshare = self
            .stats_mut(context)
            .get(index)
            .and_then(|old| old.screenshare.as_ref())
            .map(screenshare_counts);
        if let Some(screen) = next.screenshare.as_mut() {
            let key = format!("{context}:{index}");
            let current = screenshare_counts(screen);
            if let Some((prev_time, old_counts)) = self.screenshare_prev.get(&key).copied()
                && let Some(_) = prev_screenshare
            {
                let dt = (now - prev_time) / 1000.;
                if dt > 0. {
                    let fps = (current.primary.saturating_sub(old_counts.primary)) as f64 / dt;
                    let unique = (current.unique.saturating_sub(old_counts.unique)) as f64 / dt;
                    screen.insert(
                        "screenshareCapturedFps".to_owned(),
                        StatValue::Float(fps.max(0.)),
                    );
                    screen.insert(
                        "screenshareCapturedFpsUnique".to_owned(),
                        StatValue::Float(unique.max(0.)),
                    );
                }
            }
            self.screenshare_prev.insert(key, (now, current));
        }

        let list = self.stats_mut(context);
        if index >= list.len() {
            list.resize_with(index + 1, RtcConnectionDebug::default);
        }
        let previous = std::mem::take(&mut list[index]);
        list[index] = merge_connection_history(previous, next, now);
    }

    pub fn stats(&self, context: ContextKind) -> &[RtcConnectionDebug] {
        match context {
            ContextKind::Default => &self.default_stats,
            ContextKind::Stream => &self.stream_stats,
        }
    }

    fn stats_mut(&mut self, context: ContextKind) -> &mut Vec<RtcConnectionDebug> {
        match context {
            ContextKind::Default => &mut self.default_stats,
            ContextKind::Stream => &mut self.stream_stats,
        }
    }

    #[cfg(test)]
    pub fn get_inbound_stats(&self, user_id: u64, context: ContextKind) -> SummaryStats {
        let video = self
            .stats(context)
            .first()
            .and_then(|conn| conn.inbound.get(&user_id))
            .and_then(|streams| streams.iter().find(|s| s.kind == "video"));
        let Some(video) = video else {
            return SummaryStats::default();
        };
        SummaryStats {
            codec: video.codec.as_ref().map(|(_, name)| name.clone()),
            resolution: video.resolution,
            bitrate_estimate: None,
            fps: video
                .stats
                .get("frameRateRender")
                .and_then(StatValue::as_f64),
        }
    }

    #[cfg(test)]
    pub fn get_outbound_stats(&self, context: ContextKind) -> SummaryStats {
        let first = self.stats(context).first();
        let video = first.and_then(|conn| conn.outbound.iter().find(|s| s.kind == "video"));
        let bitrate_estimate = first
            .and_then(|conn| conn.transport.as_ref())
            .and_then(|t| t.stats.get("availableOutgoingBitrate"))
            .and_then(StatValue::as_f64);
        SummaryStats {
            codec: video.and_then(|v| v.codec.as_ref().map(|(_, name)| name.clone())),
            resolution: video.and_then(|v| v.resolution),
            bitrate_estimate,
            fps: video.and_then(|v| v.stats.get("frameRateEncode").and_then(StatValue::as_f64)),
        }
    }
}

#[derive(Default)]
pub(crate) struct RtcDebugGlobal {
    pub(crate) window: Option<WindowHandle<RtcDebugWindow>>,
    pub(crate) store: RtcDebugStore,
}

impl Global for RtcDebugGlobal {}

/// Secondary window handle for focus/lifecycle helpers.
pub(crate) fn rtc_debug_window(cx: &App) -> Option<AnyWindowHandle> {
    cx.try_global::<RtcDebugGlobal>()
        .and_then(|state| state.window)
        .map(Into::into)
}

/// Replaces or clears synthetic/live override debug snapshots rendered by [`RtcDebugWindow`].
#[cfg(feature = "capture")]
pub(crate) fn set_debug_stats_override(data: Option<RtcDebugData>, cx: &mut App) {
    let global = cx.default_global::<RtcDebugGlobal>();
    global.store.override_data = data;
}

/// Clears accumulated telemetry and overrides when switching voice channels (`VOICE_CHANNEL_SELECT`).
pub(in crate::ui) fn on_voice_channel_select(channel_id: Option<Id>, cx: &mut App) {
    cx.default_global::<RtcDebugGlobal>()
        .store
        .on_voice_channel_select(channel_id);
}

pub(crate) fn is_graph_key(key: &str) -> bool {
    GRAPH_KEYS.contains(&key)
}

pub(crate) fn is_ignored_key(key: &str) -> bool {
    IGNORED_KEYS.contains(&key)
}

pub(crate) fn is_warning_key(key: &str) -> bool {
    WARNING_KEYS.contains(&key)
}

pub(crate) fn stat_label(key: &str) -> &str {
    STATS_LABELS
        .iter()
        .find(|(known, _)| *known == key)
        .map_or(key, |(_, label)| *label)
}

pub(crate) fn audio_level_converter(value: f64) -> f64 {
    if value <= 0. || !value.is_finite() {
        0.
    } else {
        (20. * value.ln()).max(-100.) + 100.
    }
}

/// Formats a byte count using `humanize.filesize` (`796873`): 1024-based units with commas.
pub(crate) fn format_filesize(bytes: f64) -> String {
    const UNITS: &[&str] = &["bytes", "KB", "MB", "GB", "TB", "PB"];
    if !bytes.is_finite() || bytes <= 0. {
        return "0 bytes".to_owned();
    }
    let mut unit = 0usize;
    let mut threshold = 1024.;
    while unit + 1 < UNITS.len() && bytes >= threshold {
        unit += 1;
        threshold *= 1024.;
    }
    let scaled = bytes / 1024f64.powi(unit as i32);
    let decimals = if unit == 0 { 0 } else { 2 };
    let raw = format!("{scaled:.decimals$}");
    let (int_part, frac_part) = raw.split_once('.').unwrap_or((raw.as_str(), ""));
    let with_commas = insert_commas(int_part);
    if decimals == 0 {
        format!("{with_commas} {}", UNITS[unit])
    } else {
        format!("{with_commas}.{frac_part} {}", UNITS[unit])
    }
}

fn insert_commas(digits: &str) -> String {
    let bytes = digits.as_bytes();
    let len = bytes.len();
    if len <= 3 {
        return digits.to_owned();
    }
    let mut out = String::with_capacity(len + len / 3);
    for (idx, &ch) in bytes.iter().enumerate() {
        if idx > 0 && (len - idx).is_multiple_of(3) {
            out.push(',');
        }
        out.push(ch as char);
    }
    out
}

/// Formats one stat entry per module `950040`'s `en` map.
pub(crate) fn format_stat_value(key: &str, value: &StatValue) -> String {
    if let StatValue::Series(points) = value {
        let Some(last) = points.last() else {
            return String::new();
        };
        return format_stat_value(key, &StatValue::Float(last.value));
    }
    match value {
        StatValue::Codec { id, name } => {
            let normalized = if name.is_empty() { "unknown" } else { name };
            let mut chars = normalized.chars();
            let cap = match chars.next() {
                Some(first) => format!("{}{}", first.to_uppercase(), chars.as_str()),
                None => String::new(),
            };
            format!("{cap} ({id})")
        }
        StatValue::Resolution { width, height } => format!("{width}x{height}"),
        StatValue::Bool(flag) => dc_intl::tr(if *flag { "Yes" } else { "No" }).to_owned(),
        StatValue::Text(text) => text.clone(),
        StatValue::Int(_) | StatValue::Uint(_) | StatValue::Float(_) => {
            let num = value.as_f64().unwrap_or(0.);
            match key {
                "availableOutgoingBitrate"
                | "bitrate"
                | "bitrateTarget"
                | "inboundBitrateEstimate"
                | "outboundBitrateEstimate"
                | "receiverBitrateEstimate" => format!("{:.2} Kbps", num / 1000.),
                "bytesReceived" | "bytesSent" => format_filesize(num),
                "audioLevel" => {
                    if num <= 0. {
                        "-\u{221E} dB".to_owned()
                    } else {
                        format!("{:.2} dB", 20. * num.ln())
                    }
                }
                "currentSampleRate" => {
                    let hz = num.round() as i64;
                    if hz % 100 == 0 {
                        format!("{} kHz", hz as f64 / 1000.)
                    } else {
                        format!("{hz} Hz")
                    }
                }
                "encoderQualityPsnr" => format!("{:.2} dB", num.max(0.)),
                "encoderQualityVmaf"
                | "screenshareCapturedFps"
                | "screenshareCapturedFpsUnique"
                | "videoEntropy"
                | "voiceQualityMos" => format!("{num:.2}"),
                "accelerateRate"
                | "encodeUsage"
                | "expandRate"
                | "fractionLost"
                | "preemptiveExpandRate"
                | "sampleRateMismatchPercent"
                | "secondaryDecodedRate"
                | "speechExpandRate" => format!("{num:.0}%"),
                "averageDecodeTime"
                | "averageEncodeTime"
                | "capturedFramesMean"
                | "capturedFramesStdev"
                | "currentDelay"
                | "delayEstimate"
                | "jitter"
                | "jitterBuffer"
                | "jitterBufferPreferred"
                | "keyFrameInterval"
                | "minPlayoutDelay"
                | "pacerDelay"
                | "ping"
                | "renderDelay"
                | "targetDelay"
                | "totalFramesDuration"
                | "totalFreezesDuration"
                | "totalPausesDuration" => match value {
                    StatValue::Int(v) => format!("{v} ms"),
                    StatValue::Uint(v) => format!("{v} ms"),
                    StatValue::Float(v) => format!("{v} ms"),
                    _ => String::new(),
                },
                "secureFramesProtocolVersion" => {
                    let ver = num.round() as i64;
                    if ver > 0 {
                        format!("{} {ver}", dc_intl::tr("Version"))
                    } else {
                        dc_intl::tr("Disabled").to_owned()
                    }
                }
                "videohookBackend" => {
                    let idx = num.round().max(0.) as usize;
                    if idx == 0 {
                        dc_intl::tr("N/A").to_owned()
                    } else if let Some(name) = VIDEOHOOK_BACKENDS.get(idx) {
                        (*name).to_owned()
                    } else {
                        dc_intl::tr("Unknown").to_owned()
                    }
                }
                _ => match value {
                    StatValue::Int(v) => v.to_string(),
                    StatValue::Uint(v) => v.to_string(),
                    StatValue::Float(v) => {
                        if v.fract() == 0. && v.abs() < 1e15 {
                            format!("{}", *v as i64)
                        } else {
                            v.to_string()
                        }
                    }
                    _ => String::new(),
                },
            }
        }
        StatValue::Series(_) => String::new(),
    }
}

/// RTP field order (`em = { ssrc: 1, codec: 2 }`, non-graph alphabetical before `v.iA` alphabetical).
pub(crate) fn sort_rtp_keys<'a>(keys: impl IntoIterator<Item = &'a str>) -> Vec<&'a str> {
    let mut out: Vec<&'a str> = keys.into_iter().collect();
    out.sort_by(|a, b| {
        let rank = |k: &str| match k {
            "ssrc" => Some(1u8),
            "codec" => Some(2u8),
            _ => None,
        };
        match (rank(a), rank(b)) {
            (Some(ra), Some(rb)) => ra.cmp(&rb),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => {
                let ga = is_graph_key(a);
                let gb = is_graph_key(b);
                ga.cmp(&gb).then_with(|| a.cmp(b))
            }
        }
    });
    out
}

/// Visible transport keys honoring the `er` ignore list and Streamer Mode `hidePersonalInformation`.
pub(crate) fn visible_transport_keys(
    transport: &TransportDebug,
    include_underruns: bool,
    hide_personal_information: bool,
) -> Vec<(&str, StatValue)> {
    let mut entries = Vec::new();
    if let Some(addr) = &transport.local_address
        && !hide_personal_information
    {
        entries.push(("localAddress", StatValue::Text(addr.clone())));
    }
    if let Some(host) = &transport.hostname {
        entries.push(("hostname", StatValue::Text(host.clone())));
    }
    for (k, v) in &transport.stats {
        if is_ignored_key(k) || k == "receiverReports" {
            continue;
        }
        if k == "localAddress" && hide_personal_information {
            continue;
        }
        entries.push((k.as_str(), v.clone()));
    }
    if include_underruns {
        // Callers pass underruns separately when present so its relative placement matches `950040`.
    }
    entries
}

pub(crate) fn section_id(context: ContextKind, target: &str, index: usize) -> String {
    format!("{context}:{target}:{index}")
}

fn parse_section_id(id: &str) -> Option<(ContextKind, Option<u64>, usize)> {
    let mut parts = id.split(':');
    let context = ContextKind::from_str(parts.next()?)?;
    let target = parts.next()?;
    let index = parts.next()?.parse::<usize>().ok()?;
    let user_id = target.parse::<u64>().ok();
    Some((context, user_id, index))
}

fn screenshare_counts(map: &BTreeMap<String, StatValue>) -> ScreenshareCounts {
    let get = |k: &str| map.get(k).and_then(StatValue::as_f64).unwrap_or(0.) as u64;
    ScreenshareCounts {
        primary: get("videohookFrames")
            + get("hybridDxgiFrames")
            + get("hybridGdiFrames")
            + get("hybridVideohookFrames")
            + get("hybridGraphicsCaptureFrames")
            + get("quartzFrames")
            + get("screenCaptureKitFrames")
            + get("activityFrames"),
        unique: get("hybridDxgiFramesUnique")
            + get("hybridGdiBitBltFramesUnique")
            + get("hybridGdiPrintWindowFramesUnique")
            + get("hybridVideohookFramesUnique")
            + get("hybridGraphicsCaptureFramesUnique")
            + get("hybridExternalFramesUnique"),
    }
}

fn merge_stat_map(
    prev: Option<&BTreeMap<String, StatValue>>,
    next: BTreeMap<String, StatValue>,
    now: f64,
) -> BTreeMap<String, StatValue> {
    let mut merged = BTreeMap::new();
    for (k, v) in next {
        if is_graph_key(&k)
            && let Some(num) = v.as_f64()
        {
            let mut series = match prev.and_then(|p| p.get(&k)) {
                Some(StatValue::Series(old)) => old.clone(),
                _ => Vec::new(),
            };
            if matches!(v, StatValue::Series(_)) {
                if let StatValue::Series(incoming) = v {
                    series = incoming;
                }
            } else {
                series.push(Sample {
                    time: now,
                    value: num,
                });
            }
            while series.len() > MAX_GRAPH_POINTS {
                series.remove(0);
            }
            merged.insert(k, StatValue::Series(series));
        } else {
            merged.insert(k, v);
        }
    }
    merged
}

fn merge_stream_list(
    prev: &[RtpStreamDebug],
    next: Vec<RtpStreamDebug>,
    now: f64,
) -> Vec<RtpStreamDebug> {
    next.into_iter()
        .enumerate()
        .map(|(i, mut stream)| {
            let old = prev.get(i).map(|s| &s.stats);
            stream.stats = merge_stat_map(old, stream.stats, now);
            stream
        })
        .collect()
}

fn merge_connection_history(
    prev: RtcConnectionDebug,
    mut next: RtcConnectionDebug,
    now: f64,
) -> RtcConnectionDebug {
    if let Some(mut next_t) = next.transport.take() {
        let old_stats = prev.transport.as_ref().map(|t| &t.stats);
        next_t.stats = merge_stat_map(old_stats, next_t.stats, now);
        next.transport = Some(next_t);
    }
    next.outbound = merge_stream_list(&prev.outbound, next.outbound, now);
    let mut merged_inbound = BTreeMap::new();
    for (uid, streams) in next.inbound {
        let old = prev.inbound.get(&uid).map(Vec::as_slice).unwrap_or(&[]);
        merged_inbound.insert(uid, merge_stream_list(old, streams, now));
    }
    next.inbound = merged_inbound;
    if let Some(cam) = next.camera.take() {
        next.camera = Some(merge_stat_map(prev.camera.as_ref(), cam, now));
    }
    if let Some(screen) = next.screenshare.take() {
        next.screenshare = Some(merge_stat_map(prev.screenshare.as_ref(), screen, now));
    }
    next
}

#[derive(Clone)]
enum SidebarEntry {
    ChannelName(String),
    ConnectionState(bool),
    Divider,
    Header(String),
    Item {
        section: String,
        label: String,
        avatar_url: Option<String>,
    },
}

fn build_sidebar_entries(root: &Root, data: &RtcDebugData) -> Vec<SidebarEntry> {
    let mut items = Vec::new();
    if let Some(channel_name) = &data.channel_name {
        items.push(SidebarEntry::ChannelName(channel_name.clone()));
    }
    items.push(SidebarEntry::ConnectionState(data.connected));

    let push_context =
        |out: &mut Vec<SidebarEntry>, ctx: ContextKind, list: &[RtcConnectionDebug]| {
            for (idx, conn) in list.iter().enumerate() {
                out.push(SidebarEntry::Header(dc_intl::format(
                    "PK5fOC",
                    &[("context", ctx.as_str().into())],
                )));
                if conn.transport.is_some() {
                    out.push(SidebarEntry::Item {
                        section: section_id(ctx, "transport", idx),
                        label: dc_intl::t!("wU9IN2").to_owned(),
                        avatar_url: None,
                    });
                }
                if !conn.outbound.is_empty() {
                    out.push(SidebarEntry::Item {
                        section: section_id(ctx, "outbound", idx),
                        label: dc_intl::t!("3u0gII").to_owned(),
                        avatar_url: None,
                    });
                }
                if conn.camera.is_some() {
                    out.push(SidebarEntry::Item {
                        section: section_id(ctx, "camera", idx),
                        label: dc_intl::t!("2AGBWH").to_owned(),
                        avatar_url: None,
                    });
                }
                if !conn.inbound.is_empty() {
                    let mut peers = Vec::new();
                    for (&uid, streams) in &conn.inbound {
                        if streams.is_empty() {
                            continue;
                        }
                        let (label, avatar_url) = display_user(root, data.guild_id, uid);
                        peers.push(SidebarEntry::Item {
                            section: section_id(ctx, &uid.to_string(), idx),
                            label,
                            avatar_url,
                        });
                    }
                    if !peers.is_empty() {
                        out.push(SidebarEntry::Header(dc_intl::t!("SJmZaq").to_owned()));
                        out.extend(peers);
                    }
                }
            }
        };

    push_context(&mut items, ContextKind::Default, &data.default_stats);
    let stream_start = items.len();
    push_context(&mut items, ContextKind::Stream, &data.stream_stats);
    if items.len() > stream_start {
        items.insert(stream_start, SidebarEntry::Divider);
    }

    let has_screenshare = data
        .default_stats
        .iter()
        .chain(&data.stream_stats)
        .any(|c| c.screenshare.is_some());
    if has_screenshare {
        items.push(SidebarEntry::Divider);
        items.push(SidebarEntry::Item {
            section: "screenshare".to_owned(),
            label: dc_intl::t!("gWbr/U").to_owned(),
            avatar_url: None,
        });
    }

    let has_clips = data.stream_stats.iter().any(|c| c.clips.is_some());
    if has_clips {
        items.push(SidebarEntry::Divider);
        items.push(SidebarEntry::Item {
            section: "clips".to_owned(),
            label: dc_intl::t!("MKrFKE").to_owned(),
            avatar_url: None,
        });
    }

    items
}

fn display_user(root: &Root, guild_id: Option<Id>, user_id: Id) -> (String, Option<String>) {
    let nick = guild_id.and_then(|gid| {
        root.stores
            .members
            .get(gid, user_id)
            .and_then(|member| member.nick.clone())
    });
    if let Some(user) = root.stores.users.get(&user_id) {
        let name = nick
            .or_else(|| user.global_name.clone())
            .unwrap_or_else(|| user.username.clone());
        let (_, url) = avatar_source(user);
        return (name, Some(url));
    }
    (nick.unwrap_or_else(|| user_id.to_string()), None)
}

fn transport_debug_from_stats(
    t: &dc_voice::rtc_stats::TransportStats,
    pings: &[Sample],
    fallback_hostname: Option<String>,
    fallback_loss: Option<f32>,
    fallback_dave: u16,
    connected: bool,
) -> (TransportDebug, Option<u64>) {
    let hostname = t
        .hostname
        .as_deref()
        .map(|h| short_hostname(h).to_owned())
        .or(fallback_hostname);
    let receiver_reports = t
        .receiver_reports
        .iter()
        .map(|r| {
            let scaled_loss = if r.fraction_lost <= 1.0 {
                f64::from(r.fraction_lost) * 256.
            } else {
                f64::from(r.fraction_lost)
            };
            ReceiverReportDebug {
                id: r.user_id,
                bitrate: Some(f64::from(r.bitrate_bps)),
                fraction_lost: Some(scaled_loss),
                rtt: r.rtt_ms.map(|ms| ms as f64),
            }
        })
        .collect();
    let mut stats = BTreeMap::new();
    if !pings.is_empty() {
        stats.insert("ping".to_owned(), StatValue::Series(pings.to_vec()));
    } else if let Some(ms) = t.ping_ms {
        stats.insert("ping".to_owned(), StatValue::Float(ms as f64));
    }
    let has_transport_counters = connected
        || t.available_outgoing_bitrate > 0
        || t.outbound_bitrate_estimate > 0
        || t.inbound_bitrate_estimate > 0
        || t.outbound_bytes > 0
        || t.inbound_bytes > 0
        || t.outbound_packets > 0
        || t.inbound_packets > 0;
    if has_transport_counters {
        stats.insert(
            "availableOutgoingBitrate".to_owned(),
            StatValue::Uint(u64::from(t.available_outgoing_bitrate)),
        );
        stats.insert(
            "outboundBitrateEstimate".to_owned(),
            StatValue::Uint(u64::from(t.outbound_bitrate_estimate)),
        );
        stats.insert(
            "inboundBitrateEstimate".to_owned(),
            StatValue::Uint(u64::from(t.inbound_bitrate_estimate)),
        );
        stats.insert("bytesSent".to_owned(), StatValue::Uint(t.outbound_bytes));
        stats.insert("bytesReceived".to_owned(), StatValue::Uint(t.inbound_bytes));
        stats.insert(
            "packetsSent".to_owned(),
            StatValue::Uint(t.outbound_packets),
        );
        stats.insert(
            "packetsReceived".to_owned(),
            StatValue::Uint(t.inbound_packets),
        );
        stats.insert(
            "delayEstimate".to_owned(),
            StatValue::Uint(u64::from(t.delay_estimate_ms)),
        );
        stats.insert(
            "pacerDelay".to_owned(),
            StatValue::Uint(u64::from(t.pacer_delay_ms)),
        );
        stats.insert(
            "decryptionFailures".to_owned(),
            StatValue::Uint(t.decryption_failures),
        );
    }
    if let Some(loss) = fallback_loss {
        stats.insert("fractionLost".to_owned(), StatValue::Float(f64::from(loss)));
    }
    let dave = if t.secure_frames_protocol_version > 0 {
        u64::from(t.secure_frames_protocol_version)
    } else if connected {
        u64::from(fallback_dave)
    } else {
        0
    };
    stats.insert(
        "secureFramesProtocolVersion".to_owned(),
        StatValue::Uint(dave),
    );
    let underruns =
        (t.output_device_underruns > 0 || connected).then_some(t.output_device_underruns);
    (
        TransportDebug {
            local_address: t.local_address.clone(),
            hostname,
            receiver_reports,
            stats,
        },
        underruns,
    )
}

fn outbound_streams_from_stats(
    out: &dc_voice::rtc_stats::OutboundStats,
    fallback_camera: bool,
    fallback_self_muted: bool,
    connected: bool,
    include_synthetic_fallback: bool,
) -> Vec<RtpStreamDebug> {
    let mut streams = Vec::new();
    if let Some(a) = &out.audio {
        let mut stats = BTreeMap::new();
        stats.insert("ssrc".to_owned(), StatValue::Uint(u64::from(a.ssrc)));
        stats.insert(
            "codec".to_owned(),
            StatValue::Codec {
                id: u32::from(a.payload_type),
                name: a.codec.to_owned(),
            },
        );
        stats.insert("bitrate".to_owned(), StatValue::Uint(u64::from(a.bitrate)));
        stats.insert("bytesSent".to_owned(), StatValue::Uint(a.bytes_sent));
        stats.insert("packetsSent".to_owned(), StatValue::Uint(a.packets_sent));
        stats.insert("packetsLost".to_owned(), StatValue::Uint(a.packets_lost));
        stats.insert(
            "fractionLost".to_owned(),
            StatValue::Float(f64::from(a.fraction_lost) * 100.),
        );
        stats.insert(
            "framesCaptured".to_owned(),
            StatValue::Uint(a.frames_captured),
        );
        stats.insert(
            "audioLevel".to_owned(),
            StatValue::Float(audio_level_converter(f64::from(a.audio_level))),
        );
        if let Some(detected) = a.audio_detected {
            stats.insert("audioDetected".to_owned(), StatValue::Bool(detected));
        }
        stats.insert(
            "encryptSuccessCount".to_owned(),
            StatValue::Uint(a.encrypt_success_count),
        );
        stats.insert(
            "encryptFailureCount".to_owned(),
            StatValue::Uint(a.encrypt_failure_count),
        );
        stats.insert(
            "secureFramesProtocolVersion".to_owned(),
            StatValue::Uint(u64::from(a.secure_frames_protocol_version)),
        );
        streams.push(RtpStreamDebug {
            kind: "audio".to_owned(),
            ssrc: a.ssrc,
            resolution: None,
            codec: Some((u32::from(a.payload_type), a.codec.to_owned())),
            stats,
        });
    } else if include_synthetic_fallback {
        let mut outbound_audio = BTreeMap::new();
        outbound_audio.insert("ssrc".to_owned(), StatValue::Uint(1000));
        outbound_audio.insert(
            "codec".to_owned(),
            StatValue::Codec {
                id: 111,
                name: "opus".to_owned(),
            },
        );
        outbound_audio.insert("bitrateTarget".to_owned(), StatValue::Float(64_000.));
        outbound_audio.insert(
            "audioDetected".to_owned(),
            StatValue::Bool(connected && !fallback_self_muted),
        );
        streams.push(RtpStreamDebug {
            kind: "audio".to_owned(),
            ssrc: 1000,
            resolution: None,
            codec: Some((111, "opus".to_owned())),
            stats: outbound_audio,
        });
    }

    if let Some(v) = &out.video {
        let mut stats = BTreeMap::new();
        stats.insert("ssrc".to_owned(), StatValue::Uint(u64::from(v.ssrc)));
        stats.insert(
            "codec".to_owned(),
            StatValue::Codec {
                id: u32::from(v.payload_type),
                name: v.codec.to_owned(),
            },
        );
        stats.insert(
            "bitrateTarget".to_owned(),
            StatValue::Uint(u64::from(v.bitrate_target)),
        );
        stats.insert(
            "bitrate".to_owned(),
            StatValue::Uint(u64::from(v.bitrate_actual)),
        );
        stats.insert("bytesSent".to_owned(), StatValue::Uint(v.bytes_sent));
        stats.insert("packetsSent".to_owned(), StatValue::Uint(v.packets_sent));
        stats.insert(
            "frameRateEncode".to_owned(),
            StatValue::Uint(u64::from(v.fps_encoded)),
        );
        stats.insert(
            "framesEncoded".to_owned(),
            StatValue::Uint(v.frames_encoded),
        );
        stats.insert(
            "framesDropped".to_owned(),
            StatValue::Uint(v.frames_dropped),
        );
        stats.insert(
            "keyFramesEncoded".to_owned(),
            StatValue::Uint(v.key_frames_encoded),
        );
        stats.insert("qpSum".to_owned(), StatValue::Uint(v.qp_sum));
        stats.insert("nackCount".to_owned(), StatValue::Uint(v.nacks));
        stats.insert("pliCount".to_owned(), StatValue::Uint(v.plis));
        stats.insert("firCount".to_owned(), StatValue::Uint(v.firs));
        if let Some(name) = &v.encoder_implementation_name {
            stats.insert(
                "encoderImplementationName".to_owned(),
                StatValue::Text(name.clone()),
            );
        }
        stats.insert(
            "averageEncodeTime".to_owned(),
            StatValue::Float(f64::from(v.average_encode_time_ms)),
        );
        stats.insert(
            "encryptSuccessCount".to_owned(),
            StatValue::Uint(v.encrypt_success_count),
        );
        stats.insert(
            "encryptFailureCount".to_owned(),
            StatValue::Uint(v.encrypt_failure_count),
        );
        stats.insert(
            "secureFramesProtocolVersion".to_owned(),
            StatValue::Uint(u64::from(v.secure_frames_protocol_version)),
        );
        let resolution = (v.width > 0 && v.height > 0).then_some((v.width, v.height));
        if let Some((width, height)) = resolution {
            stats.insert(
                "resolution".to_owned(),
                StatValue::Resolution { width, height },
            );
        }
        streams.push(RtpStreamDebug {
            kind: "video".to_owned(),
            ssrc: v.ssrc,
            resolution,
            codec: Some((u32::from(v.payload_type), v.codec.to_owned())),
            stats,
        });
    } else if include_synthetic_fallback && fallback_camera {
        let mut cam_stats = BTreeMap::new();
        cam_stats.insert("ssrc".to_owned(), StatValue::Uint(2000));
        cam_stats.insert(
            "codec".to_owned(),
            StatValue::Codec {
                id: 96,
                name: "vp8".to_owned(),
            },
        );
        streams.push(RtpStreamDebug {
            kind: "video".to_owned(),
            ssrc: 2000,
            resolution: Some((1280, 720)),
            codec: Some((96, "vp8".to_owned())),
            stats: cam_stats,
        });
    }
    streams
}

fn inbound_map_from_stats(
    inbound: &HashMap<u64, dc_voice::rtc_stats::InboundUserStats>,
    fallback_rtc_users: &[u64],
    me: Option<Id>,
) -> BTreeMap<u64, Vec<RtpStreamDebug>> {
    let mut out = BTreeMap::new();
    for (&uid, peer) in inbound {
        let mut list = Vec::new();
        if let Some(a) = &peer.audio {
            let mut stats = BTreeMap::new();
            stats.insert("ssrc".to_owned(), StatValue::Uint(u64::from(a.ssrc)));
            stats.insert(
                "codec".to_owned(),
                StatValue::Codec {
                    id: u32::from(a.payload_type),
                    name: a.codec.to_owned(),
                },
            );
            stats.insert(
                "bytesReceived".to_owned(),
                StatValue::Uint(a.bytes_received),
            );
            stats.insert(
                "packetsReceived".to_owned(),
                StatValue::Uint(a.packets_received),
            );
            stats.insert("packetsLost".to_owned(), StatValue::Uint(a.packets_lost));
            stats.insert(
                "fecPacketsReceived".to_owned(),
                StatValue::Uint(a.fec_recovered),
            );
            stats.insert(
                "jitter".to_owned(),
                StatValue::Float(f64::from(a.jitter_ms)),
            );
            stats.insert(
                "jitterBuffer".to_owned(),
                StatValue::Uint(u64::from(a.jitter_buffer_ms)),
            );
            stats.insert(
                "jitterBufferPreferred".to_owned(),
                StatValue::Int(i64::from(a.jitter_buffer_preferred_ms)),
            );
            stats.insert(
                "audioLevel".to_owned(),
                StatValue::Float(audio_level_converter(f64::from(a.audio_level))),
            );
            stats.insert(
                "decodingMutedOutput".to_owned(),
                StatValue::Bool(a.decoding_muted_output),
            );
            stats.insert("opNormal".to_owned(), StatValue::Uint(a.op_normal));
            stats.insert("opExpand".to_owned(), StatValue::Uint(a.op_expand));
            stats.insert("opAccelerate".to_owned(), StatValue::Uint(a.op_accelerate));
            stats.insert(
                "opPreemptiveExpand".to_owned(),
                StatValue::Uint(a.op_preemptive_expand),
            );
            stats.insert("opCNG".to_owned(), StatValue::Uint(a.op_cng));
            stats.insert(
                "secureFramesProtocolVersion".to_owned(),
                StatValue::Uint(u64::from(a.secure_frames_protocol_version)),
            );
            list.push(RtpStreamDebug {
                kind: "audio".to_owned(),
                ssrc: a.ssrc,
                resolution: None,
                codec: Some((u32::from(a.payload_type), a.codec.to_owned())),
                stats,
            });
        }
        if let Some(v) = &peer.video {
            let mut stats = BTreeMap::new();
            stats.insert("ssrc".to_owned(), StatValue::Uint(u64::from(v.ssrc)));
            stats.insert(
                "codec".to_owned(),
                StatValue::Codec {
                    id: u32::from(v.payload_type),
                    name: v.codec.to_owned(),
                },
            );
            stats.insert(
                "bytesReceived".to_owned(),
                StatValue::Uint(v.bytes_received),
            );
            stats.insert(
                "packetsReceived".to_owned(),
                StatValue::Uint(v.packets_received),
            );
            stats.insert("packetsLost".to_owned(), StatValue::Uint(v.packets_lost));
            stats.insert(
                "framesReceived".to_owned(),
                StatValue::Uint(v.frames_received),
            );
            stats.insert(
                "framesDecoded".to_owned(),
                StatValue::Uint(v.frames_decoded),
            );
            stats.insert(
                "keyFramesDecoded".to_owned(),
                StatValue::Uint(v.key_frames_decoded),
            );
            stats.insert(
                "frameRateDecode".to_owned(),
                StatValue::Uint(u64::from(v.fps_decoded)),
            );
            stats.insert(
                "jitter".to_owned(),
                StatValue::Float(f64::from(v.jitter_ms)),
            );
            stats.insert("nackCount".to_owned(), StatValue::Uint(v.nacks));
            stats.insert("pliCount".to_owned(), StatValue::Uint(v.plis));
            stats.insert("firCount".to_owned(), StatValue::Uint(v.firs));
            stats.insert("freezeCount".to_owned(), StatValue::Uint(v.freeze_count));
            stats.insert(
                "totalFreezesDuration".to_owned(),
                StatValue::Uint(v.total_freezes_duration_ms),
            );
            if let Some(name) = &v.decoder_implementation_name {
                stats.insert(
                    "decoderImplementationName".to_owned(),
                    StatValue::Text(name.clone()),
                );
            }
            stats.insert(
                "secureFramesProtocolVersion".to_owned(),
                StatValue::Uint(u64::from(v.secure_frames_protocol_version)),
            );
            let resolution = (v.width > 0 && v.height > 0).then_some((v.width, v.height));
            if let Some((width, height)) = resolution {
                stats.insert(
                    "resolution".to_owned(),
                    StatValue::Resolution { width, height },
                );
            }
            list.push(RtpStreamDebug {
                kind: "video".to_owned(),
                ssrc: v.ssrc,
                resolution,
                codec: Some((u32::from(v.payload_type), v.codec.to_owned())),
                stats,
            });
        }
        if !list.is_empty() {
            out.insert(uid, list);
        }
    }
    for (idx, &uid) in fallback_rtc_users.iter().enumerate() {
        if Some(uid) == me || out.contains_key(&uid) {
            continue;
        }
        let mut audio_stats = BTreeMap::new();
        audio_stats.insert("ssrc".to_owned(), StatValue::Uint(3000 + idx as u64));
        audio_stats.insert(
            "codec".to_owned(),
            StatValue::Codec {
                id: 111,
                name: "opus".to_owned(),
            },
        );
        out.insert(
            uid,
            vec![RtpStreamDebug {
                kind: "audio".to_owned(),
                ssrc: 3000 + idx as u32,
                resolution: None,
                codec: Some((111, "opus".to_owned())),
                stats: audio_stats,
            }],
        );
    }
    out
}

fn camera_map_from_stats(
    cam: Option<&dc_voice::rtc_stats::CameraStats>,
) -> Option<BTreeMap<String, StatValue>> {
    let cam = cam?;
    let mut stats = BTreeMap::new();
    if cam.width > 0 && cam.height > 0 {
        stats.insert(
            "resolution".to_owned(),
            StatValue::Resolution {
                width: cam.width,
                height: cam.height,
            },
        );
    }
    stats.insert(
        "capturedFramesCount".to_owned(),
        StatValue::Uint(cam.frames_captured),
    );
    stats.insert(
        "capturedFramesDropped".to_owned(),
        StatValue::Uint(cam.frames_dropped),
    );
    if cam.fps > 0 {
        stats.insert(
            "frameRateInput".to_owned(),
            StatValue::Uint(u64::from(cam.fps)),
        );
    }
    if let Some(name) = cam.device_name.as_deref().filter(|s| !s.is_empty()) {
        stats.insert("filter".to_owned(), StatValue::Text(name.to_owned()));
    }
    Some(stats)
}

/// Converts a live [`dc_voice::RtcStatsSnapshot`] into default + stream [`RtcConnectionDebug`] structs.
pub(crate) fn connection_debug_from_rtc_stats(
    snapshot: &dc_voice::RtcStatsSnapshot,
    fallback: &FallbackCallStats<'_>,
    hide_personal_information: bool,
) -> (RtcConnectionDebug, Vec<RtcConnectionDebug>) {
    let safe;
    let snap = if hide_personal_information {
        safe = snapshot.redacted_for_streamer();
        &safe
    } else {
        snapshot
    };
    let (transport, output_device_underruns) = transport_debug_from_stats(
        &snap.transport,
        fallback.pings,
        fallback.hostname.clone(),
        fallback.loss,
        fallback.dave_version,
        fallback.connected || snap.connected,
    );
    let default_conn = RtcConnectionDebug {
        media_engine_connection_id: fallback.media_engine_connection_id.to_owned(),
        transport: Some(transport),
        output_device_underruns,
        outbound: outbound_streams_from_stats(
            &snap.outbound,
            fallback.camera,
            fallback.self_muted,
            fallback.connected || snap.connected,
            true,
        ),
        inbound: inbound_map_from_stats(&snap.inbound, fallback.rtc_users, fallback.me),
        camera: camera_map_from_stats(snap.camera.as_ref()),
        screenshare: None,
        clips: None,
    };
    let stream_conns = snap
        .streams
        .iter()
        .map(|s| {
            let (t, underruns) = transport_debug_from_stats(
                &s.transport,
                &[],
                None,
                None,
                fallback.dave_version,
                s.connected,
            );
            RtcConnectionDebug {
                media_engine_connection_id: format!("stream:{}", s.stream_key),
                transport: Some(t),
                output_device_underruns: underruns,
                outbound: outbound_streams_from_stats(&s.outbound, false, true, s.connected, false),
                inbound: inbound_map_from_stats(&s.inbound, &[], fallback.me),
                camera: None,
                screenshare: None,
                clips: None,
            }
        })
        .collect();
    (default_conn, stream_conns)
}

fn sync_video_stream_tracking(
    store: &mut RtcDebugStore,
    context: ContextKind,
    index: usize,
    next: &RtcConnectionDebug,
) {
    let prev_video_ssrcs: Vec<(u64, u32)> = store
        .stats(context)
        .get(index)
        .map(|conn| {
            conn.inbound
                .iter()
                .flat_map(|(&uid, streams)| {
                    streams
                        .iter()
                        .filter(|s| s.kind == "video")
                        .map(move |s| (uid, s.ssrc))
                })
                .collect()
        })
        .unwrap_or_default();
    let conn_id = &next.media_engine_connection_id;
    for (uid, ssrc) in prev_video_ssrcs {
        let still_present = next
            .inbound
            .get(&uid)
            .is_some_and(|list| list.iter().any(|s| s.kind == "video" && s.ssrc == ssrc));
        if !still_present {
            store.update_video_output(conn_id, uid, ssrc, "");
            store.clear_user_simulcast_on_video_end(uid, context);
        }
    }
    for (&uid, streams) in &next.inbound {
        for s in streams.iter().filter(|s| s.kind == "video") {
            let stream_id = format!("{}:{}", uid, s.ssrc);
            store.update_video_output(conn_id, uid, s.ssrc, &stream_id);
        }
    }
}

/// Samples live `dc_voice::Controls::rtc_stats()` into [`RtcDebugStore`] so time-series graphs accumulate.
pub(in crate::ui) fn ingest_call_rtc_stats(root: &Root, cx: &mut App) {
    let now = now_ms();
    let Some(call) = root.call.as_ref() else {
        return;
    };
    let snap = call.controls.rtc_stats();
    let me = root.stores.current_user.as_ref().map(|u| u.id);
    let fallback = FallbackCallStats {
        media_engine_connection_id: "default-0",
        hostname: call_hostname(root, call),
        pings: &call.stats.pings,
        loss: call.stats.loss,
        dave_version: call.dave_version,
        camera: call.camera,
        self_muted: root.self_mute,
        connected: call.connected,
        rtc_users: call.rtc_users(),
        me,
    };
    let (default_conn, stream_conns) = connection_debug_from_rtc_stats(&snap, &fallback, false);
    let store = &mut cx.default_global::<RtcDebugGlobal>().store;
    sync_video_stream_tracking(store, ContextKind::Default, 0, &default_conn);
    store.ingest_stats(ContextKind::Default, 0, now, default_conn);
    for (idx, stream_conn) in stream_conns.iter().enumerate() {
        sync_video_stream_tracking(store, ContextKind::Stream, idx, stream_conn);
    }
    for (idx, stream_conn) in stream_conns.into_iter().enumerate() {
        store.ingest_stats(ContextKind::Stream, idx, now, stream_conn);
    }
    let expected = snap.streams.len();
    if store.stream_stats.len() > expected {
        store.stream_stats.truncate(expected);
    }
}

/// Clears live ingested [`RtcDebugStore`] series when leaving a call.
pub(in crate::ui) fn clear_live_rtc_stats(cx: &mut App) {
    let global = cx.default_global::<RtcDebugGlobal>();
    global.store.default_stats.clear();
    global.store.stream_stats.clear();
    global.store.video_streams.clear();
    global.store.screenshare_prev.clear();
}

/// Builds a live snapshot from [`dc_voice::Controls::rtc_stats`] / `Root::call` when no `override_data` is installed.
pub(crate) fn build_live_data(root: &Root, cx: &App) -> RtcDebugData {
    let hide_personal = root.streamer_hides_personal();
    if let Some(store) = cx
        .try_global::<RtcDebugGlobal>()
        .map(|global| &global.store)
    {
        if let Some(mut data) = store.override_data.clone() {
            data.hide_personal_information |= hide_personal;
            return data;
        }
        let default_stats = store.stats(ContextKind::Default);
        let stream_stats = store.stats(ContextKind::Stream);
        if !default_stats.is_empty() || !stream_stats.is_empty() {
            let (channel_name, guild_id, connected) = call_metadata(root);
            let media_session_id = root
                .call
                .as_ref()
                .and_then(|c| c.controls.rtc_stats().transport.media_session_id);
            return RtcDebugData {
                channel_name,
                guild_id,
                connected,
                media_session_id,
                hide_personal_information: hide_personal,
                default_stats: default_stats.to_vec(),
                stream_stats: stream_stats.to_vec(),
            };
        }
    }

    let Some(call) = root.call.as_ref() else {
        return RtcDebugData {
            hide_personal_information: hide_personal,
            ..Default::default()
        };
    };

    let snap = call.controls.rtc_stats();
    let (channel_name, guild_id, connected) = call_metadata(root);
    let me = root.stores.current_user.as_ref().map(|u| u.id);
    let fallback = FallbackCallStats {
        media_engine_connection_id: "default-0",
        hostname: call_hostname(root, call),
        pings: &call.stats.pings,
        loss: call.stats.loss,
        dave_version: call.dave_version,
        camera: call.camera,
        self_muted: root.self_mute,
        connected: call.connected,
        rtc_users: call.rtc_users(),
        me,
    };
    let (default_conn, stream_conns) =
        connection_debug_from_rtc_stats(&snap, &fallback, hide_personal);

    RtcDebugData {
        channel_name,
        guild_id,
        connected,
        media_session_id: snap.transport.media_session_id,
        hide_personal_information: hide_personal,
        default_stats: vec![default_conn],
        stream_stats: stream_conns,
    }
}

fn call_metadata(root: &Root) -> (Option<String>, Option<Id>, bool) {
    let Some(call) = root.call.as_ref() else {
        return (None, None, false);
    };
    let channel_name = if call.guild == 0 {
        root.stores
            .private_channels
            .iter()
            .find(|c| c.id == call.channel)
            .map(|c| root.private_channel_name(c))
    } else {
        root.stores
            .guild(call.guild)
            .and_then(|g| g.channels.iter().find(|c| c.id == call.channel))
            .map(|c| c.name.clone())
    };
    let guild_id = (call.guild != 0).then_some(call.guild);
    (channel_name, guild_id, call.connected)
}

fn call_hostname(root: &Root, call: &super::super::voice::Call) -> Option<String> {
    root.stores
        .voice_server
        .as_ref()
        .filter(|server| {
            server.guild_id == call.guild
                && (call.guild != 0 || server.channel_id == Some(call.channel))
        })
        .map(|server| short_hostname(&server.endpoint).to_owned())
        .or_else(|| call.stats.hostname.clone())
}

#[derive(Clone, Copy)]
struct GraphSpec {
    dpr: f64,
    width: f32,
}

/// The secondary RTC Debug window (`PopoutWindowRTCDebug`, `854x480`).
pub(crate) struct RtcDebugWindow {
    root: WeakEntity<Root>,
}

impl Render for RtcDebugWindow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(root_entity) = self.root.upgrade() else {
            return div()
                .size_full()
                .bg(color("--background-base-lowest"))
                .into_any_element();
        };

        let dpr = f64::from(window.scale_factor());
        let viewport_w = f32::from(window.viewport_size().width);
        let content_w = (viewport_w - SIDEBAR_WIDTH - 48.).clamp(320., 660.);
        let graph_spec = GraphSpec {
            dpr,
            width: ((content_w - 20.) * 0.5).floor().clamp(180., 300.),
        };
        let id = window.window_handle().window_id();
        let data = root_entity.read_with(cx, build_live_data);

        let current_section = cx.default_global::<RtcDebugGlobal>().store.section.clone();
        let entries = root_entity.read_with(cx, |root, _| build_sidebar_entries(root, &data));
        let section_exists = entries.iter().any(|entry| match entry {
            SidebarEntry::Item { section, .. } => section == &current_section,
            _ => false,
        });
        if !section_exists && current_section != DEFAULT_SECTION {
            cx.global_mut::<RtcDebugGlobal>().store.section = DEFAULT_SECTION.to_owned();
        }
        let active_section = cx.global::<RtcDebugGlobal>().store.section.clone();
        let selected_tab = cx
            .global::<RtcDebugGlobal>()
            .store
            .selected_tabs
            .get(&active_section)
            .copied()
            .unwrap_or(0);
        let simulcast_overrides = cx
            .global::<RtcDebugGlobal>()
            .store
            .simulcast_overrides
            .clone();
        let video_streams = cx.global::<RtcDebugGlobal>().store.video_streams.clone();

        let sidebar = render_sidebar(&entries, &active_section);
        let content = root_entity.update(cx, |root, _| {
            render_active_section(
                root,
                &data,
                &active_section,
                selected_tab,
                graph_spec,
                &simulcast_overrides,
                &video_streams,
            )
        });
        let tips = root_entity.read(cx).tip_layer_in(Some(id));

        div()
            .id("rtc-debug-window")
            .relative()
            .size_full()
            .flex()
            .bg(color("--background-base-lowest"))
            .text_color(color("--text-default"))
            .child(sidebar)
            .child(
                div()
                    .id("rtc-debug-content")
                    .flex_1()
                    .min_w(px(0.))
                    .h_full()
                    .overflow_y_scroll()
                    .bg(color("--background-base-lowest"))
                    .px(px(24.))
                    .py(px(32.))
                    .child(div().w_full().max_w(px(660.)).min_w(px(0.)).child(content)),
            )
            .children(tips)
            .into_any_element()
    }
}

fn render_sidebar(entries: &[SidebarEntry], active_section: &str) -> AnyElement {
    let mut rows = Vec::with_capacity(entries.len());
    for (idx, entry) in entries.iter().enumerate() {
        let el = match entry {
            SidebarEntry::ChannelName(name) => div()
                .ml(px(8.))
                .mb(px(8.))
                .text_size(px(18.))
                .line_height(px(22.))
                .font_weight(FontWeight(600.))
                .text_color(color("--text-strong"))
                .child(name.clone())
                .into_any_element(),
            SidebarEntry::ConnectionState(connected) => div()
                .ml(px(8.))
                .mb(px(8.))
                .text_size(px(14.))
                .line_height(px(18.))
                .font_weight(FontWeight(500.))
                .text_color(color("--text-default"))
                .child(
                    dc_intl::tr(if *connected {
                        "Connected"
                    } else {
                        "Disconnected"
                    })
                    .to_owned(),
                )
                .into_any_element(),
            SidebarEntry::Divider => div()
                .my(px(8.))
                .mx(px(10.))
                .h(px(1.))
                .bg(color("--border-subtle"))
                .into_any_element(),
            SidebarEntry::Header(label) => div()
                .px(px(10.))
                .pt(px(8.))
                .pb(px(4.))
                .text_size(px(12.))
                .line_height(px(16.))
                .font_weight(FontWeight(700.))
                .text_color(color("--text-muted"))
                .child(label.clone())
                .into_any_element(),
            SidebarEntry::Item {
                section,
                label,
                avatar_url,
            } => {
                let selected = section == active_section;
                let target_section = section.clone();
                div()
                    .id(("rtc-debug-nav", idx))
                    .h(px(32.))
                    .px(px(10.))
                    .rounded(px(6.))
                    .cursor_pointer()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .bg(color(if selected {
                        "--background-mod-subtle"
                    } else {
                        "--background-base-lower"
                    }))
                    .text_size(px(14.))
                    .line_height(px(18.))
                    .font_weight(FontWeight(if selected { 600. } else { 500. }))
                    .text_color(color(if selected {
                        "--text-strong"
                    } else {
                        "--text-muted"
                    }))
                    .hover(|s| s.bg(color("--background-mod-subtle")))
                    .on_click(move |_, _, cx| {
                        cx.global_mut::<RtcDebugGlobal>()
                            .store
                            .set_section(target_section.clone());
                        cx.refresh_windows();
                    })
                    .children(avatar_url.as_ref().map(|url| {
                        img(url.clone())
                            .size(px(20.))
                            .rounded_full()
                            .into_any_element()
                    }))
                    .child(div().truncate().child(label.clone()))
                    .into_any_element()
            }
        };
        rows.push(el);
    }

    div()
        .id("rtc-debug-sidebar")
        .w(px(SIDEBAR_WIDTH))
        .flex_none()
        .h_full()
        .overflow_y_scroll()
        .bg(color("--background-base-lower"))
        .border_r_1()
        .border_color(color("--border-subtle"))
        .px(px(10.))
        .py(px(16.))
        .flex()
        .flex_col()
        .gap(px(2.))
        .children(rows)
        .into_any_element()
}

fn render_active_section(
    root: &mut Root,
    data: &RtcDebugData,
    active_section: &str,
    selected_tab: usize,
    graph: GraphSpec,
    simulcast_overrides: &HashMap<(u64, ContextKind), SimulcastQuality>,
    video_streams: &HashMap<(String, u64, u32), String>,
) -> AnyElement {
    if active_section == "screenshare" {
        let screen = data
            .default_stats
            .iter()
            .chain(&data.stream_stats)
            .find_map(|c| c.screenshare.as_ref());
        return render_screenshare(screen, graph);
    }
    if active_section == "clips" {
        let clips = data.stream_stats.iter().find_map(|c| c.clips.as_ref());
        return render_clips(clips, graph);
    }
    let Some((ctx, user_id, index)) = parse_section_id(active_section) else {
        return empty_placeholder();
    };
    let list = match ctx {
        ContextKind::Default => &data.default_stats,
        ContextKind::Stream => &data.stream_stats,
    };
    let Some(conn) = list.get(index) else {
        return empty_placeholder();
    };

    if active_section.contains(":transport:") {
        return render_transport(
            conn.transport.as_ref(),
            conn.output_device_underruns,
            data,
            root,
            graph,
        );
    }
    if active_section.contains(":outbound:") {
        return render_rtp_section(
            root,
            &conn.outbound,
            RtpSectionView {
                context: ctx,
                media_engine_connection_id: &conn.media_engine_connection_id,
                user_id: None,
                guild_id: data.guild_id,
                base_title: dc_intl::t!("3u0gII"),
                show_user_info: false,
                show_simulcast_override: false,
                active_section,
                selected_tab,
                graph,
                simulcast_overrides,
                video_streams,
            },
        );
    }
    if active_section.contains(":camera:") {
        return render_camera(conn.camera.as_ref(), graph);
    }
    if let Some(uid) = user_id {
        let streams = conn.inbound.get(&uid).map(Vec::as_slice).unwrap_or(&[]);
        return render_rtp_section(
            root,
            streams,
            RtpSectionView {
                context: ctx,
                media_engine_connection_id: &conn.media_engine_connection_id,
                user_id: Some(uid),
                guild_id: data.guild_id,
                base_title: dc_intl::t!("SJmZaq"),
                show_user_info: true,
                show_simulcast_override: false,
                active_section,
                selected_tab,
                graph,
                simulcast_overrides,
                video_streams,
            },
        );
    }
    empty_placeholder()
}

fn empty_placeholder() -> AnyElement {
    div()
        .py(px(32.))
        .flex()
        .items_center()
        .justify_center()
        .child(super::super::login::wandering_cubes())
        .into_any_element()
}

fn section_heading(title: impl Into<String>) -> AnyElement {
    div()
        .text_size(px(16.))
        .line_height(px(24.))
        .font_weight(FontWeight(600.))
        .text_color(color("--text-strong"))
        .child(title.into())
        .into_any_element()
}

fn kv_divider() -> AnyElement {
    div()
        .mt(px(4.))
        .h(px(1.))
        .w_full()
        .bg(color("--border-subtle"))
        .into_any_element()
}

fn warning_badge() -> AnyElement {
    div()
        .size(px(14.))
        .flex_none()
        .rounded(px(2.))
        .bg(rgb(0xf0b232))
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap(px(1.))
        .child(
            div()
                .w(px(2.))
                .h(px(5.))
                .bg(color("--background-base-lowest")),
        )
        .child(
            div()
                .w(px(2.))
                .h(px(2.))
                .bg(color("--background-base-lowest")),
        )
        .into_any_element()
}

fn render_kv_item(
    key: &str,
    label: &str,
    value_rendered: String,
    graph: Option<AnyElement>,
) -> AnyElement {
    div()
        .flex_1()
        .min_w(px(0.))
        .mb(px(20.))
        .flex()
        .flex_col()
        .child(
            div()
                .flex()
                .items_start()
                .justify_between()
                .gap(px(8.))
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.))
                        .text_size(px(16.))
                        .line_height(px(24.))
                        .font_weight(FontWeight(600.))
                        .text_color(color("--text-strong"))
                        .child(label.to_owned()),
                )
                .child(
                    div()
                        .flex_shrink_0()
                        .max_w(px(180.))
                        .flex()
                        .items_center()
                        .gap(px(6.))
                        .when(is_warning_key(key), |this| this.child(warning_badge()))
                        .child(
                            div()
                                .text_size(px(14.))
                                .line_height(px(24.))
                                .text_color(color("--text-muted"))
                                .truncate()
                                .child(value_rendered),
                        ),
                ),
        )
        .children(graph.map(|el| div().mt(px(4.)).child(el)))
        .child(kv_divider())
        .into_any_element()
}

fn render_stat_item(key: &str, label: &str, value: &StatValue, graph: GraphSpec) -> AnyElement {
    let graph = if is_graph_key(key) {
        let converter = (key == "audioLevel").then_some(audio_level_converter as fn(f64) -> f64);
        match value.as_series() {
            Some(points) => Some(ping_graph::graph(ping_graph::Frame::at_size(
                points,
                now_ms(),
                graph.dpr,
                graph.width,
                100.,
                converter,
            ))),
            None => value.as_f64().map(|v| {
                let sample = [Sample {
                    time: now_ms(),
                    value: v,
                }];
                ping_graph::graph(ping_graph::Frame::at_size(
                    &sample,
                    now_ms(),
                    graph.dpr,
                    graph.width,
                    100.,
                    converter,
                ))
            }),
        }
    } else {
        None
    };
    render_kv_item(key, label, format_stat_value(key, value), graph)
}

fn render_kv_grid(mut items: Vec<AnyElement>) -> AnyElement {
    let mut rows = Vec::new();
    while !items.is_empty() {
        let left = items.remove(0);
        let right = if items.is_empty() {
            div().flex_1().min_w(px(0.)).into_any_element()
        } else {
            items.remove(0)
        };
        rows.push(
            div()
                .w_full()
                .min_w(px(0.))
                .flex()
                .gap(px(20.))
                .child(left)
                .child(right),
        );
    }
    div()
        .w_full()
        .flex()
        .flex_col()
        .children(rows)
        .into_any_element()
}

fn render_transport(
    transport: Option<&TransportDebug>,
    underruns: Option<u64>,
    data: &RtcDebugData,
    root: &Root,
    graph: GraphSpec,
) -> AnyElement {
    let Some(transport) = transport else {
        return empty_placeholder();
    };
    let heading = match &data.media_session_id {
        Some(id) => format!("{} - {id}", dc_intl::t!("wU9IN2")),
        None => dc_intl::t!("wU9IN2").to_owned(),
    };
    let mut items: Vec<AnyElement> = visible_transport_keys(
        transport,
        underruns.is_some(),
        data.hide_personal_information,
    )
    .into_iter()
    .map(|(k, v)| render_stat_item(k, stat_label(k), &v, graph))
    .collect();
    if let Some(count) = underruns {
        items.push(render_stat_item(
            "outputDeviceUnderruns",
            stat_label("outputDeviceUnderruns"),
            &StatValue::Uint(count),
            graph,
        ));
    }

    let mut reports: Vec<(String, &ReceiverReportDebug)> = transport
        .receiver_reports
        .iter()
        .map(|report| (display_user(root, data.guild_id, report.id).0, report))
        .collect();
    reports.sort_by(|a, b| a.0.cmp(&b.0));
    let report_items: Vec<AnyElement> = reports
        .into_iter()
        .map(|(name, report)| {
            let mut rows = Vec::new();
            if let Some(bitrate) = report.bitrate {
                rows.push((
                    RECEIVER_LABEL_BITRATE,
                    format!("{:.0} Kbps", bitrate / 1000.),
                ));
            }
            if let Some(lost) = report.fraction_lost {
                rows.push((RECEIVER_LABEL_LOSS, format!("{:.0}%", lost * 100. / 256.)));
            }
            if let Some(rtt) = report.rtt {
                rows.push((
                    RECEIVER_LABEL_RTT,
                    if rtt > 0. {
                        format!("{rtt} ms")
                    } else {
                        "-".to_owned()
                    },
                ));
            }
            div()
                .w_1_2()
                .flex_none()
                .pr(px(16.))
                .mb(px(20.))
                .flex()
                .flex_col()
                .child(
                    div()
                        .text_size(px(16.))
                        .line_height(px(24.))
                        .font_weight(FontWeight(600.))
                        .text_color(color("--text-strong"))
                        .child(name),
                )
                .children(rows.into_iter().map(|(label, val)| {
                    div()
                        .pl(px(16.))
                        .flex()
                        .justify_between()
                        .text_size(px(13.))
                        .line_height(px(18.))
                        .text_color(color("--text-muted"))
                        .child(label)
                        .child(val)
                }))
                .child(kv_divider())
                .into_any_element()
        })
        .collect();

    div()
        .flex()
        .flex_col()
        .gap(px(16.))
        .child(section_heading(heading))
        .child(render_kv_grid(items))
        .when(!report_items.is_empty(), |this| {
            this.child(div().mb(px(8.)).h(px(1.)).bg(color("--border-subtle")))
                .child(render_kv_grid(report_items))
        })
        .into_any_element()
}

struct RtpSectionView<'a> {
    context: ContextKind,
    media_engine_connection_id: &'a str,
    user_id: Option<u64>,
    guild_id: Option<Id>,
    base_title: &'a str,
    show_user_info: bool,
    show_simulcast_override: bool,
    active_section: &'a str,
    selected_tab: usize,
    graph: GraphSpec,
    simulcast_overrides: &'a HashMap<(u64, ContextKind), SimulcastQuality>,
    video_streams: &'a HashMap<(String, u64, u32), String>,
}

fn render_rtp_section(
    root: &mut Root,
    streams: &[RtpStreamDebug],
    view: RtpSectionView<'_>,
) -> AnyElement {
    let RtpSectionView {
        context,
        media_engine_connection_id,
        user_id,
        guild_id,
        base_title,
        show_user_info,
        show_simulcast_override,
        active_section,
        selected_tab,
        graph,
        simulcast_overrides,
        video_streams,
    } = view;
    if streams.is_empty() || (show_user_info && user_id.is_none()) {
        return empty_placeholder();
    }
    let heading = if show_user_info && let Some(uid) = user_id {
        let (name, _) = display_user(root, guild_id, uid);
        format!("{base_title} \u{2014} {name}")
    } else {
        base_title.to_owned()
    };

    let tab_index = selected_tab.min(streams.len() - 1);
    let active_stream = &streams[tab_index];

    let tabs = streams.iter().enumerate().map(|(idx, stream)| {
        let selected = idx == tab_index;
        let kind_label = match stream.kind.as_str() {
            "audio" => dc_intl::tr("Audio").to_owned(),
            "video" => dc_intl::tr("Video").to_owned(),
            other => {
                let mut chars = other.chars();
                match chars.next() {
                    Some(first) => format!("{}{}", first.to_uppercase(), chars.as_str()),
                    None => String::new(),
                }
            }
        };
        let res_suffix = match stream.resolution {
            Some((w, h)) if w != 0 && h != 0 => format!(" ({w}x{h})"),
            _ => String::new(),
        };
        let label = format!("{kind_label}{res_suffix}");
        let sec = active_section.to_owned();
        div()
            .id(("rtc-debug-tab", idx))
            .relative()
            .h(px(30.))
            .mb(px(-1.))
            .cursor_pointer()
            .text_size(px(14.))
            .line_height(px(14.))
            .font_weight(FontWeight(600.))
            .text_color(color(if selected {
                "--text-brand"
            } else {
                "--text-muted"
            }))
            .on_click(move |_, _, cx| {
                cx.global_mut::<RtcDebugGlobal>()
                    .store
                    .selected_tabs
                    .insert(sec.clone(), idx);
                cx.refresh_windows();
            })
            .child(label)
            .when(selected, |this| {
                this.child(
                    div()
                        .absolute()
                        .left_0()
                        .right_0()
                        .bottom_0()
                        .h(px(2.))
                        .rounded_t(px(2.))
                        .bg(color("--text-brand")),
                )
            })
    });

    let keys = sort_rtp_keys(active_stream.stats.keys().map(String::as_str));
    let items: Vec<AnyElement> = keys
        .into_iter()
        .filter(|k| !is_ignored_key(k))
        .filter_map(|k| {
            active_stream
                .stats
                .get(k)
                .map(|val| render_stat_item(k, stat_label(k), val, graph))
        })
        .collect();

    let video_preview = (active_stream.kind == "video")
        .then(|| {
            let uid = user_id?;
            let stream_id = video_streams.get(&(
                media_engine_connection_id.to_owned(),
                uid,
                active_stream.ssrc,
            ));
            let source = match context {
                ContextKind::Default => dc_voice::VideoSource::Camera,
                ContextKind::Stream => dc_voice::VideoSource::Stream(uid),
            };
            let image = root.video.images.image(&(uid, source)).cloned();
            if image.is_none() && stream_id.is_none() {
                return None;
            }
            let inner: AnyElement = match image {
                Some(render_img) => img(render_img).size_full().into_any_element(),
                None => div()
                    .size_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_size(px(12.))
                    .text_color(color("--text-muted"))
                    .child(format!(
                        "{VIDEO_SOURCE_PREVIEW}{}",
                        stream_id.map(String::as_str).unwrap_or_default()
                    ))
                    .into_any_element(),
            };
            Some(
                div()
                    .w(px(320.))
                    .h(px(180.))
                    .mb(px(20.))
                    .rounded(px(4.))
                    .border_1()
                    .border_color(color("--border-subtle"))
                    .bg(color("--background-base-lower"))
                    .overflow_hidden()
                    .child(inner),
            )
        })
        .flatten();

    let simulcast = (show_simulcast_override
        && active_stream.kind == "video"
        && context == ContextKind::Default)
        .then_some(user_id)
        .flatten()
        .map(|uid| {
            let current = simulcast_overrides
                .get(&(uid, context))
                .copied()
                .unwrap_or(SimulcastQuality::NoOverride);
            div()
                .mb(px(20.))
                .flex()
                .flex_col()
                .gap(px(8.))
                .child(
                    div()
                        .text_size(px(12.))
                        .font_weight(FontWeight(700.))
                        .text_color(color("--text-muted"))
                        .child(SIMULCAST_HEADER),
                )
                .child(
                    div()
                        .flex()
                        .gap(px(8.))
                        .children(SIMULCAST_OPTIONS.iter().enumerate().map(
                            |(idx, &(quality, label))| {
                                let active = quality == current;
                                div()
                                    .id(("rtc-simulcast", idx))
                                    .h(px(32.))
                                    .px(px(12.))
                                    .rounded(px(6.))
                                    .border_1()
                                    .border_color(color(if active {
                                        "--text-brand"
                                    } else {
                                        "--control-secondary-border-default"
                                    }))
                                    .bg(color(if active {
                                        "--background-mod-subtle"
                                    } else {
                                        "--control-secondary-background-default"
                                    }))
                                    .cursor_pointer()
                                    .flex()
                                    .items_center()
                                    .text_size(px(13.))
                                    .text_color(color("--text-strong"))
                                    .on_click(move |_, _, cx| {
                                        cx.global_mut::<RtcDebugGlobal>()
                                            .store
                                            .set_simulcast_override(uid, context, quality);
                                        cx.refresh_windows();
                                    })
                                    .child(if quality == SimulcastQuality::NoOverride {
                                        dc_intl::tr("None").to_owned()
                                    } else {
                                        label.to_owned()
                                    })
                            },
                        )),
                )
        });

    div()
        .flex()
        .flex_col()
        .gap(px(16.))
        .child(section_heading(heading))
        .child(
            div()
                .mb(px(16.))
                .flex()
                .gap(px(24.))
                .border_b_1()
                .border_color(color("--border-subtle"))
                .children(tabs),
        )
        .children(video_preview)
        .child(div().mb(px(20.)).child(render_kv_grid(items)))
        .children(simulcast)
        .into_any_element()
}

fn render_camera(camera: Option<&BTreeMap<String, StatValue>>, graph: GraphSpec) -> AnyElement {
    let Some(camera) = camera else {
        return empty_placeholder();
    };
    let items: Vec<AnyElement> = camera
        .iter()
        .filter(|(k, _)| !is_ignored_key(k))
        .map(|(k, v)| render_stat_item(k, stat_label(k), v, graph))
        .collect();
    div()
        .flex()
        .flex_col()
        .gap(px(16.))
        .child(section_heading(dc_intl::t!("2AGBWH")))
        .child(render_kv_grid(items))
        .into_any_element()
}

fn render_screenshare(
    screenshare: Option<&BTreeMap<String, StatValue>>,
    graph: GraphSpec,
) -> AnyElement {
    let Some(screen) = screenshare else {
        return empty_placeholder();
    };
    let has_bitblt = screen.contains_key("hybridGdiBitBltFrames")
        && screen.contains_key("hybridGdiPrintWindowFrames");
    let mut items = Vec::new();
    let mut seen = Vec::new();

    for &key in SCREENSHARE_ORDER {
        if key == "hybridGdiFrames" && has_bitblt {
            continue;
        }
        let Some(val) = screen.get(key) else {
            continue;
        };
        seen.push(key);
        if let Some((_, pair_key, suffix)) = SCREENSHARE_PAIRED.iter().find(|(k, _, _)| *k == key)
            && let Some(pair_val) = screen.get(*pair_key)
        {
            seen.push(*pair_key);
            items.push(render_kv_item(
                key,
                &format!("{} ({suffix})", stat_label(key)),
                format!(
                    "{} ({})",
                    format_stat_value(key, val),
                    format_stat_value(pair_key, pair_val)
                ),
                None,
            ));
        } else {
            items.push(render_stat_item(key, stat_label(key), val, graph));
        }
    }

    for (k, v) in screen {
        if seen.contains(&k.as_str())
            || SCREENSHARE_PAIRED.iter().any(|(_, pk, _)| pk == k)
            || is_ignored_key(k)
        {
            continue;
        }
        items.push(render_stat_item(k, stat_label(k), v, graph));
    }

    div()
        .flex()
        .flex_col()
        .gap(px(16.))
        .child(section_heading(dc_intl::t!("gWbr/U")))
        .child(render_kv_grid(items))
        .into_any_element()
}

fn render_clips(clips: Option<&ClipsDebug>, graph: GraphSpec) -> AnyElement {
    let Some(clips) = clips else {
        return empty_placeholder();
    };
    let mut keys: Vec<&str> = clips.stats.keys().map(String::as_str).collect();
    keys.sort_by(|a, b| {
        let ra = CLIPS_ORDER.iter().find(|(k, _)| k == a).map(|(_, r)| *r);
        let rb = CLIPS_ORDER.iter().find(|(k, _)| k == b).map(|(_, r)| *r);
        match (ra, rb) {
            (Some(x), Some(y)) => x.cmp(&y),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => a.cmp(b),
        }
    });

    let mut items = Vec::new();
    for key in keys {
        if let Some(val) = clips.stats.get(key) {
            if key == "bitrateKbps" && !items.len().is_multiple_of(2) {
                items.push(div().flex_1().min_w(px(0.)).into_any_element());
            }
            items.push(render_stat_item(key, stat_label(key), val, graph));
        }
    }
    if let (Some(frames), Some(dur)) = (clips.recent_encoded_frames, clips.recent_duration_ms)
        && dur > 0
    {
        let fps = frames as f64 / (dur as f64 / 1000.);
        items.push(render_stat_item(
            FPS_COMPUTED_LABEL,
            FPS_COMPUTED_LABEL,
            &StatValue::Float(fps),
            graph,
        ));
    }

    let buffer_sections = clips.buffers.iter().map(|buffer| {
        let mut bkeys: Vec<&str> = buffer
            .stats
            .keys()
            .map(String::as_str)
            .filter(|k| *k != "key")
            .collect();
        bkeys.sort_by(|a, b| {
            let ra = BUFFER_ORDER.iter().find(|(k, _)| k == a).map(|(_, r)| *r);
            let rb = BUFFER_ORDER.iter().find(|(k, _)| k == b).map(|(_, r)| *r);
            match (ra, rb) {
                (Some(x), Some(y)) => x.cmp(&y),
                (Some(_), None) => std::cmp::Ordering::Less,
                (None, Some(_)) => std::cmp::Ordering::Greater,
                (None, None) => a.cmp(b),
            }
        });
        let b_items: Vec<AnyElement> = bkeys
            .into_iter()
            .filter_map(|k| {
                buffer
                    .stats
                    .get(k)
                    .map(|val| render_stat_item(k, stat_label(k), val, graph))
            })
            .collect();
        div()
            .mb(px(40.))
            .flex()
            .flex_col()
            .gap(px(16.))
            .child(section_heading(format!("{BUFFER_PREFIX}{}", buffer.key)))
            .child(render_kv_grid(b_items))
    });

    div()
        .flex()
        .flex_col()
        .gap(px(16.))
        .child(section_heading(dc_intl::t!("MKrFKE")))
        .child(render_kv_grid(items))
        .children(buffer_sections)
        .into_any_element()
}

impl Root {
    /// Returns the active secondary `PopoutWindowRTCDebug` handle if open.
    #[cfg(feature = "capture")]
    pub(crate) fn rtc_debug_window(&self, cx: &App) -> Option<AnyWindowHandle> {
        rtc_debug_window(cx)
    }

    /// Opens or focuses `PopoutWindowRTCDebug` at `854x480` (`ho` in module `233545` + `596339`).
    pub(crate) fn open_rtc_debug(&mut self, section: Option<String>, cx: &mut Context<Self>) {
        if let Some(existing) = cx.default_global::<RtcDebugGlobal>().window {
            if let Some(sec) = section {
                cx.global_mut::<RtcDebugGlobal>().store.open(Some(sec));
            }
            cx.defer(move |cx| {
                let _ = existing.update(cx, |_, window, cx| {
                    window.activate_window();
                    cx.notify();
                });
            });
            return;
        }
        cx.global_mut::<RtcDebugGlobal>().store.open(section);
        let root = cx.entity().downgrade();
        cx.defer(move |cx| {
            let bounds = Bounds::centered(None, size(px(WINDOW_WIDTH), px(WINDOW_HEIGHT)), cx);
            let view = root.clone();
            let opened = cx.open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    titlebar: Some(TitlebarOptions {
                        title: Some(dc_intl::t!("5I/1fl").into()),
                        ..Default::default()
                    }),
                    ..Default::default()
                },
                move |window, cx| {
                    crate::notifications::observe_window(window);
                    let closed = view.clone();
                    window.on_window_should_close(cx, move |window, cx| {
                        let id = window.window_handle().window_id();
                        let _ = closed.update(cx, |root, cx| {
                            root.drop_tips_of(id);
                            cx.notify();
                        });
                        if let Some(global) = cx.try_global::<RtcDebugGlobal>()
                            && global.window.is_some()
                        {
                            let g = cx.global_mut::<RtcDebugGlobal>();
                            g.window = None;
                            g.store.close();
                        }
                        true
                    });
                    cx.new(|cx| {
                        if let Some(entity) = view.upgrade() {
                            cx.observe(&entity, |_, _, cx| cx.notify()).detach();
                        }
                        RtcDebugWindow { root: view }
                    })
                },
            );
            if let Ok(handle) = opened {
                cx.default_global::<RtcDebugGlobal>().window = Some(handle);
                let _ = root.update(cx, |_, cx| cx.notify());
            }
        });
        cx.notify();
    }

    /// Cleanly removes `PopoutWindowRTCDebug` and resets `RTCDebugStore` modal state (`VN` in `233545`).
    pub(crate) fn close_rtc_debug(&mut self, cx: &mut Context<Self>) {
        let Some(handle) = rtc_debug_window(cx) else {
            return;
        };
        self.drop_tips_of(handle.window_id());
        let g = cx.global_mut::<RtcDebugGlobal>();
        g.window = None;
        g.store.close();
        cx.defer(move |cx| {
            let _ = handle.update(cx, |_, window, _| window.remove_window());
        });
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_transport(local_address: Option<&str>) -> TransportDebug {
        let mut stats = BTreeMap::new();
        stats.insert("ping".to_owned(), StatValue::Float(42.));
        stats.insert("bytesSent".to_owned(), StatValue::Uint(4096));
        stats.insert("apmFrameCount".to_owned(), StatValue::Uint(99));
        TransportDebug {
            local_address: local_address.map(str::to_owned),
            hostname: Some("c-fra14-db667712".to_owned()),
            receiver_reports: Vec::new(),
            stats,
        }
    }

    #[test]
    fn f_0389_0390_2476_2477_2480_2481_2482_store_actions_and_sections() {
        let mut store = RtcDebugStore::default();
        assert_eq!(store.section, DEFAULT_SECTION);
        assert!(!store.modal_open);

        // F-2477 / F-2482: open defaults to transport:0 or honors explicit section.
        store.open(None);
        assert!(store.modal_open);
        assert_eq!(store.section, "default:transport:0");

        // F-2480: switch sidebar section.
        store.set_section("default:42:0");
        assert_eq!(store.section, "default:42:0");

        // F-2481: video output mapping + simulcast override.
        store.update_video_output("conn-1", 42, 777, "stream-abc");
        assert_eq!(
            store.video_streams.get(&("conn-1".to_owned(), 42, 777)),
            Some(&"stream-abc".to_owned())
        );
        store.update_video_output("conn-1", 42, 777, "");
        assert!(
            !store
                .video_streams
                .contains_key(&("conn-1".to_owned(), 42, 777))
        );

        store.set_simulcast_override(42, ContextKind::Default, SimulcastQuality::High);
        assert_eq!(
            store.simulcast_override(42, ContextKind::Default),
            SimulcastQuality::High
        );
        store.clear_user_simulcast_on_video_end(42, ContextKind::Default);
        assert_eq!(
            store.simulcast_override(42, ContextKind::Default),
            SimulcastQuality::NoOverride
        );

        // F-2476: modal close clears open flag cleanly.
        store.close();
        assert!(!store.modal_open);
    }

    #[test]
    fn f_0390_graph_series_cap_at_600_and_vanished_peer_resets_section() {
        let mut store = RtcDebugStore::default();
        for tick in 0..650u64 {
            let mut t_stats = BTreeMap::new();
            t_stats.insert("ping".to_owned(), StatValue::Float(tick as f64));
            let mut inbound = BTreeMap::new();
            if tick < 649 {
                inbound.insert(
                    99,
                    vec![RtpStreamDebug {
                        kind: "video".to_owned(),
                        ssrc: 55,
                        resolution: Some((1280, 720)),
                        codec: Some((96, "vp8".to_owned())),
                        stats: BTreeMap::from([(
                            "frameRateRender".to_owned(),
                            StatValue::Float(60.),
                        )]),
                    }],
                );
            }
            store.ingest_stats(
                ContextKind::Default,
                0,
                1_700_000_000_000. + tick as f64 * 1000.,
                RtcConnectionDebug {
                    media_engine_connection_id: "c0".to_owned(),
                    transport: Some(TransportDebug {
                        stats: t_stats,
                        ..Default::default()
                    }),
                    inbound,
                    ..Default::default()
                },
            );
            if tick == 0 {
                store.set_section("default:99:0");
                let summary = store.get_inbound_stats(99, ContextKind::Default);
                assert_eq!(summary.codec.as_deref(), Some("vp8"));
                assert_eq!(summary.resolution, Some((1280, 720)));
                assert_eq!(summary.fps, Some(60.));
                let out_summary = store.get_outbound_stats(ContextKind::Default);
                assert_eq!(out_summary.codec, None);
                assert_eq!(out_summary.resolution, None);
            }
        }
        // Peer 99 absent on tick 649 resets selection back to default:transport:0.
        assert_eq!(store.section, DEFAULT_SECTION);
        let ping_series = store.default_stats[0]
            .transport
            .as_ref()
            .and_then(|t| t.stats.get("ping"))
            .and_then(StatValue::as_series)
            .expect("ping series");
        assert_eq!(ping_series.len(), 600);
        assert_eq!(ping_series[0].value, 50.);
        assert_eq!(ping_series[599].value, 649.);
    }

    #[test]
    fn streamer_mode_hide_personal_information_suppresses_local_address() {
        let transport = sample_transport(Some("192.168.1.44:50000"));
        let shown: Vec<&str> = visible_transport_keys(&transport, false, false)
            .into_iter()
            .map(|(k, _)| k)
            .collect();
        assert!(shown.contains(&"localAddress"));
        assert!(shown.contains(&"hostname"));
        assert!(!shown.contains(&"apmFrameCount"));

        let hidden: Vec<&str> = visible_transport_keys(&transport, false, true)
            .into_iter()
            .map(|(k, _)| k)
            .collect();
        assert!(!hidden.contains(&"localAddress"));
        assert!(hidden.contains(&"hostname"));
    }

    #[test]
    fn formatting_and_rtp_sort_match_950040() {
        assert_eq!(
            format_stat_value("bitrate", &StatValue::Float(64_000.)),
            "64.00 Kbps"
        );
        assert_eq!(
            format_stat_value("bytesSent", &StatValue::Uint(512)),
            "512 bytes"
        );
        assert_eq!(
            format_stat_value("bytesSent", &StatValue::Uint(2048)),
            "2.00 KB"
        );
        assert_eq!(
            format_stat_value(
                "codec",
                &StatValue::Codec {
                    id: 111,
                    name: "opus".into()
                }
            ),
            "Opus (111)"
        );
        assert_eq!(
            format_stat_value("audioLevel", &StatValue::Float(0.)),
            "-\u{221E} dB"
        );
        assert_eq!(audio_level_converter(0.), 0.);
        assert_eq!(audio_level_converter(1.), 100.);
        assert_eq!(
            format_stat_value("currentSampleRate", &StatValue::Uint(48000)),
            "48 kHz"
        );
        assert_eq!(
            format_stat_value(
                "resolution",
                &StatValue::Resolution {
                    width: 1920,
                    height: 1080
                }
            ),
            "1920x1080"
        );
        assert_eq!(
            format_stat_value("secureFramesProtocolVersion", &StatValue::Uint(1)),
            "Version 1"
        );
        assert_eq!(
            format_stat_value("secureFramesProtocolVersion", &StatValue::Uint(0)),
            "Disabled"
        );

        let sorted = sort_rtp_keys([
            "ping",
            "packetsSent",
            "audioLevel",
            "ssrc",
            "jitter",
            "codec",
        ]);
        assert_eq!(
            sorted,
            [
                "ssrc",
                "codec",
                "jitter",
                "audioLevel",
                "packetsSent",
                "ping"
            ]
        );
    }

    #[test]
    fn rtc_stats_snapshot_maps_fields_and_streamer_redaction_clears_local_address() {
        let mut inbound = HashMap::new();
        inbound.insert(
            42,
            dc_voice::rtc_stats::InboundUserStats {
                audio: Some(dc_voice::rtc_stats::InboundAudioStats {
                    ssrc: 3001,
                    codec: "opus",
                    payload_type: 111,
                    bytes_received: 24_000,
                    packets_received: 120,
                    packets_lost: 1,
                    fec_recovered: 1,
                    jitter_ms: 4.5,
                    jitter_buffer_ms: 40,
                    jitter_buffer_preferred_ms: 20,
                    audio_level: 0.5,
                    decoding_muted_output: false,
                    op_normal: 118,
                    op_expand: 2,
                    op_accelerate: 1,
                    op_preemptive_expand: 0,
                    op_cng: 0,
                    secure_frames_protocol_version: 1,
                    ..Default::default()
                }),
                video: Some(dc_voice::rtc_stats::InboundVideoStats {
                    ssrc: 3002,
                    codec: "H264",
                    payload_type: 103,
                    width: 1280,
                    height: 720,
                    frames_decoded: 180,
                    fps_decoded: 30,
                    nacks: 2,
                    decoder_implementation_name: Some("OpenH264".into()),
                    secure_frames_protocol_version: 1,
                    ..Default::default()
                }),
            },
        );
        let snap = dc_voice::RtcStatsSnapshot {
            connected: true,
            transport: dc_voice::rtc_stats::TransportStats {
                hostname: Some("fra02.discord.media".into()),
                local_address: Some("192.168.1.44:50000".into()),
                media_session_id: Some("sess-123".into()),
                ping_ms: Some(32),
                available_outgoing_bitrate: 2_500_000,
                outbound_bitrate_estimate: 1_800_000,
                inbound_bitrate_estimate: 900_000,
                output_device_underruns: 2,
                secure_frames_protocol_version: 1,
                receiver_reports: vec![dc_voice::rtc_stats::ReceiverReportStats {
                    user_id: 42,
                    ssrc: 1001,
                    bitrate_bps: 64_000,
                    fraction_lost: 0.02,
                    rtt_ms: Some(25),
                }],
                ..Default::default()
            },
            outbound: dc_voice::rtc_stats::OutboundStats {
                audio: Some(dc_voice::rtc_stats::OutboundAudioStats {
                    ssrc: 1001,
                    codec: "opus",
                    payload_type: 111,
                    bitrate: 64_000,
                    audio_level: 0.25,
                    audio_detected: Some(true),
                    secure_frames_protocol_version: 1,
                    ..Default::default()
                }),
                video: Some(dc_voice::rtc_stats::OutboundVideoStats {
                    ssrc: 2001,
                    codec: "H264",
                    payload_type: 103,
                    bitrate_target: 1_500_000,
                    bitrate_actual: 1_200_000,
                    width: 1920,
                    height: 1080,
                    fps_encoded: 60,
                    encoder_implementation_name: Some("OpenH264".into()),
                    secure_frames_protocol_version: 1,
                    ..Default::default()
                }),
            },
            camera: Some(dc_voice::rtc_stats::CameraStats {
                device_name: Some("Integrated Camera".into()),
                width: 1280,
                height: 720,
                fps: 30,
                frames_captured: 300,
                frames_dropped: 1,
                captured: true,
                encoded: true,
            }),
            inbound,
            streams: vec![dc_voice::rtc_stats::StreamStats {
                stream_key: "guild:1:2:42".into(),
                owner_id: 42,
                sending: true,
                connected: true,
                transport: dc_voice::rtc_stats::TransportStats {
                    hostname: Some("ams01.discord.media".into()),
                    local_address: Some("192.168.1.44:50001".into()),
                    ping_ms: Some(28),
                    ..Default::default()
                },
                ..Default::default()
            }],
        };
        let fallback = FallbackCallStats {
            media_engine_connection_id: "default-0",
            hostname: Some("fallback".into()),
            pings: &[],
            loss: None,
            dave_version: 1,
            camera: true,
            self_muted: false,
            connected: true,
            rtc_users: &[42],
            me: Some(1),
        };

        let (def, streams) = connection_debug_from_rtc_stats(&snap, &fallback, false);
        let t = def.transport.as_ref().expect("default transport");
        assert_eq!(t.local_address.as_deref(), Some("192.168.1.44:50000"));
        assert_eq!(t.hostname.as_deref(), Some("fra02"));
        assert_eq!(t.receiver_reports.len(), 1);
        assert_eq!(def.output_device_underruns, Some(2));
        assert_eq!(def.outbound.len(), 2);
        assert!(def.camera.is_some());
        let peer_streams = def.inbound.get(&42).expect("inbound peer");
        assert_eq!(peer_streams.len(), 2);
        assert!(peer_streams[0].stats.contains_key("opNormal"));
        assert_eq!(streams.len(), 1);
        assert_eq!(
            streams[0]
                .transport
                .as_ref()
                .and_then(|st| st.local_address.as_deref()),
            Some("192.168.1.44:50001")
        );

        let (safe_def, safe_streams) = connection_debug_from_rtc_stats(&snap, &fallback, true);
        assert_eq!(
            safe_def
                .transport
                .as_ref()
                .and_then(|st| st.local_address.as_ref()),
            None
        );
        assert_eq!(
            safe_streams[0]
                .transport
                .as_ref()
                .and_then(|st| st.local_address.as_ref()),
            None
        );
    }
}
