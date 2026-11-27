//! Video Background: the person stays, the room behind them is blurred or replaced, before the picture reaches the
//! encoder and the local preview.
//!
//! The official client does this in its native voice module (`discord_voice.node` with `mediapipe.dll`): a MediaPipe
//! graph runs the Selfie Segmentation model shipped beside it (`selfie_segmentation.tflite`, 256x256, and
//! `selfie_segmentation_landscape.tflite`, 256x144; MediaPipe, Apache-2.0), smooths the mask and mixes. Here the same
//! model runs on [`tflite`]'s CPU interpreter and the same smoothing and mixing run in [`composite`].
//!
//! The model is not bundled (nothing from the official build is). It is looked up
//! by file name in the folders [`set_model_dirs`] names; without it [`available`] is false, the app hides the
//! choice as the official client does when its engine lacks the filter, and frames pass through untouched.
//!
//! Capture never waits for the model (the graph's `FlowLimiterCalculator`): each frame hands its downscaled copy to a
//! worker thread, replacing one not yet taken, and is mixed with the newest mask the worker has finished. Until the
//! first mask exists the whole frame counts as background, so the room never shows unprocessed.

pub mod animated;
pub mod composite;
pub mod tflite;

use crate::Picture;
use animated::Animation;
use composite::{BlurBuffers, Mask};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::thread::JoinHandle;

/// The model file names, as the official build ships them.
pub const MODEL_GENERAL: &str = "selfie_segmentation.tflite";
pub const MODEL_LANDSCAPE: &str = "selfie_segmentation_landscape.tflite";

/// What is behind the person.
#[derive(Clone, Debug, Default)]
pub enum Background {
    #[default]
    None,
    Blur,
    /// An RGBA picture, scaled to cover the frame.
    Image(Arc<Picture>),
    /// A clip playing on its own thread (an MP4 preset, a GIF or MP4 custom background), its frame on show scaled to
    /// cover the frame.
    Animated(Arc<Animation>),
}

impl PartialEq for Background {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::None, Self::None) | (Self::Blur, Self::Blur) => true,
            (Self::Image(a), Self::Image(b)) => Arc::ptr_eq(a, b),
            (Self::Animated(a), Self::Animated(b)) => Arc::ptr_eq(a, b),
            _ => false,
        }
    }
}

struct Setting {
    background: Mutex<Background>,
    generation: AtomicU64,
    dirs: Mutex<Vec<PathBuf>>,
    /// The outcome of the one load since the folders were set: `Some(None)` is a load that found nothing usable.
    models: Mutex<Option<Option<Arc<Models>>>>,
}

fn setting() -> &'static Setting {
    static SETTING: OnceLock<Setting> = OnceLock::new();
    SETTING.get_or_init(|| Setting {
        background: Mutex::new(Background::None),
        generation: AtomicU64::new(0),
        dirs: Mutex::new(Vec::new()),
        models: Mutex::new(None),
    })
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|e| e.into_inner())
}

/// Chooses the background every running and future camera uses. Takes effect on the next frame.
pub fn set_background(background: Background) {
    let setting = setting();
    *lock(&setting.background) = background;
    setting.generation.fetch_add(1, Ordering::AcqRel);
}

/// The background chosen now.
pub fn background() -> Background {
    lock(&setting().background).clone()
}

/// The folders searched for the model files, first match wins.
pub fn set_model_dirs(dirs: Vec<PathBuf>) {
    let setting = setting();
    *lock(&setting.dirs) = dirs;
    *lock(&setting.models) = None;
}

/// The model files found, if any.
pub fn model_paths() -> Vec<PathBuf> {
    let dirs = lock(&setting().dirs).clone();
    [MODEL_LANDSCAPE, MODEL_GENERAL]
        .iter()
        .filter_map(|name| {
            dirs.iter()
                .map(|dir| dir.join(name))
                .find(|path| path.is_file())
        })
        .collect()
}

