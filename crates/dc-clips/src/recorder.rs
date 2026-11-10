//! The recorder: one per process. While clipping is on and a Go Live stream is live, the stream's sender feeds it the
//! encoded pictures, the stream audio and the user's own voice; it keeps the newest `clipsLength` of them. When no stream
//! is live, or clipping is off, it holds nothing.
//!
//! Official rules this follows (plan section 1.2 and 2.5, modules 489496 `yd`, 516130 `applyUserVoiceRecording`): the
//! Go Live clip comes from the stream's own encode, so it has the stream's quality; the local user's audio is recorded
//! as the user's own; other participants' voices are not recorded here, because the permission that decides it (the
//! RTC connection flag `ALLOW_VOICE_RECORDING` of each participant) is not read by this client yet.
use crate::audio::TrackEncoder;
use crate::buffer::{
    AudioPacket, AudioRing, MAX_VIDEO_BYTES, PACKET_MS, VideoFrame, VideoRing, now_ms,
};
use crate::metadata::ClipMetadata;
use crate::mp4::{self, AudioInput, VideoInput, VideoSample};
use crate::sps;
use std::fs;
use std::io::{self, BufWriter};
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard, OnceLock};
use tracing::{debug, warn};

/// The audio that is buffered keeps this much more than the video, so a clip's audio reaches back to the keyframe the
/// video window starts at.
const AUDIO_SLACK_MS: u64 = 4_000;
/// Gaps shorter than this between audio packets are capture jitter, not silence.
const GAP_MS: i64 = 30;

/// `qtSJxb`: the application name of a clip that has no source name.
pub const UNKNOWN_SOURCE: &str = "Unknown";

/// Which audio track a sample belongs to (`applicationAudio` and `voiceAudio` of the edit metadata; the soundboard
/// track is not recorded by this engine).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Track {
    /// The stream's own audio: what the viewers hear of the shared window. Stereo.
    Application,
    /// The user's microphone after processing. Mono.
    Voice,
}

/// What the stream shows: the application name of the clip (`getStreamerActiveStreamMetadata().sourceName`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceInfo {
    pub name: String,
    pub application_id: Option<String>,
}

/// Why a window could not be cut.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum SnapshotError {
    /// Clipping is off.
    #[error("clipping is not enabled")]
    NotEnabled,
    /// No stream is live (`no_eligible_source`).
    #[error("no capture source")]
    NoSource,
    /// A stream is live but nothing has arrived from its encoder yet (`recording_not_ready`).
    #[error("the recorder is not ready")]
    NotReady,
    /// Pictures arrived but no keyframe is held to start a clip from (`buffer_warming_up`).
    #[error("the buffer is warming up")]
    WarmingUp,
    /// The first keyframe has no usable parameter sets.
    #[error("the stream has no parameter sets")]
    NoParameterSets,
}

struct TrackBuf {
    encoder: Option<TrackEncoder>,
    ring: AudioRing,
    channels: u8,
}

impl TrackBuf {
    fn new(channels: u8) -> Self {
        Self {
            encoder: None,
            ring: AudioRing::default(),
            channels,
        }
    }

    fn clear(&mut self) {
        self.encoder = None;
        self.ring.clear();
    }

    fn push(&mut self, samples: &[f32], now: u64) {
        if self.encoder.is_none() {
            self.encoder = TrackEncoder::new(self.channels).ok();
        }
        if let Some(encoder) = self.encoder.as_mut() {
            for packet in encoder.push(samples, now) {
                self.ring.push(packet);
            }
        }
    }
}

struct Inner {
    /// What the app announced the next stream shows, taken by the next [`Recorder::attach`].
    announced: Option<SourceInfo>,
    source: Option<SourceInfo>,
    video: VideoRing,
    application: TrackBuf,
    voice: TrackBuf,
    length_ms: u64,
    max_bytes: usize,
}

impl Inner {
    fn new() -> Self {
        Self {
            announced: None,
            source: None,
            video: VideoRing::default(),
            application: TrackBuf::new(2),
            voice: TrackBuf::new(1),
            length_ms: 30_000,
            max_bytes: MAX_VIDEO_BYTES,
        }
    }

    fn free(&mut self) {
        self.source = None;
        self.video.clear();
        self.application.clear();
        self.voice.clear();
    }

