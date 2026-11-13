//! Changing the buffer's length without changing pitch: libwebrtc's `TimeStretch` (`time_stretch.{h,cc}`),
//! `Accelerate` (`accelerate.cc`) and `PreemptiveExpand` (`preemptive_expand.cc`), with the helpers they call
//! (`DspHelper::DownsampleTo4kHz`, `PeakDetection`, `ParabolicFit` in `dsp_helper.cc`, `AudioVector::CrossFade` in
//! `audio_vector.cc`, `WebRtcSpl_DownsampleFast`). The pitch period is found on a 4 kHz copy of the first channel, then
//! one period is cross-faded out (accelerate) or in (preemptive expand) at the 15 ms point.
//!
//! The routines run on `f32` scaled to 16-bit sample units, so the energy constants keep their meaning; correlations
//! that were Q14 are plain ratios here. The background noise estimate (`BackgroundNoise`) is not ported: the speech
//! test uses its uninitialised fallback energy, 75000, always.

/// `kCorrelationLen`, `kMinLag`, `kMaxLag`, `kDownsampledLen` (`time_stretch.h`), at 4 kHz.
const CORRELATION_LEN: usize = 50;
const MIN_LAG: usize = 10;
const MAX_LAG: usize = 60;
const DOWNSAMPLED_LEN: usize = CORRELATION_LEN + MAX_LAG;
/// `kCorrelationThreshold`: 14746 in Q14, 0.9.
const CORRELATION_THRESHOLD: f32 = 14746.0 / 16384.0;
/// The fast-accelerate threshold of `Accelerate::CheckCriteriaAndStretch`: 8192 in Q14.
const FAST_CORRELATION_THRESHOLD: f32 = 8192.0 / 16384.0;
/// `SpeechDetection`'s noise energy while `BackgroundNoise` is uninitialised.
const FALLBACK_BACKGROUND_ENERGY: f32 = 75_000.0;
/// 48 kHz: `fs_mult_` is the rate over 8 kHz.
const FS_MULT: usize = 6;
/// 15 ms at 48 kHz (`fs_mult_ * 120`).
const FS_MULT_120: usize = FS_MULT * 120;
/// `PreemptiveExpand::overlap_samples_`: 5 samples at 8 kHz.
const OVERLAP_SAMPLES: usize = 5 * FS_MULT;
/// `DownsampleTo4kHz` for 48 kHz: a 7-tap Q12 filter, every 12th sample, delay 4 with delay compensation.
const DOWNSAMPLE_COEFFICIENTS: [i32; 7] = [1019, 390, 427, 440, 427, 390, 1019];
const DOWNSAMPLE_FACTOR: usize = 12;
const DOWNSAMPLE_DELAY: usize = 4;
/// `kParabolaCoefficients` and the 48 kHz `fit_index` row of `ParabolicFit`.
const PARABOLA_COEFFICIENTS: [[i32; 3]; 17] = [
    [120, 32, 64],
    [140, 44, 75],
    [150, 50, 80],
    [160, 57, 85],
    [180, 72, 96],
    [200, 89, 107],
    [210, 98, 112],
    [220, 108, 117],
    [240, 128, 128],
    [260, 150, 139],
    [270, 162, 144],
    [280, 174, 149],
    [300, 200, 160],
    [320, 228, 171],
    [330, 242, 176],
    [340, 257, 181],
    [360, 288, 192],
];
const FIT_INDEX_48K: [usize; 13] = [0, 1, 3, 4, 5, 7, 8, 9, 11, 12, 13, 15, 16];

/// `TimeStretch::ReturnCodes`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Stretch {
    Success,
    SuccessLowEnergy,
    Unchanged,
    Error,
}

