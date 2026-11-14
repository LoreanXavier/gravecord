//! Offline measurements of the microphone chain with speech-like signals: what survives echo cancellation, noise
//! suppression, gain control and the voice gate while the other side talks at the same time, and what Mic Test plays
//! back. Nothing opens a device; every signal is synthesised here and run through the worker's own processors in the
//! order the audio worker runs them (`audio.rs` `run`): the microphone frame first, then what the speakers took.
//!
//! The talkers are voiced harmonics under a moving three-formant envelope with syllables, pauses and a little breath
//! noise, which the processors' speech detectors treat as speech; numbers are printed with `--nocapture`.
use crate::Frame;
use crate::process::{ProcessingPrefs, Processor, Suppression};
use std::time::{Duration, Instant};

pub(crate) const RATE: f32 = 48_000.0;
pub(crate) const FRAME: usize = 960;

/// Vowel formants (Hz), a classic table of adult averages.
const VOWELS: [[f32; 3]; 6] = [
    [730.0, 1090.0, 2440.0],
    [270.0, 2290.0, 3010.0],
    [530.0, 1840.0, 2480.0],
    [570.0, 840.0, 2410.0],
    [300.0, 870.0, 2240.0],
    [660.0, 1720.0, 2410.0],
];
const BANDWIDTHS: [f32; 3] = [90.0, 110.0, 160.0];

pub(crate) struct Rng(u32);
impl Rng {
    pub(crate) fn next(&mut self) -> f32 {
        self.0 = self.0.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        (self.0 >> 8) as f32 / (1u32 << 24) as f32
    }
    fn range(&mut self, low: f32, high: f32) -> f32 {
        low + (high - low) * self.next()
    }
}

/// A talker: `seconds` of syllables at fundamental `f0`, active RMS near `level`, silence (exact zeros) between
/// phrases. Returns the samples and, per 20 ms frame, whether the talker is voicing in it.
pub(crate) fn talker(seconds: f32, f0: f32, level: f32, seed: u32) -> (Vec<f32>, Vec<bool>) {
    let total = (seconds * RATE) as usize;
    let mut out = vec![0.0f32; total];
    let mut rng = Rng(seed);
    let mut at = (rng.range(0.05, 0.3) * RATE) as usize;
    let mut syllables = 0;
    let mut phases = [0.0f32; 64];
    while at < total {
        let length = (rng.range(0.14, 0.32) * RATE) as usize;
        let from = VOWELS[(rng.next() * 6.0) as usize % 6];
        let to = VOWELS[(rng.next() * 6.0) as usize % 6];
        let pitch = f0 * rng.range(0.85, 1.2);
        let glide = rng.range(-0.15, 0.15);
        for n in 0..length.min(total - at) {
            let t = n as f32 / length as f32;
            let envelope = (t * length as f32 / (0.02 * RATE)).min(1.0)
                * ((1.0 - t) * length as f32 / (0.035 * RATE)).min(1.0);
            let formants: [f32; 3] = std::array::from_fn(|i| from[i] + (to[i] - from[i]) * t);
            let f = pitch * (1.0 + glide * t) * (1.0 + 0.01 * (n as f32 * 0.0013).sin());
            let mut sample = 0.0;
            for (k, phase) in phases.iter_mut().enumerate() {
                let harmonic = f * (k + 1) as f32;
                if harmonic > 7_000.0 {
                    break;
                }
                let mut gain = 0.0;
                for (formant, bandwidth) in formants.iter().zip(BANDWIDTHS) {
                    let x = (harmonic - formant) / bandwidth;
                    gain += 1.0 / (1.0 + x * x);
                }
                *phase = (*phase + std::f32::consts::TAU * harmonic / RATE) % std::f32::consts::TAU;
                sample += gain / ((k + 1) as f32).sqrt() * phase.sin();
            }
            sample += (rng.next() - 0.5) * 0.08;
            out[at + n] = sample * envelope;
        }
        at += length;
        syllables += 1;
        at += if syllables % 6 == 0 {
            (rng.range(0.4, 0.9) * RATE) as usize
        } else {
            (rng.range(0.04, 0.16) * RATE) as usize
        };
    }
    let frames = total / FRAME;
    let active: Vec<bool> = (0..frames)
        .map(|f| out[f * FRAME..(f + 1) * FRAME].iter().any(|s| *s != 0.0))
        .collect();
    let active_energy: f32 = (0..frames)
        .filter(|f| active[*f])
        .flat_map(|f| out[f * FRAME..(f + 1) * FRAME].iter())
        .map(|s| s * s)
        .sum();
    let active_samples = active.iter().filter(|a| **a).count() * FRAME;
    let rms = (active_energy / active_samples.max(1) as f32).sqrt();
    out.iter_mut().for_each(|s| *s *= level / rms.max(1e-9));
    (out, active)
}

