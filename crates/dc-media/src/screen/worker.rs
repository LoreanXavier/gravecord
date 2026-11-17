//! The screen share pipeline: capture -> BGRA frame -> letterbox to 4:2:0 -> H.264 (hardware first, openh264 mid-stream).
//! The structure follows Serein's `screen.rs` (`crates/discord-voice`, MIT OR Apache-2.0): a capture side that drops
//! frames it cannot use, one encode thread, a pacing gate before the readback so skipped frames cost nothing, and
//! rate changes folded into the encode loop. Transport, encryption and the epoch handshakes are not part of this crate.

use super::audio::{AudioCapture, AudioChunk};
use super::capture::{Capture, Frame};
use super::{LocalSink, Settings, thumbnail_from};
use crate::convert::{Chroma, PixelOrder, Raw, SourceMeta, downscale_rgba, to_yuv420_vec};
use crate::encode::{Backend, Config, Content, Encoder, Profile};
use crate::{EncodedFrame, Error, Layout, Picture, Result, h264, rtp_ticks};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicU8, AtomicU32, AtomicU64, Ordering},
    mpsc,
};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// The widest preview kept for the stream's uploaded thumbnail.
pub const PREVIEW_WIDTH: u32 = 640;
const PREVIEW_HEIGHT: u32 = 360;
/// A preview is only produced while someone is taking them.
const PREVIEW_ACTIVE: Duration = Duration::from_secs(2);
const PREVIEW_INTERVAL: Duration = Duration::from_millis(66);
/// A still desktop delivers no frames; the last one is repeated at this rate so viewers keep a live stream.
const IDLE_REPEAT: Duration = Duration::from_millis(500);
/// A keyframe request on a still desktop re-encodes the last frame after this long.
const KEYFRAME_WAIT: Duration = Duration::from_millis(100);
/// Waiting for the capture to start covers the system consent prompt.
const START_TIMEOUT: Duration = Duration::from_secs(30);

struct State {
    stop: AtomicBool,
    finished: AtomicBool,
    keyframe: AtomicBool,
    bitrate: AtomicU32,
    backend: AtomicU8,
    fell_back: AtomicBool,
    captured: AtomicU64,
    encoded: AtomicU64,
    error: Mutex<Option<String>>,
    preview: Mutex<Option<Picture>>,
    /// The owner's own view of the share.
    local: Mutex<Option<LocalSink>>,
    /// Microseconds since `origin` of the last `take_preview`.
    preview_taken: AtomicU64,
    /// When the pacing gate takes the next frame.
    cadence: Cadence,
    /// A frame is waiting for the encoder.
    pending: AtomicBool,
    /// The network side is behind: frames are skipped before they are encoded, never after.
    held: AtomicBool,
    origin: Instant,
}

pub struct Worker {
    state: Arc<State>,
    thread: Option<JoinHandle<()>>,
    _audio: Option<AudioCapture>,
}

impl Worker {
    /// Starts sharing `settings.source`. `video` receives each encoded access unit on the encode thread; `audio` receives
    /// system audio chunks (never the microphone) on the audio thread, and only when `settings.audio` is on.
    /// Returns once capture is running, so a refusal (consent denied, source gone) is returned here.
    pub fn start(
        settings: Settings,
        video: impl FnMut(EncodedFrame) + Send + 'static,
        audio: impl FnMut(AudioChunk) + Send + 'static,
    ) -> Result<Self> {
        Self::start_at(settings, settings.bitrate, video, audio)
    }

