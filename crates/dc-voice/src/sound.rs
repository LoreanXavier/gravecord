//! UI sounds: decode an mp3 (the official client's sound files, module 696354) and play it once on an output device
//! through cpal. This is the player half of `WebAudioSound` (module 946261); which sound plays, and when, is the app's.
//!
//! The official player sets `audio.volume = min(outputVolume / 100 * volume, 1)` and routes by `outputChannel`:
//! `DEFAULT` goes to the system default speaker, `VOICE` to the speaker chosen in Voice & Video. Both are kept: the
//! caller passes the factor (`volume`), and the chosen output volume is read here from [`crate::prefs`].
use crate::audio::{output_config, pick, prefs};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::io::Cursor;
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};
use std::time::{Duration, Instant};
use symphonia::core::{
    audio::SampleBuffer,
    codecs::{CODEC_TYPE_NULL, DecoderOptions},
    errors::Error as DecodeError,
    formats::FormatOptions,
    io::MediaSourceStream,
    meta::MetadataOptions,
    probe::Hint,
};
use tracing::{debug, warn};

/// A sound file this long or longer is not a UI sound; decoding stops there.
const MAX_SECONDS: usize = 30;
/// An audio attachment is decoded up to this long (module 153440 plays any length; the clip is held in memory).
const ATTACHMENT_MAX_SECONDS: usize = 600;

/// `outputChannel` (module 257645).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Output {
    /// `DEFAULT`: the system default speaker.
    Default,
    /// `VOICE`: the speaker chosen in Voice & Video.
    Voice,
}

/// A decoded sound: interleaved f32 samples.
#[derive(Debug)]
pub struct Decoded {
    pub rate: u32,
    pub channels: u16,
    pub samples: Vec<f32>,
}

impl Decoded {
    pub fn frames(&self) -> usize {
        self.samples.len() / usize::from(self.channels.max(1))
    }
    pub fn duration(&self) -> Duration {
        Duration::from_secs_f64(self.frames() as f64 / f64::from(self.rate.max(1)))
    }
}

/// Decodes an mp3 file (an ID3 tag in front is fine).
pub fn decode_mp3(bytes: Vec<u8>) -> Result<Decoded, &'static str> {
    let mut hint = Hint::new();
    hint.with_extension("mp3");
    decode_with(bytes, &hint, MAX_SECONDS)
}

/// Decodes a sound file of any kind a server's soundboard can hold: the official client hands them to the browser's
/// `decodeAudioData`, so a soundboard file is not always an mp3. The format is read from the file itself.
pub fn decode_any(bytes: Vec<u8>) -> Result<Decoded, &'static str> {
    decode_with(bytes, &Hint::new(), MAX_SECONDS)
}

/// A whole audio attachment for the message player: the same decoders, with the limit of a message's longest clip
/// (ten minutes) instead of the UI sounds' 30 seconds.
pub fn decode_attachment(bytes: Vec<u8>) -> Result<Decoded, &'static str> {
    decode_with(bytes, &Hint::new(), ATTACHMENT_MAX_SECONDS)
}

/// Ogg Opus: symphonia reads the container but has no Opus decoder, so the packets go to libopus at 48 kHz.
fn decode_opus(
    format: &mut Box<dyn symphonia::core::formats::FormatReader>,
    track_id: u32,
    limit: usize,
) -> Result<Decoded, &'static str> {
    const CHANNELS: usize = 2;
    let mut decoder = opus2::Decoder::new(48_000, opus2::Channels::Stereo)
        .map_err(|_| "The Opus decoder is unavailable")?;
    // The longest Opus packet is 120 ms.
    let mut frame = vec![0f32; 5760 * CHANNELS];
    let mut samples = Vec::new();
    loop {
        let packet = match format.next_packet() {
            Ok(packet) => packet,
            Err(DecodeError::IoError(_)) | Err(DecodeError::ResetRequired) => break,
            Err(_) => return Err("The sound file is damaged"),
        };
        if packet.track_id() != track_id {
            continue;
        }
        if let Ok(count) = decoder.decode_float(&packet.data, &mut frame, false) {
            samples.extend_from_slice(&frame[..count * CHANNELS]);
        }
        if samples.len() > limit * 48_000 * CHANNELS {
            break;
        }
    }
    if samples.is_empty() {
        return Err("The sound file holds no audio");
    }
    Ok(Decoded {
        rate: 48_000,
        channels: CHANNELS as u16,
        samples,
    })
}

