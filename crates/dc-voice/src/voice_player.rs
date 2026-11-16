//! Playing voice messages: one engine for the whole app, one message at a time (the official player pauses the others
//! when one starts, `VOICE_MESSAGE_PLAYBACK_STARTED` in module 383055), with play and pause, seeking, the four speeds
//! of module 518950 and a volume.
//!
//! The official player is an `<audio>` element, so a speed change keeps the pitch (`preservesPitch` is on by default).
//! The same is done here with WSOLA: windows of 40 ms are taken from the source at the speed's pace, each slid up to
//! 10 ms to the place that continues the previous window best, and added together with Hann weights. At 1x the audio
//! is copied untouched.
//!
//! A worker thread decodes nothing (the file is decoded once into mono 48 kHz samples when the message is first
//! played) and fills a ring that the speaker's callback drains. With the silent sink (capture runs, which must never
//! make a sound) the worker renders the same samples at the pace of the clock and drops them, so state and counts are
//! real.

use crate::{audio::output_config, ogg_opus};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::sync::{
    Arc, Mutex, OnceLock,
    atomic::{AtomicBool, AtomicU64, Ordering},
    mpsc,
};
use std::time::{Duration, Instant};
use tracing::{debug, info};

/// Samples per second of everything the engine renders.
const RATE: usize = 48_000;
/// Hann window length, hop and search range of the time stretch.
const WINDOW: usize = 1920;
const HOP: usize = WINDOW / 2;
const SEARCH: usize = 480;
/// How much rendered audio waits for the speaker: 100 ms.
const AHEAD: usize = RATE / 10;

/// A decoded voice message: mono samples at 48 kHz.
#[derive(Debug)]
pub struct Clip {
    samples: Vec<f32>,
}

impl Clip {
    /// Decodes an Ogg Opus file.
    pub fn from_ogg(bytes: &[u8]) -> Result<Self, &'static str> {
        let pcm = ogg_opus::decode(bytes)?;
        Ok(Self {
            samples: pcm.samples,
        })
    }

    /// Any audio file (mp3, ogg, wav, flac, m4a): decoded whole, mixed down to mono at 48 kHz.
    pub fn from_file(bytes: Vec<u8>) -> Result<Self, &'static str> {
        let sound = crate::sound::decode_attachment(bytes)?;
        Ok(Self {
            samples: crate::sound::render(&sound, RATE as u32, 1, 1.0),
        })
    }

    pub fn from_samples(samples: Vec<f32>) -> Self {
        Self { samples }
    }

    pub fn seconds(&self) -> f64 {
        self.samples.len() as f64 / RATE as f64
    }

    pub fn len(&self) -> usize {
        self.samples.len()
    }

    pub fn is_empty(&self) -> bool {
        self.samples.is_empty()
    }
}

/// What the engine is doing, as the player reads it each frame.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Status {
    /// The message that is loaded (the one asked to play last).
    pub key: Option<String>,
    pub playing: bool,
    /// Played to its end; cleared by the next play or seek.
    pub ended: bool,
    /// Seconds into the message.
    pub position: f64,
    /// Samples rendered since the message was started, at 48 kHz after the speed change.
    pub rendered: u64,
    /// Why it could not play.
    pub error: Option<String>,
    /// The number of the last [`Engine::play`] applied: a player compares it with the number its own play returned to
    /// know whether this status already answers that play.
    pub play_seq: u64,
}

enum Command {
    Play {
        key: String,
        clip: Arc<Clip>,
        from: f64,
        seq: u64,
    },
    Pause,
    Seek(f64),
    Speed(f32),
    Volume(f32),
    Muted(bool),
    Stop,
}

pub struct Engine {
    commands: mpsc::Sender<Command>,
    status: Arc<Mutex<Status>>,
    plays: AtomicU64,
}

static SILENT: AtomicBool = AtomicBool::new(false);

/// Capture runs and tests never open the speaker; set before the first use of [`engine`].
pub fn set_silent(silent: bool) {
    SILENT.store(silent, Ordering::Release);
}

/// The one engine of the app, started on first use.
pub fn engine() -> &'static Engine {
    static ENGINE: OnceLock<Engine> = OnceLock::new();
    ENGINE.get_or_init(|| Engine::start(SILENT.load(Ordering::Acquire)))
}

