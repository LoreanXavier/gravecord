//! The chosen (or default) microphone and speaker through cpal, at the user panel's volumes. Device callbacks only touch preallocated
//! lock-free rings; a worker thread moves 20 ms mono 48 kHz frames between those rings and
//! the voice loop's bounded channels, and reopens devices that fail.
//!
//! Adapted from Serein `crates/discord-voice/src/audio.rs` (`Gate`, `Streams::open`,
//! `input_config`, `input_stream`, `output_stream`, `Capture`, `Playback`) and
//! `crates/platform/src/audio.rs` (`output_config`), MIT OR Apache-2.0. Dropped: echo
//! cancellation, noise suppression, input gate, the
//! microphone preview, macOS permission prompts, PulseAudio buffer sizing and diagnostics.
use crate::Frame;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::sync::{
    Arc, Mutex, OnceLock,
    atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering},
    mpsc,
};
use std::time::{Duration, Instant};
use tracing::{debug, warn};

/// The user panel's device and volume choices (`MediaEngineStore`'s input/output device and volumes). Devices are
/// named as cpal names them; `None` follows the system default. Volumes are linear gains: input 0..1 (100 % at
/// the slider's end), output 0..2 (the slider's middle is 100 %).
pub struct Prefs {
    input: Mutex<Option<String>>,
    output: Mutex<Option<String>>,
    input_gain: AtomicU32,
    output_gain: AtomicU32,
    /// Bumped when a device changes, so the worker reopens its streams.
    pub(crate) generation: AtomicU64,
    /// Echo cancellation, Reduce Room Echo and Noise Suppression, read by the worker before every frame.
    pub processing: crate::process::ProcessingPrefs,
}

pub fn prefs() -> &'static Prefs {
    static PREFS: OnceLock<Prefs> = OnceLock::new();
    PREFS.get_or_init(|| Prefs {
        input: Mutex::new(None),
        output: Mutex::new(None),
        input_gain: AtomicU32::new(1.0f32.to_bits()),
        output_gain: AtomicU32::new(1.0f32.to_bits()),
        generation: AtomicU64::new(0),
        processing: crate::process::ProcessingPrefs::new(),
    })
}

impl Prefs {
    pub fn input_device(&self) -> Option<String> {
        self.input.lock().ok().and_then(|device| device.clone())
    }
    pub fn output_device(&self) -> Option<String> {
        self.output.lock().ok().and_then(|device| device.clone())
    }
    pub fn set_input_device(&self, device: Option<String>) {
        if let Ok(mut slot) = self.input.lock() {
            *slot = device;
        }
        self.generation.fetch_add(1, Ordering::AcqRel);
    }
    pub fn set_output_device(&self, device: Option<String>) {
        if let Ok(mut slot) = self.output.lock() {
            *slot = device;
        }
        self.generation.fetch_add(1, Ordering::AcqRel);
    }
    pub fn input_gain(&self) -> f32 {
        f32::from_bits(self.input_gain.load(Ordering::Relaxed))
    }
    pub fn output_gain(&self) -> f32 {
        f32::from_bits(self.output_gain.load(Ordering::Relaxed))
    }
    pub fn set_input_gain(&self, gain: f32) {
        self.input_gain
            .store(gain.clamp(0.0, 1.0).to_bits(), Ordering::Relaxed);
    }
    pub fn set_output_gain(&self, gain: f32) {
        self.output_gain
            .store(gain.clamp(0.0, 2.0).to_bits(), Ordering::Relaxed);
    }
}

/// The system's microphones and speakers by name, and the default ones.
pub fn input_devices() -> Vec<String> {
    let host = cpal::default_host();
    host.input_devices()
        .map(|devices| devices.map(|device| device.to_string()).collect())
        .unwrap_or_default()
}
pub fn output_devices() -> Vec<String> {
    let host = cpal::default_host();
    host.output_devices()
        .map(|devices| devices.map(|device| device.to_string()).collect())
        .unwrap_or_default()
}
pub fn default_input_device() -> Option<String> {
    cpal::default_host()
        .default_input_device()
        .map(|device| device.to_string())
}
pub fn default_output_device() -> Option<String> {
    cpal::default_host()
        .default_output_device()
        .map(|device| device.to_string())
}

