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
use tracing::{debug, info, warn};

/// Samples per second of everything the engine renders.
const RATE: usize = 48_000;
/// Hann window length, hop and search range of the time stretch.
const WINDOW: usize = 1920;
const HOP: usize = WINDOW / 2;
const SEARCH: usize = 480;
/// How much rendered audio waits for the speaker: 100 ms.
const AHEAD: usize = RATE / 10;
/// How long a paused or finished player keeps its speaker stream open.
const IDLE_CLOSE: Duration = Duration::from_secs(5);
/// How often a playing message is rendered ahead and its position published.
const TICK: Duration = Duration::from_millis(5);

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
    /// Seconds into the message that are being heard now: what was rendered, less what waits in the ring and in the
    /// device (as an `<audio>` element's `currentTime`). It does not go back before the point a play or seek started
    /// from while that point's audio is still on its way out.
    pub position: f64,
    /// The speaker's output latency in seconds (from the speaker's callback until its newest sample is heard), 0 when
    /// silent or not yet known.
    pub output_latency: f64,
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
    /// Where the last play or seek started, in seconds.
    start: f64,
    /// Since when nothing has played, to let the speaker go after [`IDLE_CLOSE`].
    idle_since: Option<Instant>,
}

/// What the speaker's callback and the worker share.
#[derive(Default)]
struct Shared {
    /// Samples pushed before the last flush: the callback drops what it has not played of them.
    discard_until: AtomicU64,
    /// Samples the callback has taken from the ring, played or dropped.
    taken: AtomicU64,
    /// When the callback last ran (nanoseconds since the sink opened), and how long from then until the newest sample
    /// it took is heard: the device's own delay (`playback - callback`) and the buffer it filled.
    callback_at: AtomicU64,
    heard_in: AtomicU64,
    /// The stream reported an error (the device went away): open a new one.
    failed: AtomicBool,
}

/// The speaker: a ring the callback reads. It stays open across pauses and seeks, as an `<audio>` element's output
/// does; a flush only tells the callback to drop what was queued.
struct Sink {
    _stream: cpal::Stream,
    ring: rtrb::Producer<f32>,
    /// Samples pushed since the sink opened.
    pushed: u64,
    shared: Arc<Shared>,
    opened: Instant,
    /// The device it plays on, to follow a change of the system default between plays.
    device: Option<String>,
}

impl Sink {
    fn push(&mut self, sample: f32) {
        if self.ring.push(sample).is_ok() {
            self.pushed += 1;
        }
    }

    /// Drops everything queued: the callback skips to what is pushed next, fading out what was sounding.
    fn flush(&mut self) {
        self.shared
            .discard_until
            .store(self.pushed, Ordering::Release);
    }

    /// Samples queued that will still be played.
    fn pending(&self) -> usize {
        let from = self
            .shared
            .taken
            .load(Ordering::Acquire)
            .max(self.shared.discard_until.load(Ordering::Acquire));
        self.pushed.saturating_sub(from) as usize
    }

    /// The device's output latency: from a callback until the newest sample it took is heard.
    fn latency(&self) -> f64 {
        self.shared.heard_in.load(Ordering::Acquire) as f64 / 1e9
    }

    /// Seconds until what the callback has taken so far is heard, now.
    fn unheard(&self) -> f64 {
        unheard_at(
            self.shared.heard_in.load(Ordering::Acquire),
            self.shared.callback_at.load(Ordering::Acquire),
            self.opened.elapsed().as_nanos() as u64,
        )
    }
}