fn decode_with(bytes: Vec<u8>, hint: &Hint, limit: usize) -> Result<Decoded, &'static str> {
    let first: Vec<u8> = bytes.iter().copied().take(4).collect();
    let source = MediaSourceStream::new(Box::new(Cursor::new(bytes)), Default::default());
    let probed = symphonia::default::get_probe()
        .format(
            hint,
            source,
            &FormatOptions::default(),
            &MetadataOptions::default(),
        )
        .map_err(|_| {
            debug!(?first, "sound file of an unknown kind");
            "Not a sound file"
        })?;
    let mut format = probed.format;
    let track = format
        .tracks()
        .iter()
        .find(|track| track.codec_params.codec != CODEC_TYPE_NULL)
        .ok_or("The file has no audio track")?;
    let track_id = track.id;
    if track.codec_params.codec == symphonia::core::codecs::CODEC_TYPE_OPUS {
        return decode_opus(&mut format, track_id, limit);
    }
    let mut decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &DecoderOptions::default())
        .map_err(|_| "The sound file's codec is unavailable")?;
    let (mut rate, mut channels, mut samples) = (0u32, 0u16, Vec::new());
    loop {
        let packet = match format.next_packet() {
            Ok(packet) => packet,
            // The end of the file arrives as an unexpected EOF.
            Err(DecodeError::IoError(_)) | Err(DecodeError::ResetRequired) => break,
            Err(_) => return Err("The mp3 file is damaged"),
        };
        if packet.track_id() != track_id {
            continue;
        }
        match decoder.decode(&packet) {
            Ok(buffer) => {
                let spec = *buffer.spec();
                let mut converted = SampleBuffer::<f32>::new(buffer.capacity() as u64, spec);
                converted.copy_interleaved_ref(buffer);
                rate = spec.rate;
                channels = spec.channels.count() as u16;
                samples.extend_from_slice(converted.samples());
            }
            // A bad frame is skipped, as players do.
            Err(DecodeError::DecodeError(_)) => continue,
            Err(_) => return Err("The mp3 file is damaged"),
        }
        if channels > 0 && samples.len() > limit * rate as usize * usize::from(channels) {
            break;
        }
    }
    if rate == 0 || channels == 0 || samples.is_empty() {
        return Err("The mp3 file holds no audio");
    }
    Ok(Decoded {
        rate,
        channels,
        samples,
    })
}

/// A sound being played or about to be: `stop()` ends it, and one that never started never will.
#[derive(Clone, Default)]
pub struct Handle {
    stopped: Arc<AtomicBool>,
    finished: Arc<AtomicBool>,
}

impl Handle {
    pub fn new() -> Self {
        Self::default()
    }
    /// `WebAudioSound.stop`.
    pub fn stop(&self) {
        self.stopped.store(true, Ordering::Release);
    }
    pub fn is_stopped(&self) -> bool {
        self.stopped.load(Ordering::Acquire)
    }
    /// The sound played to its end, was stopped, or could not play.
    pub fn is_finished(&self) -> bool {
        self.finished.load(Ordering::Acquire)
    }
}

/// `min(outputVolume / 100 * volume, 1)`: the gain a sound plays at.
pub fn gain(output_gain: f32, volume: f32) -> f32 {
    (output_gain * volume).clamp(0.0, 1.0)
}