/// The chosen device, or the default when none is chosen or it is gone.
fn pick(
    devices: Option<impl Iterator<Item = cpal::Device>>,
    wanted: Option<String>,
) -> Option<cpal::Device> {
    let wanted = wanted?;
    devices?.find(|device| device.to_string() == wanted)
}

/// Frames buffered in each device ring: 160 ms.
const RING: usize = 8;
const RECOVERY_DELAY: Duration = Duration::from_millis(250);
const RETRY_DELAY: Duration = Duration::from_secs(1);
const INPUT_RETRY_DELAY: Duration = Duration::from_secs(2);

#[derive(Default)]
pub(crate) struct Gate {
    /// The call is secured; otherwise both directions stay silent.
    ready: AtomicBool,
    muted: AtomicBool,
    deafened: AtomicBool,
    stopped: AtomicBool,
    output_failed: AtomicBool,
    input_failed: AtomicBool,
    /// Bumped when the microphone gate changes; capture drops partial PCM.
    capture_generation: AtomicU64,
    /// Bumped when the speaker gate changes; playback drops queued audio.
    playback_generation: AtomicU64,
}
impl Gate {
    fn capture(&self) -> bool {
        self.ready.load(Ordering::Acquire)
            && !self.muted.load(Ordering::Acquire)
            && !self.stopped.load(Ordering::Acquire)
    }
    fn playback(&self) -> bool {
        self.ready.load(Ordering::Acquire)
            && !self.deafened.load(Ordering::Acquire)
            && !self.stopped.load(Ordering::Acquire)
    }
}

/// Owns the device worker; dropping it stops the worker and closes both devices.
pub(crate) struct Audio {
    gate: Arc<Gate>,
    thread: std::thread::Thread,
}
impl Audio {
    /// `capture` receives microphone frames; `playback` supplies mixed frames to play.
    pub fn start(
        capture: mpsc::SyncSender<Frame>,
        playback: mpsc::Receiver<Frame>,
    ) -> Result<Self, &'static str> {
        let gate = Arc::new(Gate::default());
        let worker = gate.clone();
        let thread = std::thread::Builder::new()
            .name("dc-voice-audio".into())
            .spawn(move || run(&worker, &capture, &playback))
            .map_err(|_| "Could not start audio device worker")?;
        Ok(Self {
            gate,
            thread: thread.thread().clone(),
        })
    }
    pub fn set_ready(&self, ready: bool) {
        if self.gate.ready.swap(ready, Ordering::AcqRel) != ready {
            self.gate.capture_generation.fetch_add(1, Ordering::AcqRel);
            self.gate.playback_generation.fetch_add(1, Ordering::AcqRel);
            self.thread.unpark();
        }
    }
    /// Idempotent. Each gate only invalidates its own direction.
    pub fn set_controls(&self, muted: bool, deafened: bool) {
        let muted = muted || deafened;
        if self.gate.muted.swap(muted, Ordering::AcqRel) != muted {
            self.gate.capture_generation.fetch_add(1, Ordering::AcqRel);
        }
        if self.gate.deafened.swap(deafened, Ordering::AcqRel) != deafened {
            self.gate.playback_generation.fetch_add(1, Ordering::AcqRel);
        }
    }
}
impl Drop for Audio {
    fn drop(&mut self) {
        self.gate.stopped.store(true, Ordering::Release);
        self.gate.ready.store(false, Ordering::Release);
        self.thread.unpark();
    }
}

