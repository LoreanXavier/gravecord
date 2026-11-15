//! Animated pictures drawn as tiles: the GIF picker's results (module 884037), playing avatars and server icons, and
//! the animated attachments of the chat (module 619517).
//!
//! The renderer's own `img()` decodes every frame of an animation at its full size and keeps them all, in memory and
//! in the sprite atlas (a 300-frame GIF of 500 x 280 is 170 MB), and it only moves on while the whole window is redrawn
//! every frame. A tile instead decodes its own copy at the size it is drawn, cropped to its box, and only while it is on
//! screen:
//!
//! - its first frame is kept as the poster: what shows before playing, while paused and when the animation cannot
//!   have room;
//! - while it plays, a decoder thread runs ahead of the clock by at most `AHEAD` frames (a bounded channel), and goes
//!   round again at the end of the file as many times as the file says: memory does not grow with the length of the
//!   animation;
//! - frame times follow the browser's rule for very short delays (`browser_delay`), and the window is woken when the
//!   next frame is due, on a 60 Hz grid, never by a timer that can be lost: every render looks again.
use super::nameplate_animation::Playback;
use gpui::{
    AnyElement, Context, Corners, IntoElement, RenderImage, Window, canvas, div, prelude::*, px,
};
use image::{AnimationDecoder, RgbaImage, imageops::FilterType};
use std::{
    collections::HashMap,
    io::Cursor,
    sync::{
        Arc,
        mpsc::{Receiver, SyncSender, TryRecvError, sync_channel},
    },
    time::{Duration, Instant},
};

#[cfg(feature = "capture")]
static PAINTS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
#[cfg(feature = "capture")]
static PAINT_ERRORS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
#[cfg(feature = "capture")]
static LAST_BOUNDS: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

/// Decoded frames of every tile on screen together (posters, the frame shown and the frames decoded ahead).
const TOTAL_DECODED: usize = 64 * 1024 * 1024;
/// Frames a playing tile's decoder may have ready ahead of the one shown.
const AHEAD: usize = 3;
/// Poster decodes running at once.
const DECODING: usize = 8;
/// Wakes for the next frame fall on this grid (60 a second), so tiles with their own frame times share redraws.
const GRID: Duration = Duration::from_nanos(16_666_667);
/// A tile that fell further behind than this (a window that did not draw for a while) starts its clock again
/// instead of racing through the frames it missed.
const CATCH_UP: Duration = Duration::from_millis(250);

/// A tile plays while it is on screen, the window is focused, and its owner wants it to (the picker always does;
/// an attachment while "Play GIFs when Discord is focused" is on, or under the pointer).
pub(super) fn should_play(visible: bool, focused: bool, wanted: bool) -> bool {
    visible && focused && wanted
}

/// Whether `bytes` is a picture with more than one frame: an animated WebP (`VP8X` with its animation flag), a GIF,
/// or an animated PNG (an `acTL` chunk before the first `IDAT`).
pub(super) fn animated(bytes: &[u8]) -> bool {
    (bytes.len() > 21
        && bytes.starts_with(b"RIFF")
        && &bytes[8..12] == b"WEBP"
        && &bytes[12..16] == b"VP8X"
        && bytes[20] & 0x02 != 0)
        || bytes.starts_with(b"GIF8")
        || apng(bytes)
}

fn apng(bytes: &[u8]) -> bool {
    if !bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        return false;
    }
    let mut at = 8;
    while at + 8 <= bytes.len() {
        let length = u32::from_be_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]]);
        match &bytes[at + 4..at + 8] {
            b"acTL" => return true,
            b"IDAT" => return false,
            _ => match (length as usize)
                .checked_add(12)
                .and_then(|step| at.checked_add(step))
            {
                Some(next) => at = next,
                None => return false,
            },
        }
    }
    false
}

/// The duration a browser shows a frame for. Chromium's image decoders (as WebKit and Firefox before them) show a
/// frame that asks for 10 ms or less (a GIF delay of 0 or 1 hundredths) for 100 ms instead: many files carry 0 to
/// mean "as fast as possible" and would otherwise spin.
pub(super) fn browser_delay(asked: Duration) -> Duration {
    if asked <= Duration::from_millis(10) {
        Duration::from_millis(100)
    } else {
        asked
    }
}

