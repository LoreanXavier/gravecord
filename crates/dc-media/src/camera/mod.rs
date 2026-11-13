//! User-started camera capture. Nothing opens a device until [`Camera::start`], and dropping the [`Camera`] stops the
//! capture and joins its thread.
//!
//! Adapted from Serein's `crates/discord-voice/src/camera.rs` (MIT OR Apache-2.0). Frames are 640x480 at 15 fps and
//! 600 kb/s like Serein's. The pinned official client defaults to a 1280x720 / 30 fps budget with 150 kb/s to 2.5 Mb/s
//! (`eQ` in `design/snapshot/modules/731854.js`); the constants below are the place to move to it once the sender can
//! use them. Each frame carries the raw picture for the local preview and, when the encoder produced one, an access unit.
//!
//! The chosen Video Background ([`crate::background`]) is applied to the picture first, so the preview and the
//! encoder see the same processed frame.

use crate::convert::{PixelOrder, Raw, to_yuv420};
use crate::encode::{Backend, Config, Content, Encoder, Profile};
use crate::{EncodedFrame, Error, Picture, Result, rtp_ticks};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicU8, AtomicU32, Ordering},
    mpsc,
};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

mod format;
#[cfg(windows)]
mod windows;

pub const WIDTH: usize = 640;
pub const HEIGHT: usize = 480;
/// The capture and encode rate; a device that cannot do 15 fps gets the nearest rate it supports.
pub const FPS: u32 = 15;
pub const BITRATE: u32 = 600_000;
/// The lowest rate the encoder is steered to (`videoBitrateFloor` of module 731854).
pub const MIN_BITRATE: u32 = 150_000;
/// The highest: the official 2.5 Mb/s is for the 1280x720 capture (`videoBitrate.max`, `videoCapture` of module
/// 731854), and `getVideoQuality` scales it with the share of that budget a smaller picture takes; this capture's
/// 640x480 is a third of it.
pub const MAX_BITRATE: u32 = (2_500_000u64 * (WIDTH * HEIGHT) as u64 / (1280 * 720)) as u32;
/// The largest access unit the sender accepts: about a hundred RTP packets.
pub const MAX_ENCODED_BYTES: usize = 128 * 1024;

/// The error of a start when there is no camera to open (none listed, or the chosen one is gone). The app shows the
/// official "we can't detect any camera" text for it and the error's own words for every other failure.
pub const NOT_FOUND: &str = "No camera was detected";

/// How long [`Camera::start`] waits for the device to open before giving up.
const OPEN_TIMEOUT: Duration = Duration::from_secs(15);

/// A capture device. `id` goes back into [`Camera::start`]; it is `mf:<symbolic link>` or `dshow:<moniker>`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CameraDevice {
    pub id: String,
    pub name: String,
}

/// One captured picture. `encoded` is `None` when the encoder held or dropped it; the preview still shows `preview`.
pub struct CameraFrame {
    /// RGBA, 640x480.
    pub preview: Picture,
    pub encoded: Option<EncodedFrame>,
}

/// Where an id points, after checking it is one of ours.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DeviceRef<'a> {
    MediaFoundation(&'a str),
    DirectShow(&'a str),
}

const MAX_ID_BYTES: usize = 4096;

/// Splits a device id into its backend and that backend's own name for the device.
pub(crate) fn parse_device_id(id: &str) -> Result<DeviceRef<'_>> {
    if id.len() > MAX_ID_BYTES || id.contains('\0') {
        return Err(Error::invalid(
            "camera device id is too long or holds a NUL",
        ));
    }
    if let Some(rest) = id.strip_prefix("mf:").filter(|rest| !rest.is_empty()) {
        Ok(DeviceRef::MediaFoundation(rest))
    } else if let Some(rest) = id.strip_prefix("dshow:").filter(|rest| !rest.is_empty()) {
        Ok(DeviceRef::DirectShow(rest))
    } else {
        Err(Error::invalid(
            "camera device id is not one from camera::devices()",
        ))
    }
}

/// Lists capture devices (Media Foundation first, then DirectShow-only ones such as virtual cameras). Enumeration never
/// opens a capture stream.
pub fn devices() -> Result<Vec<CameraDevice>> {
    #[cfg(windows)]
    {
        windows::devices()
    }
    #[cfg(not(windows))]
    Err(Error::Unsupported("camera capture"))
}

