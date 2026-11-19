//! Microphone processing on the audio worker, never in device callbacks: echo cancellation, then the room's echo
//! (gravecord's Reduce Room Echo, `dereverb.rs`), then noise suppression.
//!
//! The official client's Voice & Video settings choose `setEchoCancellation` and, in Noise Suppression, `KRISP`
//! (`setNoiseCancellation`), `STANDARD` (`setNoiseSuppression`) or `NONE` (module 158935). Krisp's SDK only runs in an
//! application Discord signed (`discord_krisp.node`), so Krisp is DeepFilterNet 3 here, under the same choice
//! (BLOCKED.md). Standard is WebRTC's noise suppressor and echo cancellation is WebRTC's AEC3, both through Sonora, a
//! pure Rust port of WebRTC's audio processing (BSD-3-Clause).
//!
//! The Sonora part is adapted from Serein `crates/discord-voice/src/audio/echo.rs` (`Echo::new`, `configure`,
//! `render`, `capture`), MIT OR Apache-2.0: 10 ms blocks, the speakers' frames as the echo reference, a zero stream
//! delay. DeepFilterNet is libDF (`deep_filter`, MIT OR Apache-2.0) with its bundled DeepFilterNet 3 model, loaded and
//! run on a thread of its own (`Deep`).
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU8, AtomicU32, Ordering},
    mpsc,
};
use std::time::{Duration, Instant};

use sonora::{
    AudioProcessing, Config, StreamConfig,
    config::{
        AdaptiveDigital, EchoCanceller, FixedDigital, GainController2, NoiseSuppression,
        NoiseSuppressionLevel,
    },
};
use tracing::{debug, warn};

use crate::Frame;

/// 10 ms at 48 kHz: WebRTC's block, and DeepFilterNet 3's hop.
const BLOCK: usize = 480;

/// The Noise Suppression choice.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Suppression {
    /// `KRISP` in the official client; DeepFilterNet 3 here, and named so.
    DeepFilterNet,
    /// `STANDARD`: WebRTC's noise suppressor.
    Standard,
    /// `NONE`.
    None,
}

impl Suppression {
    fn to_u8(self) -> u8 {
        match self {
            Suppression::DeepFilterNet => 0,
            Suppression::Standard => 1,
            Suppression::None => 2,
        }
    }
    fn from_u8(value: u8) -> Self {
        match value {
            0 => Suppression::DeepFilterNet,
            1 => Suppression::Standard,
            _ => Suppression::None,
        }
    }
}

/// The two Voice & Video choices the worker reads before every frame.
pub struct ProcessingPrefs {
    suppression: AtomicU8,
    echo: AtomicBool,
    room_echo: AtomicBool,
    /// `automaticGainControl`.
    agc: AtomicBool,
    /// `mode`: push to talk rather than voice activity.
    push_to_talk: AtomicBool,
    /// The push to talk keybind is down.
    push_to_talk_held: AtomicBool,
    /// `modeOptions.delay`, ms.
    push_to_talk_delay: AtomicU32,
    /// `modeOptions.threshold`, dB, as f32 bits.
    threshold: AtomicU32,
    /// `modeOptions.autoThreshold`.
    auto_threshold: AtomicBool,
    /// `modeOptions.vadUseKrisp`: Advanced Voice Activity.
    advanced_voice_activity: AtomicBool,
    /// What the gate last measured, for the meters: the level in dB as f32 bits, and whether it is open.
    level: AtomicU32,
    voice: AtomicBool,
}

