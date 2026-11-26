//! BetterAudioPlayer (Equicord `src/equicordplugins/betterAudioPlayer.desktop`, GPL-3.0-or-later): the oscilloscope and
//! spectrograph drawn over an audio player. The original routes the `<audio>` element through a Web Audio
//! `AnalyserNode` (`fftSize` 2048) and draws its byte time-domain and frequency data on a canvas every animation frame
//! while the player plays. Here the player's decoded samples are at hand, so [`Analyser`] is that node over them: the
//! window ending at the heard position, its defaults (Blackman window, smoothing 0.8, -100 to -30 dB) and its byte
//! scaling, as the Web Audio specification gives them. [`scope`] and [`bars`] are the original's `drawOscilloscope`
//! and `drawSpectrograph` as shapes. Not part of the pinned client.

use realfft::RealFftPlanner;
use realfft::num_complex::Complex;
use std::sync::Arc;

/// `analyser.fftSize`.
pub(crate) const FFT_SIZE: usize = 2048;
/// `analyser.frequencyBinCount`: the length of both byte arrays the original draws.
pub(crate) const BINS: usize = FFT_SIZE / 2;
/// The `AnalyserNode` defaults: `smoothingTimeConstant`, `minDecibels`, `maxDecibels`.
const SMOOTHING: f32 = 0.8;
const MIN_DB: f32 = -100.;
const MAX_DB: f32 = -30.;

/// A colour setting: `"R, G, B"` (each 0 to 255) or `#RRGGBB`, as the original's `validateColor` takes it. The original
/// rewrites a hex value into `"R, G, B"` when it is typed and puts the default back on anything else (with a toast);
/// here the value is read as it is and anything else reads as `fallback`.
pub(crate) fn parse_color(value: &str, fallback: [u8; 3]) -> [u8; 3] {
    let value = value.trim();
    let parts: Vec<&str> = value.split(',').map(str::trim).collect();
    if parts.len() == 3
        && parts
            .iter()
            .all(|part| (1..=3).contains(&part.len()) && part.bytes().all(|b| b.is_ascii_digit()))
    {
        // `rgb(...)` with a channel over 255 draws 255.
        let channel = |part: &str| part.parse::<u16>().map_or(0, |n| n.min(255) as u8);
        return [channel(parts[0]), channel(parts[1]), channel(parts[2])];
    }
    let hex = value.strip_prefix('#').unwrap_or(value);
    if hex.len() == 6 && hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        let channel = |at: usize| u8::from_str_radix(&hex[at..at + 2], 16).unwrap_or(0);
        return [channel(0), channel(2), channel(4)];
    }
    fallback
}

/// One frame of the analyser: `getByteTimeDomainData` and `getByteFrequencyData` into arrays of [`BINS`].
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Frame {
    pub time: Vec<u8>,
    pub frequency: Vec<u8>,
}

impl Default for Frame {
    fn default() -> Self {
        Self {
            time: vec![128; BINS],
            frequency: vec![0; BINS],
        }
    }
}

/// An `AnalyserNode` over a clip's samples, with its smoothing state (one per player, as the original makes one
/// analyser per player).
pub(crate) struct Analyser {
    fft: Arc<dyn realfft::RealToComplex<f32>>,
    window: Vec<f32>,
    input: Vec<f32>,
    output: Vec<Complex<f32>>,
    scratch: Vec<Complex<f32>>,
    smoothed: Vec<f32>,
}

impl Analyser {
    pub(crate) fn new() -> Self {
        let fft = RealFftPlanner::<f32>::new().plan_fft_forward(FFT_SIZE);
        // The specification's Blackman window: a0 0.42, a1 0.5, a2 0.08.
        let window = (0..FFT_SIZE)
            .map(|n| {
                let x = std::f32::consts::TAU * n as f32 / FFT_SIZE as f32;
                0.42 - 0.5 * x.cos() + 0.08 * (2. * x).cos()
            })
            .collect();
        Self {
            input: fft.make_input_vec(),
            output: fft.make_output_vec(),
            scratch: fft.make_scratch_vec(),
            fft,
            window,
            smoothed: vec![0.; BINS],
        }
    }