/// Where the speakers' sound comes back into the microphone.
#[derive(Clone, Copy)]
pub(crate) struct Echo {
    pub gain: f32,
    pub delay_ms: f32,
}

#[derive(Clone, Copy)]
pub(crate) struct Setup {
    pub echo: bool,
    pub suppression: Suppression,
    pub agc: bool,
    pub auto_threshold: bool,
    pub advanced_voice_activity: bool,
}

impl Setup {
    /// The official defaults (Krisp, echo cancellation, gain control, automatic sensitivity, Advanced Voice Activity).
    pub(crate) fn defaults() -> Self {
        Self {
            echo: true,
            suppression: Suppression::DeepFilterNet,
            agc: true,
            auto_threshold: true,
            advanced_voice_activity: true,
        }
    }
    pub(crate) fn prefs(self) -> ProcessingPrefs {
        let prefs = ProcessingPrefs::new();
        prefs.set_echo_cancellation(self.echo);
        prefs.set_suppression(self.suppression);
        prefs.set_automatic_gain_control(self.agc);
        prefs.set_auto_threshold(self.auto_threshold);
        prefs.set_advanced_voice_activity(self.advanced_voice_activity);
        prefs
    }
}

/// A processor whose DeepFilterNet (when chosen) has loaded.
pub(crate) fn ready_processor(prefs: &ProcessingPrefs) -> Processor {
    let mut processor = Processor::new();
    let mut frame = [0.0f32; FRAME];
    processor.capture(&mut frame, prefs);
    if prefs.suppression() == Suppression::DeepFilterNet {
        let deadline = Instant::now() + Duration::from_secs(120);
        while !processor.deep_ready() {
            assert!(Instant::now() < deadline, "DeepFilterNet did not load");
            std::thread::sleep(Duration::from_millis(20));
        }
    }
    processor
}

pub(crate) fn frame_at(signal: &[f32], index: usize) -> Frame {
    let mut frame = [0.0f32; FRAME];
    let start = index * FRAME;
    if start < signal.len() {
        let end = (start + FRAME).min(signal.len());
        frame[..end - start].copy_from_slice(&signal[start..end]);
    }
    frame
}

pub(crate) fn energy(samples: &[f32]) -> f64 {
    samples.iter().map(|s| f64::from(*s) * f64::from(*s)).sum()
}

pub(crate) fn db(ratio: f64) -> f64 {
    10.0 * ratio.max(1e-12).log10()
}

/// The lag (samples, 0..=max) at which `output` best matches `reference`, by normalised cross-correlation.
pub(crate) fn best_lag(reference: &[f32], output: &[f32], max: usize) -> usize {
    let span = reference.len().min(output.len()).saturating_sub(max);
    let mut best = (0usize, f64::MIN);
    for lag in (0..=max).step_by(8) {
        let mut dot = 0.0f64;
        let mut norm = 0.0f64;
        for n in (0..span).step_by(2) {
            dot += f64::from(reference[n]) * f64::from(output[n + lag]);
            norm += f64::from(output[n + lag]) * f64::from(output[n + lag]);
        }
        let score = dot / norm.sqrt().max(1e-9);
        if score > best.1 {
            best = (lag, score);
        }
    }
    best.0
}

/// What reached the far side (or the Mic Test's speakers) against the talker's clean voice.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Outcome {
    /// Output energy over input energy where the near talker voices, the output aligned by its processing delay, dB.
    pub retention_db: f64,
    /// The same over what the gate let out (closed frames count as silence), dB.
    pub sent_retention_db: f64,
    /// Share of the near talker's frames that came out more than 10 dB down.
    pub dropped: f64,
    /// Share of the near talker's frames the gate let out.
    pub sent: f64,
    /// What came out while only the far talker talked, against the near talker's level, dB (echo left).
    pub echo_left_db: f64,
}