/// Seconds until the newest sample a callback took is heard, `now` (nanoseconds, as `callback_at`): it was `heard_in`
/// away at the callback, and time has passed since.
fn unheard_at(heard_in: u64, callback_at: u64, now: u64) -> f64 {
    heard_in.saturating_sub(now.saturating_sub(callback_at)) as f64 / 1e9
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
            start: 0.,
            idle_since: None,
        }
    }

    fn run(&mut self, receive: &mpsc::Receiver<Command>) {
        loop {
            let received = match self.wake_in(Instant::now()) {
                Some(wait) => receive.recv_timeout(wait),
                // Nothing plays and the speaker is let go: nothing changes until a command comes.
                None => receive
                    .recv()
                    .map_err(|_| mpsc::RecvTimeoutError::Disconnected),
            };
            match received {
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

    /// How long the worker may wait for a command before it must render again: [`TICK`] while a message plays, the
    /// time left until the open speaker is let go while it is paused or ended, and no limit once it is let go.
    fn wake_in(&self, now: Instant) -> Option<Duration> {
        if self.playing {
            return Some(TICK);
        }
        self.sink.as_ref()?;
        Some(match self.idle_since {
            Some(since) => (since + IDLE_CLOSE).saturating_duration_since(now),
            // Not idle yet as far as `render` knows: it starts the count on the next pass.
            None => Duration::ZERO,
        })
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
                // A play from silence follows a change of the system's default speaker; a play over a playing
                // message stays on the open stream.
                if !self.playing
                    && self
                        .sink
                        .as_ref()
                        .is_some_and(|sink| sink.device != default_device())
                {
                    self.sink = None;
                }
                self.stretch.seek((start * RATE as f64) as usize);
                self.start = start;
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
                    // What was rendered but not heard is dropped: resume from what was heard.
                    let heard = self.heard_position();
                    self.stretch.seek((heard * RATE as f64) as usize);
                    self.start = heard;
                }
                self.playing = false;
                self.flush();
            }
            Command::Seek(seconds) => {
                if let Some(clip) = &self.clip {
                    let seconds = seconds.clamp(0., clip.seconds());
                    self.stretch.seek((seconds * RATE as f64) as usize);
                    self.start = seconds;
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
                // Nothing is loaded: the speaker is let go.
                self.sink = None;
            }
        }
    }

    /// Drops what is waiting for the speaker (after a seek or a pause). The stream stays open.
    fn flush(&mut self) {
        if let Some(sink) = &mut self.sink {
            sink.flush();
        }
    }

    fn pending(&self) -> usize {
        self.sink.as_ref().map_or(0, Sink::pending)
    }

    /// Seconds into the message being heard now: rendered, less what waits in the ring and in the device.
    fn heard_position(&self) -> f64 {
        let Some(clip) = &self.clip else {
            return 0.;
        };
        heard_seconds(
            self.stretch.position,
            self.pending(),
            self.sink.as_ref().map_or(0., Sink::unheard),
            self.speed,
            self.start,
            clip.seconds(),
        )
    }

    fn render(&mut self) {
        if !self.playing {
            self.paced_at = Instant::now();
            // Paused or ended a while: the speaker is let go (a resume soon after keeps the open stream).
            let idle = *self.idle_since.get_or_insert_with(Instant::now);
            if self.sink.is_some() && idle.elapsed() >= IDLE_CLOSE {
                self.sink = None;
            }
            return;
        }
        self.idle_since = None;
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
            if self
                .sink
                .as_ref()
                .is_some_and(|sink| sink.shared.failed.load(Ordering::Acquire))
            {
                warn!("voice message speaker stream failed; reopening");
                self.sink = None;
            }
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
                        sink.push((sample * gain).clamp(-1., 1.));
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
            Some(_) => self.heard_position(),
            None => 0.,
        };
        let output_latency = self.sink.as_ref().map_or(0., Sink::latency);
        if let Ok(mut status) = self.status.lock() {
            *status = Status {
                key: self.key.clone(),
                playing: self.playing,
                ended: self.ended,
                position,
                output_latency,
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
    let channels = usize::from(config.channels()).max(1);
    let device_rate = f64::from(config.sample_rate());
    let (producer, consumer) = rtrb::RingBuffer::<f32>::new(RATE);
    let shared = Arc::new(Shared::default());
    let opened = Instant::now();
    let mut feed = Feed::new(consumer, shared.clone(), RATE as f64 / device_rate);
    let stream_config = config.config();
    let timing = shared.clone();
    let mut fill = move |data: &mut [f32], info: &cpal::OutputCallbackInfo| {
        feed.fill(data, channels);
        // The device's delay until this buffer starts sounding, and the buffer itself.
        let stamp = info.timestamp();
        let delay = stamp.playback.duration_since(stamp.callback)
            + Duration::from_secs_f64((data.len() / channels) as f64 / device_rate);
        timing
            .heard_in
            .store(delay.as_nanos() as u64, Ordering::Release);
        timing
            .callback_at
            .store(opened.elapsed().as_nanos() as u64, Ordering::Release);
    };
    let failed = shared.clone();
    let on_error = move |error: cpal::Error| {
        warn!("voice message speaker stream error: {error}");
        failed.failed.store(true, Ordering::Release);
    };
    let stream = match config.sample_format() {
        cpal::SampleFormat::F32 => device.build_output_stream(
            stream_config,
            move |data: &mut [f32], info| fill(data, info),
            on_error,
            None,
        ),
        cpal::SampleFormat::I16 => {
            let mut scratch: Vec<f32> = Vec::new();
            device.build_output_stream(
                stream_config,
                move |data: &mut [i16], info| {
                    scratch.resize(data.len(), 0.);
                    fill(&mut scratch, info);
                    for (slot, value) in data.iter_mut().zip(&scratch) {
                        *slot = (value.clamp(-1., 1.) * f32::from(i16::MAX)) as i16;
                    }
                },
                on_error,
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
        pushed: 0,
        shared,
        opened,
        device: device_name(&device),
    })
}

fn device_name(device: &cpal::Device) -> Option<String> {
    Some(device.to_string())
}

/// The system's default speaker now.
fn default_device() -> Option<String> {
    cpal::default_host()
        .default_output_device()
        .as_ref()
        .and_then(device_name)
}

/// The second of the source being heard: the stretch's read point (source samples), less what waits in the ring
/// (`pending`, output samples) and in the device (`unheard` seconds), both at `speed`; held at the point the last play
/// or seek started from until its audio is out, and within the clip.
fn heard_seconds(
    read: f64,
    pending: usize,
    unheard: f64,
    speed: f32,
    start: f64,
    duration: f64,
) -> f64 {
    let ahead = (pending as f64 / RATE as f64 + unheard) * f64::from(speed);
    (read / RATE as f64 - ahead).clamp(start.min(duration), duration)
}

/// Fades out what was sounding when a flush cuts it: 5 ms.
const FADE_SECONDS: f64 = 0.005;

/// The callback's side of the ring: linear resampling to the device rate, dropping what a flush discarded and fading
/// out over [`FADE_SECONDS`] what was sounding, so a pause or seek makes no click.
struct Feed {
    consumer: rtrb::Consumer<f32>,
    shared: Arc<Shared>,
    step: f64,
    phase: f64,
    previous: f32,
    next: f32,
    primed: bool,
    taken: u64,
    last: f32,
    fade_from: f32,
    fade_left: usize,
    fade_frames: usize,
}

impl Feed {
    fn new(consumer: rtrb::Consumer<f32>, shared: Arc<Shared>, step: f64) -> Self {
        Self {
            consumer,
            shared,
            step,
            phase: 0.,
            previous: 0.,
            next: 0.,
            primed: false,
            taken: 0,
            last: 0.,
            fade_from: 0.,
            fade_left: 0,
            fade_frames: ((RATE as f64 / step) * FADE_SECONDS).round().max(1.) as usize,
        }
    }

    fn pop(&mut self) -> f32 {
        match self.consumer.pop() {
            Ok(sample) => {
                self.taken += 1;
                sample
            }
            Err(_) => 0.,
        }
    }

    fn fill(&mut self, data: &mut [f32], channels: usize) {
        let discard = self.shared.discard_until.load(Ordering::Acquire);
        if self.taken < discard {
            while self.taken < discard && self.consumer.pop().is_ok() {
                self.taken += 1;
            }
            // What was sounding fades out under what comes next.
            self.fade_from = self.last;
            self.fade_left = self.fade_frames;
            self.primed = false;
            self.phase = 0.;
        }
        for frame in data.chunks_mut(channels) {
            if !self.primed {
                self.previous = self.pop();
                self.next = self.pop();
                self.primed = true;
            }
            let mut value = self.previous + (self.next - self.previous) * self.phase as f32;
            self.phase += self.step;
            while self.phase >= 1. {
                self.phase -= 1.;
                self.previous = self.next;
                self.next = self.pop();
            }
            if self.fade_left > 0 {
                let old = self.fade_left as f32 / self.fade_frames as f32;
                value = value * (1. - old) + self.fade_from * old;
                self.fade_left -= 1;
            }
            self.last = value;
            frame.fill(value);
        }
        self.shared.taken.store(self.taken, Ordering::Release);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The worker waits for commands without waking while nothing plays and no speaker is held, renders every
    /// [`TICK`] while a message plays, and wakes once to let a held speaker go.
    #[test]
    fn an_idle_worker_sleeps_until_a_command() {
        let now = Instant::now();
        let mut worker = Worker::new(true, Arc::new(Mutex::new(Status::default())));
        assert_eq!(worker.wake_in(now), None);
        worker.playing = true;
        assert_eq!(worker.wake_in(now), Some(TICK));
        worker.playing = false;
        worker.idle_since = Some(now);
        // A silent worker holds no speaker.
        assert_eq!(worker.wake_in(now), None);
    }

    /// A silent engine answers a play at once after sitting idle: the blocking wait is woken by the command.
    #[test]
    fn a_command_wakes_an_idle_engine() {
        let engine = Engine::start(true);
        std::thread::sleep(Duration::from_millis(50));
        let seq = engine.play("idle", Arc::new(Clip::from_samples(tone(1., 440.))), 0.);
        let started = Instant::now();
        while engine.status().play_seq != seq {
            assert!(
                started.elapsed() < Duration::from_secs(2),
                "the play was not applied"
            );
            std::thread::sleep(Duration::from_millis(1));
        }
        assert!(engine.status().playing);
        engine.stop();
    }

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

    /// A pause or seek on the open stream: what was queued before it is dropped, nothing more, and what was sounding
    /// fades into what comes next instead of being cut (no click); the stream is never reopened for it.
    #[test]
    fn a_flush_drops_the_queue_on_the_open_stream_and_fades_without_a_click() {
        let (mut producer, consumer) = rtrb::RingBuffer::<f32>::new(RATE);
        let shared = Arc::new(Shared::default());
        let mut feed = Feed::new(consumer, shared.clone(), 1.);
        for _ in 0..2000 {
            producer.push(0.5).unwrap();
        }
        let mut out = vec![0f32; 400];
        feed.fill(&mut out, 1);
        assert!(out[10..].iter().all(|s| (*s - 0.5).abs() < 1e-6));
        // Seek: the 1600 samples of the old place still queued are dropped, the new place follows.
        let before = 2000u64;
        shared.discard_until.store(before, Ordering::Release);
        for _ in 0..2000 {
            producer.push(-0.25).unwrap();
        }
        let mut after = vec![0f32; 1000];
        feed.fill(&mut after, 1);
        let mut previous = out[399];
        let mut largest_step = 0f32;
        for sample in &after {
            largest_step = largest_step.max((sample - previous).abs());
            previous = *sample;
        }
        assert!(largest_step < 0.01, "a step of {largest_step} is a click");
        let fade = feed.fade_frames;
        assert!(after[fade + 2..].iter().all(|s| (*s + 0.25).abs() < 1e-6));
        // Everything before the flush was taken (dropped), and only what played after it counts beyond that.
        let taken = shared.taken.load(Ordering::Acquire);
        assert!(taken > before && taken <= before + 1002, "{taken}");
    }

    #[test]
    fn the_position_is_what_is_heard_not_what_was_rendered() {
        // Two seconds read, 100 ms in the ring and 50 ms in the device: 1.85 s is being heard.
        let heard = heard_seconds(2. * RATE as f64, RATE / 10, 0.05, 1., 0., 10.);
        assert!((heard - 1.85).abs() < 1e-9, "{heard}");
        // At double speed the same queue is twice as much of the source.
        let fast = heard_seconds(2. * RATE as f64, RATE / 10, 0.05, 2., 0., 10.);
        assert!((fast - 1.7).abs() < 1e-9, "{fast}");
        // Right after a seek to 1.95 s, the new audio is not out yet: the position stays at the seek point.
        let sought = heard_seconds(2. * RATE as f64, RATE / 10, 0.05, 1., 1.95, 10.);
        assert!((sought - 1.95).abs() < 1e-9, "{sought}");
        // The device's part runs down between callbacks: 40 ms away at the callback, 15 ms later 25 ms remain.
        let unheard = unheard_at(40_000_000, 1_000_000_000, 1_015_000_000);
        assert!((unheard - 0.025).abs() < 1e-9, "{unheard}");
        assert_eq!(unheard_at(40_000_000, 1_000_000_000, 2_000_000_000), 0.);
    }
}