/// The loaded models: the landscape one for wide frames, the general one for the rest; either may stand in.
pub struct Models {
    general: Option<Arc<tflite::Model>>,
    landscape: Option<Arc<tflite::Model>>,
}

impl Models {
    fn load(dirs: &[PathBuf]) -> Option<Self> {
        let read = |name: &str| -> Option<Arc<tflite::Model>> {
            let path = dirs
                .iter()
                .map(|dir| dir.join(name))
                .find(|path| path.is_file())?;
            match std::fs::read(&path)
                .map_err(|e| e.to_string())
                .and_then(|bytes| tflite::Model::parse(&bytes).map_err(|e| e.0))
            {
                Ok(model) => Some(Arc::new(model)),
                Err(error) => {
                    tracing::warn!("segmentation model {} refused: {error}", path.display());
                    None
                }
            }
        };
        let models = Self {
            general: read(MODEL_GENERAL),
            landscape: read(MODEL_LANDSCAPE),
        };
        (models.general.is_some() || models.landscape.is_some()).then_some(models)
    }

    /// The model the official graph's `model_selection` would pick for a frame of this shape.
    fn pick(&self, width: usize, height: usize) -> Option<Arc<tflite::Model>> {
        let wide = width as f32 >= height as f32 * 1.5;
        let (first, second) = if wide {
            (&self.landscape, &self.general)
        } else {
            (&self.general, &self.landscape)
        };
        first.as_ref().or(second.as_ref()).cloned()
    }
}

fn models() -> Option<Arc<Models>> {
    let setting = setting();
    let mut slot = lock(&setting.models);
    if let Some(loaded) = slot.as_ref() {
        return loaded.clone();
    }
    let dirs = lock(&setting.dirs).clone();
    // A file that is not what its name says must not take the application down: a parser fault is a failed load.
    let loaded = std::panic::catch_unwind(|| Models::load(&dirs))
        .ok()
        .flatten()
        .map(Arc::new);
    *slot = Some(loaded.clone());
    loaded
}

/// Whether the segmentation model is present and loads: the engine's `supports(VIDEO_BACKGROUND_FILTER)`.
pub fn available() -> bool {
    models().is_some()
}

/// Turns a model input (RGB 0..1 at the model's size) into a mask at the model's output size.
pub type Segment = Box<dyn FnMut(&[f32]) -> Vec<f32> + Send>;

/// A segmenter with its input and output sizes, each (width, height).
pub type Segmenter = (Segment, (usize, usize), (usize, usize));

/// Makes a [`Segmenter`] when a worker starts.
type MakeSegmenter = Box<dyn FnMut() -> Segmenter + Send>;

struct Job {
    rgb: Vec<f32>,
}

#[derive(Default)]
struct Shared {
    pending: Mutex<Option<Job>>,
    wake: Condvar,
    latest: Mutex<Option<Arc<Mask>>>,
    stop: AtomicBool,
    /// The thread is gone (the model faulted on a frame, or it never started): no mask will come.
    dead: AtomicBool,
    /// Finished masks, for tests and the debug log.
    finished: AtomicU64,
}

/// The inference thread. Dropping it stops and joins it.
struct Worker {
    shared: Arc<Shared>,
    thread: Option<JoinHandle<()>>,
    input: (usize, usize),
}

