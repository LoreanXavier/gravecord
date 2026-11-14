//! The numbers of a voice message that are not audio: the waveform a message carries, how the player turns it into
//! bars, how times are written, and the speeds the player offers. Every rule here is read from the official client:
//!
//! * the waveform of a recording, `el` in module 841817: at most 256 bytes, base64; more samples are averaged down
//!   by `A` in module 934581;
//! * the level of one sample, `handleVoiceActivity` in module 198620: the engine's dB value clamped to -100..0 and
//!   scaled to 0..255;
//! * the bars of the player, `b` in module 408822: the canvas width from the duration, one bar per 6 px, the byte
//!   values divided by 255, shorter series padded with zeros (or stretched for the fluid recorder preview) and longer
//!   ones averaged down;
//! * the time text, `ei` in module 153440, and the speeds, module 518950.

/// `CA` in module 518950: the speeds the player's speed button cycles through.
pub const SPEEDS: [f32; 4] = [0.75, 1., 1.5, 2.];
/// `iA` in module 518950, in milliseconds: the recording stops by itself here (module 841817 stops it at the
/// duration `(iA + 500) / 1000` seconds, 1200).
pub const MAX_RECORDING_MS: u64 = 1_199_500;
/// `Y_`: from here the timer blinks slowly.
pub const WARN_MS: u64 = 1_169_500;
/// `Uu`: from here the timer blinks fast.
pub const IMMINENT_MS: u64 = 1_184_500;
/// `$` in module 841817: what a recording that ran into the limit counts as.
pub const MAX_RECORDING_SECS: f64 = (MAX_RECORDING_MS as f64 + 500.) / 1000.;
/// A recording shorter than this is thrown away (`durationSecs < 0.5` in module 841817).
pub const MIN_RECORDING_SECS: f64 = 0.5;
/// The longest waveform a message carries (`e.length > 256` in `el`).
pub const MAX_WAVEFORM_BYTES: usize = 256;

/// The speed after `current` when the button is pressed: the next one in the list, wrapping (`(indexOf + 1) % 4`).
pub fn next_speed(current: f32) -> f32 {
    let at = SPEEDS
        .iter()
        .position(|speed| (*speed - current).abs() < 1e-6);
    // `indexOf` finds nothing (-1) for a speed that is not listed: the next index is 0.
    SPEEDS[at.map_or(0, |index| (index + 1) % SPEEDS.len())]
}

/// The button's label: the speed with its leading zero cut and an X (`${N.toString().replace(/^0/, "")}X`).
pub fn speed_label(speed: f32) -> String {
    format!("{}X", format!("{speed}").trim_start_matches('0'))
}

/// `ei`: whole seconds as `m:ss` (the value is cut to a whole number, minutes are not padded).
pub fn format_time(seconds: f64) -> String {
    let total = if seconds.is_finite() && seconds > 0. {
        seconds as u64
    } else {
        0
    };
    format!("{}:{:02}", total / 60, total % 60)
}

/// What the pill shows as its time (`tn` in module 383055): "--:--" until the duration is known, the whole length
/// before the first play, the time left (rounded up) after it.
pub fn pill_time(duration: Option<f64>, played: bool, current: f64) -> String {
    match duration {
        None => "--:--".to_owned(),
        Some(duration) if played => format_time((duration - current).ceil()),
        Some(duration) => format_time(duration.ceil()),
    }
}

/// `handleVoiceActivity`: the byte a level of `db` decibels becomes.
pub fn level_byte(db: f32) -> u8 {
    let clamped = if db.is_nan() {
        -100.
    } else {
        db.clamp(-100., 0.)
    };
    (((clamped + 100.) / 100.) * 255.).round() as u8
}

/// The level of a run of samples in decibels relative to full scale: the root of the mean square.
pub fn level_db(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return -100.;
    }
    let mean = samples.iter().map(|sample| sample * sample).sum::<f32>() / samples.len() as f32;
    if mean <= 0. {
        -100.
    } else {
        10. * mean.log10()
    }
}

/// `A` in module 934581: the series averaged to `count` values (`count` must not be more than the series has).
pub fn downsample(values: &[f32], count: usize) -> Vec<f32> {
    if count == 0 {
        return Vec::new();
    }
    if values.len() <= count {
        return values.to_vec();
    }
    let step = values.len() as f64 / count as f64;
    let mut out = Vec::with_capacity(count);
    let mut start = 0usize;
    while out.len() < count {
        // JavaScript's Math.round: halves go up.
        let end = (((out.len() + 1) as f64 * step) + 0.5).floor() as usize;
        let (mut sum, mut taken) = (0f32, 0u32);
        let mut index = start;
        while index < end && index < values.len() {
            sum += values[index];
            taken += 1;
            index += 1;
        }
        out.push(if taken == 0 { 0. } else { sum / taken as f32 });
        start = end;
    }
    out
}