/// How many times the file says to play: `None` for ever. A GIF's `NETSCAPE2.0` (or `ANIMEXTS1.0`) block holds the
/// number of repeats after the first play, 0 meaning for ever, and a GIF without it plays once (Chromium's
/// `SkCodec::getRepetitionCount`). An animated WebP's `ANIM` chunk and an APNG's `acTL` hold the number of plays,
/// 0 meaning for ever.
pub(super) fn plays(bytes: &[u8]) -> Option<u32> {
    if bytes.starts_with(b"GIF8") {
        for marker in [&b"NETSCAPE2.0"[..], &b"ANIMEXTS1.0"[..]] {
            if let Some(at) = bytes.windows(marker.len()).position(|w| w == marker) {
                let block = &bytes[at + marker.len()..];
                if block.len() >= 4 && block[0] == 3 && block[1] == 1 {
                    let repeats = u16::from_le_bytes([block[2], block[3]]);
                    return (repeats != 0).then(|| u32::from(repeats) + 1);
                }
            }
        }
        return Some(1);
    }
    let count = if bytes.starts_with(b"RIFF") {
        let mut at = 12usize;
        let mut found = None;
        while at + 8 <= bytes.len() {
            let length =
                u32::from_le_bytes([bytes[at + 4], bytes[at + 5], bytes[at + 6], bytes[at + 7]]);
            if &bytes[at..at + 4] == b"ANIM" {
                let data = bytes.get(at + 8..at + 14)?;
                found = Some(u32::from(u16::from_le_bytes([data[4], data[5]])));
                break;
            }
            let padded = (length as usize).checked_add(length as usize & 1)?;
            at = padded
                .checked_add(8)
                .and_then(|step| at.checked_add(step))?;
        }
        found?
    } else if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        let mut at = 8usize;
        let mut found = None;
        while at + 8 <= bytes.len() {
            let length =
                u32::from_be_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]]);
            match &bytes[at + 4..at + 8] {
                b"acTL" => {
                    let data = bytes.get(at + 8..at + 16)?;
                    found = Some(u32::from_be_bytes([data[4], data[5], data[6], data[7]]));
                    break;
                }
                b"IDAT" | b"IEND" => break,
                _ => {
                    at = (length as usize)
                        .checked_add(12)
                        .and_then(|step| at.checked_add(step))?;
                }
            }
        }
        found?
    } else {
        return None;
    };
    (count != 0).then_some(count)
}

/// What a tile's frames go through after scaling and before they are swapped for the renderer: the status ring and
/// decoration of a member's avatar are composed into every frame this way.
pub(super) type Post = Arc<dyn Fn(RgbaImage) -> RgbaImage + Send + Sync>;

enum Decoded {
    Frame(Duration, RgbaImage),
    End,
}

/// A playing tile: the decoder thread's channel and when the frame shown ends.
struct Stream {
    frames: Receiver<Decoded>,
    due: Option<Instant>,
    /// Frames shown since the stream started.
    shown: u64,
}

struct Tile {
    post: Option<Post>,
    image: Arc<gpui::Image>,
    /// The drawn box in physical pixels.
    size: (u32, u32),
    cover: bool,
    generation: u64,
    epoch: u64,
    /// Whether the code that draws it wants it to play (the setting, the pointer).
    wanted: bool,
    decoding: bool,
    failed: bool,
    poster: Option<Arc<RenderImage>>,
    stream: Option<Stream>,
    /// The animation played as many times as the file says: the last frame stays until the tile stops playing.
    ended: bool,
    /// The frame drawn now, when it is not the poster.
    frame: Option<Arc<RenderImage>>,
    /// Kept for the harness's report.
    clock: Playback,
}

impl Tile {
    fn frame_bytes(&self) -> usize {
        self.size.0.max(1) as usize * self.size.1.max(1) as usize * 4
    }

    /// The memory this tile holds or may hold: its poster, and while playing the frame shown and those ahead.
    fn bytes(&self) -> usize {
        let poster = usize::from(self.poster.is_some() || self.decoding);
        let playing = if self.stream.is_some() { AHEAD + 2 } else { 0 };
        (poster + playing) * self.frame_bytes()
    }
}

#[derive(Default)]
pub(super) struct Cache {
    entries: HashMap<String, Tile>,
    seen: HashMap<String, bool>,
    visible: HashMap<String, bool>,
    epoch: u64,
    deadline: Option<Instant>,
    generation: u64,
    /// Frames let go of between two renders, released by the next `prepare`.
    retired: Vec<Arc<RenderImage>>,
}

type DecodeJob = (
    String,
    Arc<gpui::Image>,
    (u32, u32),
    bool,
    Option<Post>,
    u64,
);

#[derive(Default)]
struct Work {
    posters: Vec<DecodeJob>,
    retired: Vec<Arc<RenderImage>>,
    deadline: Option<Instant>,
}

fn render_image(frame: RgbaImage) -> Arc<RenderImage> {
    Arc::new(RenderImage::new(vec![image::Frame::new(frame)]))
}

