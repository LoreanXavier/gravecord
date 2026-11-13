//! Animated backgrounds: the four MP4 presets and GIF or MP4 custom backgrounds. The official web layer hands the file
//! itself to the native engine (module 854325 `m`: `isVideo` presets and `a_`/`v_` asset hashes go as a blob, stills as
//! a 1280x720 canvas), which decodes and loops it behind the person.
//!
//! Here a player thread decodes the clip one frame at a time and shows each frame for its own duration, starting over
//! at the end; the camera takes whichever frame is showing when it mixes. Only the encoded file and the frame on show
//! are held, so a long clip costs no more memory than a short one. The frame is cover-fitted to the camera's size on
//! the player thread, so the capture thread only copies it.

use super::composite;
use crate::Picture;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// A clip decoded one frame at a time. Implemented by the app over its MP4 reader and GIF decoder. It is made on the
/// player thread and stays there, so it need not be `Send`.
pub trait FrameDecoder {
    /// The next frame and how long it shows, `None` at the end of the clip (or on a damaged frame).
    fn next_frame(&mut self) -> Option<(Duration, Picture)>;
    /// Back to the first frame. False when the clip cannot start over.
    fn rewind(&mut self) -> bool;
}

/// The shortest time a frame is shown: a clip that claims less (or nothing) would spin the player thread.
pub const MIN_FRAME: Duration = Duration::from_millis(10);

/// A frame on show as the camera takes it: its serial, its size and its RGBA.
pub type Frame = (u64, (usize, usize), Arc<Vec<u8>>);

/// The frame on show, fitted to `size` when the camera asked for one.
struct Showing {
    serial: u64,
    size: (usize, usize),
    fitted: Arc<Vec<u8>>,
}

#[derive(Default)]
struct Shared {
    showing: Mutex<Option<Showing>>,
    /// The camera's frame size, `width << 32 | height`; 0 until a camera asks.
    target: AtomicU64,
    stop: AtomicBool,
    wake: Condvar,
    sleep: Mutex<()>,
    /// Frames shown so far, and how many times the clip started over.
    shown: AtomicU64,
    loops: AtomicU64,
    /// The clip had no frame at all, or decoding failed before the first one.
    failed: AtomicBool,
}

/// A playing clip. Dropping the last handle stops its thread.
pub struct Animation {
    shared: Arc<Shared>,
    thread: Mutex<Option<JoinHandle<()>>>,
}

impl Animation {
    /// Starts playing on its own thread the decoder `make` makes there (`None`: the clip cannot be read).
    pub fn start(
        make: impl FnOnce() -> Option<Box<dyn FrameDecoder>> + Send + 'static,
    ) -> Arc<Self> {
        let shared = Arc::new(Shared::default());
        let worker = shared.clone();
        let thread = std::thread::Builder::new()
            .name("dc-media-background-clip".into())
            .spawn(move || match make() {
                Some(mut decoder) => play(&mut *decoder, &worker),
                None => worker.failed.store(true, Ordering::Release),
            })
            .ok();
        if thread.is_none() {
            shared.failed.store(true, Ordering::Release);
        }
        Arc::new(Self {
            shared,
            thread: Mutex::new(thread),
        })
    }

    /// Tells the player the camera's frame size, so the frames come fitted to it.
    pub fn want(&self, width: usize, height: usize) {
        let packed = (width as u64) << 32 | height as u64;
        self.shared.target.store(packed, Ordering::Release);
    }

    /// The frame on show as (serial, its size, RGBA), if one is decoded yet. The serial changes with every frame.
    pub fn current(&self) -> Option<Frame> {
        let showing = self
            .shared
            .showing
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        showing
            .as_ref()
            .map(|showing| (showing.serial, showing.size, showing.fitted.clone()))
    }

    /// Frames shown so far.
    pub fn frames_shown(&self) -> u64 {
        self.shared.shown.load(Ordering::Acquire)
    }

    /// Times the clip started over.
    pub fn loops(&self) -> u64 {
        self.shared.loops.load(Ordering::Acquire)
    }

    /// No frame will come: the clip is empty or could not be decoded.
    pub fn failed(&self) -> bool {
        self.shared.failed.load(Ordering::Acquire)
    }
}

impl std::fmt::Debug for Animation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Animation")
            .field("frames_shown", &self.frames_shown())
            .field("loops", &self.loops())
            .field("failed", &self.failed())
            .finish()
    }
}

impl Drop for Animation {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::Release);
        self.shared.wake.notify_all();
        let thread = self
            .thread
            .get_mut()
            .unwrap_or_else(|e| e.into_inner())
            .take();
        if let Some(thread) = thread {
            let _ = thread.join();
        }
    }
}

/// Waits until `until` unless the animation stops first. False when it stopped.
fn wait_until(shared: &Shared, until: Instant) -> bool {
    let mut guard = shared.sleep.lock().unwrap_or_else(|e| e.into_inner());
    loop {
        if shared.stop.load(Ordering::Acquire) {
            return false;
        }
        let now = Instant::now();
        if now >= until {
            return true;
        }
        guard = shared
            .wake
            .wait_timeout(guard, until - now)
            .unwrap_or_else(|e| e.into_inner())
            .0;
    }
}