impl ProcessingPrefs {
    /// The official defaults: Krisp and echo cancellation on.
    pub(crate) const fn new() -> Self {
        Self {
            suppression: AtomicU8::new(0),
            echo: AtomicBool::new(true),
            room_echo: AtomicBool::new(false),
            agc: AtomicBool::new(true),
            push_to_talk: AtomicBool::new(false),
            push_to_talk_held: AtomicBool::new(false),
            push_to_talk_delay: AtomicU32::new(20),
            // -60.0f32: the official default threshold.
            threshold: AtomicU32::new(0xC270_0000),
            auto_threshold: AtomicBool::new(true),
            advanced_voice_activity: AtomicBool::new(true),
            // -100.0f32: the meters' floor.
            level: AtomicU32::new(0xC2C8_0000),
            voice: AtomicBool::new(false),
        }
    }
    pub fn suppression(&self) -> Suppression {
        Suppression::from_u8(self.suppression.load(Ordering::Acquire))
    }
    pub fn set_suppression(&self, suppression: Suppression) {
        self.suppression
            .store(suppression.to_u8(), Ordering::Release);
    }
    pub fn echo_cancellation(&self) -> bool {
        self.echo.load(Ordering::Acquire)
    }
    pub fn set_echo_cancellation(&self, on: bool) {
        self.echo.store(on, Ordering::Release);
    }
    /// gravecord's Reduce Room Echo, off unless turned on.
    pub fn room_echo(&self) -> bool {
        self.room_echo.load(Ordering::Acquire)
    }
    pub fn set_room_echo(&self, on: bool) {
        self.room_echo.store(on, Ordering::Release);
    }
    pub fn automatic_gain_control(&self) -> bool {
        self.agc.load(Ordering::Acquire)
    }
    pub fn set_automatic_gain_control(&self, on: bool) {
        self.agc.store(on, Ordering::Release);
    }
    pub fn push_to_talk(&self) -> bool {
        self.push_to_talk.load(Ordering::Acquire)
    }
    pub fn set_push_to_talk(&self, on: bool) {
        self.push_to_talk.store(on, Ordering::Release);
    }
    pub fn push_to_talk_held(&self) -> bool {
        self.push_to_talk_held.load(Ordering::Acquire)
    }
    /// The keybind went down or up (`PUSH_TO_TALK_STATE_CHANGE`).
    pub fn set_push_to_talk_held(&self, held: bool) {
        self.push_to_talk_held.store(held, Ordering::Release);
    }
    pub fn push_to_talk_delay_ms(&self) -> u32 {
        self.push_to_talk_delay.load(Ordering::Acquire)
    }
    pub fn set_push_to_talk_delay_ms(&self, ms: u32) {
        self.push_to_talk_delay
            .store(ms.min(2_000), Ordering::Release);
    }
    pub fn threshold_db(&self) -> f32 {
        f32::from_bits(self.threshold.load(Ordering::Acquire))
    }
    pub fn set_threshold_db(&self, db: f32) {
        self.threshold
            .store(db.clamp(-100.0, 0.0).to_bits(), Ordering::Release);
    }
    pub fn auto_threshold(&self) -> bool {
        self.auto_threshold.load(Ordering::Acquire)
    }
    pub fn set_auto_threshold(&self, on: bool) {
        self.auto_threshold.store(on, Ordering::Release);
    }
    pub fn advanced_voice_activity(&self) -> bool {
        self.advanced_voice_activity.load(Ordering::Acquire)
    }
    pub fn set_advanced_voice_activity(&self, on: bool) {
        self.advanced_voice_activity.store(on, Ordering::Release);
    }
    /// The microphone's level in dB (-100 when silent) and whether it is being sent: the official `VoiceActivity`
    /// event's level and `VOICE` flag.
    pub fn meter(&self) -> (f32, bool) {
        (
            f32::from_bits(self.level.load(Ordering::Acquire)),
            self.voice.load(Ordering::Acquire),
        )
    }
    pub(crate) fn set_meter(&self, level: f32, voice: bool) {
        self.level.store(level.to_bits(), Ordering::Release);
        self.voice.store(voice, Ordering::Release);
    }
}

/// What the processor is set to, compared each frame with the prefs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Settings {
    suppression: Suppression,
    echo: bool,
    room_echo: bool,
    agc: bool,
}

/// What the worker sends DeepFilterNet's thread. A frame goes by value through the bounded channel; boxing it would
/// allocate fifty times a second.
#[allow(clippy::large_enum_variant)]
enum ToDeep {
    Frame(u64, Frame),
    /// A gap in the microphone: forget what came before.
    Reset,
}

/// DeepFilterNet 3 on a thread of its own. libDF's model holds `Rc`s, so it is built, run and dropped on that
/// thread; the worker hands it each frame and takes the frame back. It loads there too, so choosing it never stalls
/// the call; until it is ready the microphone goes through unchanged.
struct Deep {
    send: mpsc::SyncSender<ToDeep>,
    receive: mpsc::Receiver<(u64, Frame)>,
    ready: Arc<AtomicBool>,
    failed: Arc<AtomicBool>,
    sequence: u64,
    /// The model's output delay in samples, set once it has loaded.
    delay: Arc<AtomicU32>,
    /// The captured frames delayed by as much as the model delays its output. A frame the model does not hand back
    /// in time (loading, a full queue, a late answer) goes out from here, so the voice keeps one timing: sent as
    /// captured, it jumped about 30 ms ahead of the model's frames around it and that much speech played twice.
    line: DelayLine,
}

