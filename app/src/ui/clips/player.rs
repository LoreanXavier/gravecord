//! Playback and stills of one clip file for the editor, off the interface thread.
//!
//! One thread reads the file, keeps its video as access units and answers commands: show the picture at a time, play
//! from a time to a time at the clip's own pace, and cut the timeline's thumbnails. Pictures are decoded with openh264
//! (no reordering in these files, so every unit gives its picture at once) and scaled to the size they are drawn at
//! before they leave the thread. The interface polls [`Shared`] while it plays; nothing here waits for it.
//!
//! The official editor plays the file through a `<video>` element and cuts the strip with a worker that seeks it
//! (module 923904); here the decoder seeks by keyframe and feeds forward, and a seek inside the group of pictures it
//! is already in continues from where it stands instead of starting over.
use dc_clips::mp4;
use dc_clips::preview::{self, Unit};
use dc_media::Picture;
use dc_media::decode::Decoder;
use std::collections::VecDeque;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

/// A decoded picture ready to draw: BGRA, as the renderer takes it.
pub(in crate::ui) struct Frame {
    /// The picture's start in the clip, milliseconds.
    pub ms: u64,
    pub width: u32,
    pub height: u32,
    pub bgra: Vec<u8>,
    /// Which request it answers: a newer request makes older pictures stale.
    pub serial: u64,
}

/// One cell of the timeline's strip.
pub(in crate::ui) struct Thumb {
    pub slot: usize,
    pub width: u32,
    pub height: u32,
    pub bgra: Vec<u8>,
}

/// What the file turned out to hold, known once it is read.
pub(in crate::ui) struct Loaded {
    pub units: Arc<Vec<Unit>>,
    pub duration_ms: u64,
    pub width: u32,
    pub height: u32,
    /// The audio track names (`application`, `voice`) with their packet counts.
    pub tracks: Vec<(String, u32)>,
    /// Decoded mono 48 kHz samples per track name, in track order.
    pub audio_tracks: Vec<(String, Vec<f32>)>,
}

pub(super) enum Command {
    /// The picture at `ms`.
    Seek {
        ms: u64,
        serial: u64,
    },
    /// Play from `from` and stop before `to`.
    Play {
        from: u64,
        to: u64,
        serial: u64,
    },
    Pause,
    /// The largest size a picture is drawn at, in device pixels.
    Size {
        width: u32,
        height: u32,
    },
    /// The strip: one thumbnail per time, `height` device pixels tall.
    Thumbs {
        times: Vec<u64>,
        height: u32,
    },
    Quit,
}

#[derive(Default)]
pub(in crate::ui) struct Shared {
    pub loaded: OnceLock<Loaded>,
    pub failed: Mutex<Option<String>>,
    pub frame: Mutex<Option<Frame>>,
    pub thumbs: Mutex<Vec<Thumb>>,
    /// Set when a play reached its end or was cut off: the time of the last picture shown.
    pub ended: AtomicBool,
    /// Pictures decoded in all, for the harness and the cost measurement.
    pub decoded: AtomicU64,
    pub playing: AtomicBool,
}

pub(in crate::ui) struct Player {
    pub shared: Arc<Shared>,
    commands: Sender<Command>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Player {
    /// Starts reading `path`. Nothing is drawn until [`Shared::loaded`] is set.
    pub(super) fn open(path: &Path, size: (u32, u32)) -> Self {
        let (commands, receiver) = mpsc::channel();
        let shared = Arc::new(Shared::default());
        let thread = {
            let shared = shared.clone();
            let path = path.to_path_buf();
            std::thread::Builder::new()
                .name("clip-player".into())
                .spawn(move || run(&path, &receiver, &shared, size))
                .ok()
        };
        Self {
            shared,
            commands,
            thread,
        }
    }

    pub(super) fn send(&self, command: Command) {
        let _ = self.commands.send(command);
    }
}

impl Drop for Player {
    fn drop(&mut self) {
        let _ = self.commands.send(Command::Quit);
        // The thread ends on its own; a clip still being read finishes first, so nothing waits for it here.
        drop(self.thread.take());
    }
}

/// Reads the file into units. The audio is only counted: playback here is silent.
fn load(path: &Path) -> Result<Loaded, String> {
    let file = std::fs::File::open(path).map_err(|error| error.to_string())?;
    let mut reader = std::io::BufReader::with_capacity(1 << 20, file);
    let clip = mp4::read_clip(&mut reader).map_err(|error| error.to_string())?;
    let (units, duration_ms) = preview::units(&clip);
    if units.is_empty() {
        return Err("the clip has no video".to_owned());
    }
    Ok(Loaded {
        duration_ms,
        width: clip.video.info.width,
        height: clip.video.info.height,
        tracks: clip
            .audio
            .iter()
            .map(|track| (track.name.clone(), track.packets.len() as u32))
            .collect(),
        audio_tracks: clip
            .audio
            .iter()
            .map(|track| {
                (
                    track.name.clone(),
                    dc_clips::audio::decode_track_mono(
                        track.channels,
                        track.pre_skip,
                        &track.packets,
                    ),
                )
            })
            .collect(),
        units: Arc::new(units),
    })
}

/// A decoder fed unit by unit: it remembers the last unit it was given so a seek forward inside the same group of
/// pictures continues instead of decoding from the keyframe again.
struct Feeder {
    decoder: Decoder,
    fed: Option<usize>,
}

impl Feeder {
    fn new() -> Option<Self> {
        Some(Self {
            decoder: Decoder::new().ok()?,
            fed: None,
        })
    }

