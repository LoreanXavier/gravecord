//! Room echo removal: online weighted prediction error (WPE) dereverberation, single channel.
//!
//! Not in the official client: gravecord's "Reduce Room Echo" switch, under Echo Cancellation. Echo
//! cancellation only takes out what the speakers played; this takes out the room's own reverb on the voice, which
//! makes a speaker in a bare room sound far away.
//!
//! Written from the papers, not from any client's code: WPE is Yoshioka and Nakatani, "Generalization of
//! multi-channel linear prediction methods for blind MIMO impulse response shortening" (IEEE TASLP 2012); the online
//! form with a recursive least squares filter per frequency is Caroselli, Shafran, Narayanan and Rose, "Adaptive
//! multichannel dereverberation for automatic speech recognition" (Interspeech 2017).
//!
//! In each STFT bin, the late reverb of frame `t` is predicted from frames `t - DELAY - k` (`k < TAPS`) with a filter
//! `g`, and subtracted: `d = x - gᴴ x̃`. The filter follows RLS weighted by the inverse of the frame's power, which is
//! what makes the prediction keep the direct sound (speech is not predictable that far back) and take the tail.
//! `DELAY` frames are left alone, so the early reflections that make a voice sound whole stay.
//!
//! Frames are 40 ms with a 10 ms hop and a square-root Hann window on both sides, so the output is 30 ms later than
//! the input. The papers' settings run 30-60 ms windows at three-quarter overlap; at 20 ms and half overlap the
//! per-bin prediction held too little of the room (about 1-3 dB off the tail in the test room, against 3-10 dB now).
//! Only bins up to 8 kHz are processed (`PROCESSED_BINS`).
use std::sync::Arc;

use realfft::num_complex::Complex32;
use realfft::{ComplexToReal, RealFftPlanner, RealToComplex};

/// The 10 ms block the worker hands over.
pub(crate) const HOP: usize = 480;
/// Frames overlap this many times: the window is this many hops.
const OVERLAP: usize = 4;
const WINDOW: usize = OVERLAP * HOP;
/// How much later the output is than the input: a window less its newest hop.
pub(crate) const LATENCY: usize = WINDOW - HOP;
/// Bins up to 8 kHz are dereverberated, which holds nearly all of a voice's energy; real rooms take the treble
/// down faster anyway, and leaving the rest keeps the filter's cost (TAPS² per bin) small.
const PROCESSED_BINS: usize = 8_000 * WINDOW / 48_000 + 1;
/// Frames between a frame and the first one used to predict it: no overlap with the frame itself, so 40 ms of early
/// reflections stay.
const DELAY: usize = 4;
/// Frames the prediction looks at: 400 ms of reverb tail. 20 left the tail past 300 ms almost untouched.
const TAPS: usize = 40;
/// The RLS forgetting factor: about two seconds of memory at a 10 ms hop.
const FORGET: f32 = 0.995;
/// The weighting power `λ` follows the frame's power over a few frames (`SMOOTH` of the last estimate stays)...
const SMOOTH: f32 = 0.5;
/// ...and never drops below this share of the bin's long-run power, so a pause does not weigh a frame of silence
/// thousands of times more than speech and swing the filter.
const RELATIVE_FLOOR: f32 = 0.01;
/// How slowly the bin's long-run power follows: about ten seconds.
const LONG_RUN: f32 = 0.999;
/// The most a bin is turned down: 20 dB, so a wrong prediction cannot carve holes into the voice.
const MIN_GAIN: f32 = 0.1;
/// An absolute floor for `λ`, for digital silence.
const POWER_FLOOR: f32 = 1e-10;
/// The inverse correlation matrix starts as this times the identity.
const START: f32 = 1.0;

/// One bin's filter and the RLS state behind it.
#[derive(Clone)]
struct Bin {
    /// The prediction filter `g`.
    filter: [Complex32; TAPS],
    /// `R⁻¹`, row by row.
    inverse: Box<[[Complex32; TAPS]; TAPS]>,
    /// The smoothed power behind `λ`, and the long-run power that floors it.
    level: f32,
    long_run: f32,
}

impl Bin {
    fn new() -> Self {
        let mut inverse = Box::new([[Complex32::new(0.0, 0.0); TAPS]; TAPS]);
        for (index, row) in inverse.iter_mut().enumerate() {
            row[index] = Complex32::new(START, 0.0);
        }
        Self {
            filter: [Complex32::new(0.0, 0.0); TAPS],
            inverse,
            level: 0.0,
            long_run: 0.0,
        }
    }