    /// The analyser read at sample `at` of `samples` (what is being heard): the last [`FFT_SIZE`] samples before it,
    /// silence before the clip's start, into `frame`.
    pub(crate) fn read(&mut self, samples: &[f32], at: usize, frame: &mut Frame) {
        let end = at.min(samples.len());
        let start = end.saturating_sub(FFT_SIZE);
        let lead = FFT_SIZE - (end - start);
        let sample = |index: usize| {
            if index < lead {
                0.
            } else {
                samples[start + index - lead]
            }
        };
        // `getByteTimeDomainData` into a shorter array: its first `length` values of the newest `fftSize` samples.
        frame.time.resize(BINS, 128);
        for (index, byte) in frame.time.iter_mut().enumerate() {
            *byte = (128. * (1. + sample(index))).floor().clamp(0., 255.) as u8;
        }
        for (index, value) in self.input.iter_mut().enumerate() {
            *value = sample(index) * self.window[index];
        }
        if self
            .fft
            .process_with_scratch(&mut self.input, &mut self.output, &mut self.scratch)
            .is_err()
        {
            return;
        }
        frame.frequency.resize(BINS, 0);
        let scale = 255. / (MAX_DB - MIN_DB);
        for (bin, byte) in frame.frequency.iter_mut().enumerate() {
            let magnitude = self.output[bin].norm() / FFT_SIZE as f32;
            let smoothed = SMOOTHING * self.smoothed[bin] + (1. - SMOOTHING) * magnitude;
            // A value that decays below what an `f32` holds is the zero the node would read.
            self.smoothed[bin] = if smoothed.is_finite() { smoothed } else { 0. };
            let decibels = 20. * self.smoothed[bin].log10();
            *byte = if decibels.is_finite() {
                (scale * (decibels - MIN_DB)).floor().clamp(0., 255.) as u8
            } else {
                0
            };
        }
    }
}

/// `drawOscilloscope`: the line's points in a box of `width` by `height`, and its colour. The original sets the
/// stroke colour at every point but strokes the path once, so the whole line takes the last point's colour.
pub(crate) fn scope(
    time: &[u8],
    width: f32,
    height: f32,
    color: [u8; 3],
    solid: bool,
) -> (Vec<(f32, f32)>, [u8; 3]) {
    const AMP: f32 = 3.;
    let count = time.len().max(1);
    let slice = width / count as f32;
    let points = time
        .iter()
        .enumerate()
        .map(|(index, byte)| {
            let v = (f32::from(*byte) - 128.) / 128.;
            (index as f32 * slice, height / 2. - v * AMP * height / 2.)
        })
        .collect();
    let stroke = match time.last() {
        Some(last) if !solid => {
            let v = ((f32::from(*last) - 128.) / 128.).abs();
            let along = (count - 1) as f32 / count as f32 * 155.;
            let tone = |base: u8, by: f32| (f32::from(base) + v * by + along).min(255.) as u8;
            [
                tone(color[0], 100.),
                tone(color[1], 50.),
                tone(color[2], 150.),
            ]
        }
        _ => color,
    };
    (points, stroke)
}

/// One bar of `drawSpectrograph`: its left edge, width, top, height, and its colour at the top and the bottom (the
/// same colour when solid).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Bar {
    pub x: f32,
    pub width: f32,
    pub top: f32,
    pub height: f32,
    pub top_color: [u8; 3],
    pub bottom_color: [u8; 3],
}

