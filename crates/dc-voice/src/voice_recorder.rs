//! Recording a voice message: processed microphone frames are encoded to Ogg Opus as they arrive and measured for the
//! waveform the message carries.
//!
//! The official client asks the native voice module to record (`startLocalAudioRecording`, module 198620) and gets a
//! sample for the live waveform from every `VoiceActivity` event, the engine's level in decibels scaled to a byte
//! (`handleVoiceActivity`). The native module's event rate is not in the snapshot; here a sample is the level of
//! every 40 ms (two frames), 25 a second, so a message longer than ten seconds is averaged down to the 256 values
//! a message may carry (module 841817, `el`).
//!
//! The time of a recording is the audio that was recorded, 20 ms to a frame: with a microphone that is the clock, and
//! the capture harness's synthetic source is driven by the harness so frames show exact times.

use crate::audio::voice_input::Microphone;
use crate::ogg_opus::{FRAME_SAMPLES, Writer};
use crate::voice_wave::{self, MAX_RECORDING_SECS, MIN_RECORDING_SECS};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
use std::time::Duration;
use tracing::{debug, info};

/// Frames per level sample.
const FRAMES_PER_LEVEL: usize = 2;

/// A finished recording, ready to upload.
#[derive(Debug)]
pub struct Recording {
    /// The Ogg Opus file.
    pub file: Vec<u8>,
    /// `duration_secs` of the attachment.
    pub duration_secs: f64,
    /// `waveform` of the attachment: base64 of at most 256 bytes.
    pub waveform: String,
    /// The level samples before they were cut to 256, for the recorder's own preview.
    pub levels: Vec<u8>,
}

impl Recording {
    /// Shorter than half a second a recording is thrown away (module 841817).
    pub fn too_short(&self) -> bool {
        self.duration_secs < MIN_RECORDING_SECS
    }
}

struct Core {
    writer: Option<Writer>,
    levels: Vec<u8>,
    window: Vec<f32>,
    frames: u64,
    failure: Option<String>,
}

impl Core {
    fn new() -> Result<Self, &'static str> {
        Ok(Self {
            writer: Some(Writer::new()?),
            levels: Vec::new(),
            window: Vec::new(),
            frames: 0,
            failure: None,
        })
    }

    fn push(&mut self, frame: &[f32]) {
        if let Some(writer) = &mut self.writer {
            writer.push(frame);
        }
        self.window.extend_from_slice(frame);
        self.frames += 1;
        if (self.frames as usize).is_multiple_of(FRAMES_PER_LEVEL) {
            self.levels
                .push(voice_wave::level_byte(voice_wave::level_db(&self.window)));
            self.window.clear();
        }
    }

    fn elapsed_ms(&self) -> u64 {
        self.frames * 20
    }

    fn finish(&mut self) -> Option<Recording> {
        let writer = self.writer.take()?;
        let duration_secs = writer.seconds();
        let file = writer.finish();
        let levels = std::mem::take(&mut self.levels);
        Some(Recording {
            waveform: voice_wave::encode_waveform(&levels),
            duration_secs: duration_secs.min(MAX_RECORDING_SECS),
            file,
            levels,
        })
    }
}

/// Where a recording's 20 ms frames come from: the processed microphone, or a script in tests.
trait FrameSource {
    fn next_frame(&mut self) -> Option<crate::Frame>;
    fn failed(&self) -> bool;
}

impl FrameSource for Microphone {
    fn next_frame(&mut self) -> Option<crate::Frame> {
        self.next()
    }

    fn failed(&self) -> bool {
        Microphone::failed(self)
    }
}

/// The recording thread: every frame the source delivers goes through `Core::push`, the same call the synthetic
/// source uses; a source that fails ends the recording with the reason.
fn pump<S: FrameSource>(core: &Mutex<Core>, stop: &AtomicBool, source: &mut S) {
    while !stop.load(Ordering::Acquire) {
        match source.next_frame() {
            Some(frame) => {
                if let Ok(mut core) = core.lock() {
                    core.push(&frame);
                }
            }
            None if source.failed() => {
                if let Ok(mut core) = core.lock() {
                    core.failure = Some("The microphone stopped".to_owned());
                }
                break;
            }
            None => std::thread::sleep(Duration::from_millis(4)),
        }
    }
}

enum Feed {
    Microphone,
    /// Driven by [`Recorder::advance_synthetic`].
    Synthetic,
}