    /// `λ` for this frame: the frame's power smoothed over the last few, floored by the bin's long-run power.
    fn weight(&mut self, power: f32) -> f32 {
        self.level = SMOOTH * self.level + (1.0 - SMOOTH) * power;
        self.long_run = LONG_RUN * self.long_run + (1.0 - LONG_RUN) * power;
        self.level
            .max(RELATIVE_FLOOR * self.long_run)
            .max(POWER_FLOOR)
    }

    /// Takes the predicted reverb out of `observed` given the delayed `past` frames of this bin, and learns from it.
    fn step(&mut self, observed: Complex32, past: &[Complex32; TAPS]) -> Complex32 {
        let power = self.weight(observed.norm_sqr());
        // The a priori estimate: what the filter so far leaves.
        let predicted: Complex32 = self
            .filter
            .iter()
            .zip(past)
            .map(|(g, x)| g.conj() * x)
            .sum();
        let desired = observed - predicted;
        // k = R⁻¹ x̃ / (α λ + x̃ᴴ R⁻¹ x̃)
        let mut projected = [Complex32::new(0.0, 0.0); TAPS];
        for (row, out) in self.inverse.iter().zip(projected.iter_mut()) {
            *out = row.iter().zip(past).map(|(r, x)| r * x).sum();
        }
        let quadratic: Complex32 = past.iter().zip(&projected).map(|(x, p)| x.conj() * p).sum();
        let denominator = FORGET * power + quadratic.re;
        if !denominator.is_finite() || denominator <= 0.0 {
            self.restart();
            return observed;
        }
        let gain: [Complex32; TAPS] = std::array::from_fn(|index| projected[index] / denominator);
        // R⁻¹ ← (R⁻¹ − k x̃ᴴ R⁻¹) / α; x̃ᴴ R⁻¹ is the conjugate of R⁻¹ x̃, R⁻¹ being Hermitian.
        for (row_index, row) in self.inverse.iter_mut().enumerate() {
            for (column, cell) in row.iter_mut().enumerate() {
                *cell = (*cell - gain[row_index] * projected[column].conj()) / FORGET;
            }
        }
        // g ← g + k d*
        for (g, k) in self.filter.iter_mut().zip(&gain) {
            *g += k * desired.conj();
        }
        if self
            .filter
            .iter()
            .any(|g| !g.re.is_finite() || !g.im.is_finite())
        {
            self.restart();
            return observed;
        }
        desired
    }

    /// Back to a fresh filter, keeping what is known of the bin's power.
    fn restart(&mut self) {
        let (level, long_run) = (self.level, self.long_run);
        *self = Self::new();
        self.level = level;
        self.long_run = long_run;
    }
}

/// The filter only ever turns a bin down: the output keeps the observed bin's phase at a gain of `|d| / |x|`, at most
/// one and at least `MIN_GAIN`. Subtracting the prediction outright let a tail the filter expected but the room did
/// not make come out as new sound after each sentence, which in a live call sounded like added reverb.
fn clamp_to(observed: Complex32, desired: Complex32) -> Complex32 {
    let level = observed.norm();
    if level <= f32::EPSILON {
        return observed;
    }
    observed * (desired.norm() / level).clamp(MIN_GAIN, 1.0)
}

pub(crate) struct Dereverb {
    forward: Arc<dyn RealToComplex<f32>>,
    inverse: Arc<dyn ComplexToReal<f32>>,
    window: [f32; WINDOW],
    /// The last window of input.
    input: [f32; WINDOW],
    /// The overlapping output of the frames so far, past the hop that is ready.
    tail: [f32; LATENCY],
    /// Each bin's last `DELAY + TAPS` spectra, newest first.
    history: Vec<[Complex32; DELAY + TAPS]>,
    bins: Vec<Bin>,
    spectrum: Vec<Complex32>,
    scratch: Vec<f32>,
}