/// A fixed delay over frames: what went in `len` samples ago comes out.
struct DelayLine {
    samples: std::collections::VecDeque<f32>,
    len: usize,
}

impl DelayLine {
    fn new(len: usize) -> Self {
        Self {
            samples: std::iter::repeat_n(0.0, len).collect(),
            len,
        }
    }

    /// Changes the delay, keeping the newest history.
    fn set_len(&mut self, len: usize) {
        while self.samples.len() > len {
            self.samples.pop_front();
        }
        while self.samples.len() < len {
            self.samples.push_front(0.0);
        }
        self.len = len;
    }

    /// Takes `frame` in and gives back the frame from `len` samples before.
    fn shift(&mut self, frame: &Frame) -> Frame {
        self.samples.extend(frame.iter().copied());
        let mut out = [0.0; 960];
        for value in &mut out {
            *value = self.samples.pop_front().unwrap_or(0.0);
        }
        out
    }
}

/// DeepFilterNet 3's output delay at 48 kHz until the loaded model tells its own: the STFT's
/// `fft_size - hop_size` (960 - 480) and `lookahead` (2) hops of 480 (`libDF/src/tract.rs` `DfTract`).
const DEEP_DEFAULT_DELAY: usize = 1440;

/// How long the worker waits for a frame back: the model takes a few milliseconds of its 20.
const DEEP_WAIT: Duration = Duration::from_millis(15);

impl Deep {
    fn start() -> Option<Self> {
        let (send, frames) = mpsc::sync_channel::<ToDeep>(4);
        let (results, receive) = mpsc::sync_channel(4);
        let ready = Arc::new(AtomicBool::new(false));
        let failed = Arc::new(AtomicBool::new(false));
        let delay = Arc::new(AtomicU32::new(DEEP_DEFAULT_DELAY as u32));
        let (thread_ready, thread_failed) = (ready.clone(), failed.clone());
        let thread_delay = delay.clone();
        std::thread::Builder::new()
            .name("dc-voice-denoise".into())
            .spawn(move || {
                let params = df::tract::RuntimeParams::default_with_ch(1);
                // The bundled DeepFilterNet 3 model (`default-model`); a bad archive panics here, which drops the
                // channels and leaves the microphone as it is.
                let mut model =
                    match df::tract::DfTract::new(df::tract::DfParams::default(), &params) {
                        Ok(model) => model,
                        Err(error) => {
                            warn!("DeepFilterNet could not load: {error}");
                            thread_failed.store(true, Ordering::Release);
                            return;
                        }
                    };
                let model_delay = model.fft_size.saturating_sub(model.hop_size)
                    + model.lookahead * model.hop_size;
                thread_delay.store(model_delay as u32, Ordering::Release);
                thread_ready.store(true, Ordering::Release);
                // The worker dropping its end ends the thread.
                while let Ok(message) = frames.recv() {
                    match message {
                        ToDeep::Reset => {
                            if let Err(error) = model.init() {
                                warn!("DeepFilterNet could not restart: {error}");
                            }
                        }
                        ToDeep::Frame(sequence, mut frame) => {
                            for block in frame.as_chunks_mut::<BLOCK>().0 {
                                let mut output = [0.0; BLOCK];
                                let noisy = ndarray::ArrayView2::from_shape((1, BLOCK), &*block);
                                let enhanced =
                                    ndarray::ArrayViewMut2::from_shape((1, BLOCK), &mut output[..]);
                                if let (Ok(noisy), Ok(enhanced)) = (noisy, enhanced) {
                                    match model.process(noisy, enhanced) {
                                        Ok(_) => block.copy_from_slice(&output),
                                        Err(error) => {
                                            debug!(
                                                "DeepFilterNet could not process a block: {error}"
                                            )
                                        }
                                    }
                                }
                            }
                            if results.send((sequence, frame)).is_err() {
                                return;
                            }
                        }
                    }
                }
            })
            .ok()?;
        Some(Self {
            send,
            receive,
            ready,
            failed,
            sequence: 0,
            delay,
            line: DelayLine::new(DEEP_DEFAULT_DELAY),
        })
    }