    fn track(&mut self, track: Track) -> &mut TrackBuf {
        match track {
            Track::Application => &mut self.application,
            Track::Voice => &mut self.voice,
        }
    }
}

/// The process-wide recorder.
pub struct Recorder {
    enabled: AtomicBool,
    /// The generation of the attached stream, 0 when none is.
    live: AtomicU64,
    next_generation: AtomicU64,
    inner: Mutex<Inner>,
}

/// A recorder of its own, for tests and replays; it lives as long as the process.
pub fn standalone() -> &'static Recorder {
    Box::leak(Box::new(Recorder {
        enabled: AtomicBool::new(false),
        live: AtomicU64::new(0),
        next_generation: AtomicU64::new(1),
        inner: Mutex::new(Inner::new()),
    }))
}

/// The recorder every part of the process shares.
pub fn recorder() -> &'static Recorder {
    static RECORDER: OnceLock<Recorder> = OnceLock::new();
    RECORDER.get_or_init(|| Recorder {
        enabled: AtomicBool::new(false),
        live: AtomicU64::new(0),
        next_generation: AtomicU64::new(1),
        inner: Mutex::new(Inner::new()),
    })
}

impl Recorder {
    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Clipping on or off, and how much of the stream to keep (`setClipsRecordingEnabled`, `setClipBufferLength`).
    /// Turning it off frees the buffer at once.
    pub fn configure(&self, enabled: bool, length_ms: u64) {
        let mut inner = self.lock();
        inner.length_ms = length_ms;
        self.enabled.store(enabled, Ordering::Release);
        if enabled {
            let (now, length) = (now_ms(), inner.length_ms);
            let keep = inner.max_bytes;
            inner.video.evict(now, length, keep);
            let slack = length + AUDIO_SLACK_MS;
            inner.application.ring.evict(now, slack);
            inner.voice.ring.evict(now, slack);
        } else {
            inner.free_media();
        }
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled.load(Ordering::Acquire)
    }

    /// Whether the buffer holds a picture yet (`isClipsRecordingReady` for a stream: the encoder has delivered).
    pub fn is_ready(&self) -> bool {
        !self.lock().video.is_empty()
    }

    /// Whether a stream is attached (the Go Live path has a source).
    pub fn has_source(&self) -> bool {
        self.live.load(Ordering::Acquire) != 0
    }

    /// The app says what the stream it is about to start shows (the name becomes the clip's application name). Taken by
    /// the next [`Recorder::attach`].
    pub fn announce_source(&self, source: SourceInfo) {
        self.lock().announced = Some(source);
    }

    /// A stream of ours went live: a tap its sender feeds. The buffer starts empty. The source is what the app
    /// announced, or "Unknown" (the string `qtSJxb`, module 160330) when it did not.
    pub fn attach(&'static self) -> Tap {
        let generation = self.next_generation.fetch_add(1, Ordering::AcqRel);
        let mut inner = self.lock();
        inner.free();
        inner.source = Some(inner.announced.take().unwrap_or_else(|| SourceInfo {
            name: UNKNOWN_SOURCE.to_owned(),
            application_id: None,
        }));
        self.live.store(generation, Ordering::Release);
        debug!("clip buffer attached");
        Tap {
            recorder: self,
            generation,
        }
    }