impl Cache {
    /// Tiles registered, for the harness.
    #[cfg(feature = "capture")]
    pub(super) fn len(&self) -> String {
        self.entries
            .iter()
            .map(|(key, tile)| {
                format!(
                    "{}:{}x{} wanted={} decoding={} failed={} poster={} playing={} shown={} ended={}",
                    key.chars()
                        .rev()
                        .take(12)
                        .collect::<String>()
                        .chars()
                        .rev()
                        .collect::<String>(),
                    tile.size.0,
                    tile.size.1,
                    tile.wanted,
                    tile.decoding,
                    tile.failed,
                    tile.poster.is_some(),
                    tile.stream.is_some(),
                    tile.stream.as_ref().map_or(0, |stream| stream.shown),
                    tile.ended,
                )
            })
            .collect::<Vec<_>>()
            .join("; ")
            + &format!(
                " decoded={} paints={} errors={} at={:#x}",
                self.decoded_used(),
                PAINTS.load(std::sync::atomic::Ordering::Relaxed),
                PAINT_ERRORS.load(std::sync::atomic::Ordering::Relaxed),
                LAST_BOUNDS.load(std::sync::atomic::Ordering::Relaxed)
            )
    }

    fn decoded_used(&self) -> usize {
        self.entries.values().map(Tile::bytes).sum()
    }

    fn register(
        &mut self,
        key: &str,
        image: &Arc<gpui::Image>,
        size: (u32, u32),
        cover: bool,
        post: Option<Post>,
        wanted: bool,
    ) {
        let epoch = self.epoch;
        let tile = self.entries.entry(key.to_owned()).or_insert_with(|| Tile {
            post: post.clone(),
            image: image.clone(),
            size,
            cover,
            generation: 0,
            epoch,
            wanted,
            decoding: false,
            failed: false,
            poster: None,
            stream: None,
            ended: false,
            frame: None,
            clock: Playback::default(),
        });
        tile.epoch = epoch;
        tile.wanted = wanted;
        tile.post = post;
        if tile.size != size || tile.cover != cover || !Arc::ptr_eq(&tile.image, image) {
            // Drawn at another size, fit, or picture bytes: decoded again cleanly.
            tile.size = size;
            tile.cover = cover;
            tile.image = image.clone();
            tile.generation = tile.generation.wrapping_add(1);
            Self::stop(tile, &mut self.retired);
            self.retired.extend(tile.poster.take());
            tile.decoding = false;
            tile.failed = false;
        }
    }

    /// Ends a tile's playing: the decoder thread stops at its next frame (its channel is gone) and the poster shows.
    fn stop(tile: &mut Tile, retired: &mut Vec<Arc<RenderImage>>) {
        tile.stream = None;
        tile.ended = false;
        if let Some(frame) = tile.frame.take() {
            retired.push(frame);
        }
    }

