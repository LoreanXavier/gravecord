//! Simulated devices for the call simulator (`crates/dc-callsim`), behind the `sim` feature. Nothing here opens a
//! device or reads the screen: once [`activate`] ran, a camera start turns into a moving test picture
//! ([`camera_rgba`], used by `camera::Camera::start`), and a screen share can be replaced by a [`Source`] that draws one
//! of three kinds of content and encodes it like the screen worker does, at the rate it is steered to.
//!
//! The switch is process-wide and has no way back: a process that simulates calls never captures for real.

use crate::convert::Chroma;
use crate::encode::{Config, Content as EncoderContent, Encoder, Profile};
use crate::{EncodedFrame, Error, Layout, Picture, Result, rtp_ticks};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering},
};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

static ACTIVE: AtomicBool = AtomicBool::new(false);

/// From now on cameras and simulated shares draw test pictures; real devices are never opened by them.
pub fn activate() {
    ACTIVE.store(true, Ordering::Release);
}

/// Whether [`activate`] ran in this process.
pub fn active() -> bool {
    ACTIVE.load(Ordering::Acquire)
}

/// What a simulated share shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Content {
    /// A game-like picture: the whole frame moves every picture and carries per-picture noise, so the encoder never
    /// has an easy picture.
    Motion,
    /// A desktop that does not change, but for a small clock square once a second.
    Static,
    /// Black text-like strokes on white (sharp edges the encoder must keep legible), a blinking caret and a line
    /// scrolled in every two seconds.
    Text,
}

impl Content {
    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "motion" => Some(Self::Motion),
            "static" => Some(Self::Static),
            "text" => Some(Self::Text),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Motion => "motion",
            Self::Static => "static",
            Self::Text => "text",
        }
    }
}

/// Draws I420 pictures of one content kind. Built once per source; each picture costs row copies, not per-pixel
/// arithmetic, so 2560 x 1440 at 60 pictures a second stays cheap.
pub struct Pattern {
    width: usize,
    height: usize,
    content: Content,
    /// A diagonal luma ramp long enough to be sliced for any row and shift.
    ramp: Vec<u8>,
    noise: Vec<u8>,
    /// The text page (luma), taller than the picture so it can scroll.
    page: Vec<u8>,
}

/// Lines of the text page.
const TEXT_LINE: usize = 24;

impl Pattern {
    pub fn new(width: u32, height: u32, content: Content) -> Self {
        let (width, height) = ((width.max(2) & !1) as usize, (height.max(2) & !1) as usize);
        let span = width + height + 4096;
        let ramp = (0..span)
            .map(|i| 32 + ((i * 191 / (width + height).max(1)) % 192) as u8)
            .collect();
        let mut state = 0x9E37_79B9u32;
        let noise = (0..width.max(64) * 4 + 7)
            .map(|_| {
                state ^= state << 13;
                state ^= state >> 17;
                state ^= state << 5;
                (state & 0x1F) as u8
            })
            .collect();
        let page = if content == Content::Text {
            text_page(width, height + TEXT_LINE * 64)
        } else {
            Vec::new()
        };
        Self {
            width,
            height,
            content,
            ramp,
            noise,
            page,
        }
    }

    pub fn content(&self) -> Content {
        self.content
    }