pub(crate) fn measure(
    near: &[f32],
    near_active: &[bool],
    far_only: &[bool],
    output: &[f32],
    gated: &[f32],
    skip_frames: usize,
) -> Outcome {
    let lag = best_lag(near, output, 2_400);
    let frames = near_active.len().min(output.len().saturating_sub(lag) / FRAME);
    let (mut input, mut out, mut sent_energy, mut far_out) = (0.0, 0.0, 0.0, 0.0);
    let (mut active, mut dropped, mut sent, mut far_frames) = (0usize, 0usize, 0usize, 0usize);
    for (f, &is_near_active) in near_active.iter().enumerate().take(frames).skip(skip_frames) {
        let range = f * FRAME..(f + 1) * FRAME;
        let shifted = f * FRAME + lag..(f + 1) * FRAME + lag;
        if is_near_active {
            let e_in = energy(&near[range.clone()]);
            let e_out = energy(&output[shifted.clone()]);
            let e_sent = energy(&gated[shifted.clone()]);
            input += e_in;
            out += e_out;
            sent_energy += e_sent;
            active += 1;
            if e_out < e_in * 0.1 {
                dropped += 1;
            }
            if e_sent > 0.0 {
                sent += 1;
            }
        } else if far_only.get(f).copied().unwrap_or(false) {
            far_out += energy(&output[shifted]);
            far_frames += 1;
        }
    }
    let near_power = input / active.max(1) as f64;
    Outcome {
        retention_db: db(out / input.max(1e-12)),
        sent_retention_db: db(sent_energy / input.max(1e-12)),
        dropped: dropped as f64 / active.max(1) as f64,
        sent: sent as f64 / active.max(1) as f64,
        echo_left_db: if far_frames == 0 {
            f64::NAN
        } else {
            db(far_out / far_frames as f64 / near_power.max(1e-12))
        },
    }
}

/// One call: the near talker into the microphone (with the far talker's echo when `echo` is set), the far talker
/// through the speakers and the echo canceller's reference. `gaps`: the speakers' reference skips the frames where
/// nobody is heard, as the call's playout tap did. Returns the outcome after a five second warm up.
pub(crate) fn call(setup: Setup, echo: Option<Echo>, far_talks: bool, gaps: bool) -> Outcome {
    let seconds = 30.0;
    let (near, near_active) = talker(seconds, 115.0, 0.05, 3);
    let (far, far_voicing) = if far_talks {
        talker(seconds, 205.0, 0.1, 17)
    } else {
        (vec![0.0; near.len()], vec![false; near.len() / FRAME])
    };
    // The far side's own gate holds ten frames after speech: those frames are heard (as silence), the rest are not.
    let mut far_heard = vec![false; far_voicing.len()];
    let mut hold: usize = 0;
    for (f, voicing) in far_voicing.iter().enumerate() {
        hold = if *voicing { 10 } else { hold.saturating_sub(1) };
        far_heard[f] = hold > 0;
    }
    let prefs = setup.prefs();
    let mut processor = ready_processor(&prefs);
    let mut gate = crate::gate::Gate::new();
    let mut noise = Rng(99);
    let frames = near.len() / FRAME;
    let delay = echo.map_or(0, |e| (e.delay_ms * RATE / 1000.0) as usize);
    let mut output = Vec::with_capacity(near.len());
    let mut gated = Vec::with_capacity(near.len());
    for (f, &heard) in far_heard.iter().enumerate().take(frames) {
        let mut mic = frame_at(&near, f);
        if let Some(echo) = echo {
            for (n, sample) in mic.iter_mut().enumerate() {
                let at = (f * FRAME + n) as isize - delay as isize;
                if at >= 0 {
                    *sample += far[at as usize] * echo.gain;
                }
            }
        }
        for sample in &mut mic {
            *sample += (noise.next() - 0.5) * 0.000_6;
        }
        processor.capture(&mut mic, &prefs);
        let (_, open) = gate.update(&mic, &prefs);
        output.extend_from_slice(&mic);
        if open {
            gated.extend_from_slice(&mic);
        } else {
            gated.extend_from_slice(&[0.0; FRAME]);
        }
        if !gaps || heard {
            processor.render(&frame_at(&far, f), &prefs);
        }
    }
    let far_only: Vec<bool> = (0..frames)
        .map(|f| far_voicing[f] && !near_active[f])
        .collect();
    measure(&near, &near_active, &far_only, &output, &gated, 250)
}