    /// Once per render: retires what is no longer drawn, starts poster decodes and decoder threads, moves every
    /// playing tile on to the frame due now, and says when the next one is due.
    fn prepare(&mut self, focused: bool, now: Instant, spawn: &mut dyn FnMut(StreamJob)) -> Work {
        let mut work = Work {
            retired: std::mem::take(&mut self.retired),
            ..Work::default()
        };
        self.epoch += 1;
        self.visible = std::mem::take(&mut self.seen);
        let epoch = self.epoch;
        self.entries.retain(|_, tile| {
            if tile.epoch + 1 >= epoch {
                true
            } else {
                Self::stop(tile, &mut work.retired);
                work.retired.extend(tile.poster.take());
                false
            }
        });
        let mut decoding = self.entries.values().filter(|tile| tile.decoding).count();
        let mut used = self.decoded_used();
        let mut keys: Vec<String> = self.entries.keys().cloned().collect();
        // Older keys first is as good as any order; sorting keeps the budget's choice stable from frame to frame.
        keys.sort();
        for key in keys {
            let visible = self.visible.get(&key) == Some(&true);
            let tile = self.entries.get_mut(&key).expect("listed");
            if !visible {
                // Off screen: nothing stays decoded.
                used -= tile.bytes();
                Self::stop(tile, &mut work.retired);
                work.retired.extend(tile.poster.take());
                if tile.decoding {
                    tile.generation = tile.generation.wrapping_add(1);
                    tile.decoding = false;
                }
                tile.clock = Playback::default();
                continue;
            }
            if tile.failed {
                continue;
            }
            if tile.poster.is_none() && !tile.decoding {
                if decoding < DECODING && used + tile.frame_bytes() <= TOTAL_DECODED {
                    tile.decoding = true;
                    decoding += 1;
                    used += tile.frame_bytes();
                    work.posters.push((
                        key.clone(),
                        tile.image.clone(),
                        tile.size,
                        tile.cover,
                        tile.post.clone(),
                        tile.generation,
                    ));
                }
                continue;
            }
            if tile.poster.is_none() {
                continue;
            }
            let play = should_play(true, focused, tile.wanted);
            if !play {
                if tile.stream.is_some() || tile.ended {
                    used -= tile.bytes();
                    Self::stop(tile, &mut work.retired);
                    used += tile.bytes();
                }
                tile.clock.playing = false;
                continue;
            }
            if tile.stream.is_none() && !tile.ended {
                let more = (AHEAD + 2) * tile.frame_bytes();
                if used + more > TOTAL_DECODED {
                    // No room: the poster stays until a tile leaves the screen.
                    continue;
                }
                let (sender, frames) = sync_channel(AHEAD);
                spawn(StreamJob {
                    image: tile.image.clone(),
                    size: tile.size,
                    cover: tile.cover,
                    post: tile.post.clone(),
                    sender,
                });
                tile.stream = Some(Stream {
                    frames,
                    due: None,
                    shown: 0,
                });
                used += more;
            }
            tile.clock.playing = true;
            let Some(stream) = tile.stream.as_mut() else {
                continue;
            };
            let mut finished = false;
            loop {
                if stream.due.is_some_and(|due| now < due) {
                    break;
                }
                match stream.frames.try_recv() {
                    Ok(Decoded::Frame(delay, frame)) => {
                        let starts = match stream.due {
                            Some(due) if now.saturating_duration_since(due) <= CATCH_UP => due,
                            _ => now,
                        };
                        stream.due = Some(starts + delay);
                        stream.shown += 1;
                        if let Some(old) = tile.frame.replace(render_image(frame)) {
                            work.retired.push(old);
                        }
                    }
                    Ok(Decoded::End) | Err(TryRecvError::Disconnected) => {
                        finished = true;
                        break;
                    }
                    // The decoder is behind: the frame stays, and the next render looks again.
                    Err(TryRecvError::Empty) => {
                        stream.due = stream.due.map(|due| due.max(now));
                        break;
                    }
                }
            }
            if finished {
                tile.stream = None;
                tile.ended = true;
                used -= (AHEAD + 2) * tile.frame_bytes();
                continue;
            }
            let wake = stream
                .due
                .unwrap_or(now)
                .max(now + Duration::from_millis(1));
            work.deadline = Some(work.deadline.map_or(wake, |old| old.min(wake)));
        }
        work.deadline = work.deadline.map(|wake| on_grid(wake, now));
        work
    }

    fn poster_decoded(&mut self, key: &str, generation: u64, result: Option<RgbaImage>) {
        let Some(tile) = self.entries.get_mut(key) else {
            return;
        };
        if tile.generation != generation {
            return;
        }
        tile.decoding = false;
        match result {
            Some(frame) => self
                .retired
                .extend(tile.poster.replace(render_image(frame))),
            None => tile.failed = true,
        }
    }

    /// The picture to draw now.
    fn frame(&self, key: &str) -> Option<Arc<RenderImage>> {
        let tile = self.entries.get(key)?;
        tile.frame.clone().or_else(|| tile.poster.clone())
    }
}

/// `wake` moved up to the next slot of `GRID` counted from `now`.
fn on_grid(wake: Instant, now: Instant) -> Instant {
    let ahead = wake.saturating_duration_since(now).as_nanos();
    let slots = ahead.div_ceil(GRID.as_nanos()).max(1);
    now + Duration::from_nanos((slots * GRID.as_nanos()) as u64)
}

/// A decoder thread to start.
pub(super) struct StreamJob {
    image: Arc<gpui::Image>,
    size: (u32, u32),
    cover: bool,
    post: Option<Post>,
    sender: SyncSender<Decoded>,
}

type Frames = Box<dyn Iterator<Item = image::ImageResult<image::Frame>>>;

fn frames_of(bytes: &[u8]) -> Option<Frames> {
    if bytes.starts_with(b"GIF8") {
        let decoder = image::codecs::gif::GifDecoder::new(Cursor::new(bytes.to_vec())).ok()?;
        Some(Box::new(decoder.into_frames()))
    } else if bytes.starts_with(b"RIFF") {
        let mut decoder =
            image::codecs::webp::WebPDecoder::new(Cursor::new(bytes.to_vec())).ok()?;
        if !decoder.has_animation() {
            return None;
        }
        let _ = decoder.set_background_color(image::Rgba([0, 0, 0, 0]));
        Some(Box::new(decoder.into_frames()))
    } else {
        let decoder = image::codecs::png::PngDecoder::new(Cursor::new(bytes.to_vec())).ok()?;
        if !decoder.is_apng().ok()? {
            return None;
        }
        Some(Box::new(decoder.apng().ok()?.into_frames()))
    }
}