fn run(gate: &Arc<Gate>, capture: &mpsc::SyncSender<Frame>, playback: &mpsc::Receiver<Frame>) {
    let mut streams: Option<Streams> = None;
    let mut open_at = Instant::now();
    let mut input_retry_at = Instant::now();
    let mut devices = prefs().generation.load(Ordering::Acquire);
    // Echo cancellation and noise suppression run here, between the device rings and the voice loop.
    let mut processor = crate::process::Processor::new();
    // What goes out: voice activity or push to talk, and the level for the meters.
    let mut voice = crate::gate::Gate::new();
    let mut capturing = false;
    // Every five seconds the debug log gets how many microphone frames came and how loud, and how many were played.
    let mut tally_at = Instant::now() + Duration::from_secs(5);
    let (mut captured, mut played, mut loudest) = (0u32, 0u32, 0f32);
    // A live call keeps its place while a device is away: failures reopen, never end the call.
    while !gate.stopped.load(Ordering::Acquire) {
        let now = Instant::now();
        // A device picked in the user panel reopens both streams on it.
        let chosen = prefs().generation.load(Ordering::Acquire);
        if chosen != devices {
            devices = chosen;
            streams = None;
            open_at = now;
        }
        if gate.output_failed.swap(false, Ordering::AcqRel) && streams.is_some() {
            debug!("voice speaker stream failed; reopening devices");
            streams = None;
            open_at = now + RECOVERY_DELAY;
        }
        if streams.is_none() && now >= open_at {
            match Streams::open(gate) {
                Ok(opened) => streams = Some(opened),
                Err(error) => {
                    warn!("voice audio devices unavailable: {error}");
                    open_at = now + RETRY_DELAY;
                }
            }
        }
        let Some(active) = &mut streams else {
            while playback.try_recv().is_ok() {}
            std::thread::park_timeout(Duration::from_millis(10));
            continue;
        };
        if gate.input_failed.swap(false, Ordering::AcqRel) && active.input_stream.is_some() {
            debug!("voice microphone stream failed; retrying");
            active.input_stream = None;
            input_retry_at = now + INPUT_RETRY_DELAY;
        }
        if active.input_stream.is_none() && now >= input_retry_at {
            input_retry_at = now + INPUT_RETRY_DELAY;
            match open_input(&active.host, gate) {
                Ok((stream, ring)) => {
                    active.input_stream = Some(stream);
                    active.input = ring;
                }
                Err(error) => debug!("voice microphone unavailable: {error}"),
            }
        }
        // The microphone opening after a gap (mute, a new call) starts the processors afresh.
        let open = gate.capture();
        if open != capturing {
            capturing = open;
            if open {
                processor.reset();
            }
        }
        for _ in 0..RING {
            let Ok(mut frame) = active.input.pop() else {
                break;
            };
            captured += 1;
            loudest = loudest.max(
                frame
                    .iter()
                    .fold(0f32, |peak, sample| peak.max(sample.abs())),
            );
            if gate.capture() && process_frame(&mut frame, &mut processor, &mut voice) {
                // A full channel means the voice loop is behind; drop rather than block.
                let _ = capture.try_send(frame);
            }
        }
        for _ in 0..RING {
            let Ok(frame) = playback.try_recv() else {
                break;
            };
            if gate.playback() {
                // What the speakers play is the echo canceller's reference.
                processor.render(&frame, &prefs().processing);
                played += 1;
                let _ = active.output.push(frame);
            }
        }
        if now >= tally_at {
            debug!(
                captured,
                played,
                peak = loudest,
                capturing = gate.capture(),
                playing = gate.playback(),
                "voice audio tally"
            );
            (captured, played, loudest) = (0, 0, 0.0);
            tally_at = now + Duration::from_secs(5);
        }
        std::thread::park_timeout(Duration::from_millis(5));
    }
}