/// Mic Test: the microphone through the processors and the gate, played back `latency` frames later through the
/// speakers, which are the echo canceller's reference when `reference` is set (as the preview worker did). `echo`:
/// how much of the playback the microphone hears again (speakers rather than headphones).
pub(crate) fn mic_test(setup: Setup, reference: bool, latency: usize, echo: f32) -> Outcome {
    let seconds = 30.0;
    let (near, near_active) = talker(seconds, 115.0, 0.05, 3);
    let prefs = setup.prefs();
    let mut processor = ready_processor(&prefs);
    let mut gate = crate::gate::Gate::new();
    let mut noise = Rng(5);
    let frames = near.len() / FRAME;
    let mut played: Vec<Frame> = Vec::new();
    let mut output = Vec::with_capacity(near.len());
    let mut gated = Vec::with_capacity(near.len());
    for f in 0..frames {
        let mut mic = frame_at(&near, f);
        // What the speakers play now comes back one frame later.
        if echo > 0.0
            && f > latency
            && let Some(back) = played.get(f - latency - 1)
        {
            for (sample, from) in mic.iter_mut().zip(back) {
                *sample += from * echo;
            }
        }
        for sample in &mut mic {
            *sample += (noise.next() - 0.5) * 0.000_6;
        }
        processor.capture(&mut mic, &prefs);
        let (_, open) = gate.update(&mic, &prefs);
        output.extend_from_slice(&mic);
        let sent = if open { mic } else { [0.0; FRAME] };
        gated.extend_from_slice(&sent);
        played.push(sent);
        // The worker hands a frame to the speakers (and the reference) `latency` frames after it was captured.
        if reference
            && f >= latency
            && let Some(frame) = played.get(f - latency)
        {
            processor.render(frame, &prefs);
        }
    }
    measure(&near, &near_active, &[], &output, &gated, 250)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn show(name: &str, outcome: Outcome) {
        println!(
            "{name:<58} retention {:>7.2} dB  sent-retention {:>7.2} dB  dropped {:>5.1}%  sent {:>5.1}%  echo-left {:>7.2} dB",
            outcome.retention_db,
            outcome.sent_retention_db,
            outcome.dropped * 100.0,
            outcome.sent * 100.0,
            outcome.echo_left_db
        );
    }

    #[test]
    fn talker_is_speech_like() {
        let (voice, active) = talker(10.0, 120.0, 0.05, 1);
        let share = active.iter().filter(|a| **a).count() as f32 / active.len() as f32;
        assert!(share > 0.4 && share < 0.9, "voiced share {share}");
        let peak = voice.iter().fold(0f32, |p, s| p.max(s.abs()));
        assert!(peak < 1.0, "peak {peak}");
    }

    /// The whole matrix, printed: `cargo test -p dc-voice speech_lab::tests::report -- --nocapture --ignored`.
    #[test]
    #[ignore = "measurement report; prints numbers"]
    fn report() {
        let none = Setup {
            echo: false,
            suppression: Suppression::None,
            agc: false,
            auto_threshold: true,
            advanced_voice_activity: true,
        };
        let aec = Setup { echo: true, ..none };
        let dfn = Setup {
            suppression: Suppression::DeepFilterNet,
            ..none
        };
        let standard = Setup {
            suppression: Suppression::Standard,
            ..none
        };
        let aec_dfn = Setup {
            echo: true,
            suppression: Suppression::DeepFilterNet,
            ..none
        };
        let defaults = Setup::defaults();
        let speakers = Some(Echo {
            gain: 0.3,
            delay_ms: 50.0,
        });
        for (label, gaps) in [("gapped reference", true), ("continuous reference", false)] {
            println!("--- call, {label}");
            show("headphones, near alone, nothing", call(none, None, false, gaps));
            show("headphones, double talk, nothing", call(none, None, true, gaps));
            show("headphones, near alone, AEC", call(aec, None, false, gaps));
            show("headphones, double talk, AEC", call(aec, None, true, gaps));
            show("headphones, near alone, DFN", call(dfn, None, false, gaps));
            show("headphones, double talk, standard NS", call(standard, None, true, gaps));
            show("headphones, double talk, AEC+DFN", call(aec_dfn, None, true, gaps));
            show("headphones, double talk, defaults", call(defaults, None, true, gaps));
            show("speakers, double talk, AEC", call(aec, speakers, true, gaps));
            show("speakers, double talk, defaults", call(defaults, speakers, true, gaps));
            show("speakers, far alone (no near), nothing", call(none, speakers, true, gaps));
        }
        println!("--- mic test");
        for (label, reference) in [("playback as reference", true), ("no reference", false)] {
            show(&format!("headphones, AEC, {label}"), mic_test(aec, reference, 3, 0.0));
            show(&format!("headphones, defaults, {label}"), mic_test(defaults, reference, 3, 0.0));
            show(&format!("speakers, defaults, {label}"), mic_test(defaults, reference, 3, 0.2));
        }
        show("headphones, nothing", mic_test(none, false, 3, 0.0));
    }
}