    fn ready(&self) -> bool {
        self.ready.load(Ordering::Acquire) && !self.failed.load(Ordering::Acquire)
    }

    /// The frame through the model, or the captured frame delayed as much as the model's output while the model
    /// loads or falls behind.
    fn process(&mut self, frame: &mut Frame) {
        let delay = self.delay.load(Ordering::Acquire) as usize;
        if delay != self.line.len {
            self.line.set_len(delay);
        }
        let captured = *frame;
        *frame = self.line.shift(&captured);
        if !self.ready() {
            return;
        }
        self.sequence += 1;
        if self
            .send
            .try_send(ToDeep::Frame(self.sequence, captured))
            .is_err()
        {
            return;
        }
        let deadline = Instant::now() + DEEP_WAIT;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            match self.receive.recv_timeout(left) {
                // A frame that came back too late for its turn is dropped.
                Ok((sequence, done)) if sequence == self.sequence => {
                    *frame = done;
                    return;
                }
                Ok(_) => continue,
                Err(_) => {
                    debug!("DeepFilterNet fell behind; a frame went out as captured");
                    return;
                }
            }
        }
    }

    fn reset(&mut self) {
        let _ = self.send.try_send(ToDeep::Reset);
    }
}

pub(crate) struct Processor {
    webrtc: AudioProcessing,
    room: Option<crate::dereverb::Dereverb>,
    deep: Option<Deep>,
    /// WebRTC's AGC2, adaptive digital only, last in the chain.
    gain: Option<AudioProcessing>,
    settings: Option<Settings>,
}

fn webrtc(config: Config) -> AudioProcessing {
    AudioProcessing::builder()
        .config(config)
        .capture_config(StreamConfig::new(48_000, 1))
        .render_config(StreamConfig::new(48_000, 1))
        .build()
}

/// The official `automaticGainControlConfig` when on (754712 `eQ`): AGC2, digital only, 5 dB headroom, at most 50 dB
/// of gain starting at 15, changing at most 6 dB a second, noise kept under -50 dBFS, no fixed gain.
fn gain_config() -> Config {
    Config {
        gain_controller2: Some(GainController2 {
            input_volume_controller: false,
            adaptive_digital: Some(AdaptiveDigital {
                headroom_db: 5.0,
                max_gain_db: 50.0,
                initial_gain_db: 15.0,
                max_gain_change_db_per_second: 6.0,
                max_output_noise_level_dbfs: -50.0,
            }),
            fixed_digital: FixedDigital { gain_db: 0.0 },
        }),
        ..Default::default()
    }
}

fn webrtc_config(settings: Settings) -> Config {
    Config {
        echo_canceller: settings.echo.then_some(EchoCanceller {
            enforce_high_pass_filtering: true,
            transparent_mode: sonora::config::TransparentModeType::Hmm,
        }),
        noise_suppression: (settings.suppression == Suppression::Standard).then(|| {
            NoiseSuppression {
                level: NoiseSuppressionLevel::High,
                ..Default::default()
            }
        }),
        ..Default::default()
    }
}

fn clean(sample: &mut f32) {
    *sample = if sample.is_finite() {
        sample.clamp(-1.0, 1.0)
    } else {
        0.0
    };
}

impl Processor {
    pub fn new() -> Self {
        Self {
            webrtc: webrtc(Config::default()),
            room: None,
            deep: None,
            gain: None,
            settings: None,
        }
    }

    /// Follows the prefs: WebRTC's config when echo or Standard changes, DeepFilterNet loaded for Krisp and
    /// dropped when another choice is made.
    fn follow(&mut self, prefs: &ProcessingPrefs) {
        let settings = Settings {
            suppression: prefs.suppression(),
            echo: prefs.echo_cancellation(),
            room_echo: prefs.room_echo(),
            agc: prefs.automatic_gain_control(),
        };
        if self.settings == Some(settings) {
            return;
        }
        debug!(?settings, "voice microphone processing");
        let webrtc_changed = self.settings.is_none_or(|old| {
            old.echo != settings.echo
                || (old.suppression == Suppression::Standard)
                    != (settings.suppression == Suppression::Standard)
        });
        if webrtc_changed {
            self.webrtc.apply_config(webrtc_config(settings));
        }
        if !settings.agc {
            self.gain = None;
        } else if self.gain.is_none() {
            self.gain = Some(webrtc(gain_config()));
        }
        if !settings.room_echo {
            self.room = None;
        } else if self.room.is_none() {
            self.room = Some(crate::dereverb::Dereverb::new());
        }
        if settings.suppression == Suppression::DeepFilterNet {
            if self.deep.is_none() {
                self.deep = Deep::start();
            }
        } else {
            self.deep = None;
        }
        self.settings = Some(settings);
    }