impl Engine {
    /// Starts a worker. `silent` renders without a speaker.
    pub fn start(silent: bool) -> Self {
        let (commands, receive) = mpsc::channel();
        let status = Arc::new(Mutex::new(Status::default()));
        let shared = status.clone();
        let _ = std::thread::Builder::new()
            .name("dc-voice-message".into())
            .spawn(move || Worker::new(silent, shared).run(&receive));
        Self {
            commands,
            status,
            plays: AtomicU64::new(0),
        }
    }

    /// An engine that reports `status` and never applies a command, for tests of players that must not trust the
    /// status before the engine has caught up with their last command. The returned handle sets what it reports.
    #[doc(hidden)]
    pub fn stalled(status: Status) -> (Self, Arc<Mutex<Status>>) {
        let (commands, receive) = mpsc::channel();
        // Commands are kept, unread, for as long as the engine lives.
        std::mem::forget(receive);
        let status = Arc::new(Mutex::new(status));
        (
            Self {
                commands,
                status: status.clone(),
                plays: AtomicU64::new(0),
            },
            status,
        )
    }

    /// Starts `clip` (the message `key`) at `from` seconds; whatever played before stops. Returns the number this play
    /// will carry in [`Status::play_seq`] once applied.
    pub fn play(&self, key: &str, clip: Arc<Clip>, from: f64) -> u64 {
        let seq = self.plays.fetch_add(1, Ordering::Relaxed) + 1;
        let _ = self.commands.send(Command::Play {
            key: key.to_owned(),
            clip,
            from,
            seq,
        });
        seq
    }

    pub fn pause(&self) {
        let _ = self.commands.send(Command::Pause);
    }

    /// Resumes the loaded message, or restarts it when it has ended.
    pub fn seek(&self, seconds: f64) {
        let _ = self.commands.send(Command::Seek(seconds));
    }

    /// One of [`crate::voice_wave::SPEEDS`].
    pub fn set_speed(&self, speed: f32) {
        let _ = self.commands.send(Command::Speed(speed));
    }

    /// 0 to 1, the player's volume slider.
    pub fn set_volume(&self, volume: f32) {
        let _ = self.commands.send(Command::Volume(volume));
    }

    pub fn set_muted(&self, muted: bool) {
        let _ = self.commands.send(Command::Muted(muted));
    }

    /// Unloads the message.
    pub fn stop(&self) {
        let _ = self.commands.send(Command::Stop);
    }

    pub fn status(&self) -> Status {
        self.status
            .lock()
            .map(|status| status.clone())
            .unwrap_or_default()
    }
}

/// The time stretcher: reads a source at `speed` times real time and hands out the audio at its own pitch.
#[derive(Debug, Default)]
struct Stretch {
    /// Where the next window nominally starts, in source samples.
    position: f64,
    /// The falling half of the last window, to be added to the next hop.
    tail: Vec<f32>,
    /// Where the last window started.
    last: Option<usize>,
}

fn hann() -> &'static [f32] {
    static TABLE: OnceLock<Vec<f32>> = OnceLock::new();
    TABLE.get_or_init(|| {
        (0..WINDOW)
            .map(|index| 0.5 - 0.5 * (std::f32::consts::TAU * index as f32 / WINDOW as f32).cos())
            .collect()
    })
}

fn at(source: &[f32], index: usize) -> f32 {
    source.get(index).copied().unwrap_or(0.)
}

impl Stretch {
    fn seek(&mut self, sample: usize) {
        self.position = sample as f64;
        self.tail.clear();
        self.last = None;
    }

    fn finished(&self, source: &[f32]) -> bool {
        self.position >= source.len() as f64
    }