    /// Picture `index` at `fps`: I420, `width * height * 3 / 2` bytes.
    pub fn frame(&self, index: u32, fps: u32, out: &mut Vec<u8>) {
        let (w, h) = (self.width, self.height);
        out.resize(w * h * 3 / 2, 0);
        let (luma, chroma) = out.split_at_mut(w * h);
        let second = index / fps.max(1);
        match self.content {
            Content::Motion => {
                let shift = (index as usize * 3) % 4096;
                for (y, row) in luma.chunks_exact_mut(w).enumerate() {
                    let start = (y + shift) % (self.ramp.len() - w);
                    row.copy_from_slice(&self.ramp[start..start + w]);
                    // Noise that differs per row and per picture: a slice of the noise table at a moving offset.
                    let offset = (y * 31 + index as usize * 17) % (self.noise.len() - w);
                    for (value, grain) in row.iter_mut().zip(&self.noise[offset..offset + w]) {
                        *value = value.saturating_add(*grain);
                    }
                }
                let (u, v) = chroma.split_at_mut(w * h / 4);
                for row in u.chunks_exact_mut(w / 2) {
                    for (x, value) in row.iter_mut().enumerate() {
                        *value = 64 + (((x * 2 + shift) * 128 / w.max(1)) % 128) as u8;
                    }
                }
                for (y, row) in v.chunks_exact_mut(w / 2).enumerate() {
                    row.fill(64 + (((y * 2 + shift / 2) * 128 / w.max(1)) % 128) as u8);
                }
            }
            Content::Static => {
                for (y, row) in luma.chunks_exact_mut(w).enumerate() {
                    row.copy_from_slice(&self.ramp[y..y + w]);
                }
                // The clock square: 32 x 32 in the top right, its shade changes once a second.
                let shade = 40 + (second % 6) as u8 * 30;
                for row in luma.chunks_exact_mut(w).skip(8).take(32.min(h)) {
                    let start = w.saturating_sub(40);
                    row[start..(start + 32).min(w)].fill(shade);
                }
                chroma.fill(128);
            }
            Content::Text => {
                // A new line scrolls in every two seconds; the caret blinks twice a second.
                let top = ((second / 2) as usize * TEXT_LINE) % (self.page.len() / w - h).max(1);
                luma.copy_from_slice(&self.page[top * w..(top + h) * w]);
                if (index.wrapping_mul(4) / fps.max(1)).is_multiple_of(2) {
                    let x = (w / 3).min(w.saturating_sub(4));
                    for row in luma.chunks_exact_mut(w).skip(h.saturating_sub(40)).take(18) {
                        row[x..x + 2].fill(16);
                    }
                }
                chroma.fill(128);
            }
        }
    }
}

/// White page with dark strokes in lines of text: words of 2 to 8 letters of 5 to 9 pixels, two-pixel strokes.
fn text_page(width: usize, height: usize) -> Vec<u8> {
    let mut page = vec![235u8; width * height];
    let mut state = 0x1234_5678u32;
    let mut next = || {
        state ^= state << 13;
        state ^= state >> 17;
        state ^= state << 5;
        state
    };
    for line in 0..height / TEXT_LINE {
        let top = line * TEXT_LINE + 6;
        let mut x = 12;
        while x + 12 < width {
            let letters = 2 + (next() % 7) as usize;
            for _ in 0..letters {
                let letter = 5 + (next() % 5) as usize;
                if x + letter >= width {
                    break;
                }
                let shape = next();
                for dy in 0..12 {
                    let row = (top + dy) * width;
                    // Vertical strokes at both sides, a bar in the middle or at the top, chosen by the letter's bits.
                    if shape & 1 != 0 || dy == 0 {
                        page[row + x..row + x + 2].fill(20);
                    }
                    if shape & 2 != 0 {
                        page[row + x + letter - 2..row + x + letter].fill(20);
                    }
                    if (shape & 4 != 0 && dy == 6) || (shape & 8 != 0 && dy == 11) {
                        page[row + x..row + x + letter].fill(20);
                    }
                }
                x += letter + 2;
            }
            x += 7;
        }
    }
    page
}

/// One RGBA picture of the simulated camera (640 x 480, `index` counts pictures): the moving pattern, so a viewer can
/// tell pictures apart.
pub(crate) fn camera_rgba(pattern: &Pattern, index: u32, fps: u32) -> Vec<u8> {
    let mut yuv = Vec::new();
    pattern.frame(index, fps, &mut yuv);
    Picture {
        width: pattern.width as u32,
        height: pattern.height as u32,
        layout: Layout::I420,
        data: yuv,
    }
    .to_rgba()
    .unwrap_or_default()
}

/// Receives the source's own pictures, for the sharer's tile.
pub type LocalSink = Arc<dyn Fn(Picture) + Send + Sync>;

