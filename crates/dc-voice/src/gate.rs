//! What the microphone sends: the input level, voice activity, and push to talk (the official `setInputMode` options:
//! `vadThreshold`, `vadAutoThreshold`, `vadUseKrisp`, `vadKrispActivationThreshold`, `pttReleaseDelay`).
//!
//! The level and the manual threshold follow the only voice detector the official client has in readable code, the
//! browser engine's (module 510098, class `F`): every 20 ms, the last 512 samples go through a Blackman window and an
//! FFT, magnitudes over N are smoothed in time by 0.1 (Web Audio's `AnalyserNode`, `fftSize` 512,
//! `smoothingTimeConstant` 0.1), and the level is the loudest bin from the fifth up that is below 0 dB. A frame is
//! speech when the level is over the threshold; the gate stays open while any of the last ten frames was, and ten
//! frames after (`speakingHistory`, `silentFrames`).
//!
//! Automatic sensitivity is native and opaque in the desktop client (`vadAutoThreshold: VERY_AGGRESSIVE`, or Krisp's
//! voice model when Advanced Voice Activity is on, with an activation probability of 0.5, 0.8 while Automatic Gain
//! Control is on). Here it is WebRTC's RNN voice detector (`sonora-agc2`, BSD-3-Clause), whose speech probability is
//! held against that same activation threshold, with the same ten-frame hold.
use std::sync::Arc;
use std::time::{Duration, Instant};

use realfft::num_complex::Complex32;
use realfft::{RealFftPlanner, RealToComplex};
use sonora_agc2::vad_wrapper::VoiceActivityDetectorWrapper;

use crate::Frame;
use crate::process::ProcessingPrefs;

const FFT: usize = 512;
const BINS: usize = FFT / 2;
/// The first bin `getFloatFrequencyData`'s caller reads.
const FIRST_BIN: usize = 4;
const SMOOTHING: f32 = 0.1;
/// `speakingHistory` and `silenceThreshold`: ten 20 ms frames each.
const HISTORY: usize = 10;
/// The level shown when nothing is measured: the official meters' floor.
pub const FLOOR_DB: f32 = -100.0;

/// The official `vadKrispActivationThreshold`, and what it becomes while Automatic Gain Control is on (754712 `t3`).
const ACTIVATION: f32 = 0.5;
const ACTIVATION_WITH_AGC: f32 = 0.8;

pub(crate) struct Gate {
    fft: Arc<dyn RealToComplex<f32>>,
    window: [f32; FFT],
    scratch: Vec<f32>,
    spectrum: Vec<Complex32>,
    smoothed: [f32; BINS],
    vad: VoiceActivityDetectorWrapper,
    history: [bool; HISTORY],
    index: usize,
    counter: usize,
    silent: usize,
    /// When push to talk was let go, for its release delay.
    released_at: Option<Instant>,
    held_before: bool,
}

/// The browser engine's level: the loudest bin from `FIRST_BIN` up that is below 0 dB, or `-inf` when none is.
fn loudest(bins: &[f32]) -> f32 {
    bins.iter()
        .skip(FIRST_BIN)
        .copied()
        .filter(|db| *db < 0.0)
        .fold(f32::NEG_INFINITY, f32::max)
}

impl Gate {
    pub fn new() -> Self {
        let fft = RealFftPlanner::<f32>::new().plan_fft_forward(FFT);
        // Web Audio's Blackman window (alpha 0.16).
        let window = std::array::from_fn(|index| {
            let phase = std::f32::consts::TAU * index as f32 / FFT as f32;
            0.42 - 0.5 * phase.cos() + 0.08 * (2.0 * phase).cos()
        });
        Self {
            scratch: vec![0.0; FFT],
            spectrum: fft.make_output_vec(),
            fft,
            window,
            smoothed: [0.0; BINS],
            vad: VoiceActivityDetectorWrapper::new(sonora_simd::detect_backend(), 48_000),
            history: [false; HISTORY],
            index: 0,
            counter: 0,
            silent: HISTORY,
            released_at: None,
            held_before: false,
        }
    }

    /// `getFloatFrequencyData` over the frame's last 512 samples, then the level.
    fn level(&mut self, frame: &Frame) -> f32 {
        let tail = &frame[frame.len() - FFT..];
        for ((out, sample), weight) in self.scratch.iter_mut().zip(tail).zip(&self.window) {
            *out = sample * weight;
        }
        if self
            .fft
            .process(&mut self.scratch, &mut self.spectrum)
            .is_err()
        {
            return f32::NEG_INFINITY;
        }
        let mut decibels = [0.0f32; BINS];
        for ((smooth, bin), db) in self
            .smoothed
            .iter_mut()
            .zip(&self.spectrum)
            .zip(decibels.iter_mut())
        {
            *smooth = SMOOTHING * *smooth + (1.0 - SMOOTHING) * bin.norm() / FFT as f32;
            *db = 20.0 * smooth.log10();
        }
        loudest(&decibels)
    }