/// `WebRtcSpl_DownsampleFast` with `DownsampleTo4kHz`'s 48 kHz parameters: 110 samples at 4 kHz from the start.
fn downsample_to_4khz(signal: &[f32]) -> Option<[f32; DOWNSAMPLED_LEN]> {
    let taps = DOWNSAMPLE_COEFFICIENTS.len();
    let end = DOWNSAMPLE_DELAY + DOWNSAMPLE_FACTOR * (DOWNSAMPLED_LEN - 1) + 1;
    if signal.len() < taps - 1 + end {
        return None;
    }
    // `&input[filter_length - 1]` is the filter's origin: sample `i` reads back `j` taps from there.
    let mut out = [0.0; DOWNSAMPLED_LEN];
    for (o, i) in out
        .iter_mut()
        .zip((DOWNSAMPLE_DELAY..end).step_by(DOWNSAMPLE_FACTOR))
    {
        let mut sum = 0.0;
        for (j, coefficient) in DOWNSAMPLE_COEFFICIENTS.iter().enumerate() {
            sum += *coefficient as f32 * signal[taps - 1 + i - j];
        }
        *o = (sum / 4096.0).clamp(-32768.0, 32767.0);
    }
    Some(out)
}

/// `ParabolicFit` at 48 kHz: the peak position between samples, in 48 kHz samples.
fn parabolic_fit(points: [f32; 3], peak: usize) -> usize {
    let k = &PARABOLA_COEFFICIENTS;
    let fit = &FIT_INDEX_48K;
    let num = -3.0 * points[0] + 4.0 * points[1] - points[2];
    let den = points[0] - 2.0 * points[1] + points[2];
    let temp = num * 120.0;
    let stp = (k[fit[FS_MULT]][0] - k[fit[FS_MULT - 1]][0]) as f32;
    let strt = ((k[fit[FS_MULT]][0] + k[fit[FS_MULT - 1]][0]) / 2) as f32;
    let base = peak * 2 * FS_MULT;
    if temp < -den * strt {
        let mut lmt = strt - stp;
        let mut flag = 1;
        loop {
            if flag == FS_MULT || temp > -den * lmt {
                return base - flag;
            }
            flag += 1;
            lmt -= stp;
        }
    } else if temp > -den * (strt + stp) {
        let mut lmt = strt + 2.0 * stp;
        let mut flag = 1;
        loop {
            if flag == FS_MULT || temp < -den * lmt {
                return base + flag;
            }
            flag += 1;
            lmt += stp;
        }
    }
    base
}

/// `PeakDetection` for one peak (`RefinePeak`), in 48 kHz samples from lag 0 of `data`.
fn peak_detection(data: &[f32]) -> usize {
    let edge_index = data.len() - 1;
    let mut peak = 0;
    for (i, value) in data.iter().enumerate() {
        if *value > data[peak] {
            peak = i;
        }
    }
    if peak == 0 {
        return 0;
    }
    if peak == edge_index {
        let next = 0.0;
        if data[peak] > next {
            return parabolic_fit([data[peak - 1], data[peak], next], peak);
        }
        return (peak * 2 + 1) * FS_MULT;
    }
    parabolic_fit([data[peak - 1], data[peak], data[peak + 1]], peak)
}

/// `AudioVector::CrossFade`: the last `fade_length` samples of `output` (one channel of interleaved frames) move
/// linearly towards `append` (Q14 weights as there), and the rest of `append` follows.
fn cross_fade(output: &mut Vec<f32>, append: &[f32], fade_length: usize, channels: usize) {
    let frames = output.len() / channels;
    let fade_length = fade_length.min(frames).min(append.len() / channels);
    let position = frames - fade_length;
    let alpha_step = 16384 / (fade_length as i32 + 1);
    let mut alpha = 16384;
    for i in 0..fade_length {
        alpha -= alpha_step;
        let a = alpha as f32 / 16384.0;
        for c in 0..channels {
            let index = (position + i) * channels + c;
            output[index] = a * output[index] + (1.0 - a) * append[i * channels + c];
        }
    }
    output.extend_from_slice(&append[fade_length * channels..]);
}

/// What `TimeStretch::Process` measures before a stretch: the pitch period, how alike two periods are, and whether it
/// is speech.
struct Analysis {
    peak_index: usize,
    best_correlation: f32,
    active_speech: bool,
}