    /// The next 20 ms of output.
    fn next(&mut self, source: &[f32], speed: f32) -> Vec<f32> {
        if (speed - 1.).abs() < 1e-3 {
            let start = self.position as usize;
            self.tail.clear();
            self.last = None;
            self.position += HOP as f64;
            return (0..HOP).map(|index| at(source, start + index)).collect();
        }
        let nominal = self.position as usize;
        let start = match self.last {
            Some(previous) if !self.tail.is_empty() => {
                // The segment that would have followed the last window if the source had played on.
                let natural: Vec<f32> = (0..HOP)
                    .map(|index| at(source, previous + HOP + index))
                    .collect();
                let low = nominal.saturating_sub(SEARCH);
                let high = nominal + SEARCH;
                let (mut best, mut best_score) = (nominal, f32::MIN);
                let mut candidate = low;
                while candidate <= high {
                    let (mut dot, mut energy) = (0f32, 1e-6f32);
                    let mut index = 0;
                    while index < HOP {
                        let sample = at(source, candidate + index);
                        dot += sample * natural[index];
                        energy += sample * sample;
                        index += 4;
                    }
                    let score = dot / energy.sqrt();
                    if score > best_score {
                        best_score = score;
                        best = candidate;
                    }
                    candidate += 2;
                }
                best
            }
            _ => nominal,
        };
        let window = hann();
        let mut out: Vec<f32> = Vec::with_capacity(HOP);
        if self.tail.is_empty() {
            // The first window after a start or a seek: its rising half is played as it is.
            out.extend((0..HOP).map(|index| at(source, start + index)));
        } else {
            out.extend(
                (0..HOP).map(|index| self.tail[index] + at(source, start + index) * window[index]),
            );
        }
        self.tail = (0..HOP)
            .map(|index| at(source, start + HOP + index) * window[HOP + index])
            .collect();
        self.last = Some(start);
        self.position += HOP as f64 * f64::from(speed);
        out
    }
}

struct Worker {
    silent: bool,
    status: Arc<Mutex<Status>>,
    /// The number of the last play applied.
    play_seq: u64,
    key: Option<String>,
    clip: Option<Arc<Clip>>,
    stretch: Stretch,
    playing: bool,
    ended: bool,
    speed: f32,
    volume: f32,
    muted: bool,
    rendered: u64,
    sink: Option<Sink>,
    /// The silent sink's clock.
    paced_at: Instant,
    owed: f64,
    error: Option<String>,
}

/// The speaker: a ring the callback reads.
struct Sink {
    _stream: cpal::Stream,
    ring: rtrb::Producer<f32>,
}

impl Worker {
    fn new(silent: bool, status: Arc<Mutex<Status>>) -> Self {
        Self {
            silent,
            status,
            play_seq: 0,
            key: None,
            clip: None,
            stretch: Stretch::default(),
            playing: false,
            ended: false,
            speed: 1.,
            volume: 1.,
            muted: false,
            rendered: 0,
            sink: None,
            paced_at: Instant::now(),
            owed: 0.,
            error: None,
        }
    }