    /// [`Self::start`] with the encoder opened at `bitrate` (held to the share's bounds) rather than the top rate: a
    /// hardware encoder sizes its buffers when it opens, so the first keyframe comes at the rate the sender starts at.
    pub fn start_at(
        settings: Settings,
        bitrate: u32,
        video: impl FnMut(EncodedFrame) + Send + 'static,
        audio: impl FnMut(AudioChunk) + Send + 'static,
    ) -> Result<Self> {
        settings.validate()?;
        let state = Arc::new(State {
            stop: AtomicBool::new(false),
            finished: AtomicBool::new(false),
            keyframe: AtomicBool::new(true),
            bitrate: AtomicU32::new(settings.clamp_bitrate(bitrate)),
            backend: AtomicU8::new(0),
            fell_back: AtomicBool::new(false),
            captured: AtomicU64::new(0),
            encoded: AtomicU64::new(0),
            error: Mutex::new(None),
            preview: Mutex::new(None),
            local: Mutex::new(None),
            preview_taken: AtomicU64::new(0),
            cadence: Cadence::new(settings.fps),
            pending: AtomicBool::new(false),
            held: AtomicBool::new(false),
            origin: Instant::now(),
        });
        let (started_send, started) = mpsc::sync_channel::<Result<()>>(1);
        let worker = state.clone();
        let thread = std::thread::Builder::new()
            .name("dc-media-screen".into())
            .spawn(move || {
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    run(&worker, settings, video, &started_send)
                }))
                .unwrap_or_else(|_| Err(Error::failed("screen worker panicked")));
                if let Err(error) = result {
                    if !worker.stop.load(Ordering::Acquire) {
                        *worker.error.lock().unwrap_or_else(|e| e.into_inner()) =
                            Some(error.to_string());
                    }
                    let _ = started_send.try_send(Err(error));
                }
                worker.finished.store(true, Ordering::Release);
            })
            .map_err(|e| Error::failed(format!("screen worker could not start: {e}")))?;
        let mut worker = Self {
            state,
            thread: Some(thread),
            _audio: None,
        };
        match started.recv_timeout(START_TIMEOUT) {
            Ok(Ok(())) => {}
            Ok(Err(error)) => return Err(error),
            Err(mpsc::RecvTimeoutError::Timeout) => {
                return Err(Error::unavailable("screen capture did not start in time"));
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                return Err(Error::failed("screen worker ended"));
            }
        }
        if settings.audio {
            // On failure `worker` drops here and stops the capture again.
            worker._audio = Some(AudioCapture::start(audio)?);
        }
        Ok(worker)
    }

    /// Moves the target rate, held to the share's bounds. Applied before the next picture.
    pub fn set_bitrate(&self, bitrate: u32) {
        self.state.bitrate.store(bitrate, Ordering::Release);
    }

    /// While held the capture skips frames before encoding them. The sender holds it while encoded pictures queue up
    /// behind its pacer: a picture dropped after encoding would break the viewers' reference chain until the next
    /// keyframe, one never encoded costs nothing but a lower rate for a moment.
    pub fn hold(&self, held: bool) {
        self.state.held.store(held, Ordering::Release);
    }

    /// Makes the next picture a keyframe: a viewer asked for one, or joined.
    pub fn force_keyframe(&self) {
        self.state.keyframe.store(true, Ordering::Release);
    }

    /// Hands each admitted picture to the local sink at the encoder's size and cadence. The already converted YUV
    /// planes are copied; the UI converts them on its worker and keeps only the newest frame. `None` stops it.
    pub fn set_local_sink(&self, sink: Option<LocalSink>) {
        *self.state.local.lock().unwrap_or_else(|e| e.into_inner()) = sink;
    }

    /// The newest preview (at most 640 wide, RGBA), once; `None` when there is nothing new. Calling this is what keeps
    /// previews being produced.
    pub fn take_preview(&self) -> Option<Picture> {
        self.state
            .preview_taken
            .store(micros(self.state.origin).max(1), Ordering::Release);
        self.state
            .preview
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take()
    }

    /// Which encoder is running now; `None` before the first picture has been encoded.
    pub fn backend(&self) -> Option<Backend> {
        match self.state.backend.load(Ordering::Acquire) {
            1 => Some(Backend::Hardware),
            2 => Some(Backend::Software),
            _ => None,
        }
    }

    /// Whether a hardware encoder failed or stalled mid-stream and openh264 took over.
    pub fn fell_back(&self) -> bool {
        self.state.fell_back.load(Ordering::Acquire)
    }

    /// Frames the capture API delivered that the pacing gate accepted.
    pub fn captured(&self) -> u64 {
        self.state.captured.load(Ordering::Acquire)
    }

    /// Access units handed to the `video` callback.
    pub fn encoded(&self) -> u64 {
        self.state.encoded.load(Ordering::Acquire)
    }

    /// The share ended, by error, because the source closed, or because it was stopped.
    pub fn finished(&self) -> bool {
        self.state.finished.load(Ordering::Acquire)
    }

    /// Why the share ended by itself (source closed, capture or encoder failure), if it did.
    pub fn error(&self) -> Option<String> {
        self.state
            .error
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        self.state.stop.store(true, Ordering::Release);
        // Audio first: its thread calls back into user code.
        self._audio = None;
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn micros(origin: Instant) -> u64 {
    origin.elapsed().as_micros() as u64
}

/// The pacing gate's clock: frames are taken on a fixed grid of `1 / fps`, not "an interval after the last one".
///
/// A monitor refreshes faster than the share (144 or 165 Hz against 60 fps) and Windows Graphics Capture hands over
/// a frame per refresh. Measuring from the last accepted frame took every third frame of a 144 Hz display, 48 fps
/// with every interval 20.8 ms, and 55 fps from a 165 Hz one. On a grid the gate takes the first frame at or after each
/// due time, so the share runs at its rate (`cadence_keeps_the_rate_of_faster_sources`). A source that falls behind,
/// or a still desktop, restarts the grid instead of bursting to catch up.
struct Cadence {
    interval: u64,
    /// Microseconds since the worker's origin when the next frame is due; 0 before the first.
    due: AtomicU64,
}

impl Cadence {
    fn new(fps: u32) -> Self {
        Self {
            interval: 1_000_000 / u64::from(fps.max(1)),
            due: AtomicU64::new(0),
        }
    }

    /// Whether a frame that arrived at `now` (microseconds, never 0) is taken.
    fn admit(&self, now: u64) -> bool {
        let due = self.due.load(Ordering::Acquire);
        // A frame up to an eighth of an interval early still counts: a source at exactly the share's rate jitters
        // around the grid and must not lose every other frame.
        if due != 0 && now + self.interval / 8 < due {
            return false;
        }
        let next = if due == 0 || now >= due + self.interval {
            now + self.interval
        } else {
            due + self.interval
        };
        self.due.store(next, Ordering::Release);
        true
    }
}

fn drain_pending_outputs(
    state: &State,
    encoder: &mut Encoder,
    video: &mut impl FnMut(EncodedFrame),
    last_encoded: &mut Instant,
) -> Result<()> {
    while let Some(out) = encoder.take_pending_output()? {
        state.encoded.fetch_add(1, Ordering::AcqRel);
        video(EncodedFrame {
            data: out.data,
            keyframe: out.keyframe,
            timestamp: rtp_ticks(Instant::now().saturating_duration_since(state.origin)),
        });
        *last_encoded = Instant::now();
    }
    Ok(())
}

fn run(
    state: &Arc<State>,
    settings: Settings,
    mut video: impl FnMut(EncodedFrame),
    started: &mpsc::SyncSender<Result<()>>,
) -> Result<()> {
    let _unthrottled = crate::throttle::Guard::acquire();
    let (frames_send, frames) = mpsc::sync_channel::<Frame>(1);
    let gate = state.clone();
    let want = move || {
        if gate.stop.load(Ordering::Acquire)
            || gate.pending.load(Ordering::Acquire)
            || gate.held.load(Ordering::Acquire)
        {
            return false;
        }
        gate.cadence.admit(micros(gate.origin).max(1))
    };
    let delivered = state.clone();
    let deliver = move |frame: Frame| {
        delivered.pending.store(true, Ordering::Release);
        delivered.captured.fetch_add(1, Ordering::AcqRel);
        if frames_send.try_send(frame).is_err() {
            delivered.pending.store(false, Ordering::Release);
        }
    };
    let closed = state.clone();
    let on_closed = move || {
        *closed.error.lock().unwrap_or_else(|e| e.into_inner()) =
            Some("The shared window or display was closed".into());
        closed.stop.store(true, Ordering::Release);
    };
    // Dropped when this function returns, which removes the handlers before the channel goes away.
    let _capture = Capture::start(
        settings.source,
        settings.cursor,
        settings.fps,
        &state.stop,
        want,
        deliver,
        on_closed,
    )?;
    let _ = started.try_send(Ok(()));

    let config = Config {
        width: settings.width,
        height: settings.height,
        fps: settings.fps,
        bitrate: settings.bitrate,
        max_bytes: h264::MAX_ACCESS_UNIT,
        profile: Profile::Main,
        content: Content::Screen,
    };
    let mut encoder: Option<Encoder> = None;
    let mut yuv = vec![0u8; settings.width as usize * settings.height as usize * 3 / 2];
    let mut last_frame: Option<Frame> = None;
    let mut last_encoded = Instant::now();
    let mut last_preview = Instant::now() - PREVIEW_INTERVAL;
    let mut initial_preview_sent = false;
    let mut keyframe_wait: Option<Instant> = None;
    let mut stats = ShareStats::new();

    while !state.stop.load(Ordering::Acquire) {
        let timeout = if keyframe_wait.is_some() {
            KEYFRAME_WAIT
        } else {
            Duration::from_millis(50)
        };
        let fresh = match frames.recv_timeout(timeout) {
            Ok(frame) => Some(frame),
            Err(mpsc::RecvTimeoutError::Timeout) => None,
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        };
        // A new picture is stamped with its capture time; a repeat of the last one with now.
        let captured_at = fresh.as_ref().map_or_else(Instant::now, |frame| frame.at);
        let frame = match fresh {
            Some(frame) => {
                // Allow the capture thread to read back the next cadence-admitted frame in parallel with YUV/encode.
                state.pending.store(false, Ordering::Release);
                keyframe_wait = None;
                last_frame = Some(frame);
                last_frame.as_mut()
            }
            None => {
                if let Some(enc) = encoder.as_mut() {
                    drain_pending_outputs(state, enc, &mut video, &mut last_encoded)?;
                }
                // A still desktop: repeat the last picture for a keyframe request or to keep the stream alive.
                let wanted = state.keyframe.load(Ordering::Acquire);
                let due = if wanted {
                    let since = *keyframe_wait.get_or_insert_with(Instant::now);
                    since.elapsed() >= KEYFRAME_WAIT
                } else if state.held.load(Ordering::Acquire) {
                    false
                } else {
                    last_encoded.elapsed() >= IDLE_REPEAT
                };
                if !due {
                    continue;
                }
                match last_frame.as_mut() {
                    Some(frame) => {
                        keyframe_wait = None;
                        Some(frame)
                    }
                    None => continue,
                }
            }
        };
        let Some(frame) = frame else { continue };
        let (frame_w, frame_h) = (frame.width, frame.height);

        let taken = state.preview_taken.load(Ordering::Acquire);
        let active = !initial_preview_sent
            || (taken != 0
                && micros(state.origin).saturating_sub(taken) < PREVIEW_ACTIVE.as_micros() as u64);
        let preview_due = active && last_preview.elapsed() >= PREVIEW_INTERVAL;
        if preview_due {
            last_preview = Instant::now();
            initial_preview_sent = true;
        }

        if encoder.is_none() {
            // Opened at the rate asked for now, not the top: the first keyframe is sized for it.
            let bitrate = settings.clamp_bitrate(state.bitrate.load(Ordering::Acquire));
            encoder = Some(Encoder::new(Config { bitrate, ..config })?);
        }
        let Some(encoder) = encoder.as_mut() else {
            continue;
        };
        encoder.set_bitrate(settings.clamp_bitrate(state.bitrate.load(Ordering::Acquire)))?;
        let chroma = encoder.chroma();
        let converting = Instant::now();
        to_yuv420_vec(
            &mut frame.data,
            SourceMeta {
                width: frame_w as usize,
                height: frame_h as usize,
                stride: frame_w as usize * 4,
                order: PixelOrder::Bgra,
            },
            settings.width as usize,
            settings.height as usize,
            chroma,
            &mut yuv,
        )?;
        let force = state.keyframe.swap(false, Ordering::AcqRel);
        let encoding = Instant::now();
        let output = encoder.encode(&yuv, chroma, force)?;
        stats.frame(
            encoding - converting,
            encoding.elapsed(),
            output.as_ref().map_or(0, |output| output.data.len()),
        );
        state.backend.store(
            match encoder.backend() {
                Backend::Hardware => 1,
                Backend::Software => 2,
            },
            Ordering::Release,
        );
        state
            .fell_back
            .store(encoder.fell_back(), Ordering::Release);
        last_encoded = Instant::now();
        match output {
            Some(output) => {
                if force && !output.keyframe {
                    // The encoder did not honor it (a fallback in progress); ask again.
                    state.keyframe.store(true, Ordering::Release);
                }
                state.encoded.fetch_add(1, Ordering::AcqRel);
                video(EncodedFrame {
                    data: output.data,
                    keyframe: output.keyframe,
                    timestamp: rtp_ticks(captured_at.saturating_duration_since(state.origin)),
                });
            }
            None => {
                if force {
                    state.keyframe.store(true, Ordering::Release);
                }
            }
        }
        drain_pending_outputs(state, encoder, &mut video, &mut last_encoded)?;
        stats.report(
            (frame_w, frame_h),
            (settings.width, settings.height),
            encoder.backend(),
            settings.clamp_bitrate(state.bitrate.load(Ordering::Acquire)),
        );

        let local = state
            .local
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        if let Some(sink) = local {
            publish_local(&sink, settings.width, settings.height, chroma, &yuv);
        }
        if preview_due {
            let raw = Raw {
                width: frame_w as usize,
                height: frame_h as usize,
                stride: frame_w as usize * 4,
                order: PixelOrder::Bgra,
                data: &frame.data,
            };
            if let Ok(downscaled) = downscale_rgba(raw, PREVIEW_WIDTH, PREVIEW_HEIGHT) {
                *state.preview.lock().unwrap_or_else(|e| e.into_inner()) = Some(downscaled);
            }
        }
    }
    if let Some(error) = state
        .error
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
    {
        return Err(Error::unavailable(error));
    }
    Ok(())
}

/// Five-second totals of the share's own work, written to the debug log: the sizes in and out, the encoder in use,
/// the rate asked for against the bytes the encoder gave back (outputs drained later are not counted), and the time
/// conversion and encoding take per frame. Quality questions start here: a share at its source size never scales,
/// and an encoded rate far under the target points at the encoder, a low target at the bandwidth estimate.
struct ShareStats {
    since: Instant,
    frames: u32,
    bytes: usize,
    convert: Duration,
    encode: Duration,
}

impl ShareStats {
    const EVERY: Duration = Duration::from_secs(5);

    fn new() -> Self {
        Self {
            since: Instant::now(),
            frames: 0,
            bytes: 0,
            convert: Duration::ZERO,
            encode: Duration::ZERO,
        }
    }

    fn frame(&mut self, convert: Duration, encode: Duration, bytes: usize) {
        self.frames += 1;
        self.bytes += bytes;
        self.convert += convert;
        self.encode += encode;
    }

    fn report(&mut self, source: (u32, u32), sent: (u32, u32), backend: Backend, target: u32) {
        let elapsed = self.since.elapsed();
        if elapsed < Self::EVERY || self.frames == 0 {
            return;
        }
        let seconds = elapsed.as_secs_f64();
        let per_frame = |total: Duration| total.as_secs_f64() * 1000. / f64::from(self.frames);
        tracing::debug!(
            source = %format!("{}x{}", source.0, source.1),
            sent = %format!("{}x{}", sent.0, sent.1),
            backend = ?backend,
            fps = %format!("{:.1}", f64::from(self.frames) / seconds),
            target_kbps = target / 1000,
            encoded_kbps = (self.bytes as f64 * 8. / seconds / 1000.) as u64,
            convert_ms = %format!("{:.2}", per_frame(self.convert)),
            encode_ms = %format!("{:.2}", per_frame(self.encode)),
            "screen share"
        );
        *self = Self::new();
    }
}

/// One picture of `source`, for the picker: a single capture frame cut down to fit `max_width` x `max_height`.
pub(super) fn thumbnail(
    source: super::SourceId,
    max_width: u32,
    max_height: u32,
) -> Result<Picture> {
    let (send, receive) = mpsc::sync_channel::<Frame>(1);
    let cancel = AtomicBool::new(false);
    let _capture = Capture::start(
        source,
        false,
        30,
        &cancel,
        || true,
        move |frame| {
            let _ = send.try_send(frame);
        },
        || {},
    )?;
    let frame = receive
        .recv_timeout(Duration::from_secs(3))
        .map_err(|_| Error::unavailable("the source produced no picture"))?;
    thumbnail_from(
        &frame.data,
        frame.width,
        frame.height,
        max_width,
        max_height,
    )
}

fn publish_local(sink: &LocalSink, width: u32, height: u32, chroma: Chroma, planes: &[u8]) {
    sink(Picture {
        width,
        height,
        layout: match chroma {
            Chroma::Interleaved => Layout::Nv12,
            Chroma::Planar => Layout::I420,
        },
        data: planes.to_vec(),
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_stream_keeps_full_size_planes_and_every_admitted_frame() {
        for (chroma, layout) in [
            (Chroma::Interleaved, Layout::Nv12),
            (Chroma::Planar, Layout::I420),
        ] {
            let (width, height) = (1920, 1080);
            let mut planes = vec![128; layout.len(width, height).unwrap()];
            let received = Arc::new(Mutex::new(Vec::new()));
            let seen = received.clone();
            let sink: LocalSink = Arc::new(move |picture| {
                assert_eq!(
                    (picture.width, picture.height, picture.layout),
                    (width, height, layout)
                );
                assert_eq!(picture.data.len(), layout.len(width, height).unwrap());
                assert!(picture.data[1..].iter().all(|&value| value == 128));
                seen.lock().unwrap().push(picture.data[0]);
            });
            let cadence = Cadence::new(60);
            for frame in 0..60 {
                assert!(cadence.admit(1 + u64::from(frame) * 16_667));
                planes[0] = frame;
                publish_local(&sink, width, height, chroma, &planes);
            }
            assert_eq!(*received.lock().unwrap(), (0..60).collect::<Vec<u8>>());
        }
    }

    /// Frames of a `hz` display for one second, through the gate of a `fps` share: how many are taken and the longest
    /// gap between two taken ones, in microseconds.
    fn through(hz: u64, fps: u32) -> (usize, u64) {
        let cadence = Cadence::new(fps);
        let taken: Vec<u64> = (1..=hz)
            .map(|frame| frame * 1_000_000 / hz)
            .filter(|&at| cadence.admit(at))
            .collect();
        let gap = taken.windows(2).map(|w| w[1] - w[0]).max().unwrap_or(0);
        (taken.len(), gap)
    }

    /// The gate as it was: an interval (less a tenth) after the last taken frame.
    fn through_old(hz: u64, fps: u64) -> usize {
        let interval = 1_000_000 / fps;
        let mut last = 0u64;
        (1..=hz)
            .map(|frame| frame * 1_000_000 / hz)
            .filter(|&at| {
                let take = last == 0 || at - last >= interval * 9 / 10;
                if take {
                    last = at;
                }
                take
            })
            .count()
    }

    #[test]
    fn cadence_keeps_the_rate_of_faster_sources() {
        for hz in [60, 144, 165] {
            println!(
                "{hz} Hz display at 60 fps: {} fps before, {} fps now",
                through_old(hz, 60),
                through(hz, 60).0
            );
        }
        assert_eq!(through_old(144, 60), 48);
        // A faster monitor shared at 60 fps gives 60 pictures a second, never further apart than one interval plus
        // one refresh (21 ms from 144 Hz).
        for hz in [60, 75, 120, 144, 165, 240] {
            let (count, gap) = through(hz, 60);
            assert!((59..=61).contains(&count), "{hz} Hz gave {count} fps");
            assert!(
                gap <= 16_667 + 1_000_000 / hz + 1,
                "{hz} Hz: gap of {gap} us"
            );
        }
        assert!((29..=31).contains(&through(144, 30).0));
        // A slower source gives what it has, one picture per refresh.
        assert_eq!(through(30, 60).0, 30);
        // A source on the grid but jittering a little early keeps every frame.
        let cadence = Cadence::new(60);
        let taken = (1..=60u64)
            .map(|frame| frame * 16_667 - if frame % 2 == 0 { 1_000 } else { 0 })
            .filter(|&at| cadence.admit(at))
            .count();
        assert_eq!(taken, 60);
        // After a still desktop the grid restarts instead of bursting.
        let cadence = Cadence::new(60);
        assert!(cadence.admit(1));
        assert!(cadence.admit(5_000_000));
        assert!(!cadence.admit(5_006_944));
        assert!(cadence.admit(5_016_667));
    }
}