#[derive(Default)]
struct State {
    stop: AtomicBool,
    keyframe: AtomicBool,
    finished: AtomicBool,
    held: AtomicBool,
    /// The rate the sender asks for, 0 before it asked.
    bitrate: AtomicU32,
    produced: AtomicU64,
    skipped: AtomicU64,
    encoder_restarts: AtomicU64,
    local: Mutex<Option<LocalSink>>,
    preview: Mutex<Option<Picture>>,
}

/// A simulated screen share: the pattern of one [`Content`] at a steady frame rate on its own thread, encoded by the
/// same encoder the screen worker uses (hardware first, openh264 otherwise), with the rate following
/// [`Source::set_bitrate`] the way `screen::Worker` follows it, and pictures skipped before encoding while
/// [`Source::hold`] is on. Dropping it stops and joins the thread.
pub struct Source {
    state: Arc<State>,
    thread: Option<JoinHandle<()>>,
}

/// The largest access unit a simulated share may produce (the screen worker's bound).
const MAX_UNIT: usize = crate::h264::MAX_ACCESS_UNIT;

impl Source {
    pub fn start(
        content: Content,
        width: u32,
        height: u32,
        fps: u32,
        bitrate: u32,
        mut video: impl FnMut(EncodedFrame) + Send + 'static,
    ) -> Result<Self> {
        if width < 2
            || height < 2
            || !width.is_multiple_of(2)
            || !height.is_multiple_of(2)
            || fps == 0
        {
            return Err(Error::invalid(
                "simulated share size must be even and fps nonzero",
            ));
        }
        let (started_send, started) = std::sync::mpsc::sync_channel::<Result<()>>(1);
        let state = Arc::new(State::default());
        state.keyframe.store(true, Ordering::Release);
        let worker = state.clone();
        let thread = std::thread::Builder::new()
            .name("dc-media-sim-share".into())
            .spawn(move || {
                let config = Config {
                    width,
                    height,
                    fps,
                    bitrate,
                    max_bytes: MAX_UNIT,
                    profile: Profile::Main,
                    content: EncoderContent::Screen,
                };
                let mut encoder = match Encoder::new(config) {
                    Ok(encoder) => {
                        let _ = started_send.send(Ok(()));
                        encoder
                    }
                    Err(error) => {
                        let _ = started_send.send(Err(error));
                        worker.finished.store(true, Ordering::Release);
                        return;
                    }
                };
                let pattern = Pattern::new(width, height, content);
                let period = Duration::from_micros(1_000_000 / u64::from(fps));
                let origin = Instant::now();
                let mut next = origin;
                let mut index = 0u32;
                let mut yuv = Vec::new();
                let mut last_preview: Option<Instant> = None;
                while !worker.stop.load(Ordering::Acquire) {
                    if worker.held.load(Ordering::Acquire) {
                        worker.skipped.fetch_add(1, Ordering::Relaxed);
                    } else {
                        pattern.frame(index, fps, &mut yuv);
                        let wanted = worker.bitrate.load(Ordering::Acquire);
                        if wanted != 0 && matches!(encoder.set_bitrate(wanted), Ok(true)) {
                            worker.encoder_restarts.fetch_add(1, Ordering::Relaxed);
                        }
                        let force = worker.keyframe.swap(false, Ordering::AcqRel);
                        match encoder.encode(&yuv, Chroma::Planar, force) {
                            Ok(Some(output)) => {
                                worker.produced.fetch_add(1, Ordering::Relaxed);
                                if force && !output.keyframe {
                                    worker.keyframe.store(true, Ordering::Release);
                                }
                                video(EncodedFrame {
                                    data: output.data,
                                    keyframe: output.keyframe,
                                    timestamp: rtp_ticks(origin.elapsed()),
                                });
                            }
                            Ok(None) => {}
                            Err(error) => {
                                tracing::debug!("simulated share stopped: {error}");
                                break;
                            }
                        }
                        let picture = Picture {
                            width,
                            height,
                            layout: Layout::I420,
                            data: yuv.clone(),
                        };
                        let local = worker
                            .local
                            .lock()
                            .unwrap_or_else(|e| e.into_inner())
                            .clone();
                        if let Some(local) = local {
                            local(picture.clone());
                        }
                        // The preview is read a few times a minute at most: keeping one every 200 ms is enough.
                        if last_preview
                            .is_none_or(|at: Instant| at.elapsed() >= Duration::from_millis(200))
                        {
                            last_preview = Some(Instant::now());
                            *worker.preview.lock().unwrap_or_else(|e| e.into_inner()) =
                                Some(picture);
                        }
                    }
                    index = index.wrapping_add(1);
                    next += period;
                    let now = Instant::now();
                    if next > now {
                        std::thread::sleep(next - now);
                    } else {
                        next = now;
                    }
                }
                worker.finished.store(true, Ordering::Release);
            })
            .map_err(|e| Error::failed(format!("simulated share could not start: {e}")))?;
        let source = Self {
            state,
            thread: Some(thread),
        };
        started
            .recv_timeout(Duration::from_secs(10))
            .map_err(|_| Error::failed("simulated share did not start"))??;
        Ok(source)
    }