#[derive(Default)]
struct State {
    stop: AtomicBool,
    keyframe: AtomicBool,
    finished: AtomicBool,
    error: Mutex<Option<String>>,
    backend: AtomicU8,
    /// The rate the sender asks for, or 0 for [`BITRATE`].
    bitrate: AtomicU32,
}

pub struct Camera {
    state: Arc<State>,
    thread: Option<JoinHandle<()>>,
}

impl Camera {
    /// Opens `device_id` (or the default camera) and starts delivering frames to `on_frame` on the capture thread. Call
    /// only after an explicit camera-on gesture. Returns once the device is open and streaming has been started.
    pub fn start(
        device_id: Option<&str>,
        on_frame: impl FnMut(CameraFrame) + Send + 'static,
    ) -> Result<Self> {
        #[cfg(feature = "sim")]
        if crate::sim::active() {
            return Self::start_simulated(on_frame);
        }
        if let Some(id) = device_id {
            parse_device_id(id)?;
        }
        if !cfg!(windows) {
            return Err(Error::Unsupported("camera capture"));
        }
        let state = Arc::new(State::default());
        state.keyframe.store(true, Ordering::Release);
        let (opened_send, opened) = mpsc::sync_channel::<Result<()>>(1);
        let worker = state.clone();
        let device = device_id.map(str::to_owned);
        let thread = std::thread::Builder::new()
            .name("dc-media-camera".into())
            .spawn(move || {
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    run(&worker, device.as_deref(), on_frame, &opened_send)
                }))
                .unwrap_or_else(|_| Err(Error::failed("camera worker panicked")));
                if let Err(error) = result {
                    if !worker.stop.load(Ordering::Acquire) {
                        *worker.error.lock().unwrap_or_else(|e| e.into_inner()) =
                            Some(error.to_string());
                    }
                    // A failure before the device opened is the start's answer.
                    let _ = opened_send.try_send(Err(error));
                }
                worker.finished.store(true, Ordering::Release);
            })
            .map_err(|e| Error::failed(format!("camera worker could not start: {e}")))?;
        let camera = Self {
            state,
            thread: Some(thread),
        };
        match opened.recv_timeout(OPEN_TIMEOUT) {
            Ok(Ok(())) => Ok(camera),
            Ok(Err(error)) => Err(error),
            Err(mpsc::RecvTimeoutError::Timeout) => {
                Err(Error::unavailable("the camera did not open in time"))
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => Err(Error::failed("camera worker ended")),
        }
    }

    /// The call simulator's camera (`sim` feature, after `sim::activate`): the moving test picture through the same
    /// pipeline as a device's pictures, at [`FPS`]. No device is opened.
    #[cfg(feature = "sim")]
    fn start_simulated(mut on_frame: impl FnMut(CameraFrame) + Send + 'static) -> Result<Self> {
        let state = Arc::new(State::default());
        state.keyframe.store(true, Ordering::Release);
        let worker = state.clone();
        let (opened_send, opened) = mpsc::sync_channel::<Result<()>>(1);
        let thread = std::thread::Builder::new()
            .name("dc-media-sim-camera".into())
            .spawn(move || {
                // The encoder holds COM state on Windows, so it is made on the thread that uses it.
                let mut pipeline = match Pipeline::new() {
                    Ok(pipeline) => {
                        let _ = opened_send.send(Ok(()));
                        pipeline
                    }
                    Err(error) => {
                        let _ = opened_send.send(Err(error));
                        worker.finished.store(true, Ordering::Release);
                        return;
                    }
                };
                let pattern = crate::sim::Pattern::new(
                    WIDTH as u32,
                    HEIGHT as u32,
                    crate::sim::Content::Motion,
                );
                let period = Duration::from_micros(1_000_000 / u64::from(FPS));
                let mut next = Instant::now();
                let mut index = 0u32;
                while !worker.stop.load(Ordering::Acquire) {
                    let rgba = crate::sim::camera_rgba(&pattern, index, FPS);
                    match pipeline.process(rgba, &worker) {
                        Ok(frame) => on_frame(frame),
                        Err(error) => {
                            *worker.error.lock().unwrap_or_else(|e| e.into_inner()) =
                                Some(error.to_string());
                            break;
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
            .map_err(|e| Error::failed(format!("simulated camera could not start: {e}")))?;
        let camera = Self {
            state,
            thread: Some(thread),
        };
        opened
            .recv_timeout(OPEN_TIMEOUT)
            .map_err(|_| Error::failed("simulated camera did not start"))??;
        Ok(camera)
    }

    /// Moves the encoder's target rate, held to [`MIN_BITRATE`] and [`MAX_BITRATE`]. Applied before the next picture.
    pub fn set_bitrate(&self, bitrate: u32) {
        self.state
            .bitrate
            .store(bitrate.clamp(MIN_BITRATE, MAX_BITRATE), Ordering::Release);
    }

    /// Makes the next picture a keyframe: a viewer asked for one, or joined.
    pub fn force_keyframe(&self) {
        self.state.keyframe.store(true, Ordering::Release);
    }

    /// Which encoder is running now; `None` before the first picture has been encoded.
    pub fn backend(&self) -> Option<Backend> {
        match self.state.backend.load(Ordering::Acquire) {
            1 => Some(Backend::Hardware),
            2 => Some(Backend::Software),
            _ => None,
        }
    }

    /// The capture thread has ended, by error or because it was stopped.
    pub fn finished(&self) -> bool {
        self.state.finished.load(Ordering::Acquire)
    }

    /// Why the capture ended by itself (device unplugged, frames stopped), if it did.
    pub fn error(&self) -> Option<String> {
        self.state
            .error
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }
}

impl Drop for Camera {
    fn drop(&mut self) {
        self.state.stop.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn camera_config() -> Config {
    Config {
        width: WIDTH as u32,
        height: HEIGHT as u32,
        fps: FPS,
        bitrate: BITRATE,
        max_bytes: MAX_ENCODED_BYTES,
        profile: Profile::Baseline,
        content: Content::Camera,
    }
}

/// Turns captured RGBA pictures into [`CameraFrame`]s.
struct Pipeline {
    encoder: Encoder,
    yuv: Vec<u8>,
    origin: Instant,
    background: crate::background::Processor,
}

impl Pipeline {
    fn new() -> Result<Self> {
        Ok(Self {
            encoder: Encoder::new(camera_config())?,
            yuv: vec![0; WIDTH * HEIGHT * 3 / 2],
            origin: Instant::now(),
            background: crate::background::Processor::new(),
        })
    }

    fn process(&mut self, mut rgba: Vec<u8>, state: &State) -> Result<CameraFrame> {
        if rgba.len() == WIDTH * HEIGHT * 4 {
            self.background.apply(&mut rgba, WIDTH, HEIGHT);
        }
        let preview = Picture::rgba(WIDTH as u32, HEIGHT as u32, rgba)
            .ok_or_else(|| Error::failed("camera did not provide a 640x480 picture"))?;
        let chroma = self.encoder.chroma();
        to_yuv420(
            Raw {
                width: WIDTH,
                height: HEIGHT,
                stride: WIDTH * 4,
                order: PixelOrder::Rgba,
                data: &preview.data,
            },
            WIDTH,
            HEIGHT,
            chroma,
            &mut self.yuv,
        )?;
        let wanted = state.bitrate.load(Ordering::Acquire);
        if wanted != 0 {
            self.encoder.set_bitrate(wanted)?;
        }
        let force = state.keyframe.swap(false, Ordering::AcqRel);
        let output = self.encoder.encode(&self.yuv, chroma, force)?;
        state.backend.store(
            match self.encoder.backend() {
                Backend::Hardware => 1,
                Backend::Software => 2,
            },
            Ordering::Release,
        );
        let encoded = output.map(|output| EncodedFrame {
            timestamp: rtp_ticks(self.origin.elapsed()),
            keyframe: output.keyframe,
            data: output.data,
        });
        // A requested keyframe that did not come out as one is asked for again.
        if force && !encoded.as_ref().is_some_and(|frame| frame.keyframe) {
            state.keyframe.store(true, Ordering::Release);
        }
        Ok(CameraFrame { preview, encoded })
    }
}

#[cfg(windows)]
fn run(
    state: &State,
    device: Option<&str>,
    mut on_frame: impl FnMut(CameraFrame),
    opened: &mpsc::SyncSender<Result<()>>,
) -> Result<()> {
    let mut pipeline = Pipeline::new()?;
    let device = device.map(parse_device_id).transpose()?;
    windows::run(
        &state.stop,
        device,
        &mut || {
            let _ = opened.try_send(Ok(()));
        },
        &mut |rgba| {
            if state.stop.load(Ordering::Acquire) {
                return Ok(());
            }
            let frame = pipeline.process(rgba, state)?;
            if !state.stop.load(Ordering::Acquire) {
                on_frame(frame);
            }
            Ok(())
        },
    )
}

#[cfg(not(windows))]
fn run(
    _state: &State,
    _device: Option<&str>,
    _on_frame: impl FnMut(CameraFrame),
    _opened: &mpsc::SyncSender<Result<()>>,
) -> Result<()> {
    Err(Error::Unsupported("camera capture"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn device_ids_name_their_backend_and_reject_junk() {
        assert_eq!(
            parse_device_id("mf:\\\\?\\usb#vid_046d").unwrap(),
            DeviceRef::MediaFoundation("\\\\?\\usb#vid_046d")
        );
        assert_eq!(
            parse_device_id("dshow:@device:sw:{860BB310}\\{A8A0D6C7}").unwrap(),
            DeviceRef::DirectShow("@device:sw:{860BB310}\\{A8A0D6C7}")
        );
        let too_long = format!("mf:{}", "x".repeat(MAX_ID_BYTES));
        for bad in [
            "",
            "mf:",
            "dshow:",
            "usb:1",
            "plain",
            "dshow:bad\0id",
            too_long.as_str(),
        ] {
            assert!(parse_device_id(bad).is_err(), "{bad:?}");
        }
        // Exactly at the limit is accepted.
        let at_limit = format!("dshow:{}", "x".repeat(MAX_ID_BYTES - 6));
        assert!(parse_device_id(&at_limit).is_ok());
    }

    #[test]
    fn start_refuses_bad_ids_before_touching_any_device() {
        for id in ["x".repeat(4097), "dshow:bad\0id".into(), "nope".into()] {
            let error = Camera::start(Some(&id), |_| panic!("no capture")).err();
            assert!(matches!(error, Some(Error::Invalid(_))), "{error:?}");
        }
    }

    #[test]
    fn frames_are_640x480_and_encoded_with_keyframes_on_request() {
        // The pipeline without a device: synthetic pictures through the software encoder.
        let state = State::default();
        state.keyframe.store(true, Ordering::Release);
        let mut pipeline = Pipeline {
            encoder: Encoder::software(camera_config()).unwrap(),
            yuv: vec![0; WIDTH * HEIGHT * 3 / 2],
            origin: Instant::now(),
            background: crate::background::Processor::new(),
        };
        let picture = |shade: u8| {
            let mut rgba = vec![shade; WIDTH * HEIGHT * 4];
            for (index, pixel) in rgba.chunks_exact_mut(4).enumerate().take(WIDTH) {
                pixel.copy_from_slice(&[(index % 251) as u8, shade, (index % 97) as u8, 255]);
            }
            rgba
        };
        assert!(pipeline.process(vec![0; 10], &state).is_err());
        let first = pipeline.process(picture(40), &state).unwrap();
        assert_eq!(
            (first.preview.width, first.preview.height),
            (WIDTH as u32, HEIGHT as u32)
        );
        let encoded = first.encoded.expect("first picture is encoded");
        assert!(encoded.keyframe && crate::h264::has_parameter_sets(&encoded.data));
        assert!(encoded.data.len() <= MAX_ENCODED_BYTES);
        assert!(!state.keyframe.load(Ordering::Acquire));
        // Timestamps advance in 90 kHz ticks.
        std::thread::sleep(Duration::from_millis(20));
        let later = loop {
            let frame = pipeline.process(picture(41), &state).unwrap();
            if let Some(frame) = frame.encoded {
                break frame;
            }
        };
        assert!(!later.keyframe);
        assert!(later.timestamp > encoded.timestamp);
        // A request makes the next encoded picture a keyframe again.
        state.keyframe.store(true, Ordering::Release);
        let forced = pipeline
            .process(picture(42), &state)
            .unwrap()
            .encoded
            .unwrap();
        assert!(forced.keyframe);
    }
}