    /// The stream ended: nothing stays in memory.
    fn detach(&self, generation: u64) {
        let mut inner = self.lock();
        if self
            .live
            .compare_exchange(generation, 0, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
        {
            inner.free();
            debug!("clip buffer freed");
        }
    }

    /// Whether the tap of this generation should record now.
    fn recording(&self, generation: u64) -> bool {
        self.enabled.load(Ordering::Acquire) && self.live.load(Ordering::Acquire) == generation
    }

    /// The user's own voice, as the call sends it (mono, 48 kHz, after processing). Recorded when a stream is live and
    /// clipping is on; the official rule for the local user's audio (`setClipRecordUser(me, "audio", ...)`).
    pub fn push_voice(&'static self, samples: &[f32]) {
        let generation = self.live.load(Ordering::Acquire);
        if generation == 0 {
            return;
        }
        Tap {
            recorder: self,
            generation,
        }
        .push_audio(Track::Voice, samples);
    }

    /// Bytes of video and the number of audio packets held, for the log.
    pub fn held(&self) -> (usize, usize, usize) {
        let inner = self.lock();
        (
            inner.video.bytes(),
            inner.application.ring.len(),
            inner.voice.ring.len(),
        )
    }

    /// Cuts the newest window of the buffer, as `saveClipEx` is asked to (`startMs = now - clipsLength`, `endMs = now`),
    /// copying it out so the file can be written without holding the buffer.
    pub fn snapshot(&self) -> Result<Snapshot, SnapshotError> {
        self.snapshot_at(now_ms())
    }

    /// [`Recorder::snapshot`] with the end of the window given, for replays and tests that feed the buffer by their own
    /// clock.
    pub fn snapshot_at(&self, end: u64) -> Result<Snapshot, SnapshotError> {
        if !self.is_enabled() {
            return Err(SnapshotError::NotEnabled);
        }
        let mut guard = self.lock();
        let inner = &mut *guard;
        let Some(source) = inner.source.clone() else {
            return Err(SnapshotError::NoSource);
        };
        if inner.video.is_empty() {
            return Err(SnapshotError::NotReady);
        }
        let length = inner.length_ms;
        let window = inner
            .video
            .cut(end, length)
            .ok_or(SnapshotError::WarmingUp)?;
        let params = window[0]
            .params
            .clone()
            .or_else(|| inner.video.latest_params().cloned())
            .ok_or(SnapshotError::NoParameterSets)?;
        let info = sps::parse(&params.0).ok_or(SnapshotError::NoParameterSets)?;
        // The window ends where the picture parameters change (a restarted encoder): one clip, one size.
        let frames: Vec<&VideoFrame> = window
            .iter()
            .enumerate()
            .take_while(|(index, frame)| {
                *index == 0 || frame.params.as_ref().is_none_or(|other| *other == params)
            })
            .map(|(_, frame)| *frame)
            .collect();
        let first_arrival = frames[0].arrival_ms;
        let last_arrival = frames[frames.len() - 1].arrival_ms;
        let mut samples = Vec::with_capacity(frames.len());
        let mut fallback = 3_000u32;
        for (index, frame) in frames.iter().enumerate() {
            let duration = match frames.get(index + 1) {
                Some(next) => (next.pts - frame.pts).min(u64::from(u32::MAX)) as u32,
                None => fallback,
            };
            if duration > 0 {
                fallback = duration;
            }
            samples.push(VideoSample {
                data: mp4::to_length_prefixed(&frame.data),
                duration: duration.max(1),
                sync: frame.keyframe,
            });
        }
        // The last picture's duration was a guess (the one before it); the others are exact.
        let video = VideoInput {
            sps: params.0.clone(),
            pps: params.1.clone(),
            info,
            samples,
        };
        let duration_ms = video
            .samples
            .iter()
            .map(|sample| u64::from(sample.duration))
            .sum::<u64>()
            * 1000
            / u64::from(mp4::VIDEO_TIMESCALE);
        let end_of_clip = first_arrival + duration_ms.max(last_arrival - first_arrival);
        let mut audio = Vec::new();
        for (name, buf, channels) in [
            ("application", &mut inner.application, 2u8),
            ("voice", &mut inner.voice, 1u8),
        ] {
            let packets = buf.ring.cut(first_arrival, end_of_clip);
            if packets.is_empty() {
                continue;
            }
            let pre_skip = match buf.encoder.as_mut() {
                Some(encoder) => encoder.pre_skip(),
                None => 312,
            };
            audio.push(AudioInput {
                name: name.to_owned(),
                channels,
                pre_skip,
                packets: continuous(&packets, first_arrival, &TrackEncoder::silence(channels)),
            });
        }
        // The thumbnail's input: the parameter sets, then the first keyframe, so it decodes on its own.
        let mut first_keyframe = Vec::new();
        for nal in [&params.0, &params.1] {
            first_keyframe.extend_from_slice(&[0, 0, 0, 1]);
            first_keyframe.extend_from_slice(nal);
        }
        first_keyframe.extend_from_slice(&frames[0].data);
        Ok(Snapshot {
            source,
            start_ms: first_arrival,
            end_ms: end_of_clip,
            duration_ms,
            video,
            audio,
            first_keyframe,
        })
    }
}

impl Inner {
    /// Frees the pictures and the audio but keeps the source, as when clipping is turned off during a stream.
    fn free_media(&mut self) {
        self.video.clear();
        self.application.clear();
        self.voice.clear();
    }
}

/// Packets as one run of 20 ms packets from `start_ms`: silence packets fill the stretches in which the source gave
/// nothing (a muted microphone, a quiet application), so the track stays in step with the video.
fn continuous(packets: &[AudioPacket], start_ms: u64, silence: &[u8]) -> Vec<Vec<u8>> {
    let mut out = Vec::with_capacity(packets.len());
    let mut expected = start_ms as i64;
    for packet in packets {
        let gap = packet.start_ms as i64 - expected;
        if gap >= GAP_MS {
            let missing = (gap + PACKET_MS as i64 / 2) / PACKET_MS as i64;
            for _ in 0..missing {
                out.push(silence.to_vec());
            }
            expected += missing * PACKET_MS as i64;
        } else if packet.start_ms as i64 + PACKET_MS as i64 <= expected {
            // Entirely before where this track already is.
            continue;
        }
        out.push(packet.data.clone());
        expected += PACKET_MS as i64;
    }
    out
}

/// What a stream's sender feeds. Cheap to clone; every method returns at once and does nothing while clipping is off or
/// the stream it belongs to has ended.
#[derive(Clone)]
pub struct Tap {
    recorder: &'static Recorder,
    generation: u64,
}

impl Tap {
    /// One encoded picture of the stream, as the encoder made it.
    pub fn push_video(&self, frame: &dc_media::EncodedFrame) {
        self.push_video_at(frame, now_ms());
    }