/// One microphone frame through the processors and the voice gate; the meter follows. True when it goes out.
fn process_frame(
    frame: &mut Frame,
    processor: &mut crate::process::Processor,
    voice: &mut crate::gate::Gate,
) -> bool {
    let processing = &prefs().processing;
    processor.capture(frame, processing);
    let (level, open) = voice.update(frame, processing);
    processing.set_meter(level, open);
    open
}

struct Streams {
    host: cpal::Host,
    input_stream: Option<cpal::Stream>,
    _output_stream: cpal::Stream,
    input: rtrb::Consumer<Frame>,
    output: rtrb::Producer<Frame>,
}
impl Streams {
    fn open(gate: &Arc<Gate>) -> Result<Self, &'static str> {
        let host = cpal::default_host();
        let device = pick(host.output_devices().ok(), prefs().output_device())
            .or_else(|| host.default_output_device())
            .ok_or("No default speaker is available")?;
        let config = output_config(&device)?;
        debug!(
            rate = config.sample_rate(),
            channels = config.channels(),
            "voice speaker opened"
        );
        let (output, output_read) = rtrb::RingBuffer::new(RING);
        let render = Playback::new(config.sample_rate(), output_read);
        let stream_config = config.config();
        let stream = match config.sample_format() {
            cpal::SampleFormat::F32 => output_stream::<f32>(&device, &stream_config, render, gate),
            cpal::SampleFormat::I16 => output_stream::<i16>(&device, &stream_config, render, gate),
            cpal::SampleFormat::I32 => output_stream::<i32>(&device, &stream_config, render, gate),
            cpal::SampleFormat::U16 => output_stream::<u16>(&device, &stream_config, render, gate),
            _ => Err("Speaker sample format is not supported"),
        }?;
        stream
            .play()
            .map_err(|_| "Could not start speaker playback")?;
        // A missing microphone leaves the call listen-only; the worker keeps retrying.
        let (input_stream, input) = match open_input(&host, gate) {
            Ok((stream, ring)) => (Some(stream), ring),
            Err(error) => {
                debug!("voice microphone unavailable: {error}");
                (None, rtrb::RingBuffer::new(RING).1)
            }
        };
        Ok(Self {
            host,
            input_stream,
            _output_stream: stream,
            input,
            output,
        })
    }
}

fn open_input(
    host: &cpal::Host,
    gate: &Arc<Gate>,
) -> Result<(cpal::Stream, rtrb::Consumer<Frame>), &'static str> {
    let device = pick(host.input_devices().ok(), prefs().input_device())
        .or_else(|| host.default_input_device())
        .ok_or("No default microphone is available")?;
    let config = input_config(&device)?;
    debug!(
        rate = config.sample_rate(),
        channels = config.channels(),
        "voice microphone opened"
    );
    let (ring, read) = rtrb::RingBuffer::new(RING);
    let capture = Capture::new(config.sample_rate(), ring);
    let stream_config = config.config();
    let stream = match config.sample_format() {
        cpal::SampleFormat::F32 => input_stream::<f32>(&device, &stream_config, capture, gate),
        cpal::SampleFormat::I16 => input_stream::<i16>(&device, &stream_config, capture, gate),
        cpal::SampleFormat::I32 => input_stream::<i32>(&device, &stream_config, capture, gate),
        cpal::SampleFormat::U16 => input_stream::<u16>(&device, &stream_config, capture, gate),
        _ => Err("Microphone sample format is not supported"),
    }?;
    stream
        .play()
        .map_err(|_| "Could not start microphone; check system microphone permission")?;
    Ok((stream, read))
}

fn supported_format(format: cpal::SampleFormat) -> bool {
    matches!(
        format,
        cpal::SampleFormat::F32
            | cpal::SampleFormat::I16
            | cpal::SampleFormat::I32
            | cpal::SampleFormat::U16
    )
}
fn usable(config: &cpal::SupportedStreamConfig) -> bool {
    (1..=8).contains(&config.channels())
        && (8_000..=192_000).contains(&config.sample_rate())
        && supported_format(config.sample_format())
}