const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Standard base64 with padding, as `base64-js` `fromByteArray` writes it.
pub fn base64_encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let value = (u32::from(chunk[0]) << 16)
            | (u32::from(*chunk.get(1).unwrap_or(&0)) << 8)
            | u32::from(*chunk.get(2).unwrap_or(&0));
        out.push(ALPHABET[(value >> 18) as usize & 63] as char);
        out.push(ALPHABET[(value >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 {
            ALPHABET[(value >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            ALPHABET[value as usize & 63] as char
        } else {
            '='
        });
    }
    out
}

/// `atob`: standard base64, padding optional; anything else is refused.
pub fn base64_decode(text: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(text.len() / 4 * 3);
    let (mut value, mut bits) = (0u32, 0u32);
    for byte in text.trim_end_matches('=').bytes() {
        let digit = match byte {
            b'A'..=b'Z' => byte - b'A',
            b'a'..=b'z' => byte - b'a' + 26,
            b'0'..=b'9' => byte - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => return None,
        };
        value = (value << 6) | u32::from(digit);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((value >> bits) as u8);
            value &= (1 << bits) - 1;
        }
    }
    Some(out)
}

/// `el`: the `waveform` field of a recording's attachment. Empty for no samples; more than 256 samples are averaged
/// down to 256; every value is rounded and capped at 255.
pub fn encode_waveform(samples: &[u8]) -> String {
    if samples.is_empty() {
        return String::new();
    }
    let values: Vec<f32> = samples.iter().map(|sample| f32::from(*sample)).collect();
    let series = if values.len() > MAX_WAVEFORM_BYTES {
        downsample(&values, MAX_WAVEFORM_BYTES)
    } else {
        values
    };
    let bytes: Vec<u8> = series
        .iter()
        .map(|value| value.round().clamp(0., 255.) as u8)
        .collect();
    base64_encode(&bytes)
}

/// The inline decoder of module 408822: a base64 string of bytes becomes values from 0 to 1; a bad string is none.
pub fn decode_waveform(text: &str) -> Option<Vec<f32>> {
    base64_decode(text).map(|bytes| bytes.iter().map(|byte| f32::from(*byte) / 255.).collect())
}

/// The canvas width of a pill's waveform for a message of `duration` seconds (`I` in module 408822): 40 px up to half
/// a second, 294 px from 45 seconds (290 after the cut), linear between, then cut to whole bars (6 px each, the last without its gap).
pub fn pill_wave_width(duration: f64) -> f32 {
    let wanted = if duration <= 0.5 {
        40.
    } else if duration >= 45. {
        294.
    } else {
        ((duration.min(45.) - 0.5) / 44.5) * 254. + 40.
    };
    6. * ((wanted + 4.) / 6.).floor() as f32 - 4.
}

/// The number of bars a canvas of `width` pixels holds (`Math.floor((width + 4) / 6)`).
pub fn bar_count(width: f32) -> usize {
    ((width + 4.) / 6.).floor().max(0.) as usize
}

/// The values the canvas draws: `count` bars from the message's series. A shorter series is padded with zeros, or for
/// the recorder's `fluid` preview stretched by interpolation; a longer one is averaged down. No waveform is five zeros
/// (`f`) until the bars are counted, then `count` zeros.
pub fn bars(series: Option<&[f32]>, count: usize, fluid: bool) -> Vec<f32> {
    let Some(series) = series else {
        return vec![0.; count];
    };
    if series.len() < count {
        if fluid {
            return match series.len() {
                0 => vec![0.; count],
                1 => vec![series[0]; count],
                length => (0..count)
                    .map(|index| {
                        let position = (index * (length - 1)) as f32 / (count - 1) as f32;
                        let low = position.floor() as usize;
                        let high = (low + 1).min(length - 1);
                        let fraction = position - low as f32;
                        series[low] * (1. - fraction) + series[high] * fraction
                    })
                    .collect(),
            };
        }
        let mut padded = series.to_vec();
        padded.resize(count, 0.);
        return padded;
    }
    downsample(series, count)
}

/// How many bars are lit at `current` seconds of `duration` (`v` in module 408822): all of them before the first
/// play, else the share played, rounded.
pub fn lit_bars(played: bool, current: f64, duration: f64, bars: usize) -> usize {
    if !played {
        return bars;
    }
    if duration <= 0. {
        return 0;
    }
    ((current / duration) * bars as f64).round().max(0.) as usize
}

/// A bar's height for a value, in pixels (`y` in module 408822): 22 px of range above a 2 px floor when the bar is
/// lit, 24 px of range with no floor for an unplayed bar.
pub fn bar_height(value: f32, floor: bool) -> f32 {
    if floor { 22. * value + 2. } else { 24. * value }
}

/// `canvas_6oHqIH` in module 115921: left-edge alpha ramp from `0.0` at `0%` of canvas width to `1.0` at `12%`.
pub fn live_bar_fade(x: f32, width: f32) -> f32 {
    if width <= 0. {
        return 1.;
    }
    let span = width * 0.12;
    if span <= 0. {
        1.
    } else {
        (x / span).clamp(0., 1.)
    }
}

/// `e4` in module 989161: blink interval in milliseconds once the recording enters the warning window (`None` before
/// `WARN_MS`, `500` during approaching, `250` during imminent, `1000` under reduced motion).
pub fn timer_blink_interval_ms(elapsed_ms: u64, reduced_motion: bool) -> Option<u64> {
    if elapsed_ms < WARN_MS {
        return None;
    }
    if reduced_motion {
        return Some(1000);
    }
    if elapsed_ms >= IMMINENT_MS {
        Some(250)
    } else {
        Some(500)
    }
}

/// `easeOutBack(t, b, c, d, s)` of module 722872: from `b` to `c` over `d`, overshooting by `s`.
pub fn ease_out_back(t: f32, b: f32, c: f32, d: f32, s: f32) -> f32 {
    let t = t / d - 1.;
    (c - b) * (t * t * ((s + 1.) * t + s) + 1.) + b
}

/// `easeOutQuint(t, b, c, d)` of module 722872.
pub fn ease_out_quint(t: f32, b: f32, c: f32, d: f32) -> f32 {
    let t = t / d - 1.;
    (c - b) * (t * t * t * t * t + 1.) + b
}

/// One bar's value over time (`A` in module 539451). Falling takes 150 ms with `easeOutQuint`, rising 500 ms with
/// `easeOutBack` (overshoot 4). Times are in milliseconds on any clock.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Segment {
    pub value: f32,
    from: Option<(f32, f64)>,
    pub is_reset: bool,
}