    pub fn force_keyframe(&self) {
        self.state.keyframe.store(true, Ordering::Release);
    }

    /// The encoder follows this rate as the screen encoder does (hardware live, openh264 on large moves).
    pub fn set_bitrate(&self, bitrate: u32) {
        self.state.bitrate.store(bitrate, Ordering::Release);
    }

    /// While held, pictures are skipped before they are encoded (the screen worker's backpressure).
    pub fn hold(&self, held: bool) {
        self.state.held.store(held, Ordering::Release);
    }

    /// Every picture drawn goes here too, as I420, for the sharer's own tile.
    pub fn set_local_sink(&self, sink: Option<LocalSink>) {
        *self.state.local.lock().unwrap_or_else(|e| e.into_inner()) = sink;
    }

    /// The newest picture, taken (the screen worker's `take_preview`).
    pub fn take_preview(&self) -> Option<Picture> {
        self.state
            .preview
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take()
    }

    pub fn finished(&self) -> bool {
        self.state.finished.load(Ordering::Acquire)
    }

    /// Pictures encoded, pictures skipped while held, and encoder restarts for a rate change.
    pub fn counts(&self) -> (u64, u64, u64) {
        (
            self.state.produced.load(Ordering::Relaxed),
            self.state.skipped.load(Ordering::Relaxed),
            self.state.encoder_restarts.load(Ordering::Relaxed),
        )
    }
}

impl Drop for Source {
    fn drop(&mut self) {
        self.state.stop.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_content_draws_a_whole_picture_and_moves_as_it_should() {
        for content in [Content::Motion, Content::Static, Content::Text] {
            let pattern = Pattern::new(320, 180, content);
            let (mut a, mut b, mut c) = (Vec::new(), Vec::new(), Vec::new());
            pattern.frame(0, 30, &mut a);
            pattern.frame(1, 30, &mut b);
            pattern.frame(90, 30, &mut c);
            assert_eq!(a.len(), 320 * 180 * 3 / 2);
            match content {
                Content::Motion => assert_ne!(a, b, "every picture moves"),
                Content::Static => {
                    assert_eq!(a, b, "nothing moves within a second");
                    assert_ne!(a, c, "the clock square changes");
                }
                Content::Text => {
                    assert_ne!(a, c, "a line scrolled in");
                    let dark = a[..320 * 180].iter().filter(|v| **v < 40).count();
                    assert!(dark > 320 * 180 / 50, "strokes on the page: {dark}");
                }
            }
            assert_eq!(Content::parse(content.name()), Some(content));
        }
    }

    #[test]
    fn a_source_encodes_follows_the_rate_and_holds() {
        let (send, receive) = std::sync::mpsc::channel();
        let source = Source::start(Content::Text, 320, 180, 30, 400_000, move |frame| {
            let _ = send.send(frame);
        })
        .unwrap();
        let first = receive.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(first.keyframe);
        assert!(crate::h264::is_keyframe(&first.data));
        source.set_bitrate(150_000);
        source.hold(true);
        std::thread::sleep(Duration::from_millis(200));
        assert!(source.counts().1 > 0, "held pictures are skipped");
        assert!(source.take_preview().is_some());
        drop(source);
    }
}