    /// The RNN detector's speech probability for the frame: the higher of its two 10 ms halves.
    fn speech(&mut self, frame: &Frame) -> f32 {
        frame
            .as_chunks::<480>()
            .0
            .iter()
            .map(|block| {
                // WebRTC's float audio runs at FloatS16 range [-32768.0, 32767.0].
                let scaled: [f32; 480] = block.map(|sample| {
                    if sample.is_finite() {
                        (sample * 32_768.0).clamp(-32_768.0, 32_767.0)
                    } else {
                        0.0
                    }
                });
                self.vad.analyze(&scaled)
            })
            .fold(0.0, f32::max)
    }

    /// One frame through the detector: the level for the meters, and whether the frame goes out.
    pub fn update(&mut self, frame: &Frame, prefs: &ProcessingPrefs) -> (f32, bool) {
        let level = self.level(frame);
        let shown = if level.is_finite() {
            level.max(FLOOR_DB)
        } else {
            FLOOR_DB
        };
        let over = if prefs.auto_threshold() {
            let activation = if prefs.automatic_gain_control() && prefs.advanced_voice_activity() {
                ACTIVATION_WITH_AGC
            } else {
                ACTIVATION
            };
            self.speech(frame) >= activation
        } else {
            level > prefs.threshold_db()
        };
        // `F.update`: a ring of the last ten verdicts, then ten frames of hold.
        if self.history[self.index] {
            self.counter -= 1;
        }
        self.history[self.index] = over;
        if over {
            self.counter += 1;
        }
        self.index = (self.index + 1) % HISTORY;
        if self.counter > 0 {
            self.silent = 0;
        } else {
            self.silent += 1;
        }
        let voice = self.counter > 0 || self.silent < HISTORY;
        if prefs.push_to_talk() {
            return (shown, self.push_to_talk(prefs));
        }
        (shown, voice)
    }

    /// Push to talk: open while the keybind is held and for the release delay after (`pttReleaseDelay`).
    fn push_to_talk(&mut self, prefs: &ProcessingPrefs) -> bool {
        let held = prefs.push_to_talk_held();
        if self.held_before && !held {
            self.released_at = Some(Instant::now());
        }
        self.held_before = held;
        if held {
            self.released_at = None;
            return true;
        }
        self.released_at.is_some_and(|at| {
            at.elapsed() < Duration::from_millis(u64::from(prefs.push_to_talk_delay_ms()))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(level: f32) -> Frame {
        std::array::from_fn(|index| (index as f32 * 0.13).sin() * level)
    }

    #[test]
    fn the_level_is_the_loudest_bin_below_zero_from_the_fifth_up() {
        assert_eq!(loudest(&[-1.0, -2.0, -3.0, -4.0, -50.0, -20.0, 3.0]), -20.0);
        assert_eq!(loudest(&[-10.0; 4]), f32::NEG_INFINITY);
    }

    #[test]
    fn a_manual_threshold_opens_on_sound_and_holds_ten_frames_after() {
        let prefs = ProcessingPrefs::new();
        prefs.set_auto_threshold(false);
        prefs.set_threshold_db(-60.0);
        let mut gate = Gate::new();
        let (quiet_level, open) = gate.update(&[0.0; 960], &prefs);
        assert_eq!(quiet_level, FLOOR_DB);
        assert!(!open);
        let (level, open) = gate.update(&tone(0.3), &prefs);
        assert!(level > -60.0 && level < 0.0, "level {level}");
        assert!(open);
        // The time smoothing (0.1) leaves the first silent frame 20 dB down, still over -60 dB: that frame counts as
        // speech too. Then nine frames until it leaves the history of ten, and nine of the ten silent frames.
        let mut opened = 0;
        for _ in 0..30 {
            if gate.update(&[0.0; 960], &prefs).1 {
                opened += 1;
            }
        }
        assert_eq!(opened, 19);
    }

    #[test]
    fn push_to_talk_follows_the_key_and_its_release_delay() {
        let prefs = ProcessingPrefs::new();
        prefs.set_push_to_talk(true);
        prefs.set_push_to_talk_delay_ms(0);
        let mut gate = Gate::new();
        assert!(!gate.update(&tone(0.3), &prefs).1, "loud but not held");
        prefs.set_push_to_talk_held(true);
        assert!(gate.update(&[0.0; 960], &prefs).1, "held, even silent");
        prefs.set_push_to_talk_held(false);
        assert!(!gate.update(&tone(0.3), &prefs).1, "released, no delay");
    }
}