/// 48 kHz with the fewest channels when offered, else the device default.
fn input_config(device: &cpal::Device) -> Result<cpal::SupportedStreamConfig, &'static str> {
    let supported: Vec<_> = device
        .supported_input_configs()
        .map_err(|_| "Microphone formats are unavailable")?
        .take(64)
        .collect();
    if let Some(config) = supported
        .into_iter()
        .filter(|c| (1..=8).contains(&c.channels()) && supported_format(c.sample_format()))
        .filter_map(|c| c.try_with_sample_rate(48_000))
        .min_by_key(|c| c.channels())
    {
        return Ok(config);
    }
    let config = device
        .default_input_config()
        .map_err(|_| "Default microphone format is unavailable")?;
    usable(&config)
        .then_some(config)
        .ok_or("Microphone format is unsupported")
}

/// Windows keeps the shared-mode default (Serein: some drivers advertise converted WASAPI
/// formats they then reject). Elsewhere 48 kHz with the fewest channels, else the default.
fn output_config(device: &cpal::Device) -> Result<cpal::SupportedStreamConfig, &'static str> {
    if !cfg!(target_os = "windows")
        && let Ok(supported) = device.supported_output_configs()
        && let Some(config) = supported
            .take(64)
            .filter(|c| (1..=8).contains(&c.channels()) && supported_format(c.sample_format()))
            .filter_map(|c| c.try_with_sample_rate(48_000))
            .min_by_key(|c| c.channels())
    {
        return Ok(config);
    }
    let config = device
        .default_output_config()
        .map_err(|_| "Default speaker format is unavailable")?;
    usable(&config)
        .then_some(config)
        .ok_or("Speaker format is unsupported")
}

fn is_fatal(error: &cpal::Error) -> bool {
    !matches!(
        error.kind(),
        cpal::ErrorKind::Xrun | cpal::ErrorKind::RealtimeDenied | cpal::ErrorKind::DeviceChanged
    )
}

fn input_stream<T>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    mut capture: Capture,
    gate: &Arc<Gate>,
) -> Result<cpal::Stream, &'static str>
where
    T: cpal::SizedSample,
    f32: cpal::FromSample<T>,
{
    let channels = usize::from(config.channels);
    let (callback_gate, failure) = (gate.clone(), gate.clone());
    device
        .build_input_stream(
            *config,
            move |data: &[T], _| capture.process(data, channels, &callback_gate),
            move |error| {
                if is_fatal(&error) {
                    failure.input_failed.store(true, Ordering::Release);
                }
            },
            None,
        )
        .map_err(|_| "Could not open microphone; check device and microphone permission")
}

fn output_stream<T>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    mut render: Playback,
    gate: &Arc<Gate>,
) -> Result<cpal::Stream, &'static str>
where
    T: cpal::SizedSample + cpal::FromSample<f32>,
{
    let channels = usize::from(config.channels);
    let (callback_gate, failure) = (gate.clone(), gate.clone());
    device
        .build_output_stream(
            *config,
            move |data: &mut [T], _| render.render(data, channels, &callback_gate),
            move |error| {
                if is_fatal(&error) {
                    failure.output_failed.store(true, Ordering::Release);
                }
            },
            None,
        )
        .map_err(|_| "Could not open speaker device")
}

fn sanitize(sample: f32) -> f32 {
    if sample.is_finite() {
        sample.clamp(-1.0, 1.0)
    } else {
        0.0
    }
}