/// A recording in progress.
pub struct Recorder {
    core: Arc<Mutex<Core>>,
    stop: Arc<AtomicBool>,
    feed: Feed,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Recorder {
    /// Opens the microphone chosen in Voice & Video (or the default one) and starts recording.
    pub fn start_microphone() -> Result<Self, &'static str> {
        let recorder = Self::start_source(Microphone::open()?)?;
        info!("voice message recording started on the microphone");
        Ok(recorder)
    }

    /// Records from any frame source on its own thread. The microphone and the tests' scripted source both come
    /// through here, so what a real recording encodes is what the tests encode.
    fn start_source<S: FrameSource + Send + 'static>(mut source: S) -> Result<Self, &'static str> {
        let core = Arc::new(Mutex::new(Core::new()?));
        let stop = Arc::new(AtomicBool::new(false));
        let (worker_core, worker_stop) = (core.clone(), stop.clone());
        let thread = std::thread::Builder::new()
            .name("dc-voice-recorder".into())
            .spawn(move || pump(&worker_core, &worker_stop, &mut source))
            .map_err(|_| "Could not start the recorder")?;
        Ok(Self {
            core,
            stop,
            feed: Feed::Microphone,
            thread: Some(thread),
        })
    }

    /// A recording with no microphone, for capture runs: nothing is opened and nothing is heard. Audio is added by
    /// [`Recorder::advance_synthetic`].
    pub fn start_synthetic() -> Result<Self, &'static str> {
        info!("voice message recording started on the synthetic source");
        Ok(Self {
            core: Arc::new(Mutex::new(Core::new()?)),
            stop: Arc::new(AtomicBool::new(false)),
            feed: Feed::Synthetic,
            thread: None,
        })
    }

    /// Adds `ms` (rounded down to whole 20 ms frames) of the synthetic source: a voice-like tone with a syllable
    /// rhythm, so the waveform has quiet and loud stretches.
    pub fn advance_synthetic(&self, ms: u64) {
        if !matches!(self.feed, Feed::Synthetic) {
            return;
        }
        if let Ok(mut core) = self.core.lock() {
            for _ in 0..ms / 20 {
                let frame = synthetic_frame(core.frames);
                core.push(&frame);
            }
        }
    }

    /// Milliseconds recorded so far.
    pub fn elapsed_ms(&self) -> u64 {
        self.core.lock().map_or(0, |core| core.elapsed_ms())
    }

    /// The last `count` level samples (0 to 255), oldest first, for the live waveform.
    pub fn levels(&self) -> Vec<u8> {
        self.core
            .lock()
            .map_or_else(|_| Vec::new(), |core| core.levels.clone())
    }

    /// Why the recording cannot go on, if it cannot.
    pub fn failure(&self) -> Option<String> {
        self.core.lock().ok().and_then(|core| core.failure.clone())
    }

    /// Ends the recording and returns the file.
    pub fn stop(mut self) -> Result<Recording, &'static str> {
        self.stop.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
        let recording = self
            .core
            .lock()
            .map_err(|_| "The recorder failed")?
            .finish()
            .ok_or("The recording was already finished")?;
        debug!(
            seconds = recording.duration_secs,
            bytes = recording.file.len(),
            samples = recording.levels.len(),
            "voice message recording stopped"
        );
        Ok(recording)
    }

    /// Throws the recording away.
    pub fn cancel(self) {
        self.stop.store(true, Ordering::Release);
        drop(self);
    }
}