/// The sound as the device wants it: its rate and channel count, with `gain` applied. Resampling is linear;
/// mono feeds every channel, stereo feeds the first two, and a mono device gets the mean.
pub fn render(sound: &Decoded, rate: u32, channels: usize, gain: f32) -> Vec<f32> {
    let source_channels = usize::from(sound.channels.max(1));
    let source_frames = sound.frames();
    if source_frames == 0 || channels == 0 || rate == 0 {
        return Vec::new();
    }
    let step = f64::from(sound.rate) / f64::from(rate);
    let frames = (source_frames as f64 / step).floor() as usize;
    let sample = |frame: usize, channel: usize| sound.samples[frame * source_channels + channel];
    let mut out = Vec::with_capacity(frames * channels);
    for index in 0..frames {
        let position = index as f64 * step;
        let first = (position.floor() as usize).min(source_frames - 1);
        let second = (first + 1).min(source_frames - 1);
        let fraction = (position - first as f64) as f32;
        let at = |channel: usize| {
            let (a, b) = (sample(first, channel), sample(second, channel));
            a + (b - a) * fraction
        };
        for channel in 0..channels {
            let value = if channels == 1 {
                (0..source_channels).map(at).sum::<f32>() / source_channels as f32
            } else if source_channels == 1 {
                at(0)
            } else if channel < source_channels.min(2) {
                at(channel)
            } else {
                0.0
            };
            out.push((value * gain).clamp(-1.0, 1.0));
        }
    }
    out
}

/// Plays `sound` once on `output` at `volume` (the factor `Ak` takes, 1 for most), on its own thread. Returns at once;
/// a missing speaker or a failed stream is logged as a warning and the handle reports finished.
pub fn play(sound: Arc<Decoded>, volume: f32, output: Output, handle: &Handle) {
    spawn_play(sound, volume, output, false, handle);
}

/// Plays `sound` in a loop on `output` at `volume` until `handle.stop()` is called.
pub fn play_looped(sound: Arc<Decoded>, volume: f32, output: Output, handle: &Handle) {
    spawn_play(sound, volume, output, true, handle);
}

fn spawn_play(sound: Arc<Decoded>, volume: f32, output: Output, looped: bool, handle: &Handle) {
    let handle = handle.clone();
    let done = handle.finished.clone();
    let spawned = std::thread::Builder::new()
        .name("dc-voice-sound".into())
        .spawn({
            let handle = handle.clone();
            move || {
                if let Err(error) = run(&sound, volume, output, looped, &handle) {
                    warn!(?output, "sound not played: {error}");
                }
                handle.finished.store(true, Ordering::Release);
            }
        });
    if spawned.is_err() {
        done.store(true, Ordering::Release);
    }
}

fn next_sample(samples: &[f32], start: usize, offset: usize, looped: bool) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    if looped {
        samples[(start + offset) % samples.len()]
    } else {
        samples.get(start + offset).copied().unwrap_or(0.0)
    }
}

fn advance_position(samples_len: usize, start: usize, count: usize, looped: bool) -> usize {
    if samples_len == 0 {
        return 0;
    }
    if looped {
        (start + count) % samples_len
    } else {
        start.saturating_add(count)
    }
}

fn run(
    sound: &Decoded,
    volume: f32,
    output: Output,
    looped: bool,
    handle: &Handle,
) -> Result<(), &'static str> {
    if handle.is_stopped() {
        return Ok(());
    }
    let host = cpal::default_host();
    let device = match output {
        Output::Voice => pick(host.output_devices().ok(), prefs().output_device())
            .or_else(|| host.default_output_device())
            .or_else(|| host.output_devices().ok().and_then(|mut list| list.next())),
        Output::Default => host
            .default_output_device()
            .or_else(|| host.output_devices().ok().and_then(|mut list| list.next())),
    }
    .ok_or("No speaker is available")?;
    let config = output_config(&device)?;
    let channels = usize::from(config.channels());
    let samples = Arc::new(render(
        sound,
        config.sample_rate(),
        channels,
        gain(prefs().output_gain(), volume),
    ));
    let total = samples.len();
    if looped && total == 0 {
        return Ok(());
    }
    let position = Arc::new(AtomicUsize::new(0));
    let stream_config = config.config();
    let stream = match config.sample_format() {
        cpal::SampleFormat::F32 => {
            stream::<f32>(&device, &stream_config, &samples, &position, looped)
        }
        cpal::SampleFormat::I16 => {
            stream::<i16>(&device, &stream_config, &samples, &position, looped)
        }
        cpal::SampleFormat::I32 => {
            stream::<i32>(&device, &stream_config, &samples, &position, looped)
        }
        cpal::SampleFormat::U16 => {
            stream::<u16>(&device, &stream_config, &samples, &position, looped)
        }
        _ => Err("Speaker sample format is not supported"),
    }?;
    stream
        .play()
        .map_err(|_| "Could not start speaker playback")?;
    if looped {
        while !handle.is_stopped() {
            std::thread::sleep(Duration::from_millis(10));
        }
    } else {
        let started = Instant::now();
        let limit = sound.duration() + Duration::from_secs(2);
        while !handle.is_stopped()
            && position.load(Ordering::Acquire) < total
            && started.elapsed() < limit
        {
            std::thread::sleep(Duration::from_millis(10));
        }
        // What was handed to the device a moment ago is still in its buffer.
        if !handle.is_stopped() {
            std::thread::sleep(Duration::from_millis(120));
        }
    }
    Ok(())
}