    fn run(&mut self, receive: &mpsc::Receiver<Command>) {
        loop {
            match receive.recv_timeout(Duration::from_millis(5)) {
                Ok(command) => {
                    self.apply(command);
                    // Commands that arrive together are all read before anything is rendered.
                    while let Ok(command) = receive.try_recv() {
                        self.apply(command);
                    }
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => return,
            }
            self.render();
            self.publish();
        }
    }

    fn apply(&mut self, command: Command) {
        match command {
            Command::Play {
                key,
                clip,
                from,
                seq,
            } => {
                self.play_seq = seq;
                let same = self.key.as_deref() == Some(key.as_str());
                if !same || self.clip.is_none() {
                    self.rendered = 0;
                }
                info!(
                    samples = clip.len(),
                    seconds = clip.seconds(),
                    from,
                    speed = self.speed,
                    silent = self.silent,
                    "voice message play"
                );
                let start = if from >= clip.seconds() {
                    0.
                } else {
                    from.max(0.)
                };
                self.stretch.seek((start * RATE as f64) as usize);
                self.key = Some(key);
                self.clip = Some(clip);
                self.ended = false;
                self.error = None;
                self.playing = true;
                self.paced_at = Instant::now();
                self.owed = 0.;
                self.flush();
            }
            Command::Pause => {
                if self.playing {
                    info!(rendered = self.rendered, "voice message paused");
                }
                self.playing = false;
                self.flush();
            }
            Command::Seek(seconds) => {
                if let Some(clip) = &self.clip {
                    let seconds = seconds.clamp(0., clip.seconds());
                    self.stretch.seek((seconds * RATE as f64) as usize);
                    self.ended = false;
                    self.flush();
                }
            }
            Command::Speed(speed) => {
                self.speed = speed.clamp(0.5, 2.);
                self.stretch.tail.clear();
                self.stretch.last = None;
            }
            Command::Volume(volume) => self.volume = volume.clamp(0., 1.),
            Command::Muted(muted) => self.muted = muted,
            Command::Stop => {
                self.playing = false;
                self.clip = None;
                self.key = None;
                self.ended = false;
                self.flush();
            }
        }
    }

    /// Drops what is waiting for the speaker (after a seek or a pause).
    fn flush(&mut self) {
        // The ring has one writer and the callback is its only reader, so the way to empty it is to start over.
        self.sink = None;
    }

    fn pending(&self) -> usize {
        self.sink
            .as_ref()
            .map_or(0, |sink| sink.ring.buffer().capacity() - sink.ring.slots())
    }

    fn render(&mut self) {
        if !self.playing {
            self.paced_at = Instant::now();
            return;
        }
        let Some(clip) = self.clip.clone() else {
            return;
        };
        let source = &clip.samples;
        if self.silent {
            self.owed += self.paced_at.elapsed().as_secs_f64() * RATE as f64;
            self.paced_at = Instant::now();
            while self.owed >= HOP as f64 && !self.stretch.finished(source) {
                let hop = self.stretch.next(source, self.speed);
                self.rendered += hop.len() as u64;
                self.owed -= HOP as f64;
            }
        } else {
            if self.sink.is_none() {
                match open_sink() {
                    Ok(sink) => self.sink = Some(sink),
                    Err(error) => {
                        debug!("voice message not played: {error}");
                        self.error = Some(error.to_owned());
                        self.playing = false;
                        return;
                    }
                }
            }
            let gain = if self.muted { 0. } else { self.volume };
            while self.pending() < AHEAD && !self.stretch.finished(source) {
                let hop = self.stretch.next(source, self.speed);
                self.rendered += hop.len() as u64;
                if let Some(sink) = &mut self.sink {
                    for sample in hop {
                        let _ = sink.ring.push((sample * gain).clamp(-1., 1.));
                    }
                }
            }
        }
        if self.stretch.finished(source) && (self.silent || self.pending() == 0) {
            info!(rendered = self.rendered, "voice message ended");
            self.playing = false;
            self.ended = true;
            self.flush();
        }
    }

    fn publish(&self) {
        let position = match &self.clip {
            Some(clip) if self.ended => clip.seconds(),
            Some(clip) => {
                let ahead = self.pending() as f64 * f64::from(self.speed) / RATE as f64;
                ((self.stretch.position / RATE as f64) - ahead).clamp(0., clip.seconds())
            }
            None => 0.,
        };
        if let Ok(mut status) = self.status.lock() {
            *status = Status {
                key: self.key.clone(),
                playing: self.playing,
                ended: self.ended,
                position,
                rendered: self.rendered,
                error: self.error.clone(),
                play_seq: self.play_seq,
            };
        }
    }
}

/// Opens the system's default speaker. The volume of a voice message is the player's own slider, not the output
/// volume of Voice & Video.
fn open_sink() -> Result<Sink, &'static str> {
    let host = cpal::default_host();
    let device = host
        .default_output_device()
        .ok_or("No speaker is available")?;
    let config = output_config(&device)?;
    let channels = usize::from(config.channels());
    let step = RATE as f64 / f64::from(config.sample_rate());
    let (producer, mut consumer) = rtrb::RingBuffer::<f32>::new(RATE);
    let stream_config = config.config();
    let mut phase = 0f64;
    let (mut previous, mut next) = (0f32, 0f32);
    let mut primed = false;
    let mut fill = move |data: &mut [f32]| {
        for frame in data.chunks_mut(channels.max(1)) {
            if !primed {
                previous = consumer.pop().unwrap_or(0.);
                next = consumer.pop().unwrap_or(previous);
                primed = true;
            }
            let value = previous + (next - previous) * phase as f32;
            phase += step;
            while phase >= 1. {
                phase -= 1.;
                previous = next;
                next = consumer.pop().unwrap_or(0.);
            }
            frame.fill(value);
        }
    };
    let stream = match config.sample_format() {
        cpal::SampleFormat::F32 => device.build_output_stream(
            stream_config,
            move |data: &mut [f32], _| fill(data),
            |_| {},
            None,
        ),
        cpal::SampleFormat::I16 => {
            let mut scratch: Vec<f32> = Vec::new();
            device.build_output_stream(
                stream_config,
                move |data: &mut [i16], _| {
                    scratch.resize(data.len(), 0.);
                    fill(&mut scratch);
                    for (slot, value) in data.iter_mut().zip(&scratch) {
                        *slot = (value.clamp(-1., 1.) * f32::from(i16::MAX)) as i16;
                    }
                },
                |_| {},
                None,
            )
        }
        _ => return Err("Speaker sample format is not supported"),
    }
    .map_err(|_| "Could not open speaker device")?;
    stream
        .play()
        .map_err(|_| "Could not start speaker playback")?;
    Ok(Sink {
        _stream: stream,
        ring: producer,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(seconds: f32, hertz: f32) -> Vec<f32> {
        (0..(seconds * RATE as f32) as usize)
            .map(|index| 0.5 * (std::f32::consts::TAU * hertz * index as f32 / RATE as f32).sin())
            .collect()
    }

    fn render_all(source: &[f32], speed: f32) -> Vec<f32> {
        let mut stretch = Stretch::default();
        let mut out = Vec::new();
        while !stretch.finished(source) {
            out.extend(stretch.next(source, speed));
        }
        out
    }

    fn crossings(samples: &[f32]) -> usize {
        samples
            .windows(2)
            .filter(|pair| pair[0] < 0. && pair[1] >= 0.)
            .count()
    }

    #[test]
    fn normal_speed_copies_the_source() {
        let source = tone(1., 440.);
        let out = render_all(&source, 1.);
        assert_eq!(&out[..source.len()], &source[..]);
        assert!(out.len() - source.len() < HOP);
    }

    #[test]
    fn faster_and_slower_change_the_length_and_keep_the_pitch() {
        let source = tone(2., 440.);
        for speed in [0.75f32, 1.5, 2.] {
            let out = render_all(&source, speed);
            let expected = source.len() as f32 / speed;
            assert!(
                (out.len() as f32 - expected).abs() < 2. * HOP as f32,
                "speed {speed}: {} samples, expected {expected}",
                out.len()
            );
            // Half a second from the middle keeps 440 cycles per second of output, which is what pitch preservation means.
            let middle = &out[out.len() / 4..out.len() / 4 + RATE / 2];
            let cycles = crossings(middle);
            assert!(
                (215..=225).contains(&cycles),
                "speed {speed}: {cycles} cycles in half a second"
            );
            let energy = middle.iter().map(|s| s * s).sum::<f32>() / (RATE / 2) as f32;
            assert!(
                (0.10..0.15).contains(&energy),
                "speed {speed}: mean square {energy}"
            );
        }
    }

    #[test]
    fn seeking_restarts_the_stretch_there() {
        let source = tone(2., 440.);
        let mut stretch = Stretch::default();
        stretch.seek(RATE);
        let hop = stretch.next(&source, 1.5);
        assert_eq!(hop.len(), HOP);
        assert!((stretch.position - (RATE + HOP * 3 / 2) as f64).abs() < 1.);
        assert!(!stretch.finished(&source));
        stretch.seek(source.len());
        assert!(stretch.finished(&source));
    }

    #[test]
    fn the_silent_engine_plays_a_message_by_state_and_counts() {
        let engine = Engine::start(true);
        let clip = Arc::new(Clip::from_samples(tone(0.5, 300.)));
        assert_eq!(clip.seconds(), 0.5);
        engine.set_speed(2.);
        engine.play("a", clip.clone(), 0.);
        std::thread::sleep(Duration::from_millis(100));
        let early = engine.status();
        assert_eq!(early.key.as_deref(), Some("a"));
        assert!(early.playing && !early.ended, "{early:?}");
        assert!(early.position > 0.05 && early.position < 0.5, "{early:?}");
        assert!(early.rendered > 0);
        // At 2x the half-second message is over in about a quarter of a second.
        std::thread::sleep(Duration::from_millis(400));
        let done = engine.status();
        assert!(done.ended && !done.playing, "{done:?}");
        assert!((done.position - 0.5).abs() < 1e-9);
        let rendered = done.rendered as f64 / RATE as f64;
        assert!((0.2..0.32).contains(&rendered), "{rendered} s rendered");
        // Pausing and seeking: a paused engine renders nothing more.
        engine.play("b", clip, 0.1);
        engine.pause();
        std::thread::sleep(Duration::from_millis(60));
        let paused = engine.status();
        assert!(!paused.playing);
        let held = paused.rendered;
        std::thread::sleep(Duration::from_millis(60));
        assert_eq!(engine.status().rendered, held);
        engine.seek(0.4);
        std::thread::sleep(Duration::from_millis(40));
        assert!((engine.status().position - 0.4).abs() < 0.05);
        engine.stop();
        std::thread::sleep(Duration::from_millis(40));
        assert_eq!(engine.status().key, None);
    }
}