impl Drop for Recorder {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// One 20 ms frame of the synthetic source at frame `index`: a 190 Hz tone with a second and third harmonic whose
/// loudness swells and fades with a 1.4 Hz syllable rhythm and a slower phrase, with short pauses.
pub fn synthetic_frame(index: u64) -> [f32; FRAME_SAMPLES] {
    let mut frame = [0f32; FRAME_SAMPLES];
    for (offset, slot) in frame.iter_mut().enumerate() {
        let t = (index as f64 * FRAME_SAMPLES as f64 + offset as f64) / 48_000.;
        let syllable = (0.5 + 0.5 * (std::f64::consts::TAU * 1.4 * t).sin()).powf(1.6);
        let phrase = 0.35 + 0.65 * (0.5 + 0.5 * (std::f64::consts::TAU * 0.23 * t + 1.0).sin());
        let pause = if (t * 0.9).fract() > 0.88 { 0.04 } else { 1. };
        let glide = 190. + 40. * (std::f64::consts::TAU * 0.31 * t).sin();
        let phase = std::f64::consts::TAU * glide * t;
        let voice = phase.sin() + 0.5 * (2. * phase).sin() + 0.25 * (3. * phase).sin();
        *slot = (0.45 * syllable * phrase * pause * voice / 1.4) as f32;
    }
    frame
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ogg_opus;

    #[test]
    fn a_captured_source_feeds_the_same_encoder_as_the_synthetic_one() {
        /// Delivers `total` frames of the synthetic voice, then reports no more.
        struct Scripted {
            next: u64,
            total: u64,
        }
        impl FrameSource for Scripted {
            fn next_frame(&mut self) -> Option<crate::Frame> {
                if self.next >= self.total {
                    return None;
                }
                let frame = synthetic_frame(self.next);
                self.next += 1;
                Some(frame)
            }
            fn failed(&self) -> bool {
                false
            }
        }
        let total = 300;
        let captured = Recorder::start_source(Scripted { next: 0, total }).unwrap();
        for _ in 0..500 {
            if captured.elapsed_ms() >= total * 20 {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(captured.elapsed_ms(), total * 20, "every frame arrived");
        let captured = captured.stop().unwrap();
        let synthetic = Recorder::start_synthetic().unwrap();
        synthetic.advance_synthetic(total * 20);
        let synthetic = synthetic.stop().unwrap();
        assert_eq!(captured.file, synthetic.file, "the same Ogg Opus bytes");
        assert_eq!(captured.waveform, synthetic.waveform);
        assert_eq!(captured.levels, synthetic.levels);
        assert!((captured.duration_secs - 6.0).abs() < 0.05);
    }

    #[test]
    fn a_source_that_fails_ends_the_recording_with_a_reason() {
        struct Dies;
        impl FrameSource for Dies {
            fn next_frame(&mut self) -> Option<crate::Frame> {
                None
            }
            fn failed(&self) -> bool {
                true
            }
        }
        let recorder = Recorder::start_source(Dies).unwrap();
        for _ in 0..200 {
            if recorder.failure().is_some() {
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(
            recorder.failure().as_deref(),
            Some("The microphone stopped")
        );
        recorder.cancel();
    }

    #[test]
    fn a_synthetic_recording_has_the_length_levels_and_file_a_message_needs() {
        let recorder = Recorder::start_synthetic().expect("recorder");
        recorder.advance_synthetic(3_000);
        assert_eq!(recorder.elapsed_ms(), 3_000);
        // 3 s at 40 ms a sample.
        assert_eq!(recorder.levels().len(), 75);
        let recording = recorder.stop().expect("recording");
        assert!(!recording.too_short());
        assert!((recording.duration_secs - 3.).abs() < 1e-9);
        // The waveform is the levels as they are (75 is under 256), base64.
        let bytes = voice_wave::base64_decode(&recording.waveform).unwrap();
        assert_eq!(bytes, recording.levels);
        assert!(
            bytes.iter().max().unwrap() > &150,
            "the tone has loud stretches"
        );
        assert!(bytes.iter().min().unwrap() < &120, "and quiet ones");
        // The file decodes to the same length.
        let pcm = ogg_opus::decode(&recording.file).expect("decodes");
        assert_eq!(pcm.samples.len(), 144_000);
    }

    #[test]
    fn a_long_recording_carries_256_waveform_bytes() {
        let recorder = Recorder::start_synthetic().expect("recorder");
        recorder.advance_synthetic(20_000);
        let recording = recorder.stop().expect("recording");
        assert_eq!(recording.levels.len(), 500);
        assert_eq!(
            voice_wave::base64_decode(&recording.waveform)
                .unwrap()
                .len(),
            256
        );
    }

    #[test]
    fn a_recording_under_half_a_second_is_too_short() {
        let recorder = Recorder::start_synthetic().expect("recorder");
        recorder.advance_synthetic(400);
        let recording = recorder.stop().expect("recording");
        assert!(recording.too_short());
        let recorder = Recorder::start_synthetic().expect("recorder");
        recorder.advance_synthetic(500);
        assert!(!recorder.stop().expect("recording").too_short());
    }

    #[test]
    fn the_levels_are_the_decibel_bytes_of_the_input() {
        let mut core = Core::new().expect("core");
        // A frame at 0.1 amplitude square wave has a level of -20 dB: byte 204.
        let frame = [0.1f32; FRAME_SAMPLES];
        core.push(&frame);
        assert!(core.levels.is_empty(), "a sample needs two frames");
        core.push(&frame);
        assert_eq!(core.levels, vec![voice_wave::level_byte(-20.)]);
        assert_eq!(core.levels[0], 204);
        core.push(&[0.; FRAME_SAMPLES]);
        core.push(&[0.; FRAME_SAMPLES]);
        assert_eq!(core.levels[1], 0, "silence is the floor");
    }
}