fn play(decoder: &mut dyn FrameDecoder, shared: &Shared) {
    let mut due = Instant::now();
    let mut serial = 0u64;
    let mut in_this_loop = 0u64;
    loop {
        if shared.stop.load(Ordering::Acquire) {
            return;
        }
        let Some((duration, picture)) = decoder.next_frame() else {
            if in_this_loop == 0 || !decoder.rewind() {
                if serial == 0 {
                    shared.failed.store(true, Ordering::Release);
                }
                return;
            }
            in_this_loop = 0;
            shared.loops.fetch_add(1, Ordering::AcqRel);
            continue;
        };
        in_this_loop += 1;
        let target = shared.target.load(Ordering::Acquire);
        let (width, height) = ((target >> 32) as usize, (target & 0xffff_ffff) as usize);
        let (size, fitted) = match (width, height) {
            (0, _) | (_, 0) => (
                (picture.width as usize, picture.height as usize),
                picture.to_rgba().unwrap_or_default(),
            ),
            size => (
                size,
                composite::cover(
                    &picture.to_rgba().unwrap_or_default(),
                    picture.width as usize,
                    picture.height as usize,
                    width,
                    height,
                ),
            ),
        };
        if !wait_until(shared, due) {
            return;
        }
        serial += 1;
        *shared.showing.lock().unwrap_or_else(|e| e.into_inner()) = Some(Showing {
            serial,
            size,
            fitted: Arc::new(fitted),
        });
        shared.shown.fetch_add(1, Ordering::AcqRel);
        let now = Instant::now();
        due += duration.max(MIN_FRAME);
        // A frame that took longer to decode than to show does not make the next ones rush to catch up.
        if due < now {
            due = now;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `count` flat frames whose red channel is their index, each shown `each`; `rewinds` times it may start over.
    struct Frames {
        count: u8,
        next: u8,
        each: Duration,
        rewinds: Option<u32>,
    }

    impl FrameDecoder for Frames {
        fn next_frame(&mut self) -> Option<(Duration, Picture)> {
            if self.next >= self.count {
                return None;
            }
            let index = self.next;
            self.next += 1;
            let data = [index, 0, 0, 255].repeat(4);
            Some((self.each, Picture::rgba(2, 2, data)?))
        }

        fn rewind(&mut self) -> bool {
            match &mut self.rewinds {
                Some(0) => false,
                Some(left) => {
                    *left -= 1;
                    self.next = 0;
                    true
                }
                None => {
                    self.next = 0;
                    true
                }
            }
        }
    }

    fn red(animation: &Animation) -> Option<(u64, u8)> {
        animation
            .current()
            .map(|(serial, _, pixels)| (serial, pixels[0]))
    }

    #[test]
    fn frames_step_in_order_at_their_own_pace_and_loop() {
        let animation = Animation::start(move || -> Option<Box<dyn FrameDecoder>> {
            Some(Box::new(Frames {
                count: 3,
                next: 0,
                each: Duration::from_millis(40),
                rewinds: None,
            }))
        });
        let started = Instant::now();
        let mut seen: Vec<u8> = Vec::new();
        let mut last_serial = 0;
        while animation.loops() < 2 && started.elapsed() < Duration::from_secs(10) {
            if let Some((serial, index)) = red(&animation)
                && serial != last_serial
            {
                last_serial = serial;
                seen.push(index);
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        assert!(animation.loops() >= 2, "the clip starts over at its end");
        // Every frame shown is the one after the previous, wrapping from the last to the first. (A frame may be missed
        // by the sampling here, never shown out of order.)
        for pair in seen.windows(2) {
            let expected = (pair[0] + 1) % 3;
            assert!(
                pair[1] == expected || pair[1] == (expected + 1) % 3,
                "{seen:?}"
            );
        }
        assert!(seen.contains(&0) && seen.contains(&2), "{seen:?}");
        // Two loops of three frames of 40 ms take at least 200 ms: frames are not rushed.
        assert!(started.elapsed() >= Duration::from_millis(200));
        assert!(animation.frames_shown() >= 6);
    }

    #[test]
    fn frames_come_fitted_to_the_camera() {
        let animation = Animation::start(move || -> Option<Box<dyn FrameDecoder>> {
            Some(Box::new(Frames {
                count: 2,
                next: 0,
                each: Duration::from_millis(15),
                rewinds: None,
            }))
        });
        animation.want(8, 6);
        let started = Instant::now();
        loop {
            if let Some((_, size, pixels)) = animation.current()
                && size == (8, 6)
            {
                assert_eq!(pixels.len(), 8 * 6 * 4);
                break;
            }
            assert!(started.elapsed() < Duration::from_secs(5));
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    #[test]
    fn an_empty_clip_fails_and_a_clip_that_cannot_rewind_keeps_its_last_frame() {
        let empty = Animation::start(move || -> Option<Box<dyn FrameDecoder>> {
            Some(Box::new(Frames {
                count: 0,
                next: 0,
                each: Duration::from_millis(10),
                rewinds: None,
            }))
        });
        let started = Instant::now();
        while !empty.failed() {
            assert!(started.elapsed() < Duration::from_secs(5));
            std::thread::sleep(Duration::from_millis(2));
        }
        assert!(empty.current().is_none());
        let once = Animation::start(move || -> Option<Box<dyn FrameDecoder>> {
            Some(Box::new(Frames {
                count: 2,
                next: 0,
                each: Duration::from_millis(5),
                rewinds: Some(0),
            }))
        });
        let started = Instant::now();
        while once.frames_shown() < 2 {
            assert!(started.elapsed() < Duration::from_secs(5));
            std::thread::sleep(Duration::from_millis(2));
        }
        std::thread::sleep(Duration::from_millis(30));
        assert!(!once.failed());
        assert_eq!(red(&once).map(|(_, index)| index), Some(1));
    }
}