/// `drawSpectrograph`: every bar is `width / bins` wide and the next starts half a pixel after it, so only the low
/// bins fit in the canvas; the bars past its right edge, which the canvas clips, are left out. Heights are relative
/// to the loudest bin; nothing is drawn while every bin is zero.
pub(crate) fn bars(
    frequency: &[u8],
    width: f32,
    height: f32,
    color: [u8; 3],
    solid: bool,
) -> Vec<Bar> {
    let max = frequency.iter().copied().max().unwrap_or(0);
    if max == 0 {
        return Vec::new();
    }
    let count = frequency.len() as f32;
    let bar_width = width / count;
    let mut out = Vec::new();
    let mut x = 0.;
    for (index, value) in frequency.iter().enumerate() {
        if x >= width {
            break;
        }
        let bar_height = f32::from(*value) / f32::from(max) * height;
        let along = index as f32 / count * 155.;
        let top = color.map(|base| (f32::from(base) + along).min(255.) as u8);
        let (top_color, bottom_color) = if solid {
            (color, color)
        } else {
            (top, top.map(|channel| channel.saturating_sub(50)))
        };
        if bar_height > 0. {
            out.push(Bar {
                x,
                width: bar_width,
                top: height - bar_height,
                height: bar_height,
                top_color,
                bottom_color,
            });
        }
        x += bar_width + 0.5;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colors_read_as_rgb_or_hex_with_the_default_otherwise() {
        assert_eq!(parse_color("255, 255, 255", [0; 3]), [255, 255, 255]);
        assert_eq!(parse_color("33,150,243", [0; 3]), [33, 150, 243]);
        assert_eq!(parse_color("#2196F3", [0; 3]), [33, 150, 243]);
        assert_eq!(parse_color("2196f3", [0; 3]), [33, 150, 243]);
        assert_eq!(parse_color("999, 0, 0", [0; 3]), [255, 0, 0]);
        assert_eq!(parse_color("255, 255, 255x", [1, 2, 3]), [1, 2, 3]);
        assert_eq!(parse_color("red", [1, 2, 3]), [1, 2, 3]);
    }

    #[test]
    fn the_analyser_reads_silence_as_the_node_does() {
        let mut analyser = Analyser::new();
        let mut frame = Frame::default();
        analyser.read(&[0.; 4800], 4800, &mut frame);
        assert!(frame.time.iter().all(|byte| *byte == 128));
        assert!(frame.frequency.iter().all(|byte| *byte == 0));
        // Before the clip's start the window is silence too.
        analyser.read(&[0.5; 10], 0, &mut frame);
        assert!(frame.time.iter().all(|byte| *byte == 128));
    }

    #[test]
    fn a_tone_lights_its_bin_and_the_wave_swings() {
        // A 1 kHz sine at 48 kHz: bin 1000 / (48000 / 2048) = 42.7.
        let samples: Vec<f32> = (0..48_000)
            // Quiet enough (about -48 dB in its bin) that the bin stays under the -30 dB top of the byte scale.
            .map(|n| 0.02 * (std::f32::consts::TAU * 1000. * n as f32 / 48_000.).sin())
            .collect();
        let mut analyser = Analyser::new();
        let mut frame = Frame::default();
        // Smoothing reaches the steady value after a few reads.
        for step in 0..30 {
            analyser.read(&samples, 10_000 + step * 800, &mut frame);
        }
        let loudest = (0..BINS).max_by_key(|bin| frame.frequency[*bin]).unwrap();
        assert!((42..=43).contains(&loudest), "loudest bin {loudest}");
        assert!(frame.frequency[400] < frame.frequency[loudest] / 2);
        assert!(frame.frequency[loudest] < 255);
        // 0.02 of 128 swings the wave bytes about two and a half steps either way.
        assert!(frame.time.iter().any(|byte| *byte >= 130));
        assert!(frame.time.iter().any(|byte| *byte <= 125));
    }

    #[test]
    fn the_scope_spans_the_box_and_takes_the_last_points_colour() {
        let mut time = vec![128u8; BINS];
        time[BINS - 1] = 192;
        let (points, color) = scope(&time, 400., 100., [0, 0, 0], false);
        assert_eq!(points.len(), BINS);
        assert_eq!(points[0], (0., 50.));
        // v = 0.5 at the last point: 50 - 0.5 * 3 * 50.
        assert!((points[BINS - 1].1 - -25.).abs() < 1e-3);
        // 0.5 * 100 + 1023 / 1024 * 155 = 204.8.
        assert_eq!(color, [204, 179, 229]);
        assert_eq!(scope(&time, 400., 100., [9, 9, 9], true).1, [9, 9, 9]);
    }

    #[test]
    fn bars_stop_at_the_right_edge_and_scale_to_the_loudest() {
        let mut frequency = vec![0u8; BINS];
        frequency[0] = 200;
        frequency[1] = 100;
        frequency[BINS - 1] = 255;
        let drawn = bars(&frequency, 432., 60., [33, 150, 243], false);
        // The far bin is past the canvas; the loudest still sets the scale.
        assert_eq!(drawn.len(), 2);
        assert!((drawn[0].height - 200. / 255. * 60.).abs() < 1e-3);
        assert!((drawn[1].x - (432. / 1024. + 0.5)).abs() < 1e-4);
        assert_eq!(drawn[0].top_color, [33, 150, 243]);
        assert_eq!(drawn[0].bottom_color, [0, 100, 193]);
        assert!(bars(&[0; BINS], 432., 60., [0; 3], false).is_empty());
    }
}
