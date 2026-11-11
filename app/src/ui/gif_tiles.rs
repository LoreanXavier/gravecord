//! The GIF picker's animated tiles.
//!
//! The official picker (module 884037) shows a result as a looping, muted, autoplaying `<video>` and a category as an
//! `<img>` asked for with `animated=true`; both start as soon as the tile is mounted, the tiles outside the scrolled
//! area are not mounted at all, and nothing there reads `gif_auto_play`. The pictures this client is given are
//! animated WebP (`gif_src`, and the category art), up to 500 pixels wide and 280 frames long: the renderer would
//! decode every frame at full size (a screenful held three gigabytes), so a tile decodes its own copy instead: only
//! while it is on screen, at the size it is drawn, cropped to its box, with at most `TILE_DECODED` bytes of frames.
use super::nameplate_animation::{Clip, Playback};
use gpui::{
    AnyElement, Context, Corners, IntoElement, RenderImage, Window, canvas, div, prelude::*, px,
};
use image::{AnimationDecoder, RgbaImage, imageops::FilterType};
use std::{
    collections::HashMap,
    io::Cursor,
    sync::Arc,
    time::{Duration, Instant},
};

#[cfg(feature = "capture")]
static PAINTS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
#[cfg(feature = "capture")]
static PAINT_ERRORS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
#[cfg(feature = "capture")]
static LAST_BOUNDS: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

/// Frames of every tile on screen together.
const TOTAL_DECODED: usize = 64 * 1024 * 1024;
/// Frames of one tile. A longer animation keeps every other (every third, ...) frame and the delays add up.
const TILE_DECODED: usize = 6 * 1024 * 1024;
/// Tiles decoding at once.
const DECODING: usize = 2;
/// The shortest frame shown: the files carry some 10 ms frames.
const SHORTEST_FRAME: Duration = Duration::from_millis(20);
const MAX_FRAMES: usize = 600;
/// Redraws happen on this grid (about 30 a second): tiles with their own frame times would otherwise ask for a redraw
/// of the whole window every few milliseconds.
const GRID: Duration = Duration::from_millis(33);

/// A tile plays while it is on screen and the window is focused. The setting `gif_auto_play` does not apply here (the
/// picker's own component never reads it) and neither does the pointer.
pub(super) fn should_play(visible: bool, focused: bool) -> bool {
    visible && focused
}

/// Whether `bytes` is a picture with more than one frame: an animated WebP (`VP8X` with its animation flag) or a GIF.
pub(super) fn animated(bytes: &[u8]) -> bool {
    (bytes.len() > 21
        && bytes.starts_with(b"RIFF")
        && &bytes[8..12] == b"WEBP"
        && &bytes[12..16] == b"VP8X"
        && bytes[20] & 0x02 != 0)
        || bytes.starts_with(b"GIF8")
}

/// What a tile's frames go through after scaling and before they are swapped for the renderer: the status ring and
/// decoration of a member's avatar are composed into every frame this way.
pub(super) type Post = Arc<dyn Fn(RgbaImage) -> RgbaImage + Send + Sync>;

struct Tile {
    post: Option<Post>,
    image: Arc<gpui::Image>,
    /// The drawn box in physical pixels.
    size: (u32, u32),
    epoch: u64,
    decoding: bool,
    failed: bool,
    clip: Option<Arc<Clip>>,
    bytes: usize,
    clock: Playback,
    frame: Option<(usize, Arc<RenderImage>)>,
}

#[derive(Default)]
pub(super) struct Cache {
    entries: HashMap<String, Tile>,
    seen: HashMap<String, bool>,
    visible: HashMap<String, bool>,
    epoch: u64,
    start: Option<Instant>,
    deadline: Option<Instant>,
    generation: u64,
}

type DecodeJob = (String, Arc<gpui::Image>, (u32, u32), Option<Post>);

#[derive(Default)]
struct Work {
    decode: Vec<DecodeJob>,
    retired: Vec<Arc<RenderImage>>,
    deadline: Option<Instant>,
}

impl Cache {
    /// Tiles registered, for the harness.
    #[cfg(feature = "capture")]
    pub(super) fn len(&self) -> String {
        self.entries
            .iter()
            .map(|(key, tile)| {
                format!(
                    "{}:{}x{} decoding={} failed={} frames={} playing={} shown={:?} elapsed_ms={}",
                    key.chars()
                        .rev()
                        .take(12)
                        .collect::<String>()
                        .chars()
                        .rev()
                        .collect::<String>(),
                    tile.size.0,
                    tile.size.1,
                    tile.decoding,
                    tile.failed,
                    tile.clip.as_ref().map_or(0, |clip| clip.frames.len()),
                    tile.clock.playing,
                    tile.frame.as_ref().map(|(index, _)| *index),
                    tile.clock.elapsed.as_millis()
                )
            })
            .collect::<Vec<_>>()
            .join("; ")
            + &format!(
                " paints={} errors={} at={:#x}",
                PAINTS.load(std::sync::atomic::Ordering::Relaxed),
                PAINT_ERRORS.load(std::sync::atomic::Ordering::Relaxed),
                LAST_BOUNDS.load(std::sync::atomic::Ordering::Relaxed)
            )
    }