fn stream<T>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    samples: &Arc<Vec<f32>>,
    position: &Arc<AtomicUsize>,
    looped: bool,
) -> Result<cpal::Stream, &'static str>
where
    T: cpal::SizedSample + cpal::FromSample<f32>,
{
    let (samples, position) = (samples.clone(), position.clone());
    device
        .build_output_stream(
            *config,
            move |data: &mut [T], _| {
                let start = position.load(Ordering::Acquire);
                for (offset, slot) in data.iter_mut().enumerate() {
                    let value = next_sample(&samples, start, offset, looped);
                    *slot = T::from_sample(value);
                }
                position.store(
                    advance_position(samples.len(), start, data.len(), looped),
                    Ordering::Release,
                );
            },
            |error| warn!("sound playback failed: {error}"),
            None,
        )
        .map_err(|_| "Could not open speaker device")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Silent MPEG-1 Layer III frames: 44.1 kHz, 128 kbit/s, no CRC, no padding (417 bytes each). A header, then a
    /// zeroed side info and main data, which a decoder reads as 1152 samples of silence per channel.
    fn silent_mp3(frames: usize, mono: bool) -> Vec<u8> {
        let mode = if mono { 0xC0 } else { 0x00 };
        let mut file = Vec::new();
        for _ in 0..frames {
            let mut frame = vec![0u8; 417];
            frame[..4].copy_from_slice(&[0xFF, 0xFB, 0x90, mode]);
            file.extend_from_slice(&frame);
        }
        file
    }

    #[test]
    fn decodes_the_length_and_rate_of_an_mp3() {
        let sound = decode_mp3(silent_mp3(12, false)).expect("a valid mp3 decodes");
        assert_eq!(sound.rate, 44_100);
        assert_eq!(sound.channels, 2);
        // 12 frames of 1152 samples; a decoder may drop the first frame while its reservoir fills.
        let frames = sound.frames();
        assert!(
            (11 * 1152..=12 * 1152).contains(&frames),
            "decoded {frames} frames"
        );
        assert!(sound.samples.iter().all(|sample| sample.abs() < 1e-6));
        let seconds = sound.duration().as_secs_f64();
        assert!((seconds - frames as f64 / 44_100.).abs() < 1e-9);
        assert!((0.28..=0.32).contains(&seconds), "{seconds}");
    }

    #[test]
    fn decodes_mono_and_refuses_what_is_not_an_mp3() {
        let sound = decode_mp3(silent_mp3(8, true)).expect("mono decodes");
        assert_eq!((sound.rate, sound.channels), (44_100, 1));
        assert!(decode_mp3(b"<html>404</html>".to_vec()).is_err());
        assert!(decode_any(b"<html>404</html>".to_vec()).is_err());
        assert!(decode_any(silent_mp3(12, false)).is_ok());
        // A soundboard file need not be an mp3: a minimal 16-bit mono WAV, a tenth of a second at 8 kHz.
        let data = vec![0u8; 1600];
        let mut wav = Vec::new();
        wav.extend_from_slice(b"RIFF");
        wav.extend_from_slice(&(36 + data.len() as u32).to_le_bytes());
        wav.extend_from_slice(b"WAVEfmt ");
        wav.extend_from_slice(&16u32.to_le_bytes());
        wav.extend_from_slice(&1u16.to_le_bytes());
        wav.extend_from_slice(&1u16.to_le_bytes());
        wav.extend_from_slice(&8_000u32.to_le_bytes());
        wav.extend_from_slice(&16_000u32.to_le_bytes());
        wav.extend_from_slice(&2u16.to_le_bytes());
        wav.extend_from_slice(&16u16.to_le_bytes());
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&(data.len() as u32).to_le_bytes());
        wav.extend_from_slice(&data);
        let sound = decode_any(wav).expect("a wav file decodes");
        assert_eq!((sound.rate, sound.channels), (8_000, 1));
        assert!(decode_mp3(Vec::new()).is_err());
    }

    #[test]
    fn volume_is_the_output_volume_times_the_factor_capped_at_one() {
        assert_eq!(gain(1.0, 0.4), 0.4);
        assert_eq!(gain(0.5, 1.0), 0.5);
        assert_eq!(gain(2.0, 1.0), 1.0, "the 200 % slider cannot go past unity");
        assert_eq!(gain(2.0, 0.4), 0.8);
        assert_eq!(gain(0.0, 1.0), 0.0);
    }

    #[test]
    fn rendering_converts_the_rate_and_the_channels() {
        let stereo = Decoded {
            rate: 24_000,
            channels: 2,
            samples: (0..2400).flat_map(|_| [0.5f32, -0.5]).collect(),
        };
        // 24 kHz to 48 kHz doubles the frames; a stereo device keeps left and right.
        let out = render(&stereo, 48_000, 2, 1.0);
        assert_eq!(out.len(), 2 * 2 * 2400);
        assert!(out.chunks(2).all(|f| f == [0.5, -0.5]));
        // A mono device gets the mean, a 4-channel device only the front pair.
        assert!(
            render(&stereo, 24_000, 1, 1.0)
                .iter()
                .all(|s| s.abs() < 1e-6)
        );
        let quad = render(&stereo, 24_000, 4, 0.5);
        assert!(quad.chunks(4).all(|f| f == [0.25, -0.25, 0.0, 0.0]));

        let mono = Decoded {
            rate: 48_000,
            channels: 1,
            samples: vec![0.8; 100],
        };
        assert!(
            render(&mono, 48_000, 2, 0.5)
                .chunks(2)
                .all(|f| f == [0.4, 0.4])
        );
        assert!(render(&mono, 48_000, 0, 1.0).is_empty());
    }

    #[test]
    fn a_stopped_handle_plays_nothing() {
        let handle = Handle::new();
        handle.stop();
        let sound = Decoded {
            rate: 48_000,
            channels: 1,
            samples: vec![0.1; 480],
        };
        // Stopped before it starts: returns without opening a device.
        assert!(run(&sound, 1.0, Output::Default, false, &handle).is_ok());
        assert!(handle.is_stopped() && !handle.is_finished());
    }

    #[test]
    fn looped_samples_wrap_cleanly_across_periods() {
        let samples = [0.1f32, 0.2, 0.3];
        let mut cursor = 0usize;
        let mut out = Vec::new();
        for _ in 0..7 {
            out.push(next_sample(&samples, cursor, 0, true));
            cursor = advance_position(samples.len(), cursor, 1, true);
        }
        assert_eq!(out, [0.1, 0.2, 0.3, 0.1, 0.2, 0.3, 0.1]);
        assert_eq!(cursor, 1);

        // Multi-sample block wrapping over two period boundaries in one fill.
        let block: Vec<f32> = (0..5)
            .map(|offset| next_sample(&samples, 2, offset, true))
            .collect();
        assert_eq!(block, [0.3, 0.1, 0.2, 0.3, 0.1]);
        assert_eq!(advance_position(samples.len(), 2, 5, true), 1);

        // Non-looped fills pad with silence once the buffer runs out.
        let one_shot: Vec<f32> = (0..5)
            .map(|offset| next_sample(&samples, 1, offset, false))
            .collect();
        assert_eq!(one_shot, [0.2, 0.3, 0.0, 0.0, 0.0]);
    }
}