    /// A gap in the microphone (mute, a reopened device): the processors forget what came before.
    pub fn reset(&mut self) {
        let config = StreamConfig::new(48_000, 1);
        self.webrtc.initialize(config, config, config, config);
        if self.room.is_some() {
            self.room = Some(crate::dereverb::Dereverb::new());
        }
        if let Some(deep) = &mut self.deep {
            deep.reset();
        }
    }

    /// What the speakers are about to play, the echo canceller's reference.
    pub fn render(&mut self, frame: &Frame, prefs: &ProcessingPrefs) {
        self.follow(prefs);
        if !self.settings.is_some_and(|settings| settings.echo) {
            return;
        }
        let mut output = [0.0; BLOCK];
        for block in frame.as_chunks::<BLOCK>().0 {
            if self
                .webrtc
                .process_render_f32(&[block], &mut [&mut output])
                .is_err()
            {
                debug!("echo cancellation could not take the speakers' audio");
                return;
            }
        }
    }

    /// The microphone's frame, processed in place.
    pub fn capture(&mut self, frame: &mut Frame, prefs: &ProcessingPrefs) {
        self.follow(prefs);
        let Some(settings) = self.settings else {
            return;
        };
        frame.iter_mut().for_each(clean);
        let use_webrtc = settings.echo || settings.suppression == Suppression::Standard;
        for block in frame.as_chunks_mut::<BLOCK>().0 {
            if use_webrtc {
                let mut output = [0.0; BLOCK];
                let processed = self
                    .webrtc
                    .process_capture_f32(&[&block[..]], &mut [&mut output]);
                match processed {
                    Ok(()) => block.copy_from_slice(&output),
                    Err(_) => debug!("WebRTC could not process the microphone"),
                }
            }
            if let Some(room) = &mut self.room {
                room.block(block);
            }
        }
        if let Some(deep) = &mut self.deep {
            deep.process(frame);
        }
        if let Some(gain) = &mut self.gain {
            for block in frame.as_chunks_mut::<BLOCK>().0 {
                let mut output = [0.0; BLOCK];
                if gain
                    .process_capture_f32(&[&block[..]], &mut [&mut output])
                    .is_ok()
                {
                    block.copy_from_slice(&output);
                }
            }
        }
        frame.iter_mut().for_each(clean);
    }