    fn decoded_used(&self) -> usize {
        self.entries.values().map(|tile| tile.bytes).sum()
    }

    fn register(
        &mut self,
        key: &str,
        image: &Arc<gpui::Image>,
        size: (u32, u32),
        post: Option<Post>,
    ) {
        let epoch = self.epoch;
        let tile = self.entries.entry(key.to_owned()).or_insert_with(|| Tile {
            post,
            image: image.clone(),
            size,
            epoch,
            decoding: false,
            failed: false,
            clip: None,
            bytes: 0,
            clock: Playback::default(),
            frame: None,
        });
        tile.epoch = epoch;
    }

    fn prepare(&mut self, focused: bool) -> Work {
        let mut work = Work::default();
        self.epoch += 1;
        self.visible = std::mem::take(&mut self.seen);
        let epoch = self.epoch;
        self.entries.retain(|_, tile| {
            if tile.epoch + 1 >= epoch {
                true
            } else {
                if let Some((_, image)) = tile.frame.take() {
                    work.retired.push(image);
                }
                false
            }
        });
        let now = self.start.get_or_insert_with(Instant::now).elapsed();
        let mut decoding = self.entries.values().filter(|tile| tile.decoding).count();
        let mut used = self.decoded_used();
        for (key, tile) in &mut self.entries {
            let visible = self.visible.get(key) == Some(&true);
            if let Some(clip) = &tile.clip {
                tile.clock
                    .tick(now, should_play(visible, focused), true, clip.duration);
                if tile.clock.playing {
                    let due = now + clip.next_delay(tile.clock.elapsed);
                    let slots = due.as_nanos().div_ceil(GRID.as_nanos());
                    let on_grid = Duration::from_nanos((slots * GRID.as_nanos()) as u64);
                    let wake =
                        Instant::now() + on_grid.saturating_sub(now).max(Duration::from_millis(1));
                    work.deadline = Some(work.deadline.map_or(wake, |old| old.min(wake)));
                }
            }
            if !visible {
                // Off screen: nothing stays decoded.
                used -= tile.bytes;
                tile.clip = None;
                tile.bytes = 0;
                tile.clock = Playback::default();
                if let Some((_, image)) = tile.frame.take() {
                    work.retired.push(image);
                }
            } else if tile.clip.is_none()
                && !tile.decoding
                && !tile.failed
                && decoding < DECODING
                && used + TILE_DECODED <= TOTAL_DECODED
            {
                tile.decoding = true;
                decoding += 1;
                used += TILE_DECODED;
                work.decode.push((
                    key.clone(),
                    tile.image.clone(),
                    tile.size,
                    tile.post.clone(),
                ));
            }
        }
        work
    }

    fn decoded(&mut self, key: &str, result: Option<Vec<(Duration, RgbaImage)>>) {
        let Some(tile) = self.entries.get_mut(key) else {
            return;
        };
        tile.decoding = false;
        let bytes = result.as_ref().map_or(0, |frames| {
            frames
                .iter()
                .map(|(_, frame)| frame.as_raw().len())
                .sum::<usize>()
        });
        match result.and_then(Clip::from_frames) {
            Some(clip) => {
                tile.clip = Some(clip);
                tile.bytes = bytes;
            }
            None => tile.failed = true,
        }
    }

    /// The frame to draw now and the one it replaces (to release).
    fn frame(&mut self, key: &str) -> Option<(Arc<RenderImage>, Option<Arc<RenderImage>>)> {
        let tile = self.entries.get_mut(key)?;
        let clip = tile.clip.as_ref()?;
        let index = clip.frame_at(tile.clock.elapsed);
        if let Some((known, image)) = &tile.frame
            && *known == index
        {
            return Some((image.clone(), None));
        }
        let image = Arc::new(RenderImage::new(vec![image::Frame::new(
            clip.frames[index].as_ref().clone(),
        )]));
        let retired = tile
            .frame
            .replace((index, image.clone()))
            .map(|(_, old)| old);
        Some((image, retired))
    }
}

type Frames = Box<dyn Iterator<Item = image::ImageResult<image::Frame>>>;