    /// [`Tap::push_video`] with the arrival time given (a replay or a test).
    pub fn push_video_at(&self, frame: &dc_media::EncodedFrame, now: u64) {
        if !self.recorder.recording(self.generation) {
            return;
        }
        let mut inner = self.recorder.lock();
        if self.recorder.live.load(Ordering::Acquire) != self.generation {
            return;
        }
        // A restart of the capture starts a new group: only a keyframe may open the buffer.
        if inner.video.is_empty() && !frame.keyframe {
            return;
        }
        inner
            .video
            .push(now, frame.timestamp, frame.keyframe, frame.data.clone());
        let (length, max) = (inner.length_ms, inner.max_bytes);
        inner.video.evict(now, length, max);
    }

    /// Interleaved samples at 48 kHz that were captured just now: stereo for the stream audio, mono for the voice.
    pub fn push_audio(&self, track: Track, samples: &[f32]) {
        self.push_audio_at(track, samples, now_ms());
    }

    /// [`Tap::push_audio`] with the capture time given (a replay or a test).
    pub fn push_audio_at(&self, track: Track, samples: &[f32], now: u64) {
        if !self.recorder.recording(self.generation) {
            return;
        }
        let mut inner = self.recorder.lock();
        if self.recorder.live.load(Ordering::Acquire) != self.generation {
            return;
        }
        inner.track(track).push(samples, now);
        let keep = inner.length_ms + AUDIO_SLACK_MS;
        inner.track(track).ring.evict(now, keep);
    }

    /// Whether the stream this tap belongs to is still the live one.
    pub fn is_live(&self) -> bool {
        self.recorder.live.load(Ordering::Acquire) == self.generation
    }