impl Worker {
    /// `input` and `output` are (width, height) of the model's picture and mask; `frame` is the frame's size, which
    /// sets the bilateral filters' reach in mask pixels.
    fn start(
        mut segment: Segment,
        input: (usize, usize),
        output: (usize, usize),
        frame: (usize, usize),
    ) -> Self {
        let shared = Arc::new(Shared::default());
        let worker = shared.clone();
        let thread = std::thread::Builder::new()
            .name("dc-media-segment".into())
            .spawn(move || {
                let mut previous: Option<Vec<f32>> = None;
                let scale = output.0 as f32 / frame.0.max(1) as f32;
                loop {
                    let job = {
                        let mut pending = lock(&worker.pending);
                        loop {
                            if worker.stop.load(Ordering::Acquire) {
                                return;
                            }
                            if let Some(job) = pending.take() {
                                break job;
                            }
                            pending = worker.wake.wait(pending).unwrap_or_else(|e| e.into_inner());
                        }
                    };
                    let ran = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        segment(&job.rgb)
                    }));
                    let Ok(values) = ran else {
                        worker.dead.store(true, Ordering::Release);
                        return;
                    };
                    if values.len() != output.0 * output.1 {
                        continue;
                    }
                    let mut mask = Mask {
                        width: output.0,
                        height: output.1,
                        values,
                    };
                    for (space, color) in composite::BILATERAL {
                        composite::bilateral(&mut mask, space * scale, color);
                    }
                    if let Some(previous) = &previous {
                        composite::smooth_with_previous(
                            &mut mask.values,
                            previous,
                            composite::SMOOTHING_RATIO,
                        );
                    }
                    previous = Some(mask.values.clone());
                    *lock(&worker.latest) = Some(Arc::new(mask));
                    worker.finished.fetch_add(1, Ordering::AcqRel);
                }
            })
            .ok();
        if thread.is_none() {
            shared.dead.store(true, Ordering::Release);
        }
        Self {
            shared,
            thread,
            input,
        }
    }

    /// Hands a frame's model input over, replacing one the worker has not taken yet. Never waits for inference.
    fn submit(&self, rgb: Vec<f32>) {
        *lock(&self.shared.pending) = Some(Job { rgb });
        self.shared.wake.notify_one();
    }

    fn latest(&self) -> Option<Arc<Mask>> {
        lock(&self.shared.latest).clone()
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::Release);
        self.shared.wake.notify_all();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// Applies the chosen background to one camera's frames. Owned by the camera pipeline.
pub struct Processor {
    worker: Option<Worker>,
    frame: (usize, usize),
    input: Vec<f32>,
    blur: BlurBuffers,
    behind: Vec<u8>,
    /// The cover-fitted picture and what it was made from.
    image: Option<(Arc<Picture>, usize, usize, Vec<u8>)>,
    /// The clip frame in use, at the frame's size.
    shown: Option<Arc<Vec<u8>>>,
    /// A stand-in segmenter for tests.
    segment: Option<MakeSegmenter>,
}

impl Default for Processor {
    fn default() -> Self {
        Self::new()
    }
}

impl Processor {
    pub fn new() -> Self {
        Self {
            worker: None,
            frame: (0, 0),
            input: Vec::new(),
            blur: BlurBuffers::default(),
            behind: Vec::new(),
            image: None,
            shown: None,
            segment: None,
        }
    }

    /// A processor whose masks come from `make` (segmenter, input size, output size) instead of the model files.
    pub fn with_segmenter(make: impl FnMut() -> Segmenter + Send + 'static) -> Self {
        Self {
            segment: Some(Box::new(make)),
            ..Self::new()
        }
    }

    fn ensure_worker(&mut self, width: usize, height: usize) -> bool {
        if self.worker.is_some() && self.frame == (width, height) {
            return true;
        }
        self.worker = None;
        self.frame = (width, height);
        let made = match self.segment.as_mut() {
            Some(make) => Some(make()),
            None => models().and_then(|models| {
                let model = models.pick(width, height)?;
                let (ih, iw) = model.input_size();
                let (oh, ow) = model.output_size();
                let mut interpreter = tflite::Interpreter::new(model);
                let segment: Segment = Box::new(move |rgb| interpreter.run(rgb).to_vec());
                Some((segment, (iw, ih), (ow, oh)))
            }),
        };
        match made {
            Some((segment, input, output)) => {
                self.worker = Some(Worker::start(segment, input, output, (width, height)));
                true
            }
            None => false,
        }
    }

    /// Masks the worker has finished so far.
    pub fn masks_done(&self) -> u64 {
        self.worker
            .as_ref()
            .map_or(0, |worker| worker.shared.finished.load(Ordering::Acquire))
    }