/// A decoded frame made ready for the renderer: its delay, scaled (and cropped when `cover` is set) to `size`,
/// `post` applied, blue and red swapped.
fn prepared(
    frame: image::Frame,
    size: (u32, u32),
    cover_fit: bool,
    post: Option<&Post>,
) -> (Duration, RgbaImage) {
    let (numerator, denominator) = frame.delay().numer_denom_ms();
    let asked = Duration::from_secs_f64(numerator as f64 / denominator.max(1) as f64 / 1000.);
    let raw = frame.into_buffer();
    let mut buffer = if cover_fit {
        cover(raw, size)
    } else {
        fit(raw, size)
    };
    if let Some(post) = post {
        buffer = post(buffer);
    }
    for pixel in buffer.pixels_mut() {
        pixel.0.swap(0, 2);
    }
    (browser_delay(asked), buffer)
}

/// The first frame of an animation, ready for the renderer.
fn poster(
    bytes: &[u8],
    size: (u32, u32),
    cover_fit: bool,
    post: Option<&Post>,
) -> Option<RgbaImage> {
    let frame = frames_of(bytes)?.next()?.ok()?;
    Some(prepared(frame, size, cover_fit, post).1)
}

/// The decoder thread: frames in order, as many rounds as the file plays, blocking while `AHEAD` wait unshown. It
/// ends when the tile drops its end of the channel.
fn stream(job: StreamJob) {
    let bytes = job.image.bytes();
    let rounds = plays(bytes);
    let mut round = 0u32;
    loop {
        let Some(frames) = frames_of(bytes) else {
            let _ = job.sender.send(Decoded::End);
            return;
        };
        let mut any = false;
        for frame in frames {
            let Ok(frame) = frame else {
                break;
            };
            any = true;
            let (delay, buffer) = prepared(frame, job.size, job.cover, job.post.as_ref());
            if job.sender.send(Decoded::Frame(delay, buffer)).is_err() {
                return;
            }
        }
        round += 1;
        if !any || rounds.is_some_and(|rounds| round >= rounds) {
            let _ = job.sender.send(Decoded::End);
            return;
        }
    }
}

/// `image` scaled (never up) to fill `size` without cropping.
fn fit(image: RgbaImage, size: (u32, u32)) -> RgbaImage {
    let (width, height) = image.dimensions();
    let (box_width, box_height) = (size.0.max(1) as f32, size.1.max(1) as f32);
    let scale = (box_width / width as f32)
        .min(box_height / height as f32)
        .min(1.);
    if scale < 1. {
        image::imageops::resize(
            &image,
            ((width as f32 * scale).round() as u32).max(1),
            ((height as f32 * scale).round() as u32).max(1),
            FilterType::Triangle,
        )
    } else {
        image
    }
}

/// `image` scaled (never up) so it covers `size` and cropped to the proportions of `size`.
fn cover(image: RgbaImage, size: (u32, u32)) -> RgbaImage {
    let (width, height) = image.dimensions();
    let (box_width, box_height) = (size.0.max(1) as f32, size.1.max(1) as f32);
    let scale = (box_width / width as f32)
        .max(box_height / height as f32)
        .min(1.);
    let scaled = if scale < 1. {
        image::imageops::resize(
            &image,
            ((width as f32 * scale).round() as u32).max(1),
            ((height as f32 * scale).round() as u32).max(1),
            FilterType::Triangle,
        )
    } else {
        image
    };
    let (width, height) = scaled.dimensions();
    let crop_width = width.min(((height as f32 * box_width / box_height).round() as u32).max(1));
    let crop_height = height.min(((width as f32 * box_height / box_width).round() as u32).max(1));
    if (crop_width, crop_height) == (width, height) {
        return scaled;
    }
    image::imageops::crop_imm(
        &scaled,
        (width - crop_width) / 2,
        (height - crop_height) / 2,
        crop_width,
        crop_height,
    )
    .to_image()
}

/// How a tile is drawn: `pixels` is the box its frames are decoded to, `radius` its corners, `post` what is done to
/// each frame after scaling, `wanted` whether it should play now (it also needs to be on screen in a focused window).
pub(super) struct TileLook {
    pub pixels: (u32, u32),
    pub radius: f32,
    pub cover: bool,
    pub post: Option<Post>,
    pub wanted: bool,
}