    /// The stream ended.
    pub fn finish(&self) {
        self.recorder.detach(self.generation);
    }
}

/// Frees the buffer when the stream's task ends, however it ends.
pub struct TapGuard(pub Tap);

impl Drop for TapGuard {
    fn drop(&mut self) {
        self.0.finish();
    }
}

/// A cut window, ready to be written.
pub struct Snapshot {
    pub source: SourceInfo,
    /// The steady-clock times the window covers.
    pub start_ms: u64,
    pub end_ms: u64,
    /// Length of the video in milliseconds: the clip's `length`.
    pub duration_ms: u64,
    pub video: VideoInput,
    pub audio: Vec<AudioInput>,
    /// The first picture (a keyframe, Annex B, with the parameter sets in front), for the thumbnail.
    pub first_keyframe: Vec<u8>,
}

impl Snapshot {
    /// Writes the clip to `path` (its folder is created) through a temporary file next to it, so a failed save never
    /// leaves a half file under the clip's name. `meta.length` is set to the clip's length. Returns the size in bytes.
    pub fn write(&self, path: &Path, meta: &mut ClipMetadata) -> io::Result<u64> {
        meta.length = self.duration_ms;
        if let Some(folder) = path.parent() {
            fs::create_dir_all(folder)?;
        }
        let part = path.with_extension("mp4.part");
        let result = (|| {
            let file = fs::File::create(&part)?;
            let mut writer = BufWriter::new(file);
            let size = mp4::write(
                &mut writer,
                &self.video,
                &self.audio,
                meta.to_json().as_bytes(),
            )?;
            use std::io::Write;
            writer.flush()?;
            writer
                .into_inner()
                .map_err(|e| e.into_error())?
                .sync_all()?;
            fs::rename(&part, path)?;
            Ok(size)
        })();
        if result.is_err()
            && let Err(error) = fs::remove_file(&part)
        {
            warn!("could not remove the unfinished clip: {error}");
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn packet(start_ms: u64, tag: u8) -> AudioPacket {
        AudioPacket {
            start_ms,
            data: vec![tag],
        }
    }

    #[test]
    fn gaps_become_silence_and_jitter_does_not() {
        let silence = [0xAA];
        let packets = [packet(1_000, 1), packet(1_021, 2), packet(1_100, 3)];
        let out = continuous(&packets, 1_000, &silence);
        // 1000, 1021 (jitter, no fill), then a 59 ms gap from the expected 1040: three silences, then the packet.
        let tags: Vec<u8> = out.iter().map(|p| p[0]).collect();
        assert_eq!(tags, vec![1, 2, 0xAA, 0xAA, 0xAA, 3]);
    }

    #[test]
    fn audio_that_starts_late_is_padded_at_the_front() {
        let out = continuous(&[packet(1_100, 9)], 1_000, &[0xAA]);
        assert_eq!(out.len(), 6);
        assert_eq!(out[5], vec![9]);
    }

    #[test]
    fn a_snapshot_needs_a_source_a_picture_and_a_keyframe() {
        let recorder = standalone();
        assert_eq!(recorder.snapshot().err(), Some(SnapshotError::NotEnabled));
        recorder.configure(true, 30_000);
        assert_eq!(recorder.snapshot().err(), Some(SnapshotError::NoSource));
        recorder.announce_source(SourceInfo {
            name: "Game".into(),
            application_id: None,
        });
        let tap = recorder.attach();
        assert!(recorder.has_source());
        assert_eq!(recorder.snapshot().err(), Some(SnapshotError::NotReady));
        // A delta picture alone does not open the buffer.
        tap.push_video(&dc_media::EncodedFrame {
            data: vec![0, 0, 0, 1, 0x41, 1],
            keyframe: false,
            timestamp: 0,
        });
        assert_eq!(recorder.snapshot().err(), Some(SnapshotError::NotReady));
        tap.finish();
        assert!(!recorder.has_source());
        assert_eq!(recorder.held(), (0, 0, 0));
    }

    #[test]
    fn turning_clipping_off_frees_what_was_buffered() {
        let recorder = standalone();
        recorder.configure(true, 30_000);
        recorder.announce_source(SourceInfo {
            name: "Game".into(),
            application_id: None,
        });
        let tap = recorder.attach();
        tap.push_video(&dc_media::EncodedFrame {
            data: vec![0, 0, 0, 1, 0x65, 1, 2, 3],
            keyframe: true,
            timestamp: 0,
        });
        tap.push_audio(Track::Application, &vec![0.1; 1920 * 3]);
        let (video, application, _) = recorder.held();
        assert!(video > 0 && application > 0);
        recorder.configure(false, 30_000);
        assert_eq!(recorder.held(), (0, 0, 0));
        // And the tap records nothing while it is off.
        tap.push_video(&dc_media::EncodedFrame {
            data: vec![0, 0, 0, 1, 0x65, 1],
            keyframe: true,
            timestamp: 9,
        });
        assert_eq!(recorder.held(), (0, 0, 0));
    }
}