/// `TimeStretch::Process` up to `CheckCriteriaAndStretch`. `input` is interleaved in 16-bit units.
fn analyse(input: &[f32], channels: usize) -> Option<Analysis> {
    let signal: Vec<f32> = input.iter().step_by(channels).copied().collect();
    let downsampled = downsample_to_4khz(&signal)?;
    // `AutoCorrelation`: lags kMinLag..kMaxLag of the 4 kHz copy.
    let mut correlation = [0.0f32; CORRELATION_LEN];
    for (i, value) in correlation.iter_mut().enumerate() {
        let lag_start = MAX_LAG - MIN_LAG - i;
        *value = (0..CORRELATION_LEN)
            .map(|j| downsampled[MAX_LAG + j] * downsampled[lag_start + j])
            .sum();
    }
    let peak_index = peak_detection(&correlation) + MIN_LAG * FS_MULT * 2;
    if peak_index > FS_MULT_120 || FS_MULT_120 + peak_index > signal.len() {
        return None;
    }
    let vec1 = &signal[FS_MULT_120 - peak_index..FS_MULT_120];
    let vec2 = &signal[FS_MULT_120..FS_MULT_120 + peak_index];
    let vec1_energy: f32 = vec1.iter().map(|s| s * s).sum();
    let vec2_energy: f32 = vec2.iter().map(|s| s * s).sum();
    let cross_corr: f32 = vec1.iter().zip(vec2).map(|(a, b)| a * b).sum();
    // `SpeechDetection`: mean energy per sample above eight times the noise.
    let active_speech =
        (vec1_energy + vec2_energy) / 16.0 > peak_index as f32 * FALLBACK_BACKGROUND_ENERGY;
    let best_correlation = if vec1_energy > 0.0 && vec2_energy > 0.0 {
        (cross_corr.max(0.0) / (vec1_energy * vec2_energy).sqrt()).min(1.0)
    } else {
        0.0
    };
    Some(Analysis {
        peak_index,
        best_correlation,
        active_speech,
    })
}

/// `Accelerate::Process`: at least 30 ms in, one pitch period (several in `fast` mode) shorter out. Returns the result
/// and the samples per channel removed.
pub(crate) fn accelerate(
    input: &[f32],
    channels: usize,
    fast: bool,
    output: &mut Vec<f32>,
) -> (Stretch, usize) {
    let frames = input.len() / channels;
    if channels == 0 || frames < (2 * 120 - 1) * FS_MULT {
        output.extend_from_slice(input);
        return (Stretch::Error, 0);
    }
    let Some(analysis) = analyse(input, channels) else {
        output.extend_from_slice(input);
        return (Stretch::Error, 0);
    };
    let best_correlation = if analysis.active_speech {
        analysis.best_correlation
    } else {
        0.0
    };
    let threshold = if fast {
        FAST_CORRELATION_THRESHOLD
    } else {
        CORRELATION_THRESHOLD
    };
    if best_correlation > threshold || !analysis.active_speech {
        let mut peak_index = analysis.peak_index;
        if fast {
            peak_index = (FS_MULT_120 / peak_index) * peak_index;
        }
        output.extend_from_slice(&input[..FS_MULT_120 * channels]);
        cross_fade(
            output,
            &input[FS_MULT_120 * channels..(FS_MULT_120 + peak_index) * channels],
            peak_index,
            channels,
        );
        output.extend_from_slice(&input[(FS_MULT_120 + peak_index) * channels..]);
        let result = if analysis.active_speech {
            Stretch::Success
        } else {
            Stretch::SuccessLowEnergy
        };
        (result, peak_index)
    } else {
        output.extend_from_slice(input);
        (Stretch::Unchanged, 0)
    }
}