/// Downmix to mono and resample linearly to 48 kHz 960-sample frames.
// Serein: linear conversion is a fallback for devices lacking 48 kHz.
struct Capture {
    generation: u64,
    previous: Option<f32>,
    phase: f64,
    step: f64,
    frame: Frame,
    index: usize,
    output: rtrb::Producer<Frame>,
}
impl Capture {
    fn new(rate: u32, output: rtrb::Producer<Frame>) -> Self {
        Self {
            generation: 0,
            previous: None,
            phase: 0.0,
            step: f64::from(rate) / 48_000.0,
            frame: [0.0; 960],
            index: 0,
            output,
        }
    }
    fn process<T: cpal::SizedSample>(&mut self, data: &[T], channels: usize, gate: &Gate)
    where
        f32: cpal::FromSample<T>,
    {
        let generation = gate.capture_generation.load(Ordering::Acquire);
        if generation != self.generation {
            self.generation = generation;
            self.reset();
        }
        if !gate.capture() || channels == 0 {
            self.reset();
            return;
        }
        for frame in data.chunks_exact(channels) {
            let sample = frame.iter().map(|v| v.to_sample::<f32>()).sum::<f32>() / channels as f32;
            self.sample(sanitize(sample) * prefs().input_gain());
        }
    }
    fn reset(&mut self) {
        self.previous = None;
        self.phase = 0.0;
        self.index = 0;
        self.frame.fill(0.0);
    }
    fn sample(&mut self, sample: f32) {
        if let Some(previous) = self.previous {
            while self.phase < 1.0 {
                self.frame[self.index] = previous + (sample - previous) * self.phase as f32;
                self.index += 1;
                if self.index == 960 {
                    // A full ring drops this frame; the worker drains it every 5 ms.
                    let _ = self.output.push(self.frame);
                    self.index = 0;
                }
                self.phase += self.step;
            }
            self.phase -= 1.0;
        }
        self.previous = Some(sample);
    }
}

/// Resample 48 kHz mono frames linearly to the device rate, duplicated to every channel.
struct Playback {
    generation: u64,
    input: rtrb::Consumer<Frame>,
    frame: Frame,
    index: usize,
    previous: f32,
    next: f32,
    phase: f64,
    step: f64,
}
impl Playback {
    fn new(rate: u32, input: rtrb::Consumer<Frame>) -> Self {
        Self {
            generation: 0,
            input,
            frame: [0.0; 960],
            index: 960,
            previous: 0.0,
            next: 0.0,
            phase: 1.0,
            step: 48_000.0 / f64::from(rate),
        }
    }
    fn render<T: cpal::SizedSample + cpal::FromSample<f32>>(
        &mut self,
        data: &mut [T],
        channels: usize,
        gate: &Gate,
    ) {
        let generation = gate.playback_generation.load(Ordering::Acquire);
        if generation != self.generation {
            self.generation = generation;
            self.reset();
        }
        if !gate.playback() || channels == 0 {
            self.reset();
            data.fill(T::from_sample(0.0));
            return;
        }
        let gain = prefs().output_gain();
        for frame in data.chunks_mut(channels) {
            frame.fill(T::from_sample((self.sample() * gain).clamp(-1.0, 1.0)));
        }
    }
    fn reset(&mut self) {
        for _ in 0..RING {
            if self.input.pop().is_err() {
                break;
            }
        }
        self.frame.fill(0.0);
        self.index = 960;
        self.previous = 0.0;
        self.next = 0.0;
        self.phase = 1.0;
    }
    fn pull(&mut self) -> f32 {
        if self.index == 960 {
            // An underrun plays one frame of silence.
            self.frame = self.input.pop().unwrap_or([0.0; 960]);
            self.index = 0;
        }
        let value = self.frame[self.index];
        self.index += 1;
        sanitize(value)
    }
    fn sample(&mut self) -> f32 {
        while self.phase >= 1.0 {
            self.previous = self.next;
            self.next = self.pull();
            self.phase -= 1.0;
        }
        let value = self.previous + (self.next - self.previous) * self.phase as f32;
        self.phase += self.step;
        value
    }
}

/// The microphone outside a call: the settings page's meters, and Mic Test's playback of what would be sent. The same
/// processors and gate as a call run on the same worker; nothing goes to the network. Dropping it closes the devices.
pub struct Preview {
    _audio: Audio,
    loopback: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
}