    /// DeepFilterNet is loaded and running (tests, and the log).
    #[cfg(test)]
    pub(crate) fn deep_ready(&self) -> bool {
        self.deep.as_ref().is_some_and(Deep::ready)
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_delay_line_gives_back_what_went_in_its_length_before() {
        let mut line = super::DelayLine::new(1440);
        let frame = |n: usize| -> crate::Frame { std::array::from_fn(|i| (n * 960 + i) as f32) };
        // 1440 samples of silence first, then sample k of the input comes out 1440 samples later.
        let first = line.shift(&frame(0));
        assert!(first.iter().all(|value| *value == 0.0));
        let second = line.shift(&frame(1));
        assert!(second[..480].iter().all(|value| *value == 0.0));
        assert_eq!(second[480], 0.0);
        assert_eq!(second[481], 1.0);
        let third = line.shift(&frame(2));
        assert_eq!(third[0], 480.0);
        // A shorter delay keeps the newest history and stays continuous.
        line.set_len(960);
        let fourth = line.shift(&frame(3));
        assert_eq!(fourth[0], 1920.0);
    }

    use super::*;

    fn energy(frames: &[Frame]) -> f32 {
        frames
            .iter()
            .flat_map(|frame| frame.iter())
            .map(|sample| sample * sample)
            .sum()
    }

    /// A tone the speakers play comes back into the microphone quieter and later; with echo cancellation on,
    /// once AEC3 has adapted, far less of it is sent than without.
    #[test]
    fn echo_cancellation_takes_out_what_the_speakers_played() {
        let prefs = ProcessingPrefs::new();
        prefs.set_suppression(Suppression::None);
        let run = |echo: bool| {
            prefs.set_echo_cancellation(echo);
            let mut processor = Processor::new();
            let mut seed = 1u32;
            let mut noise = move || {
                seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                (seed >> 8) as f32 / (1u32 << 24) as f32 - 0.5
            };
            let mut played: Vec<f32> = Vec::new();
            let mut sent = Vec::new();
            for index in 0..300 {
                let mut speaker = [0.0f32; 960];
                for sample in &mut speaker {
                    *sample = noise() * 0.5;
                }
                processor.render(&speaker, &prefs);
                played.extend_from_slice(&speaker);
                // 10 ms of delay and half the level, as a room would send it back.
                let mut microphone = [0.0f32; 960];
                for (offset, sample) in microphone.iter_mut().enumerate() {
                    let at = played.len() as isize - 960 + offset as isize - 480;
                    if at >= 0 {
                        *sample = played[at as usize] * 0.5;
                    }
                }
                processor.capture(&mut microphone, &prefs);
                if index >= 200 {
                    sent.push(microphone);
                }
            }
            energy(&sent)
        };
        let without = run(false);
        let with = run(true);
        assert!(
            with < without * 0.1,
            "echo left: {with} of {without} without cancellation"
        );
    }

    /// Krisp's stand-in loads off the worker, then turns steady noise down.
    #[test]
    fn deepfilternet_loads_and_turns_noise_down() {
        let prefs = ProcessingPrefs::new();
        prefs.set_echo_cancellation(false);
        prefs.set_suppression(Suppression::DeepFilterNet);
        let mut processor = Processor::new();
        let mut seed = 7u32;
        let mut noise = move || {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            ((seed >> 8) as f32 / (1u32 << 24) as f32 - 0.5) * 0.05
        };
        let deadline = Instant::now() + Duration::from_secs(120);
        while !processor.deep_ready() {
            let mut frame = [0.0f32; 960];
            processor.capture(&mut frame, &prefs);
            let deep = processor.deep.as_ref().expect("Krisp starts DeepFilterNet");
            assert!(
                !deep.failed.load(Ordering::Acquire),
                "DeepFilterNet failed to load"
            );
            assert!(
                Instant::now() < deadline,
                "DeepFilterNet did not load in time"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
        let mut input = Vec::new();
        let mut output = Vec::new();
        for index in 0..150 {
            let mut frame = [0.0f32; 960];
            for sample in &mut frame {
                *sample = noise();
            }
            let before = frame;
            processor.capture(&mut frame, &prefs);
            // Frames arrive every 20 ms in a call; fed faster, a loaded machine's model thread falls behind and
            // frames pass through unfiltered, which made this test fail only under load.
            std::thread::sleep(Duration::from_millis(20));
            if index >= 50 {
                input.push(before);
                output.push(frame);
            }
        }
        let (before, after) = (energy(&input), energy(&output));
        assert!(after < before * 0.1, "noise left: {after} of {before}");
    }

    /// Standard is WebRTC's suppressor: steady noise comes out quieter once it has learned it.
    #[test]
    fn standard_suppression_turns_steady_noise_down() {
        let prefs = ProcessingPrefs::new();
        prefs.set_echo_cancellation(false);
        prefs.set_suppression(Suppression::Standard);
        let mut processor = Processor::new();
        let mut seed = 11u32;
        let mut noise = move || {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            ((seed >> 8) as f32 / (1u32 << 24) as f32 - 0.5) * 0.05
        };
        let (mut input, mut output) = (Vec::new(), Vec::new());
        for index in 0..300 {
            let mut frame = [0.0f32; 960];
            for sample in &mut frame {
                *sample = noise();
            }
            let before = frame;
            processor.capture(&mut frame, &prefs);
            if index >= 150 {
                input.push(before);
                output.push(frame);
            }
        }
        let (before, after) = (energy(&input), energy(&output));
        assert!(after < before * 0.5, "noise left: {after} of {before}");
    }

    #[test]
    fn choosing_another_suppression_drops_deepfilternet() {
        let prefs = ProcessingPrefs::new();
        let mut processor = Processor::new();
        let mut frame = [0.0f32; 960];
        processor.capture(&mut frame, &prefs);
        assert!(processor.deep.is_some());
        prefs.set_suppression(Suppression::Standard);
        processor.capture(&mut frame, &prefs);
        assert!(processor.deep.is_none());
    }
}