/// The frames of `bytes` scaled to cover `size` and cropped to its proportions, blue and red swapped for the renderer,
/// or `None` when the file is not an animation or cannot be read.
fn decode(
    bytes: &[u8],
    size: (u32, u32),
    post: Option<&Post>,
) -> Option<Vec<(Duration, RgbaImage)>> {
    let frames: Frames = if bytes.starts_with(b"GIF8") {
        let decoder = image::codecs::gif::GifDecoder::new(Cursor::new(bytes.to_vec())).ok()?;
        Box::new(decoder.into_frames())
    } else {
        let mut decoder =
            image::codecs::webp::WebPDecoder::new(Cursor::new(bytes.to_vec())).ok()?;
        if !decoder.has_animation() {
            return None;
        }
        let _ = decoder.set_background_color(image::Rgba([0, 0, 0, 0]));
        Box::new(decoder.into_frames())
    };
    let mut kept: Vec<(Duration, RgbaImage)> = Vec::new();
    let mut per_frame;
    let mut step = 1usize;
    for (index, frame) in frames.enumerate().take(MAX_FRAMES) {
        let frame = frame.ok()?;
        let (numerator, denominator) = frame.delay().numer_denom_ms();
        let delay = Duration::from_secs_f64(numerator as f64 / denominator.max(1) as f64 / 1000.)
            .max(SHORTEST_FRAME);
        if index % step != 0 {
            // A frame dropped to stay in budget: its time goes to the one kept before it.
            if let Some(last) = kept.last_mut() {
                last.0 += delay;
            }
            continue;
        }
        let mut buffer = cover(frame.into_buffer(), size);
        if let Some(post) = post {
            buffer = post(buffer);
        }
        for pixel in buffer.pixels_mut() {
            pixel.0.swap(0, 2);
        }
        per_frame = buffer.as_raw().len().max(1);
        kept.push((delay, buffer));
        // Over budget: keep every second frame from here on, and the time of the dropped ones goes to the kept.
        if kept.len() * per_frame > TILE_DECODED {
            let mut thinned: Vec<(Duration, RgbaImage)> = Vec::with_capacity(kept.len() / 2 + 1);
            for (at, (delay, frame)) in kept.into_iter().enumerate() {
                if at % 2 == 0 {
                    thinned.push((delay, frame));
                } else if let Some(last) = thinned.last_mut() {
                    last.0 += delay;
                }
            }
            kept = thinned;
            step *= 2;
        }
    }
    (!kept.is_empty()).then_some(kept)
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

impl super::Root {
    /// Starts the decodes the tiles drawn last frame call for and releases what left the screen. Called once per
    /// render, before the tiles are built.
    pub(super) fn prepare_gif_tiles(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let work = self
            .gif_tiles
            .borrow_mut()
            .prepare(window.is_window_active());
        for image in work.retired {
            cx.drop_image(image, Some(window));
        }
        for (key, image, size, post) in work.decode {
            cx.spawn(async move |this, cx| {
                let decoded = cx
                    .background_executor()
                    .spawn(async move { decode(image.bytes(), size, post.as_ref()) })
                    .await;
                let _ = this.update(cx, |this, cx| {
                    this.gif_tiles.borrow_mut().decoded(&key, decoded);
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

    /// The tile itself: `pixels` is the box the frames are decoded to, `post` what is done to each frame after that.
    pub(super) fn gif_art_pixels(
        &self,
        key: &str,
        image: Arc<gpui::Image>,
        pixels: (u32, u32),
        radius: f32,
        post: Option<Post>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if !animated(image.bytes()) {
            return gpui::img(image)
                .id(gpui::SharedString::from(format!("{key}:still")))
                .absolute()
                .size_full()
                .object_fit(gpui::ObjectFit::Cover)
                .into_any_element();
        }
        self.gif_tiles
            .borrow_mut()
            .register(key, &image, pixels, post);
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
                        if let Some((image, retired)) = cache.borrow_mut().frame(&key) {
                            if let Some(retired) = retired {
                                cx.drop_image(retired, Some(window));
                            }
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
        let images: Vec<_> = self
            .gif_tiles
            .borrow_mut()
            .entries
            .drain()
            .filter_map(|(_, tile)| tile.frame.map(|(_, image)| image))
            .collect();
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

    #[test]
    fn a_tile_plays_only_on_screen_and_in_a_focused_window() {
        // Neither the setting nor the pointer takes part (module 884037); only visibility and focus do.
        for (visible, focused, plays) in [
            (true, true, true),
            (true, false, false),
            (false, true, false),
            (false, false, false),
        ] {
            assert_eq!(should_play(visible, focused), plays);
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

    #[test]
    fn off_screen_tiles_hold_nothing_and_a_tile_decodes_when_it_appears() {
        let image = Arc::new(gpui::Image::from_bytes(gpui::ImageFormat::Webp, Vec::new()));
        let mut cache = Cache::default();
        cache.register("a", &image, (100, 50), None);
        // Not seen yet: nothing to decode.
        assert!(cache.prepare(true).decode.is_empty());
        cache.register("a", &image, (100, 50), None);
        cache.seen.insert("a".to_owned(), true);
        let work = cache.prepare(true);
        assert_eq!(work.decode.len(), 1);
        // The decode runs once, not on every frame.
        cache.register("a", &image, (100, 50), None);
        cache.seen.insert("a".to_owned(), true);
        assert!(cache.prepare(true).decode.is_empty());
        let frames = vec![
            (Duration::from_millis(40), RgbaImage::new(4, 4)),
            (Duration::from_millis(40), RgbaImage::new(4, 4)),
        ];
        cache.decoded("a", Some(frames));
        assert_eq!(cache.decoded_used(), 2 * 64);
        // Off screen again: the frames are dropped.
        cache.register("a", &image, (100, 50), None);
        cache.prepare(true);
        assert_eq!(cache.decoded_used(), 0);
    }
}