    /// The picture of unit `index`, feeding from the keyframe (or from where the decoder stands).
    fn picture_at(&mut self, units: &[Unit], index: usize) -> Option<Picture> {
        let key = preview::keyframe_for(units, units[index].start_ms);
        let from = match self.fed {
            Some(fed) if fed < index && fed >= key => fed + 1,
            _ => key,
        };
        let mut last = None;
        for unit in &units[from..=index] {
            if let Ok(Some(picture)) = self.decoder.decode(&unit.data) {
                last = Some(picture);
            }
        }
        self.fed = Some(index);
        last
    }

    /// Feeds one unit in play order, keeping the state in step.
    fn next(&mut self, units: &[Unit], index: usize) -> Option<Picture> {
        self.fed = Some(index);
        self.decoder.decode(&units[index].data).ok().flatten()
    }

    fn forget(&mut self) {
        self.fed = None;
    }
}

fn to_frame(picture: &Picture, size: (u32, u32), ms: u64, serial: u64) -> Option<Frame> {
    let (width, height, mut rgba) = preview::scaled_rgba_area(picture, size.0, size.1)?;
    for pixel in rgba.as_chunks_mut::<4>().0 {
        pixel.swap(0, 2);
    }
    Some(Frame {
        ms,
        width,
        height,
        bgra: rgba,
        serial,
    })
}

fn run(
    path: &std::path::Path,
    receiver: &Receiver<Command>,
    shared: &Shared,
    mut size: (u32, u32),
) {
    let loaded = match load(path) {
        Ok(loaded) => loaded,
        Err(error) => {
            *shared.failed.lock().unwrap() = Some(error);
            return;
        }
    };
    let units = loaded.units.clone();
    let _ = shared.loaded.set(loaded);
    let (Some(mut main), Some(mut strip)) = (Feeder::new(), Feeder::new()) else {
        *shared.failed.lock().unwrap() = Some("no H.264 decoder".to_owned());
        return;
    };
    let mut pending: VecDeque<(usize, u64)> = VecDeque::new();
    let mut thumb_height = 56u32;
    let mut queued: Option<Command> = None;
    loop {
        let command = match queued.take() {
            Some(command) => command,
            None => match receiver.try_recv() {
                Ok(command) => command,
                Err(TryRecvError::Disconnected) => return,
                Err(TryRecvError::Empty) => {
                    if let Some((slot, ms)) = pending.pop_front() {
                        if let Some(thumb) = cut_thumb(&units, &mut strip, slot, ms, thumb_height) {
                            shared.thumbs.lock().unwrap().push(thumb);
                        }
                        continue;
                    }
                    match receiver.recv() {
                        Ok(command) => command,
                        Err(_) => return,
                    }
                }
            },
        };
        match command {
            Command::Quit => return,
            Command::Size { width, height } => size = (width.max(1), height.max(1)),
            Command::Pause => shared.playing.store(false, Ordering::Relaxed),
            Command::Thumbs { times, height } => {
                thumb_height = height.max(1);
                pending = times.into_iter().enumerate().collect();
            }
            Command::Seek { ms, serial } => {
                // Only the newest seek of a burst is shown.
                let mut ms = ms;
                let mut serial = serial;
                loop {
                    match receiver.try_recv() {
                        Ok(Command::Seek {
                            ms: newer,
                            serial: newer_serial,
                        }) => {
                            ms = newer;
                            serial = newer_serial;
                        }
                        Ok(Command::Size { width, height }) => size = (width.max(1), height.max(1)),
                        Ok(other) => {
                            queued = Some(other);
                            break;
                        }
                        Err(_) => break,
                    }
                }
                let index = preview::unit_at(&units, ms);
                if let Some(picture) = main.picture_at(&units, index) {
                    shared.decoded.fetch_add(1, Ordering::Relaxed);
                    if let Some(frame) = to_frame(&picture, size, units[index].start_ms, serial) {
                        *shared.frame.lock().unwrap() = Some(frame);
                    }
                }
            }
            Command::Play { from, to, serial } => {
                shared.ended.store(false, Ordering::Relaxed);
                shared.playing.store(true, Ordering::Relaxed);
                queued = play(
                    &units,
                    &mut main,
                    receiver,
                    shared,
                    &mut size,
                    (from, to),
                    serial,
                );
                shared.playing.store(false, Ordering::Relaxed);
            }
        }
    }
}

/// Plays `range` against the clock. Returns the command that interrupted it, if one did.
fn play(
    units: &[Unit],
    feeder: &mut Feeder,
    receiver: &Receiver<Command>,
    shared: &Shared,
    size: &mut (u32, u32),
    range: (u64, u64),
    serial: u64,
) -> Option<Command> {
    let (from, to) = range;
    let first = preview::unit_at(units, from);
    // Catch up silently to the first picture, then start the clock.
    let mut shown = feeder.picture_at(units, first);
    if let Some(picture) = shown.take()
        && let Some(frame) = to_frame(&picture, *size, units[first].start_ms, serial)
    {
        *shared.frame.lock().unwrap() = Some(frame);
    }
    if feeder.fed != Some(first) {
        feeder.forget();
    }
    let origin = units[first].start_ms;
    let clock = Instant::now();
    let mut index = first + 1;
    while index < units.len() && units[index].start_ms < to {
        let due = Duration::from_millis(units[index].start_ms - origin);
        let picture = feeder.next(units, index);
        shared.decoded.fetch_add(1, Ordering::Relaxed);
        // Wait for the picture's time; a command that arrives meanwhile ends the wait.
        loop {
            let received = match due.checked_sub(clock.elapsed()) {
                Some(left) if !left.is_zero() => receiver.recv_timeout(left),
                _ => receiver.try_recv().map_err(|error| match error {
                    TryRecvError::Empty => mpsc::RecvTimeoutError::Timeout,
                    TryRecvError::Disconnected => mpsc::RecvTimeoutError::Disconnected,
                }),
            };
            match received {
                Ok(Command::Pause) => return None,
                Ok(Command::Size { width, height }) => *size = (width.max(1), height.max(1)),
                Ok(other) => return Some(other),
                Err(mpsc::RecvTimeoutError::Timeout) => break,
                Err(mpsc::RecvTimeoutError::Disconnected) => return Some(Command::Quit),
            }
        }
        let next_due = units
            .get(index + 1)
            .map_or(u64::MAX, |unit| unit.start_ms - origin);
        let late = clock.elapsed() > Duration::from_millis(next_due);
        if !late
            && let Some(picture) = picture
            && let Some(frame) = to_frame(&picture, *size, units[index].start_ms, serial)
        {
            *shared.frame.lock().unwrap() = Some(frame);
        }
        index += 1;
    }
    shared.ended.store(true, Ordering::Relaxed);
    None
}

fn cut_thumb(
    units: &[Unit],
    feeder: &mut Feeder,
    slot: usize,
    ms: u64,
    height: u32,
) -> Option<Thumb> {
    let index = preview::unit_at(units, ms);
    let picture = feeder.picture_at(units, index)?;
    let aspect = f64::from(picture.width) / f64::from(picture.height);
    let width = (f64::from(height) * aspect).ceil() as u32;
    let frame = to_frame(&picture, (width, height), 0, 0)?;
    Some(Thumb {
        slot,
        width: frame.width,
        height: frame.height,
        bgra: frame.bgra,
    })
}

/// A still of the clip at `ms`, scaled to fit `fit`, as a JPEG data URL at quality 90 (module 956050: fit 640 x 360).
/// `None` when the picture cannot be decoded.
pub(super) fn still_data_url(units: &[Unit], ms: u64, fit: (u32, u32)) -> Option<String> {
    use base64::Engine as _;
    use image::{DynamicImage, RgbaImage, codecs::jpeg::JpegEncoder};
    let mut feeder = Feeder::new()?;
    let picture = feeder.picture_at(units, preview::unit_at(units, ms))?;
    let (width, height, rgba) = preview::scaled_rgba_area(&picture, fit.0, fit.1)?;
    let rgb = DynamicImage::ImageRgba8(RgbaImage::from_raw(width, height, rgba)?).to_rgb8();
    let mut jpeg = Vec::new();
    JpegEncoder::new_with_quality(&mut jpeg, 90)
        .encode_image(&rgb)
        .ok()?;
    Some(format!(
        "data:image/jpeg;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(jpeg)
    ))
}