impl super::Root {
    /// Starts the decodes the tiles drawn last frame call for and releases what left the screen. Called once per
    /// render, before the tiles are built.
    pub(super) fn prepare_gif_tiles(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let mut jobs = Vec::new();
        // The client's focus, not this window's: a popout of the client that has the focus keeps tiles playing
        // (and the harness's `dmcall fake-unfocused` draws the client as unfocused).
        let focused = !super::voice_video::client_unfocused(cx);
        let work = self
            .gif_tiles
            .borrow_mut()
            .prepare(focused, Instant::now(), &mut |job| jobs.push(job));
        for job in jobs {
            // A plain thread: the decoder blocks while its frames wait to be shown, which would hold a worker of
            // the shared pool.
            let _ = std::thread::Builder::new()
                .name("gif-stream".into())
                .spawn(move || stream(job));
        }
        for image in work.retired {
            cx.drop_image(image, Some(window));
        }
        for (key, image, size, cover_fit, post, generation) in work.posters {
            cx.spawn(async move |this, cx| {
                let decoded = cx
                    .background_executor()
                    .spawn(async move { poster(image.bytes(), size, cover_fit, post.as_ref()) })
                    .await;
                let _ = this.update(cx, |this, cx| {
                    this.gif_tiles
                        .borrow_mut()
                        .poster_decoded(&key, generation, decoded);
                    super::regions::wake(cx);
                });
            })
            .detach();
        }
        let scheduled = {
            let mut cache = self.gif_tiles.borrow_mut();
            match work.deadline {
                Some(deadline) if cache.deadline.is_none_or(|old| deadline < old) => {
                    cache.deadline = Some(deadline);
                    cache.generation += 1;
                    Some((cache.generation, deadline))
                }
                None if cache.deadline.is_some() => {
                    cache.deadline = None;
                    cache.generation += 1;
                    None
                }
                _ => None,
            }
        };
        if let Some((generation, deadline)) = scheduled {
            cx.spawn(async move |this, cx| {
                cx.background_executor()
                    .timer(deadline.saturating_duration_since(Instant::now()))
                    .await;
                let _ = this.update(cx, |this, cx| {
                    let mut cache = this.gif_tiles.borrow_mut();
                    if cache.generation == generation {
                        cache.deadline = None;
                        drop(cache);
                        super::regions::wake(cx);
                    }
                });
            })
            .detach();
        }
    }

    /// The picture of a GIF tile `width` x `height` logical pixels: an animation plays (see the module comment), a
    /// still picture is drawn as it is. Nothing is drawn until the first frame is ready, so the tile's own colour
    /// shows.
    pub(super) fn gif_art(
        &self,
        key: &str,
        image: Arc<gpui::Image>,
        size: (f32, f32),
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        self.gif_art_rounded(key, image, size, 5., window, cx)
    }

    /// `gif_art` with the corner radius of the box it fills (an avatar's is half its side).
    pub(super) fn gif_art_rounded(
        &self,
        key: &str,
        image: Arc<gpui::Image>,
        size: (f32, f32),
        radius: f32,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let scale = window.scale_factor().max(1.);
        let pixels = (
            (size.0 * scale).round() as u32,
            (size.1 * scale).round() as u32,
        );
        self.gif_art_pixels(key, image, pixels, radius, None, cx)
    }

    /// A tile that always wants to play: `pixels` is the box the frames are decoded to, `post` what is done to each
    /// frame after that.
    pub(super) fn gif_art_pixels(
        &self,
        key: &str,
        image: Arc<gpui::Image>,
        pixels: (u32, u32),
        radius: f32,
        post: Option<Post>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        self.media_tile(
            key,
            image,
            TileLook {
                pixels,
                radius,
                cover: true,
                post,
                wanted: true,
            },
            cx,
        )
    }

    /// The tile itself.
    pub(super) fn media_tile(
        &self,
        key: &str,
        image: Arc<gpui::Image>,
        look: TileLook,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let TileLook {
            pixels,
            radius,
            cover: cover_fit,
            post,
            wanted,
        } = look;
        if !animated(image.bytes()) {
            return gpui::img(image)
                .id(gpui::SharedString::from(format!("{key}:still")))
                .absolute()
                .size_full()
                .rounded(px(radius))
                .when(cover_fit, |this| this.object_fit(gpui::ObjectFit::Cover))
                .into_any_element();
        }
        self.gif_tiles
            .borrow_mut()
            .register(key, &image, pixels, cover_fit, post, wanted);
        // The next frame redraws the view this tile is drawn in.
        super::regions::note_drawn();
        let cache = self.gif_tiles.clone();
        let key = key.to_owned();
        let root = cx.entity().downgrade();
        div()
            .absolute()
            .top(px(0.))
            .left(px(0.))
            .size_full()
            .child(
                canvas(
                    |bounds, _, _| bounds,
                    move |bounds, _, window, cx| {
                        let area = f32::from(bounds.size.width) * f32::from(bounds.size.height);
                        let shown = bounds.intersect(&window.content_mask().bounds);
                        let on_screen = area > 0.
                            && f32::from(shown.size.width) > 0.
                            && f32::from(shown.size.height) > 0.;
                        let changed = {
                            let mut cache = cache.borrow_mut();
                            if on_screen {
                                cache.seen.insert(key.clone(), true);
                                let epoch = cache.epoch;
                                if let Some(tile) = cache.entries.get_mut(&key) {
                                    tile.epoch = epoch;
                                }
                            }
                            cache.visible.get(&key).copied().unwrap_or(false) != on_screen
                        };
                        if changed {
                            let root = root.clone();
                            cx.defer(move |cx| {
                                let _ = root.update(cx, |_, cx| cx.notify());
                            });
                        }
                        if !on_screen {
                            return;
                        }
                        let frame = cache.borrow().frame(&key);
                        if let Some(image) = frame {
                            let painted = window.paint_image(
                                bounds,
                                bounds,
                                Corners::all(px(radius)),
                                image,
                                0,
                                false,
                            );
                            #[cfg(feature = "capture")]
                            {
                                use std::sync::atomic::Ordering::Relaxed;
                                PAINTS.fetch_add(1, Relaxed);
                                if painted.is_err() {
                                    PAINT_ERRORS.fetch_add(1, Relaxed);
                                }
                                LAST_BOUNDS.store(
                                    (f32::from(bounds.origin.x) as u32) << 16
                                        | f32::from(bounds.origin.y) as u32,
                                    Relaxed,
                                );
                            }
                            #[cfg(not(feature = "capture"))]
                            let _ = painted;
                        }
                    },
                )
                .size_full(),
            )
            .into_any_element()
    }

