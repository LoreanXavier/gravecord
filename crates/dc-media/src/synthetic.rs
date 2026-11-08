//! A made-up video source: a moving test pattern encoded as H.264 by openh264. It exists for the loopback pipeline
//! test and for a capture-only harness step that streams without touching the owner's screen or camera; no device is
//! opened and nothing is captured.

use crate::convert::Chroma;
use crate::encode::{Config, Content, Encoder, Profile};
use crate::{EncodedFrame, Error, Result, rtp_ticks};
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU64, Ordering},
};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// The largest access unit the pattern encoder may produce.
const MAX_BYTES: usize = 512 * 1024;

/// The pattern as I420 at `width` x `height`: a diagonal luma ramp that shifts with `index`, with chroma that drifts
/// too, so consecutive pictures differ and every plane carries signal.
pub fn pattern_i420(width: u32, height: u32, index: u32) -> Vec<u8> {
    let (w, h) = (width as usize, height as usize);
    let mut data = Vec::with_capacity(w * h * 3 / 2);
    let shift = index as usize * 3;
    for y in 0..h {
        for x in 0..w {
            data.push(32 + ((x + y + shift) * 191 / (w + h)) as u8 % 192);
        }
    }
    for plane in 0..2usize {
        for y in 0..h / 2 {
            for x in 0..w / 2 {
                let value = if plane == 0 {
                    x * 2 + shift
                } else {
                    y * 2 + shift / 2
                };
                data.push(64 + (value * 128 / w.max(1)) as u8 % 128);
            }
        }
    }
    data
}

/// Encodes the pattern one picture at a time, on the caller's thread.
pub struct Generator {
    encoder: Encoder,
    width: u32,
    height: u32,
    index: u32,
    origin: Instant,
    fps: u32,
}

impl Generator {
    /// A software encoder for `width` x `height` (even) at `fps` and `bitrate`.
    pub fn new(width: u32, height: u32, fps: u32, bitrate: u32) -> Result<Self> {
        let config = Config {
            width,
            height,
            fps,
            bitrate,
            max_bytes: MAX_BYTES,
            profile: Profile::Main,
            content: Content::Screen,
        };
        Ok(Self {
            encoder: Encoder::software(config)?,
            width,
            height,
            index: 0,
            origin: Instant::now(),
            fps,
        })
    }

    /// The next picture, encoded. `None` when the encoder held it back. The timestamp follows the frame index, so it is
    /// steady however the caller paces.
    pub fn next(&mut self, keyframe: bool) -> Result<Option<EncodedFrame>> {
        let yuv = pattern_i420(self.width, self.height, self.index);
        let timestamp = rtp_ticks(Duration::from_micros(
            u64::from(self.index) * 1_000_000 / u64::from(self.fps),
        ));
        self.index += 1;
        let output = self
            .encoder
            .encode(&yuv, Chroma::Planar, keyframe || self.index == 1)?;
        Ok(output.map(|output| EncodedFrame {
            data: output.data,
            keyframe: output.keyframe,
            timestamp,
        }))
    }

    /// Time since this generator was made.
    pub fn elapsed(&self) -> Duration {
        self.origin.elapsed()
    }
}

#[derive(Default)]
struct State {
    stop: AtomicBool,
    keyframe: AtomicBool,
    finished: AtomicBool,
    produced: AtomicU64,
}

/// The pattern on its own thread at a steady frame rate. Dropping it stops and joins the thread.
pub struct Synthetic {
    state: Arc<State>,
    thread: Option<JoinHandle<()>>,
}

impl Synthetic {
    pub fn start(
        width: u32,
        height: u32,
        fps: u32,
        bitrate: u32,
        mut video: impl FnMut(EncodedFrame) + Send + 'static,
    ) -> Result<Self> {
        let (started_send, started) = std::sync::mpsc::sync_channel::<Result<()>>(1);
        let state = Arc::new(State::default());
        let worker = state.clone();
        let thread = std::thread::Builder::new()
            .name("dc-media-synthetic".into())
            .spawn(move || {
                // The encoder holds COM state on Windows, so it is made on the thread that uses it.
                let mut generator = match Generator::new(width, height, fps, bitrate) {
                    Ok(generator) => {
                        let _ = started_send.send(Ok(()));
                        generator
                    }
                    Err(error) => {
                        let _ = started_send.send(Err(error));
                        worker.finished.store(true, Ordering::Release);
                        return;
                    }
                };
                let period = Duration::from_micros(1_000_000 / u64::from(fps.max(1)));
                let mut next = Instant::now();
                while !worker.stop.load(Ordering::Acquire) {
                    let force = worker.keyframe.swap(false, Ordering::AcqRel);
                    match generator.next(force) {
                        Ok(Some(frame)) => {
                            worker.produced.fetch_add(1, Ordering::Relaxed);
                            video(frame);
                        }
                        Ok(None) => {}
                        Err(error) => {
                            tracing::debug!("synthetic source stopped: {error}");
                            break;
                        }
                    }
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
            .map_err(|e| Error::failed(format!("synthetic source could not start: {e}")))?;
        let source = Self {
            state,
            thread: Some(thread),
        };
        started
            .recv_timeout(Duration::from_secs(10))
            .map_err(|_| Error::failed("synthetic source did not start"))??;
        Ok(source)
    }

    /// Makes the next picture a keyframe.
    pub fn force_keyframe(&self) {
        self.state.keyframe.store(true, Ordering::Release);
    }

    pub fn produced(&self) -> u64 {
        self.state.produced.load(Ordering::Relaxed)
    }

    pub fn finished(&self) -> bool {
        self.state.finished.load(Ordering::Acquire)
    }
}

impl Drop for Synthetic {
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
    use crate::{decode::Decoder, h264};

    #[test]
    fn the_pattern_encodes_decodes_and_moves() {
        let mut generator = Generator::new(320, 240, 15, 500_000).unwrap();
        let mut decoder = Decoder::software().unwrap();
        let mut pictures = Vec::new();
        for i in 0..6 {
            let frame = generator.next(false).unwrap().expect("a picture");
            assert_eq!(frame.keyframe, i == 0);
            assert_eq!(h264::is_keyframe(&frame.data), frame.keyframe);
            if let Some(picture) = decoder.decode(&frame.data).unwrap() {
                assert_eq!((picture.width, picture.height), (320, 240));
                pictures.push(picture);
            }
        }
        assert!(pictures.len() >= 4);
        assert_ne!(pictures[0].data, pictures[3].data);
    }

    #[test]
    fn the_thread_delivers_and_stops() {
        let (send, receive) = std::sync::mpsc::channel();
        let source = Synthetic::start(160, 120, 30, 300_000, move |frame| {
            let _ = send.send(frame);
        })
        .unwrap();
        let first = receive.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(first.keyframe);
        source.force_keyframe();
        drop(source);
    }
}