impl Segment {
    pub fn new(value: f32) -> Self {
        Self {
            value,
            from: None,
            is_reset: false,
        }
    }

    pub fn current(&self, now: f64) -> f32 {
        let Some((last, started)) = self.from else {
            return self.value;
        };
        let elapsed = (now - started) as f32;
        let duration = if self.value < last { 150. } else { 500. };
        if elapsed > duration {
            self.value
        } else if self.value < last {
            ease_out_quint(elapsed, last, self.value, duration)
        } else {
            ease_out_back(elapsed, last, self.value, duration, 4.)
        }
    }

    pub fn animate_to(&mut self, value: f32, now: f64) {
        self.is_reset = false;
        if (self.value - value).abs() > f32::EPSILON {
            self.from = Some((self.value, now));
            self.value = value;
        }
    }

    pub fn reset(&mut self, now: f64) {
        self.animate_to(0., now);
        self.is_reset = true;
    }

    pub fn is_animating(&self, now: f64) -> bool {
        match self.from {
            None => false,
            Some((last, started)) => {
                let duration = if self.value < last { 150. } else { 500. };
                now - started <= duration
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn times_are_whole_seconds_with_unpadded_minutes() {
        assert_eq!(format_time(0.), "0:00");
        assert_eq!(format_time(5.9), "0:05");
        assert_eq!(format_time(65.), "1:05");
        assert_eq!(format_time(600.), "10:00");
        assert_eq!(format_time(f64::NAN), "0:00");
        assert_eq!(format_time(-3.), "0:00");
    }

    #[test]
    fn the_pill_shows_its_length_then_the_time_left() {
        assert_eq!(pill_time(None, false, 0.), "--:--");
        assert_eq!(pill_time(Some(7.2), false, 0.), "0:08");
        assert_eq!(pill_time(Some(7.2), true, 0.), "0:08");
        assert_eq!(pill_time(Some(7.2), true, 3.), "0:05");
        assert_eq!(pill_time(Some(7.2), true, 7.2), "0:00");
    }

    #[test]
    fn the_speed_button_cycles_and_names_its_speed() {
        assert_eq!(next_speed(1.), 1.5);
        assert_eq!(next_speed(1.5), 2.);
        assert_eq!(next_speed(2.), 0.75);
        assert_eq!(next_speed(0.75), 1.);
        assert_eq!(next_speed(3.), 0.75, "an unknown speed goes to the first");
        assert_eq!(speed_label(0.75), ".75X");
        assert_eq!(speed_label(1.), "1X");
        assert_eq!(speed_label(1.5), "1.5X");
        assert_eq!(speed_label(2.), "2X");
    }

    #[test]
    fn levels_map_minus_100_to_0_db_onto_a_byte() {
        assert_eq!(level_byte(-100.), 0);
        assert_eq!(level_byte(0.), 255);
        assert_eq!(level_byte(-50.), 128);
        assert_eq!(level_byte(-200.), 0);
        assert_eq!(level_byte(6.), 255);
        assert_eq!(level_byte(f32::NAN), 0);
        assert_eq!(level_byte(-30.), 179);
    }

    #[test]
    fn the_level_of_samples_is_their_rms_in_decibels() {
        assert!((level_db(&[1.; 100]) - 0.).abs() < 1e-4);
        assert!((level_db(&[0.1; 100]) - -20.).abs() < 1e-3);
        assert_eq!(level_db(&[0.; 10]), -100.);
        assert_eq!(level_db(&[]), -100.);
        // A full-scale sine has a mean square of one half: -3 dB.
        let sine: Vec<f32> = (0..4800)
            .map(|index| (std::f32::consts::TAU * 100. * index as f32 / 48_000.).sin())
            .collect();
        assert!((level_db(&sine) - -3.0103).abs() < 0.01);
    }

    #[test]
    fn base64_matches_the_standard_vectors() {
        for (plain, coded) in [
            ("", ""),
            ("f", "Zg=="),
            ("fo", "Zm8="),
            ("foo", "Zm9v"),
            ("foob", "Zm9vYg=="),
            ("fooba", "Zm9vYmE="),
            ("foobar", "Zm9vYmFy"),
        ] {
            assert_eq!(base64_encode(plain.as_bytes()), coded);
            assert_eq!(base64_decode(coded).unwrap(), plain.as_bytes());
        }
        assert_eq!(base64_decode("Zm9v").unwrap(), b"foo");
        assert_eq!(
            base64_decode("Zm9vYg").unwrap(),
            b"foob",
            "padding is optional"
        );
        assert!(base64_decode("Zm9v!").is_none());
        assert_eq!(base64_encode(&[0, 255, 128, 7]), "AP+ABw==");
    }

    #[test]
    fn a_waveform_of_up_to_256_samples_is_sent_as_it_is() {
        assert_eq!(encode_waveform(&[]), "");
        assert_eq!(encode_waveform(&[0, 128, 255]), "AID/");
        let samples: Vec<u8> = (0..=255).collect();
        let text = encode_waveform(&samples);
        assert_eq!(base64_decode(&text).unwrap(), samples);
    }

    #[test]
    fn a_longer_waveform_is_averaged_down_to_256() {
        // 512 samples: pairs are averaged (1, 3 -> 2).
        let samples: Vec<u8> = (0..512)
            .map(|index| if index % 2 == 0 { 0 } else { 100 })
            .collect();
        let bytes = base64_decode(&encode_waveform(&samples)).unwrap();
        assert_eq!(bytes.len(), 256);
        assert!(bytes.iter().all(|byte| *byte == 50));
        // 300 samples: still 256 values, the first one the mean of the first 1.17 samples' window (round(1 * 1.1719) = 1).
        let ramp: Vec<u8> = (0..300).map(|index| (index % 256) as u8).collect();
        let bytes = base64_decode(&encode_waveform(&ramp)).unwrap();
        assert_eq!(bytes.len(), 256);
        assert_eq!(bytes[0], 0, "the first window is only the first sample");
        // Mean of the window ending at round(2 * 1.171875) = 2 starting at 1: sample 1.
        assert_eq!(bytes[1], 1);
    }

    #[test]
    fn the_downsample_of_the_bundle_rounds_its_window_ends() {
        let values: Vec<f32> = (0..10).map(|value| value as f32).collect();
        // step 2.5: window ends round(2.5)=3, round(5)=5, round(7.5)=8, round(10)=10
        assert_eq!(downsample(&values, 4), vec![1., 3.5, 6., 8.5]);
        assert_eq!(downsample(&values, 10), values);
        assert_eq!(downsample(&values, 1), vec![4.5]);
    }

    #[test]
    fn the_pill_grows_with_the_message_up_to_45_seconds() {
        assert_eq!(pill_wave_width(0.), 38.);
        assert_eq!(pill_wave_width(0.5), 38.);
        assert_eq!(pill_wave_width(45.), 290.);
        assert_eq!(pill_wave_width(600.), 290.);
        // 10 s: 40 + 9.5 / 44.5 * 254 = 94.2; (94.2 + 4) / 6 = 16.4 -> 16 bars -> 92 px.
        assert_eq!(pill_wave_width(10.), 92.);
        assert_eq!(bar_count(92.), 16);
        assert_eq!(bar_count(290.), 49);
        assert_eq!(bar_count(38.), 7);
    }

    #[test]
    fn bars_follow_the_canvas_rules() {
        let series = [1., 0.5, 0.25];
        assert_eq!(bars(Some(&series), 5, false), vec![1., 0.5, 0.25, 0., 0.]);
        let stretched = bars(Some(&[0., 1.]), 5, true);
        assert_eq!(stretched, vec![0., 0.25, 0.5, 0.75, 1.]);
        assert_eq!(bars(Some(&[0.4]), 3, true), vec![0.4; 3]);
        assert_eq!(bars(Some(&[]), 3, true), vec![0.; 3]);
        assert_eq!(bars(None, 4, false), vec![0.; 4]);
        assert_eq!(bars(Some(&[1., 3., 5., 7.]), 2, false), vec![2., 6.]);
        assert_eq!(bars(Some(&series), 3, false), series.to_vec());
    }

    #[test]
    fn the_bars_lit_follow_the_playing_position() {
        assert_eq!(lit_bars(false, 0., 10., 16), 16);
        assert_eq!(lit_bars(true, 0., 10., 16), 0);
        assert_eq!(lit_bars(true, 5., 10., 16), 8);
        assert_eq!(lit_bars(true, 10., 10., 16), 16);
        assert_eq!(lit_bars(true, 3., 0., 16), 0);
    }

    #[test]
    fn bar_heights_have_a_floor_only_when_lit() {
        assert_eq!(bar_height(0., true), 2.);
        assert_eq!(bar_height(1., true), 24.);
        assert_eq!(bar_height(0.5, false), 12.);
        assert_eq!(bar_height(0., false), 0.);
    }

    #[test]
    fn a_bar_rises_with_an_overshoot_and_falls_quickly() {
        let mut bar = Segment::new(0.);
        bar.animate_to(1., 1000.);
        assert!(!bar.is_reset);
        assert_eq!(bar.current(1000.), 0.);
        // easeOutBack with s = 4 overshoots before settling.
        let peak = (0..=50)
            .map(|step| bar.current(1000. + f64::from(step) * 10.))
            .fold(0f32, f32::max);
        assert!(peak > 1.1, "peak {peak}");
        assert_eq!(bar.current(1600.), 1.);
        assert!(bar.is_animating(1400.));
        assert!(!bar.is_animating(1600.));
        bar.reset(2000.);
        assert!(bar.is_reset);
        assert!(bar.current(2075.) < 0.5);
        assert_eq!(bar.current(2200.), 0.);
    }

    #[test]
    fn the_limits_are_the_bundles() {
        assert_eq!(MAX_RECORDING_SECS, 1200.);
        assert_eq!(format_time(MAX_RECORDING_SECS), "20:00");
        // Warnings come at 30 s and 15 s before the end.
        assert_eq!(MAX_RECORDING_MS - WARN_MS, 30_000);
        assert_eq!(MAX_RECORDING_MS - IMMINENT_MS, 15_000);
    }

    #[test]
    fn live_wave_fades_from_zero_to_twelve_percent_width() {
        assert_eq!(live_bar_fade(0., 500.), 0.);
        assert!((live_bar_fade(30., 500.) - 0.5).abs() < 1e-6);
        assert_eq!(live_bar_fade(60., 500.), 1.);
        assert_eq!(live_bar_fade(250., 500.), 1.);
        assert_eq!(live_bar_fade(0., 0.), 1.);
    }

    #[test]
    fn timer_blink_matches_approaching_imminent_and_reduced_motion() {
        assert_eq!(timer_blink_interval_ms(0, false), None);
        assert_eq!(timer_blink_interval_ms(WARN_MS - 1, false), None);
        assert_eq!(timer_blink_interval_ms(WARN_MS, false), Some(500));
        assert_eq!(timer_blink_interval_ms(IMMINENT_MS - 1, false), Some(500));
        assert_eq!(timer_blink_interval_ms(IMMINENT_MS, false), Some(250));
        assert_eq!(timer_blink_interval_ms(WARN_MS, true), Some(1000));
        assert_eq!(timer_blink_interval_ms(IMMINENT_MS, true), Some(1000));
    }
}