    pub(super) fn release_gif_tiles(&mut self, cx: &mut gpui::App) {
        let images: Vec<_> = {
            let mut cache = self.gif_tiles.borrow_mut();
            cache.seen.clear();
            cache.visible.clear();
            let mut images = std::mem::take(&mut cache.retired);
            images.extend(
                cache
                    .entries
                    .drain()
                    .flat_map(|(_, tile)| tile.frame.into_iter().chain(tile.poster)),
            );
            images
        };
        cx.defer(move |cx| {
            for image in images {
                cx.drop_image(image, None);
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A GIF of `frames` 2 x 2 frames of `delay_cs` hundredths each, with a `NETSCAPE2.0` block of `repeats` when given.
    fn gif(frames: usize, delay_cs: u16, repeats: Option<u16>) -> Vec<u8> {
        let mut bytes = Vec::new();
        {
            let mut encoder = image::codecs::gif::GifEncoder::new(&mut bytes);
            if let Some(repeats) = repeats {
                encoder
                    .set_repeat(if repeats == 0 {
                        image::codecs::gif::Repeat::Infinite
                    } else {
                        image::codecs::gif::Repeat::Finite(repeats)
                    })
                    .unwrap();
            }
            let frames = (0..frames).map(|index| {
                image::Frame::from_parts(
                    RgbaImage::from_pixel(2, 2, image::Rgba([(index % 256) as u8, 0, 0, 255])),
                    0,
                    0,
                    image::Delay::from_numer_denom_ms(u32::from(delay_cs) * 10, 1),
                )
            });
            encoder.encode_frames(frames).unwrap();
        }
        bytes
    }

    #[test]
    fn a_tile_plays_only_on_screen_in_a_focused_window_when_wanted() {
        for visible in [false, true] {
            for focused in [false, true] {
                for wanted in [false, true] {
                    assert_eq!(
                        should_play(visible, focused, wanted),
                        visible && focused && wanted
                    );
                }
            }
        }
    }

    #[test]
    fn only_a_file_with_frames_is_an_animation() {
        let mut webp = b"RIFF\0\0\0\0WEBPVP8X\n\0\0\0".to_vec();
        webp.extend([0x02, 0, 0, 0]);
        assert!(animated(&webp));
        webp[20] = 0;
        assert!(!animated(&webp));
        assert!(animated(b"GIF89a....................."));
        assert!(!animated(b"\x89PNG\r\n\x1a\n.............."));
        let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
        png.extend([0, 0, 0, 13]);
        png.extend(b"IHDR");
        png.extend([0; 17]);
        png.extend([0, 0, 0, 8]);
        png.extend(b"acTL");
        assert!(animated(&png));
    }

    #[test]
    fn very_short_delays_last_a_tenth_of_a_second_as_in_the_browser() {
        assert_eq!(browser_delay(Duration::ZERO), Duration::from_millis(100));
        assert_eq!(
            browser_delay(Duration::from_millis(10)),
            Duration::from_millis(100)
        );
        assert_eq!(
            browser_delay(Duration::from_millis(20)),
            Duration::from_millis(20)
        );
    }

    #[test]
    fn the_file_says_how_many_times_it_plays() {
        assert_eq!(plays(&gif(2, 5, Some(0))), None);
        assert_eq!(plays(&gif(2, 5, Some(2))), Some(3));
        assert_eq!(plays(&gif(2, 5, None)), Some(1));
    }

    #[test]
    fn a_tile_is_scaled_to_cover_and_cropped_to_its_box() {
        // A 400 x 200 picture in a 100 x 100 box: scaled to 200 x 100 would cover it, then cropped square.
        let picture = RgbaImage::new(400, 200);
        let out = cover(picture, (100, 100));
        assert_eq!(out.dimensions(), (100, 100));
        // A picture smaller than its box is not enlarged, only cropped to the box's proportions.
        let out = cover(RgbaImage::new(60, 30), (100, 100));
        assert_eq!(out.dimensions(), (30, 30));
    }

    fn seen(cache: &mut Cache, key: &str, image: &Arc<gpui::Image>, wanted: bool) {
        cache.register(key, image, (2, 2), true, None, wanted);
        cache.seen.insert(key.to_owned(), true);
    }

    /// Runs `prepare`, starting the decoder threads it asks for.
    fn step(cache: &mut Cache, focused: bool, now: Instant) -> Work {
        cache.prepare(focused, now, &mut |job| {
            std::thread::spawn(move || stream(job));
        })
    }

    #[test]
    fn a_long_animation_streams_with_bounded_frames_and_loops_for_ever() {
        let image = Arc::new(gpui::Image::from_bytes(
            gpui::ImageFormat::Gif,
            gif(300, 4, Some(0)),
        ));
        let mut cache = Cache::default();
        // Not seen yet: nothing to decode.
        cache.register("a", &image, (2, 2), true, None, true);
        assert!(step(&mut cache, true, Instant::now()).posters.is_empty());
        seen(&mut cache, "a", &image, true);
        let work = step(&mut cache, true, Instant::now());
        assert_eq!(work.posters.len(), 1);
        cache.poster_decoded(
            "a",
            work.posters[0].5,
            poster(image.bytes(), (2, 2), true, None),
        );
        // Plays: frames arrive in order, 40 ms apart, past the end of the file and round again.
        let start = Instant::now();
        let mut shown = 0;
        for tick in 0..2000u64 {
            seen(&mut cache, "a", &image, true);
            step(&mut cache, true, start + Duration::from_millis(tick * 10));
            // At most the shown frame, the poster and `AHEAD` more are ever decoded.
            assert!(cache.decoded_used() <= (AHEAD + 3) * 16);
            shown = cache.entries["a"].stream.as_ref().map_or(0, |s| s.shown);
            if shown > 320 {
                break;
            }
            std::thread::sleep(Duration::from_micros(200));
        }
        assert!(shown > 300, "the animation stopped at frame {shown}");
        assert!(!cache.entries["a"].ended);
    }

    #[test]
    fn a_finite_animation_ends_on_its_last_frame_and_a_pause_shows_the_poster() {
        let image = Arc::new(gpui::Image::from_bytes(
            gpui::ImageFormat::Gif,
            gif(3, 2, None),
        ));
        let mut cache = Cache::default();
        seen(&mut cache, "a", &image, true);
        let work = step(&mut cache, true, Instant::now());
        cache.poster_decoded(
            "a",
            work.posters[0].5,
            poster(image.bytes(), (2, 2), true, None),
        );
        let start = Instant::now();
        for tick in 0..400u64 {
            seen(&mut cache, "a", &image, true);
            step(&mut cache, true, start + Duration::from_millis(tick * 10));
            if cache.entries["a"].ended {
                break;
            }
            std::thread::sleep(Duration::from_micros(500));
        }
        assert!(
            cache.entries["a"].ended,
            "a GIF without a loop block plays once"
        );
        assert!(cache.entries["a"].frame.is_some());
        // Not wanted any more (the pointer left, the setting is off): back to the poster, and it may play again.
        seen(&mut cache, "a", &image, false);
        step(&mut cache, true, start + Duration::from_secs(10));
        let tile = &cache.entries["a"];
        assert!(tile.frame.is_none() && tile.stream.is_none() && !tile.ended);
        // An unfocused window does not play either.
        seen(&mut cache, "a", &image, true);
        step(&mut cache, false, start + Duration::from_secs(11));
        assert!(cache.entries["a"].stream.is_none());
        // Off screen: nothing stays decoded.
        cache.register("a", &image, (2, 2), true, None, true);
        step(&mut cache, true, start + Duration::from_secs(12));
        assert_eq!(cache.decoded_used(), 0);
    }

    #[test]
    fn wakes_fall_on_the_sixty_hertz_grid() {
        let now = Instant::now();
        assert_eq!(on_grid(now + Duration::from_millis(5), now), now + GRID);
        assert_eq!(
            on_grid(now + Duration::from_millis(20), now),
            now + GRID * 2
        );
    }
}