/// `PreemptiveExpand::Process`: one pitch period longer, leaving the first `old_data_length` samples per channel (already
/// played) untouched. Returns the result and the samples per channel added.
pub(crate) fn preemptive_expand(
    input: &[f32],
    channels: usize,
    old_data_length: usize,
    output: &mut Vec<f32>,
) -> (Stretch, usize) {
    let frames = input.len() / channels;
    if channels == 0
        || frames < (2 * 120 - 1) * FS_MULT
        || old_data_length >= frames.saturating_sub(OVERLAP_SAMPLES)
    {
        output.extend_from_slice(input);
        return (Stretch::Error, 0);
    }
    let Some(analysis) = analyse(input, channels) else {
        output.extend_from_slice(input);
        return (Stretch::Error, 0);
    };
    let (best_correlation, peak_index) = if analysis.active_speech {
        (analysis.best_correlation, analysis.peak_index)
    } else {
        // `SetParametersForPassiveSpeech`.
        (0.0, analysis.peak_index.min(frames - old_data_length))
    };
    if (best_correlation > CORRELATION_THRESHOLD && old_data_length <= FS_MULT_120)
        || !analysis.active_speech
    {
        let unmodified_length = old_data_length.max(FS_MULT_120);
        if unmodified_length + peak_index > frames || peak_index > unmodified_length {
            output.extend_from_slice(input);
            return (Stretch::Unchanged, 0);
        }
        output.extend_from_slice(&input[..(unmodified_length + peak_index) * channels]);
        cross_fade(
            output,
            &input[(unmodified_length - peak_index) * channels..unmodified_length * channels],
            peak_index,
            channels,
        );
        output.extend_from_slice(&input[unmodified_length * channels..]);
        let result = if analysis.active_speech {
            Stretch::Success
        } else {
            Stretch::SuccessLowEnergy
        };
        (result, peak_index)
    } else {
        output.extend_from_slice(input);
        (Stretch::Unchanged, 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A voiced 30 ms at 16-bit scale: a 160 Hz pulse train through a resonance.
    fn voiced(period: usize) -> Vec<f32> {
        (0..1440)
            .map(|i| {
                let phase = (i % period) as f32 / period as f32;
                12000.0 * (phase * std::f32::consts::TAU).sin()
                    + 6000.0 * (phase * 3.0 * std::f32::consts::TAU).sin()
            })
            .collect()
    }

    #[test]
    fn accelerate_removes_one_pitch_period_without_a_jump() {
        let input = voiced(300);
        let mut output = Vec::new();
        let (result, removed) = accelerate(&input, 1, false, &mut output);
        assert_eq!(result, Stretch::Success);
        assert!((295..=305).contains(&removed), "{removed}");
        assert_eq!(output.len(), input.len() - removed);
        let largest_step = output
            .windows(2)
            .map(|w| (w[1] - w[0]).abs())
            .fold(0.0, f32::max);
        let source_step = input
            .windows(2)
            .map(|w| (w[1] - w[0]).abs())
            .fold(0.0, f32::max);
        assert!(
            largest_step <= source_step * 1.5,
            "{largest_step} vs {source_step}"
        );
    }

    #[test]
    fn preemptive_expand_adds_one_period_and_keeps_played_samples() {
        let input = voiced(240);
        let mut output = Vec::new();
        let (result, added) = preemptive_expand(&input, 1, 480, &mut output);
        assert_eq!(result, Stretch::Success);
        assert!((235..=245).contains(&added), "{added}");
        assert_eq!(output.len(), input.len() + added);
        assert_eq!(&output[..480], &input[..480]);
        // Stereo: the same stretch on both channels.
        let stereo: Vec<f32> = input.iter().flat_map(|s| [*s, *s * 0.5]).collect();
        let mut out = Vec::new();
        let (_, added_stereo) = preemptive_expand(&stereo, 2, 480, &mut out);
        assert_eq!(out.len(), stereo.len() + 2 * added_stereo);
        assert!(
            out.chunks(2)
                .all(|pair| (pair[1] - pair[0] * 0.5).abs() < 0.01)
        );
    }

    #[test]
    fn noise_like_speech_is_not_stretched_but_silence_is() {
        let mut seed = 1u32;
        let noise: Vec<f32> = (0..1440)
            .map(|_| {
                seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                (seed >> 16) as f32 - 32768.0
            })
            .collect();
        let mut output = Vec::new();
        assert_eq!(
            accelerate(&noise, 1, false, &mut output).0,
            Stretch::Unchanged
        );
        let quiet = vec![1.0f32; 1440];
        output.clear();
        assert_eq!(
            accelerate(&quiet, 1, false, &mut output).0,
            Stretch::SuccessLowEnergy
        );
        // Too short to work on.
        output.clear();
        assert_eq!(
            accelerate(&quiet[..1000], 1, false, &mut output).0,
            Stretch::Error
        );
        assert_eq!(output.len(), 1000);
    }
}