impl Dereverb {
    pub fn new() -> Self {
        let mut planner = RealFftPlanner::<f32>::new();
        let forward = planner.plan_fft_forward(WINDOW);
        let inverse = planner.plan_fft_inverse(WINDOW);
        let spectrum = forward.make_output_vec();
        // Periodic Hann, square-rooted: analysis times synthesis sums to `OVERLAP / 2` over the overlapping frames.
        let window = std::array::from_fn(|index| {
            let phase = std::f32::consts::TAU * index as f32 / WINDOW as f32;
            (0.5 - 0.5 * phase.cos()).sqrt()
        });
        Self {
            forward,
            inverse,
            window,
            input: [0.0; WINDOW],
            tail: [0.0; LATENCY],
            history: vec![[Complex32::new(0.0, 0.0); DELAY + TAPS]; PROCESSED_BINS],
            bins: vec![Bin::new(); PROCESSED_BINS],
            spectrum,
            scratch: vec![0.0; WINDOW],
        }
    }

    /// Takes one 10 ms block in and gives back the block `LATENCY` before it, dereverberated.
    pub fn block(&mut self, block: &mut [f32; HOP]) {
        self.input.copy_within(HOP.., 0);
        self.input[LATENCY..].copy_from_slice(block);
        for ((out, sample), weight) in self.scratch.iter_mut().zip(&self.input).zip(&self.window) {
            *out = sample * weight;
        }
        if self
            .forward
            .process(&mut self.scratch, &mut self.spectrum)
            .is_err()
        {
            return;
        }
        for ((value, history), bin) in self
            .spectrum
            .iter_mut()
            .zip(self.history.iter_mut())
            .zip(self.bins.iter_mut())
        {
            let observed = *value;
            let past: [Complex32; TAPS] = std::array::from_fn(|index| history[DELAY - 1 + index]);
            *value = clamp_to(observed, bin.step(observed, &past));
            history.copy_within(..DELAY + TAPS - 1, 1);
            history[0] = observed;
        }
        // The DC and Nyquist bins must stay real for the inverse transform.
        if let Some(first) = self.spectrum.first_mut() {
            first.im = 0.0;
        }
        if let Some(last) = self.spectrum.last_mut() {
            last.im = 0.0;
        }
        if self
            .inverse
            .process(&mut self.spectrum, &mut self.scratch)
            .is_err()
        {
            return;
        }
        let scale = 2.0 / (WINDOW * OVERLAP) as f32;
        for (sample, weight) in self.scratch.iter_mut().zip(&self.window) {
            *sample *= weight * scale;
        }
        for (out, (tail, sample)) in block
            .iter_mut()
            .zip(self.tail.iter().zip(&self.scratch[..HOP]))
        {
            *out = tail + sample;
        }
        // What overlaps the next hops: the rest of the tail, plus this frame's later part.
        self.tail.copy_within(HOP.., 0);
        self.tail[LATENCY - HOP..].fill(0.0);
        for (tail, sample) in self.tail.iter_mut().zip(&self.scratch[HOP..]) {
            *tail += sample;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Noise(u32);
    impl Noise {
        fn next(&mut self) -> f32 {
            self.0 = self.0.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (self.0 >> 8) as f32 / (1u32 << 24) as f32 - 0.5
        }
    }

    fn run(dereverb: &mut Dereverb, signal: &[f32]) -> Vec<f32> {
        let mut out = Vec::with_capacity(signal.len());
        for chunk in signal.as_chunks::<HOP>().0 {
            let mut block = *chunk;
            dereverb.block(&mut block);
            out.extend_from_slice(&block);
        }
        out
    }

    #[test]
    fn a_signal_without_reverb_comes_back_whole_a_window_later() {
        let mut noise = Noise(3);
        // Short bursts of noise with silence between: nothing a 20 ms delay can predict.
        let signal: Vec<f32> = (0..48_000 * 4)
            .map(|index| {
                if (index / 4800) % 3 == 0 {
                    noise.next() * 0.3
                } else {
                    0.0
                }
            })
            .collect();
        let out = run(&mut Dereverb::new(), &signal);
        let (mut error, mut energy) = (0.0, 0.0);
        for index in 48_000..signal.len() - LATENCY {
            error += (out[index + LATENCY] - signal[index]).powi(2);
            energy += signal[index].powi(2);
        }
        // Dry speech-like input keeps at least 90 % of itself.
        assert!(error < energy * 0.1, "changed {error} of {energy}");
        // Nothing comes out of the silences after each burst: a tail the filter expects is never added as sound.
        let mut gaps = 0.0f32;
        for index in 48_000..signal.len() - LATENCY {
            // The silence that starts a window after a burst ends, for the rest of the gap.
            let phase = index % 14_400;
            if (4_800 + 1_920..12_000).contains(&phase) {
                gaps += out[index + LATENCY].powi(2);
            }
        }
        assert!(
            gaps < energy * 1e-6,
            "{gaps} came out of silence, of {energy}"
        );
    }

    #[test]
    fn the_reverb_tail_after_a_burst_is_turned_down() {
        let mut noise = Noise(9);
        // A room: the direct sound, then a diffuse tail decaying 60 dB in 0.6 s, as loud in all as the direct sound
        // (a direct-to-reverberant ratio of 0 dB, a microphone near the mouth in a bare room). The tail's taps are
        // uniform noise of standard deviation 0.0289 times `level`; 0.076 makes their energy sum to one.
        let length = 48_000 * 6 / 10;
        let decay = (-6.9 / length as f32).exp();
        let mut room = vec![0.0f32; length];
        room[0] = 1.0;
        let mut level = 0.076;
        for tap in room.iter_mut().skip(240) {
            *tap = noise.next() * level;
            level *= decay;
        }
        // 100 ms bursts every 700 ms, so each tail is heard alone, of noise falling off above 1 kHz as a voice's
        // long-run spectrum does (two one-pole low-passes).
        let (mut first, mut second) = (0.0f32, 0.0f32);
        let pole = (-std::f32::consts::TAU * 1_000.0 / 48_000.0).exp();
        let dry: Vec<f32> = (0..48_000 * 12)
            .map(|index| {
                let white = if index % 33_600 < 4_800 {
                    noise.next()
                } else {
                    0.0
                };
                first = pole * first + (1.0 - pole) * white;
                second = pole * second + (1.0 - pole) * first;
                second
            })
            .collect();
        let mut wet = vec![0.0f32; dry.len()];
        for (index, sample) in dry.iter().enumerate() {
            if *sample == 0.0 {
                continue;
            }
            for (tap_index, tap) in room.iter().enumerate() {
                if let Some(out) = wet.get_mut(index + tap_index) {
                    *out += sample * tap;
                }
            }
        }
        let out = run(&mut Dereverb::new(), &wet);
        // The tails of the last half, past the first 50 ms after each burst, once the filter has learned the room.
        let (mut before, mut after) = (0.0, 0.0);
        for index in dry.len() / 2..dry.len() - LATENCY {
            let phase = index % 33_600;
            if (4_800 + 2_400..33_000).contains(&phase) {
                before += wet[index].powi(2);
                after += out[index + LATENCY].powi(2);
            }
        }
        assert!(after < before * 0.5, "reverb tail kept {after} of {before}");
        // And no 10 ms of a tail comes out louder than it went in: a prediction the room did not bear out must not be
        // added as new sound (in a live call that was a reverb-like smear after each sentence).
        let mut louder = 0;
        for start in (dry.len() / 2..dry.len() - LATENCY - HOP).step_by(HOP) {
            let phase = start % 33_600;
            // Up to a window before the next burst, whose onset the bins' gains spread back within their window
            // (about 60 dB under the burst).
            if !(4_800..33_600 - 1_920).contains(&phase) {
                continue;
            }
            let into: f32 = wet[start..start + HOP].iter().map(|v| v * v).sum();
            let came: f32 = out[start + LATENCY..start + LATENCY + HOP]
                .iter()
                .map(|v| v * v)
                .sum();
            // 1 dB of room for the windows' overlap.
            if came > into * 1.26 && came > 1e-9 {
                louder += 1;
            }
        }
        assert_eq!(
            louder, 0,
            "{louder} blocks of the tails came out louder than they went in"
        );
    }
}

#[cfg(test)]
mod cost {
    use super::*;

    /// Not a check: how much of a core the filter takes, to read with `--ignored --nocapture`.
    #[test]
    #[ignore]
    fn real_time_share() {
        let mut dereverb = Dereverb::new();
        let mut seed = 5u32;
        let mut block = [0.0f32; HOP];
        let blocks = 1_000;
        let start = std::time::Instant::now();
        for _ in 0..blocks {
            for sample in &mut block {
                seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                *sample = (seed >> 8) as f32 / (1u32 << 24) as f32 - 0.5;
            }
            dereverb.block(&mut block);
        }
        let spent = start.elapsed().as_secs_f64();
        eprintln!(
            "COST {blocks} blocks of 10 ms in {spent:.3} s: {:.1} % of a core",
            spent / (blocks as f64 * 0.01) * 100.0
        );
    }
}