    /// Applies the current background to an RGBA frame in place. Returns whether it changed anything.
    pub fn apply(&mut self, rgba: &mut [u8], width: usize, height: usize) -> bool {
        self.apply_with(&background(), rgba, width, height)
    }

    /// [`Self::apply`] with an explicit background.
    pub fn apply_with(
        &mut self,
        background: &Background,
        rgba: &mut [u8],
        width: usize,
        height: usize,
    ) -> bool {
        if matches!(background, Background::None)
            || rgba.len() < width * height * 4
            || width == 0
            || height == 0
        {
            self.worker = None;
            return false;
        }
        if !self.ensure_worker(width, height) {
            return false;
        }
        let Some(worker) = self.worker.as_ref() else {
            return false;
        };
        if worker.shared.dead.load(Ordering::Acquire) {
            // Without masks the picture goes out as the camera gave it, not with the person blurred away.
            return false;
        }
        let (iw, ih) = worker.input;
        composite::model_input(rgba, width, height, iw, ih, &mut self.input);
        worker.submit(std::mem::take(&mut self.input));
        let mask = worker
            .latest()
            .unwrap_or_else(|| Arc::new(Mask::background(1, 1)));
        let frame = &mut rgba[..width * height * 4];
        let behind: &[u8] = match background {
            Background::None => return false,
            Background::Blur => {
                composite::blur(
                    frame,
                    width,
                    height,
                    &mask,
                    composite::BLUR_ITERATIONS,
                    &mut self.blur,
                    &mut self.behind,
                );
                &self.behind
            }
            Background::Image(picture) => {
                let fresh = self.image.as_ref().is_some_and(|(source, w, h, _)| {
                    Arc::ptr_eq(source, picture) && (*w, *h) == (width, height)
                });
                if !fresh {
                    let rgba_source = picture.to_rgba().unwrap_or_default();
                    let fitted = composite::cover(
                        &rgba_source,
                        picture.width as usize,
                        picture.height as usize,
                        width,
                        height,
                    );
                    self.image = Some((picture.clone(), width, height, fitted));
                }
                match &self.image {
                    Some((_, _, _, fitted)) => fitted,
                    None => return false,
                }
            }
            Background::Animated(animation) => {
                animation.want(width, height);
                let Some((_, size, pixels)) = animation.current() else {
                    // Until the first frame is decoded the room is black, never the unprocessed camera.
                    self.behind.clear();
                    self.behind.resize(width * height * 4, 0);
                    composite::light_wrap(
                        frame,
                        width,
                        height,
                        &self.behind,
                        &mask,
                        composite::CONFIDENCE_THRESHOLD,
                    );
                    return true;
                };
                if size == (width, height) {
                    self.shown = Some(pixels);
                } else {
                    // The first frames, decoded before the player knew this size.
                    self.shown = Some(Arc::new(composite::cover(
                        &pixels, size.0, size.1, width, height,
                    )));
                }
                match &self.shown {
                    Some(pixels) => pixels,
                    None => return false,
                }
            }
        };
        composite::light_wrap(
            frame,
            width,
            height,
            behind,
            &mask,
            composite::CONFIDENCE_THRESHOLD,
        );
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    const W: usize = 64;
    const H: usize = 48;

    /// A segmenter that calls the left half of its 16x16 input the person, after `delay` (after `late` from the
    /// second call on, when given).
    fn halves(delay: Duration, late: Option<Duration>) -> Processor {
        Processor::with_segmenter(move || {
            let mut calls = 0u32;
            let segment: Segment = Box::new(move |rgb: &[f32]| {
                calls += 1;
                std::thread::sleep(if calls > 1 {
                    late.unwrap_or(delay)
                } else {
                    delay
                });
                assert_eq!(rgb.len(), 16 * 16 * 3);
                (0..16 * 16)
                    .map(|i| if i % 16 < 8 { 1.0 } else { 0.0 })
                    .collect()
            });
            (segment, (16, 16), (16, 16))
        })
    }

    fn frame(rgb: [u8; 3]) -> Vec<u8> {
        let mut out = Vec::with_capacity(W * H * 4);
        for _ in 0..W * H {
            out.extend_from_slice(&[rgb[0], rgb[1], rgb[2], 255]);
        }
        out
    }

    fn picture(rgb: [u8; 3], width: u32, height: u32) -> Arc<Picture> {
        let mut data = Vec::new();
        for _ in 0..width * height {
            data.extend_from_slice(&[rgb[0], rgb[1], rgb[2], 255]);
        }
        Arc::new(Picture::rgba(width, height, data).unwrap())
    }

    fn wait_for_mask(processor: &mut Processor, background: &Background) {
        let started = Instant::now();
        while processor.masks_done() == 0 {
            assert!(
                started.elapsed() < Duration::from_secs(5),
                "no mask arrived"
            );
            let mut scratch = frame([0, 0, 0]);
            processor.apply_with(background, &mut scratch, W, H);
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn none_leaves_the_frame_alone_and_starts_nothing() {
        let mut processor = halves(Duration::ZERO, None);
        let mut pixels = frame([1, 2, 3]);
        assert!(!processor.apply_with(&Background::None, &mut pixels, W, H));
        assert_eq!(pixels, frame([1, 2, 3]));
        assert!(processor.worker.is_none());
    }

    #[test]
    fn replacement_keeps_the_person_and_swaps_the_room() {
        let image = Background::Image(picture([0, 0, 255], 32, 18));
        let mut processor = halves(Duration::ZERO, None);
        wait_for_mask(&mut processor, &image);
        let mut pixels = frame([200, 50, 0]);
        assert!(processor.apply_with(&image, &mut pixels, W, H));
        let at = |x: usize, y: usize| pixels[(y * W + x) * 4..(y * W + x) * 4 + 3].to_vec();
        assert_eq!(at(2, 24), vec![200, 50, 0], "person");
        assert_eq!(at(W - 3, 24), vec![0, 0, 255], "room");
        // The edge between them is feathered, not a hard cut.
        let blue: Vec<u8> = (0..W).map(|x| at(x, 24)[2]).collect();
        assert!(blue.iter().any(|b| *b > 20 && *b < 235), "{blue:?}");
    }

    #[test]
    fn before_the_first_mask_everything_is_room() {
        let image = Background::Image(picture([9, 9, 9], 4, 4));
        let mut processor = halves(Duration::from_millis(300), None);
        let mut pixels = frame([200, 50, 0]);
        let started = Instant::now();
        assert!(processor.apply_with(&image, &mut pixels, W, H));
        assert!(
            started.elapsed() < Duration::from_millis(250),
            "the first frame does not wait for the model"
        );
        assert!(pixels.chunks_exact(4).all(|p| p == [9, 9, 9, 255]));
    }

    #[test]
    fn a_late_model_never_stalls_capture_and_its_last_mask_is_reused() {
        let blur = Background::Blur;
        let mut processor = halves(Duration::ZERO, Some(Duration::from_millis(400)));
        wait_for_mask(&mut processor, &blur);
        let done = processor.masks_done();
        let mut slowest = Duration::ZERO;
        for _ in 0..10 {
            let mut pixels = frame([255, 255, 255]);
            let started = Instant::now();
            assert!(processor.apply_with(&blur, &mut pixels, W, H));
            slowest = slowest.max(started.elapsed());
            // The person half is still the camera: the earlier mask is in use.
            let p = (24 * W + 2) * 4;
            assert_eq!(&pixels[p..p + 3], &[255, 255, 255]);
        }
        assert!(
            slowest < Duration::from_millis(100),
            "a frame took {slowest:?}"
        );
        assert!(
            processor.masks_done() <= done + 1,
            "the model is still busy with one frame"
        );
    }

    #[test]
    fn the_choice_is_global() {
        set_background(Background::Blur);
        assert_eq!(background(), Background::Blur);
        set_background(Background::None);
        assert_eq!(background(), Background::None);
    }

    #[test]
    fn without_model_files_nothing_is_available_and_frames_pass() {
        set_model_dirs(vec![std::env::temp_dir().join("dc-media-no-models-here")]);
        assert!(!available());
        assert!(model_paths().is_empty());
        let mut processor = Processor::new();
        let mut pixels = frame([5, 6, 7]);
        assert!(!processor.apply_with(&Background::Blur, &mut pixels, W, H));
        assert_eq!(pixels, frame([5, 6, 7]));
    }

    /// Milliseconds per frame `apply_with` takes on the capture thread at `fw` x `fh`, the mask coming from a stand-in
    /// at the model size the official graph would pick for that shape.
    fn ms_per_frame(background: &Background, fw: usize, fh: usize) -> f64 {
        let camera = test_camera(fw, fh);
        let (mw, mh) = if fw as f32 >= fh as f32 * 1.5 {
            (256, 144)
        } else {
            (256, 256)
        };
        let mut processor = Processor::with_segmenter(move || {
            let segment: Segment = Box::new(move |_: &[f32]| {
                (0..mw * mh)
                    .map(|i| if (i % mw) < mw / 2 { 1.0 } else { 0.0 })
                    .collect()
            });
            (segment, (mw, mh), (mw, mh))
        });
        let started = Instant::now();
        while processor.masks_done() == 0 && started.elapsed() < Duration::from_secs(5) {
            let mut pixels = camera.clone();
            processor.apply_with(background, &mut pixels, fw, fh);
        }
        // Warm the buffers, then time.
        for _ in 0..3 {
            let mut pixels = camera.clone();
            processor.apply_with(background, &mut pixels, fw, fh);
        }
        let frames = 20;
        let mut total = Duration::ZERO;
        for _ in 0..frames {
            let mut pixels = camera.clone();
            let started = Instant::now();
            processor.apply_with(background, &mut pixels, fw, fh);
            total += started.elapsed();
        }
        total.as_secs_f64() * 1000.0 / f64::from(frames)
    }

    fn test_camera(fw: usize, fh: usize) -> Vec<u8> {
        let mut camera = Vec::with_capacity(fw * fh * 4);
        for i in 0..fw * fh {
            let (x, y) = (i % fw, i / fw);
            camera.extend_from_slice(&[
                (x % 256) as u8,
                (y % 256) as u8,
                ((x + y) % 256) as u8,
                255,
            ]);
        }
        camera
    }

    #[test]
    fn bench_ms_per_frame() {
        for (fw, fh) in [(640, 480), (1280, 720)] {
            for (name, background) in [
                ("blur", Background::Blur),
                (
                    "image",
                    Background::Image(picture([30, 60, 90], 1920, 1080)),
                ),
            ] {
                let ms = ms_per_frame(&background, fw, fh);
                eprintln!("video background {name} at {fw}x{fh}: {ms:.2} ms per frame");
            }
        }
        // The model itself, when the local snapshot holds it.
        const FW: usize = 1280;
        const FH: usize = 720;
        let camera = test_camera(FW, FH);
        let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../design/snapshot/desktop/modules/discord_voice-1/discord_voice")
            .join(MODEL_LANDSCAPE);
        if let Ok(bytes) = std::fs::read(path) {
            let model = Arc::new(tflite::Model::parse(&bytes).unwrap());
            let mut interpreter = tflite::Interpreter::new(model);
            let mut input = Vec::new();
            composite::model_input(&camera, FW, FH, 256, 144, &mut input);
            interpreter.run(&input);
            let started = Instant::now();
            for _ in 0..5 {
                interpreter.run(&input);
            }
            eprintln!(
                "segmentation model (landscape): {:.2} ms per inference on its own thread",
                started.elapsed().as_secs_f64() * 200.0
            );
        }
    }
}