/// Opens the microphone (and the speaker, for `loopback`).
pub fn preview(loopback: bool) -> Result<Preview, &'static str> {
    let (capture_send, capture) = mpsc::sync_channel::<Frame>(8);
    let (playback, playback_receive) = mpsc::sync_channel::<Frame>(8);
    let audio = Audio::start(capture_send, playback_receive)?;
    audio.set_ready(true);
    audio.set_controls(false, false);
    let loopback = Arc::new(AtomicBool::new(loopback));
    let stop = Arc::new(AtomicBool::new(false));
    let (forward_loopback, forward_stop) = (loopback.clone(), stop.clone());
    std::thread::Builder::new()
        .name("dc-voice-preview".into())
        .spawn(move || {
            while !forward_stop.load(Ordering::Acquire) {
                match capture.recv_timeout(Duration::from_millis(100)) {
                    Ok(frame) if forward_loopback.load(Ordering::Acquire) => {
                        let _ = playback.try_send(frame);
                    }
                    Ok(_) | Err(mpsc::RecvTimeoutError::Timeout) => {}
                    Err(mpsc::RecvTimeoutError::Disconnected) => break,
                }
            }
        })
        .map_err(|_| "Could not start the microphone preview")?;
    Ok(Preview {
        _audio: audio,
        loopback,
        stop,
    })
}

impl Preview {
    /// Mic Test: hear what would be sent (`setLoopback("mic_test", …)`).
    pub fn set_loopback(&self, on: bool) {
        self.loopback.store(on, Ordering::Release);
    }
}

impl Drop for Preview {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn open_gate() -> Gate {
        let gate = Gate::default();
        gate.ready.store(true, Ordering::Release);
        gate
    }

    #[test]
    fn capture_resamples_downmixes_and_respects_the_gate() {
        // 44.1 kHz stereo in: 882 input frames make one 960-sample 48 kHz frame.
        let (ring, mut read) = rtrb::RingBuffer::new(RING);
        let mut capture = Capture::new(44_100, ring);
        let gate = open_gate();
        let stereo: Vec<f32> = std::iter::repeat_n([0.5f32, -0.1], 900).flatten().collect();
        capture.process(&stereo, 2, &gate);
        let frame = read.pop().expect("one frame");
        assert!(
            frame.iter().all(|s| (s - 0.2).abs() < 1e-6),
            "mean of both channels"
        );
        assert!(read.pop().is_err());

        gate.muted.store(true, Ordering::Release);
        capture.process(&stereo, 2, &gate);
        assert!(read.pop().is_err(), "muted microphone produces nothing");
    }

    #[test]
    fn playback_upsamples_fills_channels_and_silences_when_deafened() {
        let (mut ring, read) = rtrb::RingBuffer::new(RING);
        let mut playback = Playback::new(96_000, read);
        let gate = open_gate();
        ring.push([0.25; 960]).unwrap();
        ring.push([0.25; 960]).unwrap();
        let mut data = vec![0.0f32; 2 * 1920];
        playback.render(&mut data, 2, &gate);
        // The first sample ramps from silence; the rest hold the frame's value.
        assert!(data[4..].iter().all(|s| (s - 0.25).abs() < 1e-6));
        assert_eq!(data[2], data[3], "every channel gets the same sample");

        gate.deafened.store(true, Ordering::Release);
        ring.push([0.25; 960]).unwrap();
        playback.render(&mut data, 2, &gate);
        assert!(data.iter().all(|s| *s == 0.0));
        assert!(playback.input.pop().is_err(), "queued audio was flushed");
    }

    #[test]
    fn non_finite_samples_become_silence() {
        assert_eq!(sanitize(f32::NAN), 0.0);
        assert_eq!(sanitize(3.0), 1.0);
        assert_eq!(sanitize(-0.5), -0.5);
    }
}
